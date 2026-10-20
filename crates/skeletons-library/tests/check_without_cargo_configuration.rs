//! `check_without_cargo_configuration`: the read for a caller that does not
//! build with Cargo. It reads the manifests, `Cargo.lock` and the locations it
//! is given, and nothing from Cargo's configuration: no `.cargo/config*` at or
//! above the directory and nothing from the Cargo home. For a workspace with no
//! Cargo configuration it gives the report `check_without_cargo` gives, and it
//! refuses everything else exactly as `check_without_cargo` does.
//!
//! The environment of a process cannot be changed from inside a test, so every
//! case runs in a child process born with a `CARGO_HOME` and `HOME` of the
//! test's own, and no case reads the machine's. The oracles are stated on each
//! test: `check`, which is cargo's own reading; `check_without_cargo`, the
//! other read, for what the two must agree on; and the refusals that
//! `check_without_cargo` documents, written out as fixed values.

#[path = "support/lock.rs"]
mod lock;
mod support;

use std::error::Error;
use std::path::Path;

use skeletons_library::{
    FileState, LockedPackage, PackageLocations, ReadWorkspaceError, RefusalKind, Report, check,
    check_without_cargo, check_without_cargo_configuration,
};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, HOUSE_NOTES_PATH, TestOutcome, checked_files,
    wearing_table, worn_once, write_file,
};

/// The variable under which the parent hands the child the project.
const PROJECT: &str = "SKELETONS_LIBRARY_TEST_PROJECT";
/// The variable under which the parent hands the child the configuration file
/// to plant.
const CONFIGURATION: &str = "SKELETONS_LIBRARY_TEST_CONFIGURATION";
/// The variable under which the parent hands the child the detail
/// `check_without_cargo` must refuse with once the file is planted.
const DETAIL: &str = "SKELETONS_LIBRARY_TEST_DETAIL";
/// The variable under which the parent hands the child a skeleton's directory.
const LOCATION: &str = "SKELETONS_LIBRARY_TEST_LOCATION";

/// A configuration file that puts a package at another directory.
const PATHS_OVERRIDE: &[u8] = b"paths = [\"../override\"]\n";
/// A configuration file whose `[patch.crates-io]` has an entry.
const PATCH_WITH_AN_ENTRY: &[u8] = b"[patch.crates-io]\nhouse-rules = { path = \"../override\" }\n";
/// A configuration file that pulls in another one.
const INCLUDE: &[u8] = b"include = [\"other.toml\"]\n";

/// What a caller can observe of an error: each predicate, in the order the
/// type documents them, the phrase `Display` gives and the detail. The type has
/// no `PartialEq`, so two errors are alike when these are.
type Observed = ([bool; 7], String, Option<String>);

fn observed(error: &ReadWorkspaceError) -> Observed {
    (
        [
            error.is_lockfile_not_current(),
            error.is_cargo_metadata_failed(),
            error.is_cargo_not_started(),
            error.is_directory_not_found(),
            error.is_metadata_unreadable(),
            error.is_manifest_or_lockfile_unreadable(),
            error.is_unsupported_without_cargo(),
        ],
        error.to_string(),
        error.detail().map(str::to_owned),
    )
}

/// What each read gives for `directory` with no locations, both required to be
/// errors; returns what a caller can observe of each, the new read's first.
fn both_refuse(directory: &Path) -> Result<(Observed, Observed), Box<dyn Error>> {
    let Err(new) = check_without_cargo_configuration(directory, &PackageLocations::new()) else {
        return Err("check_without_cargo_configuration gave a report".into());
    };
    let Err(old) = check_without_cargo(directory, &PackageLocations::new()) else {
        return Err("check_without_cargo gave a report".into());
    };
    Ok((observed(&new), observed(&old)))
}

// ---------------------------------------------------------------------------
// Configuration the new read ignores and `check_without_cargo` refuses.
// ---------------------------------------------------------------------------

/// In the child: with the handed file absent the new read gives a report, with
/// the values written here and the one `check_without_cargo` gives; once the
/// file holds `contents`, the new read gives that same report again and
/// `check_without_cargo` refuses as unsupported, with the handed detail.
fn assert_ignored_once_planted(contents: &[u8]) -> TestOutcome {
    let project = lock::handed_path(PROJECT)?;
    let configuration = lock::handed_path(CONFIGURATION)?;
    let detail = lock::handed_path(DETAIL)?;
    let detail = detail.to_str().ok_or("the handed detail is not text")?;
    let locations = PackageLocations::new();

    let before = check_without_cargo_configuration(&project, &locations)?;
    assert_eq!(before, check_without_cargo(&project, &locations)?);
    let skeleton = worn_once(&before, "house-rules")?;
    assert_eq!(skeleton.version().to_string(), "4.5.6");
    assert_eq!(skeleton.dependency_key(), "rules");
    assert!(before.refusals().is_empty(), "{:?}", before.refusals());

    write_file(&configuration, contents)?;

    let after = check_without_cargo_configuration(&project, &locations)?;
    assert_eq!(after, before);
    let Err(error) = check_without_cargo(&project, &locations) else {
        return Err("check_without_cargo read past the planted configuration".into());
    };
    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert_eq!(error.detail(), Some(detail));
    Ok(())
}

/// Starts the child for `test` under `environment` with the three handed values.
fn run_planting(
    test: &str,
    environment: &lock::Environment<'_>,
    fixture: &Fixture,
    configuration: &Path,
    detail: &str,
) -> TestOutcome {
    lock::run_in_child(
        test,
        environment,
        &[
            (PROJECT, fixture.project().as_os_str()),
            (CONFIGURATION, configuration.as_os_str()),
            (DETAIL, detail.as_ref()),
        ],
    )
}

const CARGO_HOME_TEST: &str = "a_paths_override_in_the_cargo_home_is_ignored";

#[test]
fn a_paths_override_in_the_cargo_home_is_ignored() -> TestOutcome {
    // Given a workspace and a `CARGO_HOME` that, once the child plants its
    // `config.toml`, holds `paths`, the new read reports exactly as it did
    // before the file was there. Oracle: `check_without_cargo`, which
    // refuses the same file with the detail it documents today
    // (`` `paths` in <file> ``), and fixed values for the report.
    if lock::is_child() {
        return assert_ignored_once_planted(PATHS_OVERRIDE);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let configuration = fixture.cargo_home().join("config.toml");

    run_planting(
        CARGO_HOME_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &fixture,
        &configuration,
        &format!("`paths` in {}", configuration.display()),
    )
}

const HOME_TEST: &str = "a_paths_override_in_the_cargo_directory_of_home_is_ignored";

#[test]
fn a_paths_override_in_the_cargo_directory_of_home_is_ignored() -> TestOutcome {
    // The same with `CARGO_HOME` unset: cargo then keeps its home in `.cargo`
    // under `HOME`, so that is the file `check_without_cargo` refuses and the
    // new read must not look at.
    if lock::is_child() {
        return assert_ignored_once_planted(PATHS_OVERRIDE);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let configuration = fixture.home().join(".cargo").join("config.toml");

    run_planting(
        HOME_TEST,
        &lock::Environment::HomeOnly {
            home: &fixture.home(),
        },
        &fixture,
        &configuration,
        &format!("`paths` in {}", configuration.display()),
    )
}

/// Runs `test` as a child that plants `.cargo/<file_name>` under `directory_of`
/// of the fixture, which is where a repository's own configuration sits. The
/// refusal names the file by its canonical path, which on macOS is under
/// `/private/var` for a path under `/var`.
fn run_planting_in_a_repository(
    test: &str,
    fixture: &Fixture,
    directory: &Path,
    file_name: &str,
    what: &str,
) -> TestOutcome {
    let configuration = directory.join(".cargo").join(file_name);
    let canonical = std::fs::canonicalize(directory)?
        .join(".cargo")
        .join(file_name);
    run_planting(
        test,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        fixture,
        &configuration,
        &format!("{what} in {}", canonical.display()),
    )
}

const REPOSITORY_PATCH_TEST: &str = "a_patch_in_the_repository_configuration_is_ignored";

#[test]
fn a_patch_in_the_repository_configuration_is_ignored() -> TestOutcome {
    // Given `<project>/.cargo/config.toml` planted with a `[patch.crates-io]`
    // that has an entry, the new read reports as before. `check_without_cargo`
    // refuses a `[patch.<name>]` with an entry, naming the file by its
    // canonical path.
    if lock::is_child() {
        return assert_ignored_once_planted(PATCH_WITH_AN_ENTRY);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;

    run_planting_in_a_repository(
        REPOSITORY_PATCH_TEST,
        &fixture,
        &fixture.project(),
        "config.toml",
        "`[patch.crates-io]`",
    )
}

const UNSUFFIXED_TEST: &str =
    "a_paths_override_in_a_configuration_file_without_an_extension_is_ignored";

#[test]
fn a_paths_override_in_a_configuration_file_without_an_extension_is_ignored() -> TestOutcome {
    // The same for the older spelling, `<project>/.cargo/config`.
    if lock::is_child() {
        return assert_ignored_once_planted(PATHS_OVERRIDE);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;

    run_planting_in_a_repository(
        UNSUFFIXED_TEST,
        &fixture,
        &fixture.project(),
        "config",
        "`paths`",
    )
}

const ABOVE_TEST: &str = "a_paths_override_in_a_directory_above_the_project_is_ignored";

#[test]
fn a_paths_override_in_a_directory_above_the_project_is_ignored() -> TestOutcome {
    // Cargo reads `.cargo/config.toml` in each directory above the one it
    // starts in. Planted in the directory that holds the project, the file is
    // refused by `check_without_cargo` and must be unread by the new read.
    if lock::is_child() {
        return assert_ignored_once_planted(PATHS_OVERRIDE);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let above = fixture
        .project()
        .parent()
        .ok_or("the project has no parent")?
        .to_path_buf();

    run_planting_in_a_repository(ABOVE_TEST, &fixture, &above, "config.toml", "`paths`")
}

const INCLUDE_TEST: &str = "an_include_in_the_repository_configuration_is_ignored";

#[test]
fn an_include_in_the_repository_configuration_is_ignored() -> TestOutcome {
    // `include` pulls in files the new read must not follow, and it must not
    // refuse the file that holds it either. `check_without_cargo` refuses it
    // by name.
    if lock::is_child() {
        return assert_ignored_once_planted(INCLUDE);
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;

    run_planting_in_a_repository(
        INCLUDE_TEST,
        &fixture,
        &fixture.project(),
        "config.toml",
        "`include`",
    )
}

const UNREADABLE_TEST: &str = "a_configuration_file_that_is_not_toml_is_not_read";

#[test]
fn a_configuration_file_that_is_not_toml_is_not_read() -> TestOutcome {
    // A file the new read never opens cannot fail to parse. Planted as
    // `[package` in the Cargo home, the new read still gives its report, and
    // `check_without_cargo` fails on it as a file it could not read, the
    // refusal it documents for a configuration file that is not TOML.
    if lock::is_child() {
        let project = lock::handed_path(PROJECT)?;
        let configuration = lock::handed_path(CONFIGURATION)?;
        let locations = PackageLocations::new();
        let before = check_without_cargo_configuration(&project, &locations)?;
        assert_eq!(
            worn_once(&before, "house-rules")?.version().to_string(),
            "4.5.6"
        );

        write_file(&configuration, b"[package\n")?;

        assert_eq!(
            check_without_cargo_configuration(&project, &locations)?,
            before
        );
        let Err(error) = check_without_cargo(&project, &locations) else {
            return Err("check_without_cargo read a configuration file that is not TOML".into());
        };
        assert!(error.is_manifest_or_lockfile_unreadable(), "{error:?}");
        assert!(
            error
                .detail()
                .ok_or("no detail")?
                .contains(&configuration.display().to_string()),
            "{error:?}"
        );
        return Ok(());
    }
    let fixture = Fixture::wearing_house_rules()?;
    let configuration = fixture.cargo_home().join("config.toml");

    lock::run_in_child(
        UNREADABLE_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (PROJECT, fixture.project().as_os_str()),
            (CONFIGURATION, configuration.as_os_str()),
        ],
    )
}

// ---------------------------------------------------------------------------
// With no Cargo configuration the two reads give the same report.
// ---------------------------------------------------------------------------

/// The states of the files of the skeleton `package`, in the order reported.
fn states(report: &Report, package: &str) -> Result<Vec<FileState>, Box<dyn Error>> {
    Ok(checked_files(worn_once(report, package)?)?
        .iter()
        .map(skeletons_library::ClaimedFile::state)
        .collect())
}

/// The new read's report and `check_without_cargo`'s for `directory`, which
/// must be equal; returns the one.
fn equal_reports(directory: &Path, locations: &PackageLocations) -> Result<Report, Box<dyn Error>> {
    let new = check_without_cargo_configuration(directory, locations)?;
    let old = check_without_cargo(directory, locations)?;
    assert_eq!(new, old);
    Ok(new)
}

/// The identity cargo wrote for `name` in the project's lockfile.
fn locked(project: &Path, name: &str) -> Result<LockedPackage, Box<dyn Error>> {
    let entry = lock::lock_entry(project, name)?;
    Ok(LockedPackage {
        name: entry.name,
        version: entry.version,
        source: entry.source,
    })
}

const PATH_TEST: &str = "a_path_wearing_gives_the_report_check_without_cargo_gives";

#[test]
fn a_path_wearing_gives_the_report_check_without_cargo_gives() -> TestOutcome {
    // Given a path wearing with one file as rendered and one changed and no
    // Cargo configuration anywhere, asked from the workspace root and from the
    // member's directory, the new read's report equals `check_without_cargo`'s
    // and `check`'s (cargo's own reading), and holds the values written here.
    if lock::is_child() {
        let project = lock::handed_path(PROJECT)?;
        let member = project.join("tools");
        for directory in [&project, &member] {
            let report = equal_reports(directory, &PackageLocations::new())?;
            assert_eq!(report, check(directory)?);
            let skeleton = worn_once(&report, "house-rules")?;
            assert_eq!(skeleton.version().to_string(), "4.5.6");
            assert_eq!(skeleton.dependency_key(), "rules");
            assert_eq!(skeleton.manifest().to_str(), Some("tools/Cargo.toml"));
            assert_eq!(
                states(&report, "house-rules")?,
                [FileState::Matches, FileState::Changed]
            );
            assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        }
        return Ok(());
    }
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, b"edited\n")?;

    lock::run_in_child(
        PATH_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[(PROJECT, fixture.project().as_os_str())],
    )
}

const GIT_TEST: &str = "a_git_wearing_gives_the_report_check_without_cargo_gives";

#[test]
fn a_git_wearing_gives_the_report_check_without_cargo_gives() -> TestOutcome {
    // Given a git wearing and the directory holding its skeleton, the new
    // read's report equals `check_without_cargo`'s and `check`'s, run in a
    // child born with the fixture's cargo home so that `check` can read the
    // source it fetched.
    if lock::is_child() {
        let project = lock::handed_path(PROJECT)?;
        let mut locations = PackageLocations::new();
        locations.insert(
            locked(&project, "house-rules")?,
            lock::handed_path(LOCATION)?,
        );

        let report = equal_reports(&project, &locations)?;

        assert_eq!(report, check(&project)?);
        let skeleton = worn_once(&report, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(
            states(&report, "house-rules")?,
            [FileState::Matches, FileState::Changed]
        );
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        return Ok(());
    }
    let fixture = lock::wearing_from_git()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, b"edited\n")?;

    lock::run_in_child(
        GIT_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (PROJECT, fixture.project().as_os_str()),
            (LOCATION, fixture.house_rules_directory().as_os_str()),
        ],
    )
}

const REGISTRY_TEST: &str = "a_registry_wearing_gives_the_report_check_without_cargo_gives";

#[test]
fn a_registry_wearing_gives_the_report_check_without_cargo_gives() -> TestOutcome {
    // Given a wearing of a crate from the default registry, which the
    // fixture's cargo home serves from a directory of vendored crates, and
    // that directory as its location, the reports of the two reads and
    // `check` are equal and hold the values written here.
    if lock::is_child() {
        let project = lock::handed_path(PROJECT)?;
        let mut locations = PackageLocations::new();
        locations.insert(
            locked(&project, lock::REGISTRY_PACKAGE)?,
            lock::handed_path(LOCATION)?,
        );

        let report = equal_reports(&project, &locations)?;

        assert_eq!(report, check(&project)?);
        let skeleton = worn_once(&report, lock::REGISTRY_PACKAGE)?;
        assert_eq!(skeleton.version().to_string(), "2.0.0");
        let files = checked_files(skeleton)?;
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(files[0].path().to_str(), Some(lock::REGISTRY_FILE));
        assert_eq!(files[0].state(), FileState::Changed);
        assert_eq!(files[0].rendered(), lock::REGISTRY_RENDERED.as_bytes());
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        return Ok(());
    }
    let (fixture, vendored) = lock::wearing_from_registry()?;
    fixture.write(lock::REGISTRY_FILE, b"edited\n")?;

    lock::run_in_child(
        REGISTRY_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (PROJECT, fixture.project().as_os_str()),
            (LOCATION, vendored.as_os_str()),
        ],
    )
}

const UNLOCATED_TEST: &str = "an_unlocated_wearing_is_refused_as_check_without_cargo_refuses_it";

#[test]
fn an_unlocated_wearing_is_refused_as_check_without_cargo_refuses_it() -> TestOutcome {
    // Given a git wearing and no location, the new read's report equals
    // `check_without_cargo`'s, and holds one `Unlocated` refusal naming the
    // package exactly as `Cargo.lock` writes it, and no skeleton.
    if lock::is_child() {
        let project = lock::handed_path(PROJECT)?;

        let report = equal_reports(&project, &PackageLocations::new())?;

        let package = locked(&project, "house-rules")?;
        let [refusal] = report.refusals() else {
            return Err(format!("expected one refusal, got {:?}", report.refusals()).into());
        };
        assert_eq!(refusal.kind(), RefusalKind::Unlocated);
        assert_eq!(refusal.unlocated_package(), Some(&package));
        assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
        return Ok(());
    }
    let fixture = lock::wearing_from_git()?;

    lock::run_in_child(
        UNLOCATED_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[(PROJECT, fixture.project().as_os_str())],
    )
}

// ---------------------------------------------------------------------------
// Everything else is refused as `check_without_cargo` refuses it.
// ---------------------------------------------------------------------------

const PATCH_TABLE_TEST: &str =
    "a_patch_in_the_root_manifest_is_refused_as_check_without_cargo_refuses_it";

#[test]
fn a_patch_in_the_root_manifest_is_refused_as_check_without_cargo_refuses_it() -> TestOutcome {
    // Given a root `Cargo.toml` with a `[patch.crates-io]` that has an entry
    // and a current lockfile, both reads fail alike: the same predicates,
    // phrase and detail. The detail is also pinned to the documented refusal:
    // unsupported, naming `[patch]` and the manifest.
    if lock::is_child() {
        let (new, old) = both_refuse(&lock::handed_path(PROJECT)?)?;

        assert_eq!(new, old);
        assert_eq!(new.0, [false, false, false, false, false, false, true]);
        assert_eq!(new.1, "the workspace uses something only cargo can read");
        let detail = new.2.ok_or("no detail")?;
        assert!(detail.starts_with("`[patch]` in "), "{detail}");
        assert!(detail.ends_with("Cargo.toml"), "{detail}");
        return Ok(());
    }
    let fixture = Fixture::wearing_house_rules()?;
    let manifest = fixture.project().join("Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest)?;
    text.push_str("\n[patch.crates-io]\nhouse-rules = { path = \"skeletons/override\" }\n");
    write_file(&manifest, text.as_bytes())?;

    lock::run_in_child(
        PATCH_TABLE_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[(PROJECT, fixture.project().as_os_str())],
    )
}

const REGISTRY_NAME_TEST: &str =
    "a_worn_dependency_with_a_registry_name_is_refused_as_check_without_cargo_refuses_it";

#[test]
fn a_worn_dependency_with_a_registry_name_is_refused_as_check_without_cargo_refuses_it()
-> TestOutcome {
    // Given a member that wears `rules`, declared with `registry = "internal"`,
    // and a lockfile holding a registry package for it, both reads fail alike
    // and the detail is the documented one: the manifest, the dependency key
    // and the registry name.
    if lock::is_child() {
        let (new, old) = both_refuse(&lock::handed_path(PROJECT)?)?;

        assert_eq!(new, old);
        assert_eq!(new.0, [false, false, false, false, false, false, true]);
        let detail = new.2.ok_or("no detail")?;
        assert!(detail.contains("tools/Cargo.toml: "), "{detail}");
        assert!(
            detail.ends_with(
                "the worn dependency `rules` is declared with `registry = \"internal\"`, and only \
                 cargo's configuration says which index that names"
            ),
            "{detail}"
        );
        return Ok(());
    }
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "rules = { package = \"house-rules\", version = \"4\", registry = \"internal\" }\n",
        &wearing_table("rules"),
    )?;
    fixture.write(
        "Cargo.lock",
        b"version = 4\n\n\
          [[package]]\nname = \"tools\"\nversion = \"0.1.0\"\ndependencies = [\"house-rules\"]\n\n\
          [[package]]\nname = \"house-rules\"\nversion = \"4.5.6\"\n\
          source = \"registry+https://registry.example.test/index\"\n\
          checksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n",
    )?;

    lock::run_in_child(
        REGISTRY_NAME_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[(PROJECT, fixture.project().as_os_str())],
    )
}

const NO_LOCKFILE_TEST: &str =
    "a_workspace_with_no_lockfile_is_refused_as_check_without_cargo_refuses_it";

#[test]
fn a_workspace_with_no_lockfile_is_refused_as_check_without_cargo_refuses_it() -> TestOutcome {
    // Given a workspace with no `Cargo.lock`, both reads fail alike with the
    // documented words: the lockfile is missing or out of date, no detail.
    if lock::is_child() {
        let (new, old) = both_refuse(&lock::handed_path(PROJECT)?)?;

        assert_eq!(new, old);
        assert_eq!(new.0, [true, false, false, false, false, false, false]);
        assert_eq!(new.1, "the lockfile is missing or out of date");
        assert_eq!(new.2, None);
        return Ok(());
    }
    let fixture = Fixture::new()?;

    lock::run_in_child(
        NO_LOCKFILE_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[(PROJECT, fixture.project().as_os_str())],
    )
}
