//! Finding the workspace a manifest belongs to by walking up the directories above it, the way
//! Cargo does, and reading a manifest that inherits values from it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::error::{ManifestFailure, ReadWithoutCargoError};
use super::file_system::{MANIFEST_FILE_NAME, normalized};
use super::package_manifest::{PackageManifest, Unfollowed, WorkspaceRoot};
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
/// directory above holds a manifest with a `[workspace]`. The root is read as `unfollowed` says,
/// so a root whose `[workspace.dependencies]` writes a shape Cargo loads and this reader does
/// not follow is refused or read past with the manifests beneath it.
pub(super) fn nearest_above(
    directory: &Path,
    unfollowed: Unfollowed,
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
        let manifest = PackageManifest::parse_with(&text, ancestor, None, unfollowed)
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
fn inherited_workspace(
    directory: &Path,
    unfollowed: Unfollowed,
) -> Result<Option<WorkspaceRoot>, ReadWithoutCargoError> {
    Ok(nearest_above(directory, unfollowed)?.and_then(|above| {
        let excluded = excludes(&above.directory, above.workspace(), directory);
        (!excluded).then(|| above.workspace().clone())
    }))
}

/// Reads the manifest text `text` of the package in `directory`, which is not a member of a
/// workspace already read.
///
/// The text is read alone when it takes nothing from a workspace. When it does, the workspace it
/// inherits from is the nearest root above `directory`, found only then, so a manifest that needs
/// no inheritance never reads another file. The text, and the root it inherits from, are read as
/// `unfollowed` says.
///
/// # Errors
///
/// The text's own error when it cannot be read alone and no workspace above gives it what it
/// needs, which carries the shape Cargo fails on too when the text is one, and an error from
/// reading a manifest above, which carries none: the manifest above is not the one asked for.
pub(super) fn parse_inheriting(
    text: &str,
    directory: &Path,
    manifest_path: &Path,
    unfollowed: Unfollowed,
) -> Result<PackageManifest, ManifestFailure> {
    let alone = PackageManifest::parse_with(text, directory, None, unfollowed);
    let error = match alone {
        Ok(manifest) => return Ok(manifest),
        Err(error) => error,
    };
    let Some(workspace) =
        inherited_workspace(directory, unfollowed).map_err(ManifestFailure::refused)?
    else {
        return Err(ManifestFailure::text(manifest_path, &error));
    };
    PackageManifest::parse_with(text, directory, Some(&workspace), unfollowed)
        .map_err(|error| ManifestFailure::text(manifest_path, &error))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{declares_workspace, excludes, nearest_above, parse_inheriting};
    use crate::workspace::without_cargo::cargo_cannot_load::CargoCannotLoad;
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;
    use crate::workspace::without_cargo::package_manifest::{
        PackageManifest, Unfollowed, WorkspaceRoot,
    };

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

        let from_inner = nearest_above(&inner, Unfollowed::Refuse).expect("readable");
        assert_eq!(
            from_inner.map(|above| above.directory),
            Some(root),
            "the manifest in `inner` itself is not above `inner`"
        );
        assert!(
            nearest_above(Path::new("/"), Unfollowed::Refuse)
                .expect("readable")
                .is_none(),
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
            Unfollowed::Refuse,
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
            Unfollowed::Refuse,
        )
        .expect("a member inheriting its version");

        assert_eq!(manifest.package().expect("a package").version, "4.5.6");
    }

    #[test]
    fn a_root_that_writes_a_git_query_is_read_past_only_when_asked() {
        // A member takes `g` from a root whose `[workspace.dependencies]` gives it a git URL with
        // a query of its own, which Cargo loads. Reading past, the member is read with that
        // declaration; refusing, the root's query is the refusal, named as unsupported.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().canonicalize().expect("a real path");
        let member = root.join("m");
        std::fs::create_dir(&member).expect("a directory");
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\n[workspace.dependencies]\n\
             g = { git = \"https://example.invalid/g?x=1\" }\n",
        )
        .expect("a manifest");
        let text = "[package]\nname = \"m\"\n[dependencies]\ng.workspace = true\n";

        let read_past = parse_inheriting(
            text,
            &member,
            &member.join("Cargo.toml"),
            Unfollowed::ReadPast,
        )
        .expect("a manifest Cargo reads");
        let refused = parse_inheriting(
            text,
            &member,
            &member.join("Cargo.toml"),
            Unfollowed::Refuse,
        )
        .expect_err("the root's query is refused");
        let refused = ReadWithoutCargoError::from(refused);

        let [declaration] = read_past.declarations() else {
            panic!(
                "expected one declaration, got {:?}",
                read_past.declarations()
            );
        };
        assert_eq!(declaration.key, "g");
        assert!(
            matches!(refused, ReadWithoutCargoError::Unsupported { .. }),
            "{refused:?}"
        );
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
            Unfollowed::Refuse,
        )
        .expect_err("nothing gives the version");

        assert_eq!(error.cargo_cannot_load(), None);
        let error = ReadWithoutCargoError::from(error);
        assert!(
            matches!(error, ReadWithoutCargoError::ManifestUnreadable { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_git_and_path_dependency_is_refused_carrying_its_shape() {
        // The shape travels with the refusal: a read that passes over a manifest Cargo cannot
        // load needs to tell this one from any other text error.
        let error = parse_inheriting(
            "[package]\nname = \"m\"\n[dependencies]\n\
             g = { git = \"https://example.invalid/g\", path = \"../g\" }\n",
            Path::new("/absent/m"),
            Path::new("/absent/m/Cargo.toml"),
            Unfollowed::Refuse,
        )
        .expect_err("git and path at once");

        assert_eq!(error.cargo_cannot_load(), Some(CargoCannotLoad::GitAndPath));
    }
}
