//! Which workspace a directory belongs to, which packages are its members, and which other
//! packages its members reach by path: the shape of a workspace, read from its manifests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::error::ReadWithoutCargoError;
use super::file_system::MANIFEST_FILE_NAME;
use super::member_patterns;
use super::package_manifest::{PackageManifest, WorkspaceRoot};
use super::path_package::{Followed, Inherits, PathPackage, read_manifest};
use super::workspace_search::{declares_workspace, excludes, nearest_above};

/// A workspace read from its manifests.
#[derive(Debug)]
pub(super) struct Layout {
    /// The workspace root's directory.
    pub(super) root_directory: PathBuf,
    /// Whether the root's manifest writes `[patch]`.
    pub(super) root_writes_patch: bool,
    /// Whether the root's manifest writes `[replace]`.
    pub(super) root_writes_replace: bool,
    /// Every package whose files are on disk: the members, and each package they reach by path,
    /// in the order of their directories.
    pub(super) packages: Vec<PathPackage>,
}

/// The manifest that makes a workspace, and the directory it is in.
struct Root {
    directory: PathBuf,
    manifest: PackageManifest,
}

impl Root {
    /// What the root's `[workspace]` says, or `None` for a package that is its own root with no
    /// `[workspace]`, whose members are only itself.
    const fn workspace(&self) -> Option<&WorkspaceRoot> {
        self.manifest.workspace()
    }
}

/// Reads the workspace that the directory `start` belongs to.
///
/// `start` is a real directory, as `std::fs::canonicalize` gives one.
///
/// # Errors
///
/// Unreadable when no manifest is at or above `start`, when a manifest cannot be read, when the
/// package at or above `start` believes it is in a workspace it is not a member of, when the root
/// is a virtual manifest and the workspace has no members, and for each other way Cargo refuses a
/// manifest that this reader notices.
pub(super) fn discover(start: &Path) -> Result<Layout, ReadWithoutCargoError> {
    let package_directory = nearest_manifest_directory(start)?;
    let root = find_root(&package_directory)?;
    let packages = read_packages(&root)?;

    let is_member = packages
        .iter()
        .any(|package| package.is_member && package.directory == package_directory);
    if !is_member && package_directory != root.directory {
        return Err(ReadWithoutCargoError::unreadable(format!(
            "{} believes it is in the workspace of {}, which does not list it as a member",
            package_directory.join(MANIFEST_FILE_NAME).display(),
            root.directory.join(MANIFEST_FILE_NAME).display()
        )));
    }
    if root.manifest.package().is_none() && packages.iter().all(|package| !package.is_member) {
        return Err(ReadWithoutCargoError::unreadable(format!(
            "{} is a virtual manifest, and the workspace has no members",
            root.directory.join(MANIFEST_FILE_NAME).display()
        )));
    }

    assert!(
        packages.iter().all(|package| package.is_member
            || !package.directory.starts_with(&root.directory)
            || root.workspace().is_none_or(|workspace| excludes(
                &root.directory,
                workspace,
                &package.directory
            ))),
        "a package under the root is a member unless the root excludes it"
    );
    Ok(Layout {
        root_directory: root.directory,
        root_writes_patch: root.manifest.writes_patch(),
        root_writes_replace: root.manifest.writes_replace(),
        packages,
    })
}

/// The nearest directory at or above `start` with a `Cargo.toml` in it.
fn nearest_manifest_directory(start: &Path) -> Result<PathBuf, ReadWithoutCargoError> {
    start
        .ancestors()
        .find(|directory| directory.join(MANIFEST_FILE_NAME).is_file())
        .map(Path::to_owned)
        .ok_or_else(|| {
            ReadWithoutCargoError::unreadable(format!(
                "no {MANIFEST_FILE_NAME} at or above {}",
                start.display()
            ))
        })
}

/// The root of the workspace that the package in `package_directory` is in.
///
/// That is the package's own directory when its manifest has a `[workspace]`, or when no root
/// above it exists or the root above excludes it. Otherwise it is the nearest root above.
fn find_root(package_directory: &Path) -> Result<Root, ReadWithoutCargoError> {
    let manifest_path = package_directory.join(MANIFEST_FILE_NAME);
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| ReadWithoutCargoError::file(&manifest_path, &error))?;
    let declares = declares_workspace(&text)
        .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;

    if !declares {
        if let Some(above) = nearest_above(package_directory)? {
            if !excludes(&above.directory, above.workspace(), package_directory) {
                return Ok(Root {
                    directory: above.directory,
                    manifest: above.manifest,
                });
            }
        }
    }
    let manifest = PackageManifest::parse(&text, package_directory, None)
        .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;
    Ok(Root {
        directory: package_directory.to_owned(),
        manifest,
    })
}

/// Reads every package of the workspace: the members first, then the other packages they reach.
fn read_packages(root: &Root) -> Result<Vec<PathPackage>, ReadWithoutCargoError> {
    let mut by_directory: BTreeMap<PathBuf, PathPackage> = BTreeMap::new();
    read_members(root, &mut by_directory)?;
    let member_count = by_directory.len();
    assert!(
        by_directory.values().all(|package| package.is_member),
        "read_members reads members and nothing else"
    );
    read_outsiders(&mut by_directory)?;

    // The map is keyed by directory, so a directory holds one package; the key must be that
    // package's own directory, or a lookup by directory would find another package's entry.
    for (directory, package) in &by_directory {
        assert!(
            *directory == package.directory,
            "a package is held under its own directory"
        );
    }
    assert!(
        by_directory
            .values()
            .filter(|package| package.is_member)
            .count()
            == member_count,
        "a package read as an outsider is not a member, and no member is read as one"
    );
    assert!(
        by_directory.len() >= member_count,
        "reading outsiders only adds packages"
    );
    Ok(by_directory.into_values().collect())
}

/// Reads each member into `by_directory`: the root's own package, each directory `members`
/// names, and, in a workspace, each path dependency of a member that sits under the root and is
/// not excluded.
///
/// Each directory is read once, so the loop runs at most once per distinct directory reached.
fn read_members(
    root: &Root,
    by_directory: &mut BTreeMap<PathBuf, PathPackage>,
) -> Result<(), ReadWithoutCargoError> {
    let mut pending: Vec<PathBuf> = Vec::new();
    if root.manifest.package().is_some() {
        pending.push(root.directory.clone());
    }
    if let Some(workspace) = root.workspace() {
        pending.extend(
            member_patterns::expand(&root.directory, &workspace.members)?
                .into_iter()
                .filter(|directory| !excludes(&root.directory, workspace, directory)),
        );
    }

    while let Some(directory) = pending.pop() {
        if by_directory.contains_key(&directory) {
            continue;
        }
        let package = read_member(root, &directory)?;
        if let Some(workspace) = root.workspace() {
            pending.extend(
                package
                    .path_dependency_directories(Followed::All)
                    .into_iter()
                    .filter(|dependency| dependency.starts_with(&root.directory))
                    .filter(|dependency| !excludes(&root.directory, workspace, dependency)),
            );
        }
        by_directory.insert(directory, package);
    }
    Ok(())
}

/// The member in `directory`.
fn read_member(root: &Root, directory: &Path) -> Result<PathPackage, ReadWithoutCargoError> {
    if directory == root.directory {
        return PathPackage::from_manifest(directory, &root.manifest, true);
    }
    let manifest = read_manifest(directory, Inherits::Workspace(root.workspace()))?;
    if manifest.workspace().is_some() {
        return Err(ReadWithoutCargoError::unreadable(format!(
            "multiple workspace roots found in the same workspace: {} and {}",
            root.directory.join(MANIFEST_FILE_NAME).display(),
            directory.join(MANIFEST_FILE_NAME).display()
        )));
    }
    PathPackage::from_manifest(directory, &manifest, true)
}

/// Reads into `by_directory` each package a member reaches by path that is not itself a member,
/// and each such package those reach, following only what a package outside the workspace
/// brings along.
///
/// A directory already in `by_directory` is skipped, so each is read once and pushes its own
/// finite list of path dependencies once. That bounds the loop by the members' path dependencies
/// plus those of every outsider read, however many of them name the same directory.
fn read_outsiders(
    by_directory: &mut BTreeMap<PathBuf, PathPackage>,
) -> Result<(), ReadWithoutCargoError> {
    let mut pending: Vec<PathBuf> = by_directory
        .values()
        .flat_map(|member| member.path_dependency_directories(Followed::All))
        .collect();

    while let Some(directory) = pending.pop() {
        if by_directory.contains_key(&directory) {
            continue;
        }
        let manifest = read_manifest(&directory, Inherits::Ancestors)?;
        let package = PathPackage::from_manifest(&directory, &manifest, false)?;
        pending.extend(package.path_dependency_directories(Followed::NeededByAnOutsider));
        by_directory.insert(directory, package);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::discover;
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;
    use crate::workspace::without_cargo::fixture::Fixture as Tree;

    impl Tree {
        /// Every package found from `relative`, as its directory under the root and whether it
        /// is a member.
        fn discover(&self, relative: &str) -> Result<Vec<(String, bool)>, ReadWithoutCargoError> {
            let layout = discover(&self.root.join(relative))?;
            let root = layout.root_directory.clone();
            Ok(layout
                .packages
                .iter()
                .map(|package| {
                    let relative = package.directory.strip_prefix(&root).map_or_else(
                        |_| package.directory.display().to_string(),
                        |relative| relative.display().to_string(),
                    );
                    (relative, package.is_member)
                })
                .collect())
        }
    }

    fn detail(error: ReadWithoutCargoError) -> String {
        let ReadWithoutCargoError::ManifestUnreadable { detail } = error else {
            panic!("expected an unreadable manifest, got {error:?}");
        };
        detail
    }

    #[test]
    fn a_member_may_sit_outside_the_root() {
        // A `members` entry such as `../far` names a directory above the root, and Cargo reads
        // the package there as a member, so a member's directory is not always under the root.
        let tree = Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"../far\"]\n");
        tree.package("../far", "far", "workspace = \"../w\"\n");

        let found = tree.discover("").expect("a workspace");

        assert_eq!(found.len(), 1);
        assert!(found[0].1, "the package outside the root is a member");
    }

    #[test]
    fn a_lone_package_is_its_own_workspace() {
        let tree = Tree::new();
        tree.package("", "solo", "[dependencies]\ninner = { path = \"inner\" }\n");
        tree.package("inner", "inner", "");

        // Without a `[workspace]`, a path dependency under the root is not a member.
        assert_eq!(
            tree.discover("").expect("a lone package"),
            [(String::new(), true), ("inner".to_owned(), false)]
        );
    }

    #[test]
    fn members_come_from_globs_and_in_root_path_dependencies_and_exclusion_wins() {
        let tree = Tree::new();
        tree.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/old\", \"tools\"]\n",
        );
        tree.package(
            "crates/a",
            "a",
            "[dependencies]\ntool = { path = \"../../tools/tool\" }\nhelper = { path = \"../../helpers/helper\" }\nfar = { path = \"../../../far\" }\n",
        );
        tree.package("crates/old", "old", "");
        tree.package("tools/tool", "tool", "");
        tree.package("helpers/helper", "helper", "");
        tree.package("../far", "far", "");
        tree.write("crates/file", "");

        // `old` matches the glob and is excluded; `tool` is a path dependency and is excluded; `helper`
        // is a path dependency under the root and becomes a member; `far` is outside the root.
        let found = tree.discover("").expect("a workspace");
        assert_eq!(
            found
                .iter()
                .filter(|(_, is_member)| *is_member)
                .map(|(directory, _)| directory.as_str())
                .collect::<Vec<_>>(),
            ["crates/a", "helpers/helper"]
        );
        assert_eq!(
            found.len(),
            4,
            "tool and far are read as non-members: {found:?}"
        );
    }

    #[test]
    fn a_member_directory_without_a_manifest_is_refused() {
        let tree = Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        std::fs::create_dir_all(tree.root.join("crates/empty")).expect("a directory");

        let detail = detail(tree.discover("").expect_err("no manifest in a member"));

        assert!(
            detail.contains("crates/empty/Cargo.toml could not be read"),
            "{detail}"
        );
    }

    #[test]
    fn no_manifest_at_or_above_is_refused() {
        let tree = Tree::new();
        std::fs::create_dir_all(tree.root.join("empty/deeper")).expect("directories");

        // `/` and the system temp directory hold no manifest in an ordinary environment.
        let error = tree
            .discover("empty/deeper")
            .expect_err("no manifest anywhere");

        assert!(detail(error).starts_with("no Cargo.toml at or above"));
    }

    #[test]
    fn a_member_is_found_from_a_directory_below_it() {
        let tree = Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"m\"]\n");
        tree.package("m", "m", "");
        std::fs::create_dir_all(tree.root.join("m/src/deep")).expect("directories");

        let found = tree.discover("m/src/deep").expect("from below a member");

        assert_eq!(found, [("m".to_owned(), true)]);
    }

    #[test]
    fn a_package_that_the_root_above_does_not_list_is_refused() {
        let tree = Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"listed\"]\n");
        tree.package("listed", "listed", "");
        tree.package("stray", "stray", "");

        let detail = detail(tree.discover("stray").expect_err("not a member"));

        assert!(detail.contains("does not list it as a member"), "{detail}");
    }

    #[test]
    fn a_package_the_root_above_excludes_is_its_own_workspace() {
        let tree = Tree::new();
        tree.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"listed\"]\nexclude = [\"apart\"]\n",
        );
        tree.package("listed", "listed", "");
        tree.package("apart", "apart", "");

        let found = tree.discover("apart").expect("an excluded package");

        assert_eq!(found, [(String::new(), true)]);
    }

    #[test]
    fn a_member_with_a_workspace_of_its_own_is_refused() {
        let tree = Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"m\"]\n");
        tree.package("m", "m", "[workspace]\n");

        let detail = detail(tree.discover("").expect_err("two roots"));

        assert!(detail.starts_with("multiple workspace roots found in the same workspace"));
    }

    #[test]
    fn a_member_inherits_the_roots_version() {
        let tree = Tree::new();
        tree.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"m\"]\n[workspace.package]\nversion = \"4.5.6\"\n",
        );
        tree.package("m", "m", "version.workspace = true\n");

        let layout = discover(&tree.root).expect("a workspace");

        assert_eq!(layout.packages[0].version, "4.5.6");
    }

    #[test]
    fn the_roots_patch_and_replace_are_noticed() {
        let tree = Tree::new();
        tree.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"m\"]\n[patch.crates-io]\nx = { path = \"x\" }\n",
        );
        tree.package("m", "m", "");

        let layout = discover(&tree.root).expect("a workspace");

        assert!(layout.root_writes_patch);
        assert!(!layout.root_writes_replace);
    }
}
