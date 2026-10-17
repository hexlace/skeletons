//! Reading a workspace from its manifests and its lockfile, running no Cargo.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::cargo_configuration::{self, HomeVariables, cargo_home_directory};
use super::currency::verify_lockfile_is_current;
use super::document;
use super::error::ReadWithoutCargoError;
use super::layout;
use super::lockfile::Lockfile;
use super::optional_requirements::refuse_unmet_optional_requirements;
use super::unsupported;
use crate::workspace::schema::Document;
use crate::workspace::{LockedPackageId, Workspace, from_document};

/// The name of the lockfile, beside the root manifest.
const LOCKFILE_NAME: &str = "Cargo.lock";

/// Reads `directory`'s workspace from its manifests and lockfile, running no Cargo.
///
/// The result is what [`read`](crate::read) gives for the same files, or, in the one stale shape
/// the lockfile can be in, what it gives once Cargo brings the lock up to date: every wearing
/// table any member declares. No Cargo is needed, and a Cargo home need not exist.
///
/// Cargo's configuration files are read as Cargo reads them, because three things in them change
/// what Cargo reads: `.cargo/config`, or `.cargo/config.toml` when there is no `config`, in
/// `directory` and in each directory above it, and then the Cargo home's `config` or
/// `config.toml`. The home is `CARGO_HOME` when that is set and not empty, taken from
/// `directory` when relative, and otherwise `.cargo` in a `HOME` that is set and not empty. Only
/// those two variables are read, and nothing is read when neither is set.
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
/// [`ReadWithoutCargoError::ManifestUnreadable`] when a manifest, the lockfile or a configuration
/// file cannot be, or Cargo refuses it too (a virtual workspace with no members, a
/// configuration file that is not TOML and a `git` dependency whose URL is not one Cargo reads,
/// such as `git@github.com:foo/bar.git`, are three),
/// [`ReadWithoutCargoError::Unsupported`] when the workspace uses something only Cargo reads
/// (`[patch]`, `[replace]`, `package.workspace`, one package under two dependency keys, a lockfile
/// format other than 3 and 4, a `git` URL with a query of its own and a locked git source whose
/// query is not one `branch`, `tag` or `rev`, an optional dependency of a package outside the
/// workspace that asks a worn package for a version the locked one does not meet and Cargo cannot
/// add beside it, which is a version in the locked one's range for a registry package and any for a
/// path or git package, and in a configuration file `paths`, `[patch]` or `include`), and
/// [`ReadWithoutCargoError::Lockfile`] when `Cargo.lock` is missing or out of date. A missing
/// `Cargo.lock` is that last error even when the workspace also uses something only Cargo reads.
pub fn read_without_cargo(
    directory: &Path,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
) -> Result<Workspace, ReadWithoutCargoError> {
    read_workspace(directory, locations, &HomeVariables::from_process())
}

/// [`read_without_cargo`] with the variables that find the Cargo home given, so that a test reads
/// a home of its own and not the one of whoever runs it.
pub(super) fn read_workspace(
    directory: &Path,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
    variables: &HomeVariables,
) -> Result<Workspace, ReadWithoutCargoError> {
    Ok(from_document(&read_document(
        directory, locations, variables,
    )?))
}

/// The document [`read_without_cargo`] builds its workspace from, which stands in for the one
/// `cargo metadata` prints for the same files, or, in the one stale shape the lockfile can be in,
/// for the one it prints once Cargo brings the lock up to date.
pub(super) fn read_document(
    directory: &Path,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
    variables: &HomeVariables,
) -> Result<Document, ReadWithoutCargoError> {
    let start = canonical_directory(directory)?;
    // Cargo fails on a configuration file it cannot read before it looks at anything else, so
    // the files are read first. What they ask of Cargo that this reader does not follow is
    // refused only after the lockfile is found and parsed, as with every other refusal.
    let cargo_home = cargo_home_directory(variables, &start);
    let configuration = cargo_configuration::read(&start, cargo_home.as_deref())?;
    let layout = layout::discover(&start)?;
    let (lockfile_path, lockfile_text) = lockfile_text(&layout.root_directory)?;
    let lockfile = Lockfile::parse(&lockfile_text)
        .map_err(|error| ReadWithoutCargoError::text(&lockfile_path, &error))?;
    // The lockfile says which optional dependencies of a package outside the workspace a
    // feature enabled, so it is read before the packages are final, and the check for shapes
    // only Cargo reads follows, to cover every package on disk.
    let layout = layout::follow_locked_optional_dependencies(layout, &lockfile)?;
    configuration.refuse_what_only_cargo_follows()?;
    unsupported::check(&layout)?;
    verify_lockfile_is_current(&lockfile, &layout.packages)?;
    refuse_unmet_optional_requirements(&layout, &lockfile)?;
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

/// The path and text of the lockfile in `root_directory`. A missing lockfile is read before the
/// workspace is checked for shapes only Cargo reads, because Cargo's `--locked` aborts on a missing
/// lockfile before it reads anything else.
fn lockfile_text(root_directory: &Path) -> Result<(PathBuf, String), ReadWithoutCargoError> {
    let path = root_directory.join(LOCKFILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(ReadWithoutCargoError::Lockfile),
        Err(error) => Err(ReadWithoutCargoError::file(&path, &error)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::read_workspace;
    use crate::workspace::without_cargo::cargo_configuration::HomeVariables;
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
        read_workspace(&fixture.root, &BTreeMap::new(), &HomeVariables::unset())
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

        let workspace = read_workspace(&fixture.root, &locations, &HomeVariables::unset())
            .expect("a workspace");

        assert_eq!(
            outcomes(&workspace),
            (
                vec!["app/Cargo.toml house-rules house-rules 4.5.6".to_owned()],
                vec![]
            )
        );
    }

    #[test]
    fn a_lock_with_a_query_of_the_urls_own_is_refused_not_called_out_of_date() {
        // The manifest writes the URL plain, so the lock's `?x=1` is not what it asks for and
        // currency would call the lock stale. The lockfile is read first, and refuses it by
        // name, as unsupported and not as `Lockfile`.
        let source = "git+https://example.test/house-rules?x=1#0123456789abcdef";
        let fixture = wearer(
            "{ git = \"https://example.test/house-rules\" }",
            &lock_of_app_and_skeleton(Some(source)),
        );

        let error = read(&fixture).err().expect("a source this reader refuses");

        let ReadWithoutCargoError::Unsupported { detail } = error else {
            panic!("expected a refusal, got {error:?}");
        };
        assert_eq!(
            detail,
            format!(
                "{}: Cargo.lock: package `house-rules` 4.5.6 is locked from `{source}`, whose \
                 query is not one `branch`, `tag` or `rev`; Cargo keeps a query of the URL's own \
                 in the source, which this reader does not follow",
                fixture.root.join("Cargo.lock").display()
            )
        );
    }

    #[test]
    fn a_directory_that_does_not_exist_is_not_found() {
        let fixture = Fixture::new();

        let error = read_workspace(
            &fixture.root.join("absent"),
            &BTreeMap::new(),
            &HomeVariables::unset(),
        )
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

        let error = read_workspace(
            &fixture.root.join("Cargo.toml"),
            &BTreeMap::new(),
            &HomeVariables::unset(),
        )
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
    /// The lockfile is present and stale, because an unsupported shape is refused before the
    /// lockfile is checked for currency, though not before it is found missing.
    fn refusal_with(root_extra: &str, app_extra: &str) -> ReadWithoutCargoError {
        let fixture = Fixture::new();
        fixture.write(
            "Cargo.toml",
            &format!("[workspace]\nmembers = [\"app\"]\n{root_extra}"),
        );
        fixture.package("app", "app", app_extra);
        fixture.write("Cargo.lock", "version = 4\n");
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
            "[dependencies]\nd = \"1\"\n[dev-dependencies]\n\
             d = { package = \"e\", version = \"1\" }\n",
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
        fixture.write("Cargo.lock", "version = 4\n");

        let error = read(&fixture).err().expect("two twins");

        assert!(matches!(error, ReadWithoutCargoError::Unsupported { .. }));
        assert!(detail_of(error).contains("are both the package `twin` 1.0.0"));
    }

    #[test]
    fn a_missing_lockfile_wins_over_an_unsupported_shape() {
        // One member declares the same package under two keys, which only Cargo reads, and no
        // lockfile exists. Cargo's `--locked` aborts on the lockfile first, so the error is the
        // lockfile's.
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package(
            "app",
            "app",
            "[dependencies]\ndep = { path = \"../dep\" }\n\
             alias = { package = \"dep\", path = \"../dep\" }\n",
        );
        fixture.package("dep", "dep", "");

        let error = read(&fixture).err().expect("no lockfile");

        assert_eq!(error, ReadWithoutCargoError::Lockfile);
    }

    #[test]
    fn a_declaration_without_a_path_locked_as_a_package_on_disk_is_out_of_date() {
        // `house-rules` is declared from a registry and also exists on disk, and the lockfile
        // resolves the declaration to the one on disk: only a patch does that, and Cargo would
        // lock the registry package instead.
        let fixture = wearer("\"4\"", &lock_of_app_and_skeleton(None));
        write_skeleton(&fixture, "house-rules");
        fixture.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"house-rules\"]\n",
        );

        let error = read(&fixture).err().expect("an out of date lockfile");

        assert_eq!(error, ReadWithoutCargoError::Lockfile);
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
    fn a_lockfile_that_is_not_toml_wins_over_an_unsupported_shape() {
        // The root writes `[patch]`, which only Cargo reads, and the lockfile cannot be parsed.
        // Cargo refuses an unparseable lockfile too, so the error is the lockfile's.
        let fixture = Fixture::new();
        fixture.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\"]\n[patch.crates-io]\nx = { path = \"x\" }\n",
        );
        fixture.package("app", "app", "");
        fixture.write("Cargo.lock", "not [toml");

        let error = read(&fixture).err().expect("not TOML");

        assert!(matches!(
            error,
            ReadWithoutCargoError::ManifestUnreadable { .. }
        ));
        assert!(detail_of(error).contains("Cargo.lock"));
    }

    /// A wearer of a path skeleton, with a lockfile that is current, and `text` as the
    /// configuration file of the workspace root.
    fn wearer_with_configuration(text: &str) -> Fixture {
        let fixture = wearer(
            "{ path = \"../house-rules\" }",
            &lock_of_app_and_skeleton(None),
        );
        write_skeleton(&fixture, "house-rules");
        fixture.write(".cargo/config.toml", text);
        fixture
    }

    #[test]
    fn a_configuration_file_asking_for_what_only_cargo_follows_is_unsupported() {
        let fixture = wearer_with_configuration("paths = [\"../override\"]\n");

        let error = read(&fixture).err().expect("a paths override");

        assert_eq!(
            error,
            ReadWithoutCargoError::unsupported(format!(
                "`paths` in {}",
                fixture.root.join(".cargo/config.toml").display()
            ))
        );
    }

    #[test]
    fn a_configuration_that_changes_nothing_is_read_past() {
        let fixture = wearer_with_configuration("[source.crates-io]\nreplace-with = \"v\"\n");

        assert!(read(&fixture).is_ok());
    }

    #[test]
    fn a_missing_or_unreadable_lockfile_wins_over_a_configuration_refusal() {
        // Cargo's `--locked` aborts on the lockfile before it resolves anything a configuration
        // file asks for, so the lockfile's error is the one given.
        let missing = wearer_with_configuration("paths = [\"../override\"]\n");
        std::fs::remove_file(missing.root.join("Cargo.lock")).expect("remove the lockfile");
        let unparsable = wearer_with_configuration("paths = [\"../override\"]\n");
        unparsable.write("Cargo.lock", "not [toml");

        assert_eq!(
            read(&missing).err().expect("no lockfile"),
            ReadWithoutCargoError::Lockfile
        );
        let error = read(&unparsable).err().expect("not TOML");
        assert!(detail_of(error).contains("Cargo.lock"));
    }

    #[test]
    fn a_configuration_file_that_is_not_toml_is_unreadable_before_the_lockfile_is_looked_at() {
        // Cargo fails on a configuration file it cannot parse before it reads anything else,
        // even when the lockfile is missing.
        let fixture = wearer_with_configuration("paths = [\n");
        std::fs::remove_file(fixture.root.join("Cargo.lock")).expect("remove the lockfile");

        let error = read(&fixture).err().expect("not TOML");

        assert!(detail_of(error).starts_with(&format!(
            "{}: ",
            fixture.root.join(".cargo/config.toml").display()
        )));
    }

    #[test]
    fn the_cargo_home_is_read_where_the_variables_say() {
        // `paths` in a Cargo home the test owns is found through `CARGO_HOME`, and through
        // `HOME/.cargo` when `CARGO_HOME` is not set.
        let fixture = wearer_with_configuration("");
        let cargo_home = fixture.root.join("../cargo-home");
        fixture.write("../cargo-home/config.toml", "paths = [\"../override\"]\n");
        fixture.write("../user/.cargo/config.toml", "paths = [\"../override\"]\n");

        let through_cargo_home = read_workspace(
            &fixture.root,
            &BTreeMap::new(),
            &HomeVariables::cargo_home(&cargo_home),
        )
        .err()
        .expect("a paths override in the Cargo home");
        let through_home = read_workspace(
            &fixture.root,
            &BTreeMap::new(),
            &HomeVariables::home(&fixture.root.join("../user")),
        )
        .err()
        .expect("a paths override in HOME/.cargo");

        assert_eq!(
            through_cargo_home,
            ReadWithoutCargoError::unsupported(format!(
                "`paths` in {}",
                cargo_home.join("config.toml").display()
            ))
        );
        assert_eq!(
            through_home,
            ReadWithoutCargoError::unsupported(format!(
                "`paths` in {}",
                fixture.root.join("../user/.cargo/config.toml").display()
            ))
        );
    }

    #[test]
    fn the_workspace_is_found_from_a_directory_below_a_member() {
        let fixture = wearer(
            "{ path = \"../house-rules\" }",
            &lock_of_app_and_skeleton(None),
        );
        write_skeleton(&fixture, "house-rules");
        let below: PathBuf = fixture.root.join("app/src");

        let workspace =
            read_workspace(&below, &BTreeMap::new(), &HomeVariables::unset()).expect("a workspace");

        assert_eq!(workspace.root, fixture.root);
    }
}
