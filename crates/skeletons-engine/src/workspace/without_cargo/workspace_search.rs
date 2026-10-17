//! Finding the workspace a manifest belongs to by walking up the directories above it, the way
//! Cargo does, and reading a manifest that inherits values from it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::error::ReadWithoutCargoError;
use super::file_system::{MANIFEST_FILE_NAME, normalized};
use super::package_manifest::{PackageManifest, WorkspaceRoot};
use super::text_error::TextError;

/// The nearest workspace root above a directory.
#[derive(Debug)]
pub(super) struct AncestorWorkspace {
    /// The directory holding the root's manifest.
    pub(super) directory: PathBuf,
    /// The root's manifest, which has a `[workspace]`.
    pub(super) manifest: PackageManifest,
}

impl AncestorWorkspace {
    /// What the root's `[workspace]` says.
    pub(super) fn workspace(&self) -> &WorkspaceRoot {
        let Some(workspace) = self.manifest.workspace() else {
            unreachable!("an ancestor workspace is only built from a manifest with a [workspace]")
        };
        workspace
    }
}

/// Whether `text`, a manifest, writes a `[workspace]` table.
///
/// This reads nothing else, so a manifest that inherits from a workspace not yet found can be
/// asked the question.
///
/// # Errors
///
/// Unreadable when `text` is not TOML.
pub(super) fn declares_workspace(text: &str) -> Result<bool, TextError> {
    let table: toml::Table = toml::from_str(text)
        .map_err(|error| TextError::unreadable(format!("Cargo.toml is not valid TOML: {error}")))?;
    Ok(table.contains_key("workspace"))
}

/// The workspace root nearest above `directory` (not `directory` itself), or `None` when no
/// directory above holds a manifest with a `[workspace]`.
pub(super) fn nearest_above(
    directory: &Path,
) -> Result<Option<AncestorWorkspace>, ReadWithoutCargoError> {
    for ancestor in directory.ancestors().skip(1) {
        let manifest_path = ancestor.join(MANIFEST_FILE_NAME);
        let text = match std::fs::read_to_string(&manifest_path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(ReadWithoutCargoError::file(&manifest_path, &error)),
        };
        let declares = declares_workspace(&text)
            .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;
        if !declares {
            continue;
        }
        let manifest = PackageManifest::parse(&text, ancestor, None)
            .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;
        assert!(
            manifest.workspace().is_some(),
            "a manifest that declares a [workspace] has one"
        );
        return Ok(Some(AncestorWorkspace {
            directory: ancestor.to_owned(),
            manifest,
        }));
    }
    Ok(None)
}

/// Whether the root at `root_directory`, which wrote `workspace`, excludes the package in
/// `package_directory`.
///
/// A package is excluded when its directory is at or under an `exclude` path, unless a `members`
/// path written out literally names it or a directory above it. That is Cargo's own rule, and it
/// compares the paths as written, so a `members` glob never rescues an excluded package.
pub(super) fn excludes(
    root_directory: &Path,
    workspace: &WorkspaceRoot,
    package_directory: &Path,
) -> bool {
    let is_under_any = |paths: &[String]| {
        paths
            .iter()
            .any(|path| package_directory.starts_with(normalized(&root_directory.join(path))))
    };
    is_under_any(&workspace.exclude) && !is_under_any(&workspace.members)
}

/// The workspace `directory`'s package inherits from, when it is not a workspace member: the
/// nearest root above it, unless that root excludes it.
fn inherited_workspace(directory: &Path) -> Result<Option<WorkspaceRoot>, ReadWithoutCargoError> {
    Ok(nearest_above(directory)?.and_then(|above| {
        let excluded = excludes(&above.directory, above.workspace(), directory);
        (!excluded).then(|| above.workspace().clone())
    }))
}

/// Reads the manifest text `text` of the package in `directory`, which is not a member of a
/// workspace already read.
///
/// The text is read alone when it takes nothing from a workspace. When it does, the workspace it
/// inherits from is the nearest root above `directory`, found only then, so a manifest that needs
/// no inheritance never reads another file.
///
/// # Errors
///
/// The text's own error when it cannot be read alone and no workspace above gives it what it
/// needs, and an error from reading a manifest above.
pub(super) fn parse_inheriting(
    text: &str,
    directory: &Path,
    manifest_path: &Path,
) -> Result<PackageManifest, ReadWithoutCargoError> {
    let alone = PackageManifest::parse(text, directory, None);
    let error = match alone {
        Ok(manifest) => return Ok(manifest),
        Err(error) => error,
    };
    let Some(workspace) = inherited_workspace(directory)? else {
        return Err(ReadWithoutCargoError::text(manifest_path, &error));
    };
    PackageManifest::parse(text, directory, Some(&workspace))
        .map_err(|error| ReadWithoutCargoError::text(manifest_path, &error))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{declares_workspace, excludes, nearest_above, parse_inheriting};
    use crate::workspace::without_cargo::package_manifest::{PackageManifest, WorkspaceRoot};

    fn workspace(members: &[&str], exclude: &[&str]) -> WorkspaceRoot {
        let text = format!("[workspace]\nmembers = {members:?}\nexclude = {exclude:?}\n");
        PackageManifest::parse(&text, Path::new("/w"), None)
            .expect("a root")
            .workspace()
            .cloned()
            .expect("a workspace")
    }

    #[test]
    fn a_workspace_table_is_noticed_without_reading_anything_else() {
        assert_eq!(declares_workspace("[workspace]\n"), Ok(true));
        assert_eq!(
            declares_workspace("[package]\nname = \"a\"\nversion.workspace = true\n"),
            Ok(false)
        );
        assert!(declares_workspace("not = [toml").is_err());
    }

    #[test]
    fn a_package_under_an_exclude_path_is_excluded() {
        let root = workspace(&["crates/*"], &["crates/old"]);

        assert!(excludes(Path::new("/w"), &root, Path::new("/w/crates/old")));
        assert!(excludes(
            Path::new("/w"),
            &root,
            Path::new("/w/crates/old/deep")
        ));
        assert!(!excludes(
            Path::new("/w"),
            &root,
            Path::new("/w/crates/older")
        ));
        assert!(!excludes(
            Path::new("/w"),
            &root,
            Path::new("/w/crates/new")
        ));
    }

    #[test]
    fn a_literal_member_is_not_excluded_but_a_glob_member_is() {
        // Cargo compares the paths as written: `crates/old` named out loud stays a member of an
        // excluded `crates`, and `crates/*` does not rescue anything.
        let literal = workspace(&["crates/old"], &["crates"]);
        let globbed = workspace(&["crates/*"], &["crates"]);

        assert!(!excludes(
            Path::new("/w"),
            &literal,
            Path::new("/w/crates/old")
        ));
        assert!(excludes(
            Path::new("/w"),
            &globbed,
            Path::new("/w/crates/old")
        ));
    }

    #[test]
    fn the_nearest_workspace_above_is_found_and_the_directory_itself_is_not_searched() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().canonicalize().expect("a real path");
        let outer = root.join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).expect("directories");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("a manifest");
        std::fs::write(inner.join("Cargo.toml"), "[workspace]\nmembers = [\"x\"]\n")
            .expect("a manifest");

        let from_inner = nearest_above(&inner).expect("readable");
        assert_eq!(
            from_inner.map(|above| above.directory),
            Some(root),
            "the manifest in `inner` itself is not above `inner`"
        );
        assert!(
            nearest_above(Path::new("/")).expect("readable").is_none(),
            "nothing is above the file system root"
        );
    }

    #[test]
    fn a_manifest_that_needs_no_inheritance_reads_no_other_file() {
        // The directory does not exist, so reading any manifest above it would fail: parsing
        // alone must succeed without looking.
        let manifest = parse_inheriting(
            "[package]\nname = \"a\"\n",
            Path::new("/absent/a"),
            Path::new("/absent/a/Cargo.toml"),
        )
        .expect("a manifest that inherits nothing");

        assert_eq!(manifest.package().expect("a package").name, "a");
    }

    #[test]
    fn a_manifest_that_inherits_takes_the_nearest_workspace_above() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().canonicalize().expect("a real path");
        let member = root.join("m");
        std::fs::create_dir(&member).expect("a directory");
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\n[workspace.package]\nversion = \"4.5.6\"\n",
        )
        .expect("a manifest");

        let manifest = parse_inheriting(
            "[package]\nname = \"m\"\nversion.workspace = true\n",
            &member,
            &member.join("Cargo.toml"),
        )
        .expect("a member inheriting its version");

        assert_eq!(manifest.package().expect("a package").version, "4.5.6");
    }

    #[test]
    fn a_manifest_that_inherits_with_no_workspace_above_is_refused_by_its_own_error() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let member = directory.path().join("m");
        std::fs::create_dir(&member).expect("a directory");

        let error = parse_inheriting(
            "[package]\nname = \"m\"\nversion.workspace = true\n",
            &member,
            &member.join("Cargo.toml"),
        )
        .expect_err("nothing gives the version");

        assert!(
            matches!(
                error,
                crate::workspace::without_cargo::error::ReadWithoutCargoError::ManifestUnreadable { .. }
            ),
            "{error:?}"
        );
    }
}
