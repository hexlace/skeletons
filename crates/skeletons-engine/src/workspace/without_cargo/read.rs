//! Reading a workspace from its manifests and its lockfile, running no Cargo.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::currency::verify_lockfile_is_current;
use super::document;
use super::error::ReadWithoutCargoError;
use super::layout;
use super::lockfile::Lockfile;
use super::unsupported;
use crate::workspace::schema::Document;
use crate::workspace::{LockedPackageId, Workspace, from_document};

/// The name of the lockfile, beside the root manifest.
const LOCKFILE_NAME: &str = "Cargo.lock";

/// Reads `directory`'s workspace from its manifests and lockfile, running no Cargo.
///
/// The result is what [`read`](crate::read) gives for the same workspace: every wearing table any
/// member declares. No Cargo and no Cargo home is needed.
///
/// `directory` may be any directory at or below the workspace; the manifests above it are found
/// as Cargo finds them. `locations` says where the files of each locked package with a `source`
/// are, keyed by the package's name, version and source as `Cargo.lock` writes them, each value
/// the directory holding that package's own `Cargo.toml`. Packages without a source are found
/// from the manifests. A package a wearing resolves to that `locations` does not hold, or holds
/// at a directory whose `Cargo.toml` names another package or version, is refused as unlocated
/// in the result, and every other wearing is still read.
///
/// # Errors
///
/// [`ReadWithoutCargoError::DirectoryNotFound`] when `directory` cannot be read,
/// [`ReadWithoutCargoError::ManifestUnreadable`] when a manifest or the lockfile cannot be, or
/// Cargo refuses it too, [`ReadWithoutCargoError::Unsupported`] when the workspace uses
/// something only Cargo reads (`[patch]`, `[replace]`, `package.workspace`, one package under
/// two dependency keys, a lockfile format other than 3 and 4), and
/// [`ReadWithoutCargoError::Lockfile`] when `Cargo.lock` is missing or out of date.
pub fn read_without_cargo(
    directory: &Path,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
) -> Result<Workspace, ReadWithoutCargoError> {
    Ok(from_document(&read_document(directory, locations)?))
}

/// The document [`read_without_cargo`] builds its workspace from, which stands in for the one
/// `cargo metadata` prints for the same files.
pub(super) fn read_document(
    directory: &Path,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
) -> Result<Document, ReadWithoutCargoError> {
    let start = canonical_directory(directory)?;
    let layout = layout::discover(&start)?;
    unsupported::check(&layout)?;
    let lockfile = read_lockfile(&layout.root_directory)?;
    verify_lockfile_is_current(&lockfile, &layout.packages)?;
    document::build(&layout, &lockfile, locations)
}

/// `directory` as a real path, which is where Cargo itself would start.
fn canonical_directory(directory: &Path) -> Result<PathBuf, ReadWithoutCargoError> {
    let canonical = std::fs::canonicalize(directory).map_err(|error| {
        ReadWithoutCargoError::DirectoryNotFound {
            detail: format!("{}: {error}", directory.display()),
        }
    })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(ReadWithoutCargoError::DirectoryNotFound {
            detail: format!("{} is not a directory", directory.display()),
        })
    }
}

/// The lockfile in `root_directory`.
fn read_lockfile(root_directory: &Path) -> Result<Lockfile, ReadWithoutCargoError> {
    let path = root_directory.join(LOCKFILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(ReadWithoutCargoError::Lockfile);
        }
        Err(error) => return Err(ReadWithoutCargoError::file(&path, &error)),
    };
    Lockfile::parse(&text).map_err(|error| ReadWithoutCargoError::text(&path, &error))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::read_without_cargo;
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;
    use crate::workspace::without_cargo::fixture::Fixture;
    use crate::workspace::{LockedPackageId, Wearing, WearingRefusal, Workspace};

    const SKELETON_SOURCE: &str = "git+https://example.test/house-rules#0123456789abcdef";

    /// A lockfile with the wearer `app` depending on the skeleton `house-rules` 4.5.6, which has
    /// a `source` when `skeleton_source` has one.
    fn lock_of_app_and_skeleton(skeleton_source: Option<&str>) -> String {
        let source =
            skeleton_source.map_or_else(String::new, |source| format!("source = \"{source}\"\n"));
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\
             dependencies = [\"house-rules\"]\n\n[[package]]\nname = \"house-rules\"\n\
             version = \"4.5.6\"\n{source}"
        )
    }

    /// The skeleton `house-rules` 4.5.6 at `relative`: a library with a skeletons table.
    fn write_skeleton(fixture: &Fixture, relative: &str) {
        fixture.package(
            relative,
            "house-rules",
            "version = \"4.5.6\"\n[package.metadata.skeletons]\n",
        );
    }

    /// A workspace of one member `app` that wears `house-rules` through `dependency`, declared
    /// as written, with the lockfile text given.
    fn wearer(dependency: &str, lock: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package(
            "app",
            "app",
            &format!(
                "version = \"0.1.0\"\n[dependencies]\nhouse-rules = {dependency}\n\
                 [package.metadata.skeletons.house-rules]\n"
            ),
        );
        fixture.write("Cargo.lock", lock);
        fixture
    }

    fn read(fixture: &Fixture) -> Result<Workspace, ReadWithoutCargoError> {
        read_without_cargo(&fixture.root, &BTreeMap::new())
    }

    /// The worn dependencies of `workspace` as `manifest key package version`, and its refusals.
    fn outcomes(workspace: &Workspace) -> (Vec<String>, Vec<WearingRefusal>) {
        let mut worn = Vec::new();
        let mut refused = Vec::new();
        for wearing in &workspace.wearing {
            match wearing {
                Wearing::Worn(dependency) => worn.push(format!(
                    "{} {} {} {}",
                    dependency.manifest, dependency.key, dependency.package, dependency.version
                )),
                Wearing::Refused(refusal) => refused.push(refusal.clone()),
            }
        }
        (worn, refused)
    }

    fn detail_of(error: ReadWithoutCargoError) -> String {
        match error {
            ReadWithoutCargoError::DirectoryNotFound { detail }
            | ReadWithoutCargoError::ManifestUnreadable { detail }
            | ReadWithoutCargoError::Unsupported { detail } => detail,
            ReadWithoutCargoError::Lockfile => panic!("expected an error with a detail"),
        }
    }

    #[test]
    fn a_path_skeleton_is_worn_with_its_locked_version() {
        let fixture = wearer(
            "{ path = \"../house-rules\" }",
            &lock_of_app_and_skeleton(None),
        );
        write_skeleton(&fixture, "house-rules");

        let workspace = read(&fixture).expect("a workspace");

        assert_eq!(workspace.root, fixture.root);
        assert_eq!(
            outcomes(&workspace),
            (
                vec!["app/Cargo.toml house-rules house-rules 4.5.6".to_owned()],
                vec![]
            )
        );
    }

    #[test]
    fn a_git_skeleton_with_no_location_is_refused_as_unlocated_naming_its_lock_identity() {
        let fixture = wearer(
            "{ git = \"https://example.test/house-rules\" }",
            &lock_of_app_and_skeleton(Some(SKELETON_SOURCE)),
        );

        let (worn, refused) = outcomes(&read(&fixture).expect("a workspace"));

        assert_eq!(worn, Vec::<String>::new());
        assert_eq!(
            refused,
            [WearingRefusal::Unlocated {
                manifest: "app/Cargo.toml".to_owned(),
                dependency: "house-rules".to_owned(),
                package: LockedPackageId {
                    name: "house-rules".to_owned(),
                    version: "4.5.6".to_owned(),
                    source: SKELETON_SOURCE.to_owned(),
                },
            }]
        );
    }

    #[test]
    fn a_git_skeleton_given_its_location_is_worn() {
        let fixture = wearer(
            "{ git = \"https://example.test/house-rules\" }",
            &lock_of_app_and_skeleton(Some(SKELETON_SOURCE)),
        );
        write_skeleton(&fixture, "../checkout");
        let locations = BTreeMap::from([(
            LockedPackageId {
                name: "house-rules".to_owned(),
                version: "4.5.6".to_owned(),
                source: SKELETON_SOURCE.to_owned(),
            },
            fixture.root.join("../checkout"),
        )]);

        let workspace = read_without_cargo(&fixture.root, &locations).expect("a workspace");

        assert_eq!(
            outcomes(&workspace),
            (
                vec!["app/Cargo.toml house-rules house-rules 4.5.6".to_owned()],
                vec![]
            )
        );
    }

    #[test]
    fn a_directory_that_does_not_exist_is_not_found() {
        let fixture = Fixture::new();

        let error = read_without_cargo(&fixture.root.join("absent"), &BTreeMap::new())
            .err()
            .expect("nothing is there");

        assert!(
            matches!(error, ReadWithoutCargoError::DirectoryNotFound { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_is_not_a_directory() {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\n");

        let error = read_without_cargo(&fixture.root.join("Cargo.toml"), &BTreeMap::new())
            .err()
            .expect("a file");

        assert!(
            matches!(error, ReadWithoutCargoError::DirectoryNotFound { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_directory_with_no_manifest_at_or_above_is_unreadable() {
        let fixture = Fixture::new();

        let error = read(&fixture).err().expect("no manifest");

        assert!(
            detail_of(error).starts_with("no Cargo.toml at or above"),
            "names what is missing"
        );
    }

    #[test]
    fn a_member_glob_matching_a_directory_without_a_manifest_is_unreadable() {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        std::fs::create_dir_all(fixture.root.join("crates/empty")).expect("a directory");

        let error = read(&fixture).err().expect("a member with no manifest");

        assert!(
            matches!(error, ReadWithoutCargoError::ManifestUnreadable { .. }),
            "{error:?}"
        );
        assert!(
            detail_of(read(&fixture).err().expect("again")).contains("crates/empty/Cargo.toml")
        );
    }

    #[test]
    fn a_package_with_no_target_is_unreadable() {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[package]\nname = \"bare\"\n");
        fixture.write("readme.md", "");

        let error = read(&fixture).err().expect("no target");

        assert!(
            matches!(error, ReadWithoutCargoError::ManifestUnreadable { .. }),
            "{error:?}"
        );
        assert!(detail_of(read(&fixture).err().expect("again")).contains("no target"));
    }

    /// The error of reading a workspace whose root holds `root_extra` after
    /// `[workspace] members = ["app"]` and whose member `app` has `app_extra` in its package.
    /// No lockfile is written: an unsupported shape is refused before the lockfile is read.
    fn refusal_with(root_extra: &str, app_extra: &str) -> ReadWithoutCargoError {
        let fixture = Fixture::new();
        fixture.write(
            "Cargo.toml",
            &format!("[workspace]\nmembers = [\"app\"]\n{root_extra}"),
        );
        fixture.package("app", "app", app_extra);
        read(&fixture).err().expect("an unsupported workspace")
    }

    #[test]
    fn patch_and_replace_are_unsupported_by_name() {
        let patched = refusal_with("[patch.crates-io]\nx = { path = \"x\" }\n", "");
        let replaced = refusal_with("[replace]\n\"x:1.0.0\" = { path = \"x\" }\n", "");

        assert!(matches!(patched, ReadWithoutCargoError::Unsupported { .. }));
        assert!(detail_of(patched).starts_with("`[patch]` in "));
        assert!(matches!(
            replaced,
            ReadWithoutCargoError::Unsupported { .. }
        ));
        assert!(detail_of(replaced).starts_with("`[replace]` in "));
    }

    #[test]
    fn a_package_workspace_key_is_unsupported_by_name() {
        let error = refusal_with("", "workspace = \"..\"\n");

        assert!(matches!(error, ReadWithoutCargoError::Unsupported { .. }));
        assert!(detail_of(error).starts_with("`package.workspace` in "));
    }

    #[test]
    fn one_package_under_two_keys_is_unsupported_and_so_is_one_key_naming_two_packages() {
        let two_keys = refusal_with(
            "",
            "[dependencies]\nserde = \"1\"\nolder = { package = \"serde\", version = \"0.9\" }\n",
        );
        let two_packages = refusal_with(
            "",
            "[dependencies]\nd = \"1\"\n[dev-dependencies]\nd = { package = \"e\", version = \"1\" }\n",
        );

        assert!(matches!(
            two_keys,
            ReadWithoutCargoError::Unsupported { .. }
        ));
        assert!(
            detail_of(two_keys).contains("the package `serde` is declared under more than one key")
        );
        assert!(matches!(
            two_packages,
            ReadWithoutCargoError::Unsupported { .. }
        ));
        assert!(detail_of(two_packages).contains("the key `d` names more than one package"));
    }

    #[test]
    fn two_path_packages_with_one_name_and_version_are_unsupported() {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"one\", \"two\"]\n");
        fixture.package("one", "twin", "version = \"1.0.0\"\n");
        fixture.package("two", "twin", "version = \"1.0.0\"\n");

        let error = read(&fixture).err().expect("two twins");

        assert!(matches!(error, ReadWithoutCargoError::Unsupported { .. }));
        assert!(detail_of(error).contains("are both the package `twin` 1.0.0"));
    }

    #[test]
    fn a_declaration_without_a_path_locked_as_a_package_on_disk_is_unsupported() {
        // `house-rules` is declared from a registry and also exists on disk, and the lockfile
        // resolves the declaration to the one on disk: only a patch does that.
        let fixture = wearer("\"4\"", &lock_of_app_and_skeleton(None));
        write_skeleton(&fixture, "house-rules");
        fixture.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"house-rules\"]\n",
        );

        let error = read(&fixture).err().expect("an unsupported resolution");

        assert!(matches!(error, ReadWithoutCargoError::Unsupported { .. }));
        assert!(
            detail_of(error).contains("locked as a package on disk but declared without a `path`")
        );
    }

    #[test]
    fn a_missing_lockfile_is_the_lockfile_error() {
        let fixture = wearer("{ path = \"../house-rules\" }", "");
        write_skeleton(&fixture, "house-rules");
        std::fs::remove_file(fixture.root.join("Cargo.lock")).expect("remove the lockfile");

        let error = read(&fixture).err().expect("no lockfile");

        assert_eq!(error, ReadWithoutCargoError::Lockfile);
    }

    #[test]
    fn a_path_package_the_lockfile_does_not_hold_is_the_lockfile_error() {
        let fixture = wearer("{ path = \"../house-rules\" }", "version = 4\n");
        write_skeleton(&fixture, "house-rules");

        let error = read(&fixture).err().expect("a stale lockfile");

        assert_eq!(error, ReadWithoutCargoError::Lockfile);
    }

    #[test]
    fn a_lockfile_in_an_older_format_is_unsupported() {
        let fixture = wearer("{ path = \"../house-rules\" }", "version = 2\n");
        write_skeleton(&fixture, "house-rules");

        let error = read(&fixture).err().expect("format 2");

        assert!(matches!(error, ReadWithoutCargoError::Unsupported { .. }));
        assert!(detail_of(error).contains("format version 2"));
    }

    #[test]
    fn a_lockfile_that_is_not_toml_is_unreadable() {
        let fixture = wearer("{ path = \"../house-rules\" }", "not [toml");
        write_skeleton(&fixture, "house-rules");

        let error = read(&fixture).err().expect("not TOML");

        assert!(matches!(
            error,
            ReadWithoutCargoError::ManifestUnreadable { .. }
        ));
    }

    #[test]
    fn the_workspace_is_found_from_a_directory_below_a_member() {
        let fixture = wearer(
            "{ path = \"../house-rules\" }",
            &lock_of_app_and_skeleton(None),
        );
        write_skeleton(&fixture, "house-rules");
        let below: PathBuf = fixture.root.join("app/src");

        let workspace = read_without_cargo(&below, &BTreeMap::new()).expect("a workspace");

        assert_eq!(workspace.root, fixture.root);
    }
}
