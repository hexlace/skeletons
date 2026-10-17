//! A package whose files sit in the workspace's own file system: what a reader without Cargo
//! keeps of its manifest and files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::error::{ManifestFailure, ReadWithoutCargoError};
use super::file_system::{MANIFEST_FILE_NAME, normalized};
use super::package_manifest::{
    Declaration, DeclaredSource, PackageManifest, Unfollowed, WorkspaceRoot,
};
use super::targets::library_name;
use super::workspace_search::parse_inheriting;

/// Where a manifest takes the values it inherits from.
#[derive(Clone, Copy)]
pub(super) enum Inherits<'workspace> {
    /// From the workspace of which the package is a member, when it has one.
    Workspace(Option<&'workspace WorkspaceRoot>),
    /// From the nearest workspace root above the package's directory, found only if the manifest
    /// inherits anything.
    Ancestors,
}

/// Which of a package's path dependencies to follow.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Followed<'names> {
    /// Every one, of any kind: a workspace member's dev, build and optional dependencies are all
    /// in the graph.
    All,
    /// Only those a package outside the workspace brings along: not its dev-dependencies, and not
    /// its optional dependencies until a feature enables one.
    NeededByAnOutsider,
    /// Only the optional dependencies of a package outside the workspace that a feature has
    /// enabled, which the lockfile shows by listing them: those, not dev-dependencies, whose
    /// package is among `locked_path_packages`.
    LockedOptional {
        /// The names of the path packages the package's lock entry lists.
        locked_path_packages: &'names BTreeSet<&'names str>,
    },
}

/// One package whose manifest and files are read from disk.
#[derive(Debug, Clone)]
pub(super) struct PathPackage {
    /// The package's directory, normalized.
    pub(super) directory: PathBuf,
    /// The package's `Cargo.toml`.
    pub(super) manifest_path: PathBuf,
    pub(super) name: String,
    pub(super) version: String,
    /// `[package.metadata]` as `cargo metadata` reports it.
    pub(super) metadata: serde_json::Value,
    /// Whether the manifest writes `package.workspace`.
    pub(super) has_workspace_key: bool,
    /// Every dependency the manifest declares, of any kind.
    pub(super) declarations: Vec<Declaration>,
    /// The library's name, or `None` for a package with no library.
    pub(super) library: Option<String>,
    /// Whether the package is a member of the workspace read.
    pub(super) is_member: bool,
}

impl PathPackage {
    /// The package in `directory` whose manifest has been read.
    ///
    /// # Errors
    ///
    /// Unreadable for a manifest with no `[package]`, a package with no target or no library
    /// file, and a dependency key declared with two different paths: each is a manifest Cargo
    /// refuses too.
    pub(super) fn from_manifest(
        directory: &Path,
        manifest: &PackageManifest,
        is_member: bool,
    ) -> Result<Self, ReadWithoutCargoError> {
        let manifest_path = directory.join(MANIFEST_FILE_NAME);
        let Some(package) = manifest.package() else {
            return Err(ReadWithoutCargoError::unreadable(format!(
                "{} is a virtual manifest, which has no package to be a member or a dependency",
                manifest_path.display()
            )));
        };
        let library = library_name(directory, &package.name, manifest.targets())
            .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;
        let declarations = manifest.declarations().to_vec();
        refuse_a_key_with_two_paths(&manifest_path, &declarations)?;

        assert!(
            directory.is_absolute(),
            "a package directory is absolute, as every directory below the root is"
        );
        Ok(Self {
            directory: directory.to_owned(),
            manifest_path,
            name: package.name.clone(),
            version: package.version.clone(),
            metadata: package.metadata.clone(),
            has_workspace_key: package.has_workspace_key,
            declarations,
            library,
            is_member,
        })
    }

    /// The directories of this package's path dependencies, normalized, of the kinds `followed`
    /// names.
    pub(super) fn path_dependency_directories(&self, followed: Followed<'_>) -> Vec<PathBuf> {
        self.declarations
            .iter()
            .filter(|declaration| match followed {
                Followed::All => true,
                Followed::NeededByAnOutsider => !declaration.is_dev && !declaration.is_optional,
                Followed::LockedOptional {
                    locked_path_packages,
                } => {
                    !declaration.is_dev
                        && declaration.is_optional
                        && locked_path_packages.contains(declaration.package.as_str())
                }
            })
            .filter_map(|declaration| match &declaration.source {
                DeclaredSource::Path { directory } => Some(normalized(directory)),
                DeclaredSource::Git { .. } | DeclaredSource::Registry { .. } => None,
            })
            .collect()
    }
}

/// Reads the manifest in `directory`, handling a shape Cargo loads and this reader does not
/// follow as `unfollowed` says.
///
/// # Errors
///
/// Unreadable when the file is missing or unreadable, is not a manifest, or inherits what nothing
/// gives it. Unsupported for a shape Cargo loads and this reader does not follow, when
/// `unfollowed` refuses it. The failure says which shape Cargo cannot load either it is, when it
/// is one: a manifest that is not there, or a dependency that writes both `git` and `path`.
pub(super) fn read_manifest(
    directory: &Path,
    inherits: Inherits<'_>,
    unfollowed: Unfollowed,
) -> Result<PackageManifest, ManifestFailure> {
    let manifest_path = directory.join(MANIFEST_FILE_NAME);
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| ManifestFailure::file(&manifest_path, &error))?;
    match inherits {
        Inherits::Workspace(workspace) => {
            PackageManifest::parse_with(&text, directory, workspace, unfollowed)
                .map_err(|error| ManifestFailure::text(&manifest_path, &error))
        }
        Inherits::Ancestors => parse_inheriting(&text, directory, &manifest_path, unfollowed),
    }
}

/// Refuses a manifest that declares one key with two different path directories, which Cargo
/// refuses because a dependency must have one source path whatever the build target.
fn refuse_a_key_with_two_paths(
    manifest_path: &Path,
    declarations: &[Declaration],
) -> Result<(), ReadWithoutCargoError> {
    let mut directory_of_key: BTreeMap<&str, PathBuf> = BTreeMap::new();
    for declaration in declarations {
        let DeclaredSource::Path { directory } = &declaration.source else {
            continue;
        };
        let directory = normalized(directory);
        let first = directory_of_key
            .entry(declaration.key.as_str())
            .or_insert_with(|| directory.clone());
        if *first != directory {
            return Err(ReadWithoutCargoError::unreadable(format!(
                "{}: dependency `{}` has different source paths depending on the build target",
                manifest_path.display(),
                declaration.key
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::{Followed, PathPackage, refuse_a_key_with_two_paths};
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;
    use crate::workspace::without_cargo::package_manifest::PackageManifest;

    /// A package at `/w/p` with a library file, built from `manifest`'s text.
    fn package(manifest: &str) -> Result<PathPackage, ReadWithoutCargoError> {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().canonicalize().expect("a real path");
        std::fs::create_dir_all(root.join("src")).expect("a directory");
        std::fs::write(root.join("src/lib.rs"), "").expect("a file");
        let manifest = PackageManifest::parse(manifest, &root, None)
            .expect("the manifest in this test is well formed");
        PathPackage::from_manifest(&root, &manifest, true)
    }

    const DEPENDENCIES: &str = r#"
[package]
name = "p"
[dependencies]
normal = { path = "../normal" }
inside = { path = "./sub/../inside" }
optional = { path = "../optional", optional = true }
registry = "1"
[dev-dependencies]
development = { path = "../development" }
[build-dependencies]
building = { path = "../building" }
"#;

    #[test]
    fn a_member_follows_every_path_dependency_of_every_kind_and_normalizes_it() {
        let package = package(DEPENDENCIES).expect("a package");
        let parent = package.directory.parent().expect("a parent").to_owned();

        let mut followed = package.path_dependency_directories(Followed::All);
        followed.sort();

        let mut expected = vec![
            parent.join("normal"),
            package.directory.join("inside"),
            parent.join("optional"),
            parent.join("development"),
            parent.join("building"),
        ];
        expected.sort();
        assert_eq!(followed, expected);
    }

    #[test]
    fn an_outsider_brings_along_only_its_required_non_development_path_dependencies() {
        // Cargo locks a package outside the workspace without its dev-dependencies and without
        // an optional dependency no feature enabled.
        let package = package(DEPENDENCIES).expect("a package");
        let parent = package.directory.parent().expect("a parent").to_owned();

        let mut followed = package.path_dependency_directories(Followed::NeededByAnOutsider);
        followed.sort();

        let mut expected = vec![
            parent.join("normal"),
            package.directory.join("inside"),
            parent.join("building"),
        ];
        expected.sort();
        assert_eq!(followed, expected);
    }

    #[test]
    fn an_outsider_follows_only_the_optional_dependencies_the_lock_lists() {
        // `optional` is in the set and optional: followed. `normal` is in the set but required,
        // `development` is dev, and `unlisted` is optional but the set lacks it, and none of the
        // three is selected. The set is matched on the package a declaration names, not its key.
        let package = package(
            "[package]\nname = \"p\"\n\
             [dependencies]\n\
             optional = { path = \"../optional\", optional = true }\n\
             renamed = { path = \"../renamed\", optional = true, package = \"actual\" }\n\
             unlisted = { path = \"../unlisted\", optional = true }\n\
             normal = { path = \"../normal\" }\n\
             [dev-dependencies]\n\
             development = { path = \"../development\", optional = true }\n",
        )
        .expect("a package");
        let parent = package.directory.parent().expect("a parent").to_owned();
        let locked = BTreeSet::from(["optional", "actual", "normal", "development"]);

        let mut followed = package.path_dependency_directories(Followed::LockedOptional {
            locked_path_packages: &locked,
        });
        followed.sort();

        let mut expected = vec![parent.join("optional"), parent.join("renamed")];
        expected.sort();
        assert_eq!(followed, expected);
    }

    #[test]
    fn an_empty_set_of_locked_packages_follows_nothing() {
        let package = package(DEPENDENCIES).expect("a package");
        let locked = BTreeSet::new();

        let followed = package.path_dependency_directories(Followed::LockedOptional {
            locked_path_packages: &locked,
        });

        assert_eq!(followed, Vec::<PathBuf>::new());
    }

    #[test]
    fn a_virtual_manifest_is_no_package() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let manifest = PackageManifest::parse("[workspace]\n", directory.path(), None)
            .expect("a virtual root");

        let error = PathPackage::from_manifest(directory.path(), &manifest, false)
            .expect_err("a virtual manifest has no package");

        assert!(matches!(
            error,
            ReadWithoutCargoError::ManifestUnreadable { .. }
        ));
    }

    #[test]
    fn a_key_declared_with_two_paths_is_refused_as_cargo_refuses_it() {
        let error = package(
            "[package]\nname = \"p\"\n[dependencies]\nd = { path = \"../one\" }\n\
             [dev-dependencies]\nd = { path = \"../two\" }\n",
        )
        .expect_err("two source paths for one key");

        let ReadWithoutCargoError::ManifestUnreadable { detail } = error else {
            panic!("expected an unreadable manifest, got {error:?}");
        };
        assert!(
            detail.ends_with(
                "dependency `d` has different source paths depending on the build target"
            ),
            "{detail}"
        );
    }

    #[test]
    fn a_key_declared_twice_with_one_path_is_fine() {
        let manifest = PackageManifest::parse(
            "[package]\nname = \"p\"\n[dependencies]\nd = { path = \"../one\" }\n\
             [dev-dependencies]\nd = { path = \"../one/./\" }\n",
            Path::new("/w/p"),
            None,
        )
        .expect("a manifest");

        refuse_a_key_with_two_paths(Path::new("/w/p/Cargo.toml"), manifest.declarations())
            .expect("one path under one key");
    }
}
