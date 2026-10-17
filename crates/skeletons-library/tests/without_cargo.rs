//! Reading a workspace without running cargo: `check_without_cargo` answers
//! from the manifests and the lockfile, given where each locked package that is
//! not a path on disk can be read.
//!
//! The oracle for a report is `check` on the same workspace and files, held
//! equal with `assert_eq!` and, so that two equal wrong reports cannot pass,
//! pinned to values written out here. The oracle for a package's identity is the
//! text of `Cargo.lock`.

#[path = "support/lock.rs"]
mod lock;
mod support;

use std::error::Error;
use std::path::Path;

use skeletons_library::{
    FileState, LockedPackage, PackageLocations, Refusal, RefusalKind, Report, check,
    check_without_cargo,
};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED,
    TestOutcome, checked_files, path_dependency, wearing_table, worn_once, write_file,
};

/// The identity cargo wrote for `name` in the project's lockfile.
fn locked(project: &Path, name: &str) -> Result<LockedPackage, Box<dyn Error>> {
    let entry = lock::lock_entry(project, name)?;
    Ok(LockedPackage {
        name: entry.name,
        version: entry.version,
        source: entry.source,
    })
}

/// `check`'s report and `check_without_cargo`'s for the same directory and
/// `locations`, which must be equal; returns the one.
fn equal_reports(directory: &Path, locations: &PackageLocations) -> Result<Report, Box<dyn Error>> {
    let expected = check(directory)?;
    let actual = check_without_cargo(directory, locations)?;
    assert_eq!(actual, expected);
    Ok(actual)
}

/// The states of the files of the skeleton `package`, in the order reported.
fn states(report: &Report, package: &str) -> Result<Vec<FileState>, Box<dyn Error>> {
    let skeleton = worn_once(report, package)?;
    Ok(checked_files(skeleton)?
        .iter()
        .map(skeletons_library::ClaimedFile::state)
        .collect())
}

/// The one refusal of a report, or an error saying what the report held.
fn only_refusal(report: &Report) -> Result<&Refusal, Box<dyn Error>> {
    match report.refusals() {
        [only] => Ok(only),
        other => Err(format!("expected one refusal, got {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// Needing no cargo at all.
// ---------------------------------------------------------------------------

const NO_CARGO_TEST: &str = "a_git_wearing_is_read_with_no_cargo_no_cargo_home_and_no_path";

#[test]
fn a_git_wearing_is_read_with_no_cargo_no_cargo_home_and_no_path() -> TestOutcome {
    // Given a git wearing whose location is given, in a process whose
    // `CARGO_HOME` and `HOME` are empty directories, whose `PATH` is an empty
    // directory and which has no `CARGO`: `check_without_cargo` returns the
    // report. The positive control is `check` in the same process, which cannot
    // start cargo, so the report did not come from running it. Run without the
    // marker the test is the parent: it builds the workspace with a cargo of its
    // own, starts itself as the child and requires that the child ran this test
    // to a pass. Run with the marker it is the child.
    if lock::is_child() {
        let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
        let location = lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?;
        let mut locations = PackageLocations::new();
        locations.insert(locked(&project, "house-rules")?, location);

        let report = check_without_cargo(&project, &locations)?;

        let skeleton = worn_once(&report, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(skeleton.dependency_key(), "rules");
        assert_eq!(
            states(&report, "house-rules")?,
            [FileState::Matches, FileState::Missing]
        );
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        let Err(error) = check(&project) else {
            return Err("check ran a cargo that is not there".into());
        };
        assert!(error.is_cargo_not_started(), "{error:?}");
        return Ok(());
    }
    let fixture = lock::wearing_from_git()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let empty = tempfile::tempdir()?;

    lock::run_in_child(
        NO_CARGO_TEST,
        &lock::Environment::NoCargoAtAll {
            empty: empty.path(),
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

// ---------------------------------------------------------------------------
// The same report as `check`: path, git and registry wearings.
// ---------------------------------------------------------------------------

#[test]
fn a_path_wearing_gives_the_report_check_gives_asked_from_the_root_and_from_a_member() -> TestOutcome
{
    // Given a path wearing with one file as rendered and one changed, no
    // location is needed: a path dependency is located by its manifest. Asked
    // from the workspace root and from the member's own directory, the report
    // equals `check`'s, and it holds the values written here.
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, b"edited\n")?;

    for directory in [fixture.project(), fixture.project().join("tools")] {
        let report = equal_reports(&directory, &PackageLocations::new())?;

        let skeleton = worn_once(&report, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(skeleton.dependency_key(), "rules");
        assert_eq!(skeleton.manifest().to_str(), Some("tools/Cargo.toml"));
        assert_eq!(
            states(&report, "house-rules")?,
            [FileState::Matches, FileState::Changed]
        );
        assert_eq!(
            checked_files(skeleton)?[1].rendered(),
            HOUSE_NOTES_RENDERED.as_bytes()
        );
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        assert_eq!(report.root(), std::fs::canonicalize(fixture.project())?);
    }
    Ok(())
}

#[test]
fn a_dependency_with_a_binary_and_no_library_is_refused_as_check_refuses_it() -> TestOutcome {
    // Given a wearing of a package that has a binary and no library target,
    // cargo writes no resolve edge for it, so `check` reports the wearing as
    // resolving to no locked package. `check_without_cargo` gives the same
    // report: no skeleton, and one `Unresolved` refusal naming the wearing.
    let fixture = Fixture::new()?;
    let directory = fixture
        .house_rules_directory()
        .with_file_name("binary-only");
    write_file(
        &directory.join("Cargo.toml"),
        b"[package]\nname = \"binary-only\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
          publish = false\n\n[package.metadata.skeletons]\n",
    )?;
    write_file(&directory.join("src/main.rs"), b"fn main() {}\n")?;
    fixture.write_member(
        "tools",
        &path_dependency("binary-only", "binary-only", &directory),
        &wearing_table("binary-only"),
    )?;
    fixture.generate_lockfile()?;

    let report = equal_reports(&fixture.project(), &PackageLocations::new())?;

    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    let refusal = only_refusal(&report)?;
    assert_eq!(refusal.kind(), RefusalKind::Unresolved);
    assert_eq!(
        support::wearings_of(refusal),
        [("tools/Cargo.toml", Some("binary-only"), Some("binary-only"))]
    );
    assert!(refusal.unlocated_package().is_none());
    Ok(())
}

const GIT_EQUALS_CHECK_TEST: &str = "a_git_wearing_gives_the_report_check_gives";

#[test]
fn a_git_wearing_gives_the_report_check_gives() -> TestOutcome {
    // Given a git wearing, with the cargo home `check` needs for the source it
    // fetched, and `check_without_cargo` given the directory holding the same
    // skeleton instead: the two reports are equal and hold the values written
    // here. `check` reads the process's cargo home, and the tests never change
    // their own environment, so the comparison runs in a child that is born
    // with the fixture's cargo home.
    if lock::is_child() {
        let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
        let mut locations = PackageLocations::new();
        locations.insert(
            locked(&project, "house-rules")?,
            lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?,
        );

        let report = equal_reports(&project, &locations)?;

        let skeleton = worn_once(&report, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(skeleton.dependency_key(), "rules");
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
        GIT_EQUALS_CHECK_TEST,
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

const REGISTRY_EQUALS_CHECK_TEST: &str = "a_registry_wearing_gives_the_report_check_gives";

#[test]
fn a_registry_wearing_gives_the_report_check_gives() -> TestOutcome {
    // Given a wearing of a crate from the default registry, which the fixture's
    // cargo home serves from a directory of vendored crates, and
    // `check_without_cargo` given that directory: the reports are equal and
    // hold the values written here. The comparison runs in a child born with
    // the fixture's cargo home, as for the git wearing.
    if lock::is_child() {
        let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
        let mut locations = PackageLocations::new();
        locations.insert(
            locked(&project, lock::REGISTRY_PACKAGE)?,
            lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?,
        );

        let report = equal_reports(&project, &locations)?;

        let skeleton = worn_once(&report, lock::REGISTRY_PACKAGE)?;
        assert_eq!(skeleton.version().to_string(), "2.0.0");
        assert_eq!(skeleton.dependency_key(), "registry-rules");
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
        REGISTRY_EQUALS_CHECK_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (
                "SKELETONS_LIBRARY_TEST_PROJECT",
                fixture.project().as_os_str(),
            ),
            ("SKELETONS_LIBRARY_TEST_LOCATION", vendored.as_os_str()),
        ],
    )
}

// ---------------------------------------------------------------------------
// A locked package given no location.
// ---------------------------------------------------------------------------

/// Asserts the one refusal of `report` is an `Unlocated` one for the wearing of
/// `house-rules` under `rules` in `tools`, naming `package`, and that no
/// skeleton is reported.
fn assert_unlocated(report: &Report, package: &LockedPackage) -> TestOutcome {
    let refusal = only_refusal(report)?;
    assert_eq!(refusal.kind(), RefusalKind::Unlocated);
    assert_eq!(refusal.unlocated_package(), Some(package));
    assert_eq!(
        support::wearings_of(refusal),
        [("tools/Cargo.toml", Some("rules"), Some("house-rules"))]
    );
    assert_eq!(refusal.wearings()[0].package(), Some(package.name.as_str()));
    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    Ok(())
}

#[test]
fn a_git_wearing_with_no_location_is_unlocated_and_names_the_locked_package() -> TestOutcome {
    // Given a git wearing and no location at all, the report holds one refusal
    // of kind `Unlocated`, worded `unlocated`, whose `unlocated_package()` is
    // the name, version and source written in `Cargo.lock`, and no skeleton.
    let fixture = lock::wearing_from_git()?;
    let entry = lock::lock_entry(&fixture.project(), "house-rules")?;
    assert!(entry.source.starts_with("git+file://"), "{entry:?}");

    let report = check_without_cargo(fixture.project(), &PackageLocations::new())?;

    let package = LockedPackage {
        name: entry.name,
        version: entry.version,
        source: entry.source,
    };
    assert_unlocated(&report, &package)?;
    assert_eq!(only_refusal(&report)?.kind().as_str(), "unlocated");
    assert_eq!(package.name, "house-rules");
    assert_eq!(package.version, "4.5.6");
    Ok(())
}

#[test]
fn a_registry_wearing_with_no_location_is_unlocated_and_names_the_locked_package() -> TestOutcome {
    // The same for a package from the default registry: its source is the
    // registry's, as written in `Cargo.lock`.
    let (fixture, _vendored) = lock::wearing_from_registry()?;

    let report = check_without_cargo(fixture.project(), &PackageLocations::new())?;

    let package = locked(&fixture.project(), lock::REGISTRY_PACKAGE)?;
    assert_eq!(
        package.source,
        "registry+https://github.com/rust-lang/crates.io-index"
    );
    let refusal = only_refusal(&report)?;
    assert_eq!(refusal.kind(), RefusalKind::Unlocated);
    assert_eq!(refusal.unlocated_package(), Some(&package));
    assert_eq!(
        refusal.wearings()[0].package(),
        Some(lock::REGISTRY_PACKAGE)
    );
    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    Ok(())
}

#[test]
fn giving_the_location_the_refusal_names_removes_the_refusal() -> TestOutcome {
    // Given the refusal of a wearing with no location, the package it names is
    // the key to insert: with that key and the directory holding the skeleton,
    // the next call has no refusal and reports the skeleton with its files.
    let fixture = lock::wearing_from_git()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let mut locations = PackageLocations::new();
    let first = check_without_cargo(fixture.project(), &locations)?;
    let package = only_refusal(&first)?
        .unlocated_package()
        .ok_or("the refusal named no package")?
        .clone();

    locations.insert(package, fixture.house_rules_directory());
    let second = check_without_cargo(fixture.project(), &locations)?;

    assert!(second.refusals().is_empty(), "{:?}", second.refusals());
    let skeleton = worn_once(&second, "house-rules")?;
    assert_eq!(skeleton.version().to_string(), "4.5.6");
    assert_eq!(
        states(&second, "house-rules")?,
        [FileState::Matches, FileState::Missing]
    );
    Ok(())
}

#[test]
fn a_location_holding_another_package_locates_nothing() -> TestOutcome {
    // Given the locked package's key and a directory whose `Cargo.toml` names a
    // different package, the package is still unlocated, with the same
    // refusal as when no location was given.
    let fixture = lock::wearing_from_git()?;
    let package = locked(&fixture.project(), "house-rules")?;
    let mut locations = PackageLocations::new();
    locations.insert(package.clone(), fixture.other_rules_directory());

    let report = check_without_cargo(fixture.project(), &locations)?;

    assert_unlocated(&report, &package)
}

#[test]
fn a_location_holding_the_package_at_another_version_locates_nothing() -> TestOutcome {
    // Given the locked package's key and a directory whose `Cargo.toml` names
    // the right package at version 4.5.7, while the lockfile locked 4.5.6, the
    // package is unlocated, as for a directory naming another package.
    let fixture = lock::wearing_from_git()?;
    let package = locked(&fixture.project(), "house-rules")?;
    fixture.write_house_rules("4.5.7")?;
    let mut locations = PackageLocations::new();
    locations.insert(package.clone(), fixture.house_rules_directory());

    let report = check_without_cargo(fixture.project(), &locations)?;

    assert_unlocated(&report, &package)
}

#[test]
fn a_location_for_a_package_no_wearing_resolves_to_is_not_an_error() -> TestOutcome {
    // Given a location keyed by a package the workspace never locked, whose
    // directory does not exist, the call still returns the report for the
    // wearing it was asked about: nothing reads a location nothing resolves to.
    let fixture = Fixture::wearing_house_rules()?;
    let mut locations = PackageLocations::new();
    locations.insert(
        LockedPackage {
            name: "never-locked".to_owned(),
            version: "0.0.1".to_owned(),
            source: "registry+https://example.invalid/index".to_owned(),
        },
        fixture.project().join("no-such-directory"),
    );

    let report = check_without_cargo(fixture.project(), &locations)?;

    assert_eq!(report, check(fixture.project())?);
    assert!(report.refusals().is_empty(), "{:?}", report.refusals());
    assert_eq!(
        worn_once(&report, "house-rules")?.version().to_string(),
        "4.5.6"
    );
    Ok(())
}

#[test]
fn only_an_unlocated_refusal_names_a_locked_package() -> TestOutcome {
    // Given a refusal of another kind, a wearing table that names no
    // dependency, `unlocated_package()` is `None`; the refusal is the one
    // `check` reports for the same workspace.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &fixture.rules_dependency(),
        &format!("{}{}", wearing_table("rules"), wearing_table("ghost")),
    )?;
    fixture.generate_lockfile()?;

    let report = equal_reports(&fixture.project(), &PackageLocations::new())?;

    let refusal = only_refusal(&report)?;
    assert_eq!(refusal.kind(), RefusalKind::NamesNoDependency);
    assert!(refusal.unlocated_package().is_none());
    assert_eq!(worn_once(&report, "house-rules")?.dependency_key(), "rules");
    Ok(())
}

// ---------------------------------------------------------------------------
// The words and the locations themselves.
// ---------------------------------------------------------------------------

#[test]
fn an_unlocated_refusal_reports_the_word_unlocated() {
    // The word is the identifier a caller keys on, written out here so a change
    // to it alone fails.
    assert_eq!(RefusalKind::Unlocated.as_str(), "unlocated");
}

#[test]
fn package_locations_insert_get_replace_and_collect_by_locked_package() {
    // Given three locked packages, `insert` returns the directory it replaced
    // as a map does, `get` finds a location by the whole identity (a package
    // differing in one field finds none), `len` counts distinct packages, and
    // collecting pairs, with a later pair for the same package winning, gives
    // the locations built by inserting.
    let package = |name: &str, version: &str| LockedPackage {
        name: name.to_owned(),
        version: version.to_owned(),
        source: "registry+https://github.com/rust-lang/crates.io-index".to_owned(),
    };
    let mut locations = PackageLocations::new();
    assert!(locations.is_empty());
    assert_eq!(locations.len(), 0);
    assert_eq!(locations, PackageLocations::default());

    assert_eq!(locations.insert(package("a", "1.0.0"), "/first"), None);
    assert_eq!(
        locations.insert(package("a", "1.0.0"), "/second"),
        Some("/first".into())
    );
    locations.insert(package("a", "1.0.1"), "/other-version");
    locations.insert(package("b", "1.0.0"), String::from("/other-name"));

    assert!(!locations.is_empty());
    assert_eq!(locations.len(), 3);
    assert_eq!(
        locations.get(&package("a", "1.0.0")),
        Some(Path::new("/second"))
    );
    assert_eq!(
        locations.get(&package("a", "1.0.1")),
        Some(Path::new("/other-version"))
    );
    assert_eq!(locations.get(&package("c", "1.0.0")), None);
    let collected: PackageLocations = [
        (package("a", "1.0.0"), "/first"),
        (package("a", "1.0.0"), "/second"),
        (package("a", "1.0.1"), "/other-version"),
        (package("b", "1.0.0"), "/other-name"),
    ]
    .into_iter()
    .collect();
    assert_eq!(collected, locations);
    let mut extended = PackageLocations::new();
    extended.extend([(package("a", "1.0.0"), "/second")]);
    assert_eq!(extended.len(), 1);
    assert_eq!(
        extended.get(&package("a", "1.0.0")),
        Some(Path::new("/second"))
    );
}

// ---------------------------------------------------------------------------
// A lockfile the default path refuses is refused here too.
// ---------------------------------------------------------------------------

/// Asserts both paths refuse `directory` as a lockfile that is not current,
/// with no detail and the same words.
fn assert_both_refuse_the_lockfile(directory: &Path) -> TestOutcome {
    let Err(by_cargo) = check(directory) else {
        return Err("check read a lockfile that is not current".into());
    };
    let Err(without) = check_without_cargo(directory, &PackageLocations::new()) else {
        return Err("check_without_cargo read a lockfile that is not current".into());
    };

    assert!(by_cargo.is_lockfile_not_current(), "{by_cargo:?}");
    assert!(without.is_lockfile_not_current(), "{without:?}");
    assert!(!without.is_manifest_or_lockfile_unreadable(), "{without:?}");
    assert!(!without.is_unsupported_without_cargo(), "{without:?}");
    assert_eq!(without.to_string(), by_cargo.to_string());
    assert_eq!(
        without.to_string(),
        "the lockfile is missing or out of date"
    );
    assert_eq!(without.detail(), by_cargo.detail());
    assert_eq!(without.detail(), None);
    Ok(())
}

#[test]
fn a_workspace_with_no_lockfile_is_refused_as_check_refuses_it() -> TestOutcome {
    // Given a workspace with no `Cargo.lock`, both paths return the error and
    // never a report.
    let fixture = Fixture::new()?;

    assert_both_refuse_the_lockfile(&fixture.project())
}

#[test]
fn a_lockfile_made_stale_by_an_added_dependency_is_refused_as_check_refuses_it() -> TestOutcome {
    // Given a lockfile made for a member that wore one skeleton, and then a
    // dependency added to that member without locking again, both paths return
    // the same error and never a report resting on the older versions.
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            fixture.rules_dependency(),
            fixture.more_dependency()
        ),
        &wearing_table("rules"),
    )?;

    assert_both_refuse_the_lockfile(&fixture.project())
}
