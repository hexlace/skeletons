//! Which locations `check_without_cargo` reads: only the ones a wearing
//! resolves to. A caller that builds without Cargo hands over the location of
//! every locked package that has a source, and a package nothing wears must not
//! stop a report about the ones that are worn.
//!
//! The oracle for a report is `check` on the same workspace, held equal with
//! `assert_eq!` and, so that two equal wrong reports cannot pass, pinned to the
//! values written out here.

#[path = "support/lock.rs"]
mod lock;
mod support;

use std::error::Error;
use std::path::Path;

use skeletons_library::{LockedPackage, PackageLocations, check, check_without_cargo};
use support::{Fixture, TestOutcome, git, wearing_table, worn_once, write_file};

/// The dependency nothing wears, which the project takes from git.
const UNWORN: &str = "plain";

/// A fixture whose member `tools` wears `house-rules` by path and also depends
/// on [`UNWORN`], a crate with a library that nothing wears, taken from a git
/// repository by `file://` URL, with its lockfile made by a cargo whose home has
/// fetched it.
fn wearing_beside_an_unworn_git_dependency() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let directory = fixture.write_crate(UNWORN, "0.3.0", "")?;
    let url = git::commit_repository(&directory, &fixture.home())?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{UNWORN} = {{ git = \"{url}\" }}\n",
            fixture.rules_dependency()
        ),
        &wearing_table("rules"),
    )?;
    fixture.cargo(&["generate-lockfile"])?;
    Ok(fixture)
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

/// Runs the test `test` again in a child born with the fixture's cargo home,
/// handing it the project and `location`, the directory given for [`UNWORN`].
/// `check` reads the process's cargo home, and the tests never change their own
/// environment, so the comparison with it has to run in a child.
fn run_in_child_with(test: &str, fixture: &Fixture, location: &Path) -> TestOutcome {
    lock::run_in_child(
        test,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (
                "SKELETONS_LIBRARY_TEST_PROJECT",
                fixture.project().as_os_str(),
            ),
            ("SKELETONS_LIBRARY_TEST_LOCATION", location.as_os_str()),
        ],
    )
}

/// The body every child runs: the call given the location for [`UNWORN`] gives a
/// report equal to the one `check` gives, and that report holds the path
/// skeleton worn and nothing refused.
fn child_gives_the_report_check_gives() -> TestOutcome {
    let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
    let mut locations = PackageLocations::new();
    locations.insert(
        locked(&project, UNWORN)?,
        lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?,
    );

    let report = check_without_cargo(&project, &locations)?;

    assert_eq!(report, check(&project)?);
    assert!(report.refusals().is_empty(), "{:?}", report.refusals());
    let skeleton = worn_once(&report, "house-rules")?;
    assert_eq!(skeleton.version().to_string(), "4.5.6");
    assert_eq!(skeleton.dependency_key(), "rules");
    Ok(())
}

/// The name of [`UNWORN`] and the version the fixture writes it at, as the
/// manifests at the locations below spell them.
fn manifest_of_unworn(tail: &str) -> String {
    format!("[package]\nname = \"{UNWORN}\"\nversion = \"0.3.0\"\nedition = \"2021\"\n{tail}")
}

const NO_TARGET_TEST: &str = "a_location_with_no_target_for_an_unworn_dependency_is_not_read";

#[test]
fn a_location_with_no_target_for_an_unworn_dependency_is_not_read() -> TestOutcome {
    // Given a location for a dependency nothing wears, whose manifest names the
    // package and has no target, which Cargo refuses a package for: the report
    // is the one `check` gives, because a location no wearing resolves to is
    // never read.
    if lock::is_child() {
        return child_gives_the_report_check_gives();
    }
    let fixture = wearing_beside_an_unworn_git_dependency()?;
    let location = tempfile::tempdir()?;
    write_file(
        &location.path().join("Cargo.toml"),
        manifest_of_unworn("").as_bytes(),
    )?;

    run_in_child_with(NO_TARGET_TEST, &fixture, location.path())
}

const HYPHENATED_LIBRARY_TEST: &str =
    "a_location_with_a_hyphenated_library_name_for_an_unworn_dependency_is_not_read";

#[test]
fn a_location_with_a_hyphenated_library_name_for_an_unworn_dependency_is_not_read() -> TestOutcome {
    // Given a location for a dependency nothing wears, whose manifest names a
    // library with a hyphen, which Cargo refuses: the report is the one
    // `check` gives, as the manifest is never read.
    if lock::is_child() {
        return child_gives_the_report_check_gives();
    }
    let fixture = wearing_beside_an_unworn_git_dependency()?;
    let location = tempfile::tempdir()?;
    write_file(
        &location.path().join("Cargo.toml"),
        manifest_of_unworn("\n[lib]\nname = \"plain-library\"\n").as_bytes(),
    )?;
    write_file(&location.path().join("src/lib.rs"), b"")?;

    run_in_child_with(HYPHENATED_LIBRARY_TEST, &fixture, location.path())
}

const NO_MANIFEST_TEST: &str = "a_location_with_no_manifest_for_an_unworn_dependency_is_not_read";

#[test]
fn a_location_with_no_manifest_for_an_unworn_dependency_is_not_read() -> TestOutcome {
    // Given a location for a dependency nothing wears, which holds no manifest:
    // the report is the one `check` gives.
    if lock::is_child() {
        return child_gives_the_report_check_gives();
    }
    let fixture = wearing_beside_an_unworn_git_dependency()?;
    let location = tempfile::tempdir()?;

    run_in_child_with(NO_MANIFEST_TEST, &fixture, location.path())
}

#[test]
fn a_location_with_no_target_for_the_worn_skeleton_is_still_unreadable() -> TestOutcome {
    // Given the same location with no target, now for the git skeleton that is
    // worn: reading it is what the wearing needs, so the call is refused for the
    // manifest, as Cargo refuses a package with no target.
    let fixture = lock::wearing_from_git()?;
    let location = tempfile::tempdir()?;
    write_file(
        &location.path().join("Cargo.toml"),
        b"[package]\nname = \"house-rules\"\nversion = \"4.5.6\"\nedition = \"2021\"\n",
    )?;
    let mut locations = PackageLocations::new();
    locations.insert(
        locked(&fixture.project(), "house-rules")?,
        location.path().to_path_buf(),
    );

    let outcome = check_without_cargo(fixture.project(), &locations);

    let Err(error) = outcome else {
        return Err(format!("expected an error, got {outcome:?}").into());
    };
    assert!(error.is_manifest_or_lockfile_unreadable(), "{error:?}");
    Ok(())
}
