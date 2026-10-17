//! Two workspaces on which `check_without_cargo` once differed from `check`,
//! each held to what `check` gives: one repository wearing under two git
//! references, and a missing lockfile beside a shape only cargo reads.
//!
//! The oracle is `check` on the same workspace and files, and, so that two
//! equal wrong answers cannot pass, values written out here.

#[path = "support/lock.rs"]
mod lock;
mod support;

use std::error::Error;
use std::path::Path;

use skeletons_library::{
    FileState, LockedPackage, PackageLocations, ReadWorkspaceError, check, check_without_cargo,
};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, HOUSE_NOTES_PATH, TestOutcome, checked_files,
    git, path_dependency, wearing_table, worn_once,
};

/// The seven predicates of `error`, in the order the type documents them.
fn predicates(error: &ReadWorkspaceError) -> [bool; 7] {
    [
        error.is_lockfile_not_current(),
        error.is_cargo_metadata_failed(),
        error.is_cargo_not_started(),
        error.is_directory_not_found(),
        error.is_metadata_unreadable(),
        error.is_manifest_or_lockfile_unreadable(),
        error.is_unsupported_without_cargo(),
    ]
}

/// The identity cargo wrote for a lockfile entry.
fn locked(entry: lock::LockEntry) -> LockedPackage {
    LockedPackage {
        name: entry.name,
        version: entry.version,
        source: entry.source,
    }
}

/// The error `check` gives for `directory`, or an error saying it gave a report.
fn check_error(directory: &Path) -> Result<ReadWorkspaceError, Box<dyn Error>> {
    match check(directory) {
        Ok(report) => Err(format!("expected check to fail, got {report:?}").into()),
        Err(error) => Ok(error),
    }
}

/// The error `check_without_cargo` gives for `directory`, or an error saying it
/// gave a report.
fn check_without_cargo_error(directory: &Path) -> Result<ReadWorkspaceError, Box<dyn Error>> {
    match check_without_cargo(directory, &PackageLocations::new()) {
        Ok(report) => Err(format!("expected check_without_cargo to fail, got {report:?}").into()),
        Err(error) => Ok(error),
    }
}

const TWO_REFERENCES_TEST: &str =
    "two_git_references_to_one_repository_give_the_report_check_gives";

#[test]
fn two_git_references_to_one_repository_give_the_report_check_gives() -> TestOutcome {
    // Given one repository wearing through two members, the first at its
    // default branch and the second at a `rev`, so the lockfile holds two
    // `house-rules` entries that differ in their query and each dependency
    // line spells the source without its commit: the two reports are equal and
    // hold the values written here. `check` reads the process's cargo home,
    // and the tests never change their own environment, so the comparison runs
    // in a child that is born with the fixture's cargo home.
    if lock::is_child() {
        let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
        let location = lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?;
        let mut locations = PackageLocations::new();
        for entry in lock::lock_entries(&project, "house-rules")? {
            locations.insert(locked(entry), location.clone());
        }

        let expected = check(&project)?;
        let actual = check_without_cargo(&project, &locations)?;

        assert_eq!(actual, expected);
        let skeleton = worn_once(&actual, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(skeleton.dependency_key(), "rules");
        let states: Vec<FileState> = checked_files(skeleton)?
            .iter()
            .map(skeletons_library::ClaimedFile::state)
            .collect();
        assert_eq!(states, [FileState::Matches, FileState::Changed]);
        assert!(actual.refusals().is_empty(), "{:?}", actual.refusals());
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let url = git::commit_repository(&fixture.house_rules_directory(), &fixture.home())?;
    let commit = git::head_commit(&fixture.house_rules_directory(), &fixture.home())?;
    fixture.write_workspace_manifest(&["tools", "other"])?;
    fixture.write_member(
        "tools",
        &format!("rules = {{ package = \"house-rules\", git = \"{url}\" }}\n"),
        &wearing_table("rules"),
    )?;
    fixture.write_member(
        "other",
        &format!("rules = {{ package = \"house-rules\", git = \"{url}\", rev = \"{commit}\" }}\n"),
        "",
    )?;
    fixture.cargo(&["generate-lockfile"])?;
    let entries = lock::lock_entries(&fixture.project(), "house-rules")?;
    assert_eq!(
        entries.len(),
        2,
        "the lockfile must hold the two references: {entries:?}"
    );
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, b"edited\n")?;

    lock::run_in_child(
        TWO_REFERENCES_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (
                "SKELETONS_LIBRARY_TEST_PROJECT",
                fixture.project().as_os_str(),
            ),
            (
                "SKELETONS_LIBRARY_TEST_LOCATION",
                fixture.house_rules_directory().as_os_str(),
            ),
        ],
    )
}

#[test]
fn a_missing_lockfile_with_an_unsupported_shape_is_the_lockfile_error_check_gives() -> TestOutcome {
    // Given a workspace with no `Cargo.lock` whose member declares one package
    // under two keys, which only cargo reads: both `check` and
    // `check_without_cargo` fail with the lockfile error and no other, because
    // cargo's `--locked` aborts on the missing lockfile before it reads the
    // rest. Neither writes the lockfile.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            fixture.rules_dependency(),
            path_dependency("older", "house-rules", &fixture.house_rules_directory())
        ),
        &wearing_table("rules"),
    )?;

    let expected = check_error(&fixture.project())?;
    let actual = check_without_cargo_error(&fixture.project())?;

    assert_eq!(
        predicates(&expected),
        [true, false, false, false, false, false, false]
    );
    assert_eq!(
        predicates(&actual),
        [true, false, false, false, false, false, false]
    );
    assert!(
        !fixture.project().join("Cargo.lock").exists(),
        "neither call writes a lockfile"
    );
    Ok(())
}
