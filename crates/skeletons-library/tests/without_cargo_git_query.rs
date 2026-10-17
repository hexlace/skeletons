//! A git URL with a query of its own, read without Cargo. Cargo keeps the query in the
//! lockfile's source and reads it back as the same one, and this reader compares a source's
//! query only as one `branch`, `tag` or `rev`, so it refuses the shape by name: in a manifest,
//! and in a lockfile, whichever is read. A manifest that Cargo reads and that is not one of the
//! workspace's own, such as a located package's, is read past the query, because nothing there
//! is compared with a source.

#[path = "support/lock.rs"]
mod lock;
mod support;

use std::error::Error;
use std::path::Path;

use skeletons_library::{
    ClaimedFile, FileState, LockedPackage, PackageLocations, check, check_without_cargo,
};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, HOUSE_NOTES_PATH, TestOutcome, checked_files,
    worn_once,
};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A lockfile of the member `tools` depending on `plain`, a git package whose `source` is as
/// given.
fn lockfile_of_tools_and_plain(source: &str) -> String {
    format!(
        "version = 4\n\n\
         [[package]]\nname = \"plain\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n\n\
         [[package]]\nname = \"tools\"\nversion = \"0.1.0\"\ndependencies = [\"plain\"]\n"
    )
}

#[test]
fn a_git_url_with_a_query_in_a_manifest_is_unsupported_and_names_the_url() -> TestOutcome {
    // Given a member that depends on a git URL written with `?x=1`, the read is refused as
    // unsupported and not as a lockfile out of date: the detail names the manifest, the table,
    // the dependency key and the URL.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "plain = { git = \"https://registry.example.test/plain?x=1\" }\n",
        "",
    )?;
    fixture.write(
        "Cargo.lock",
        lockfile_of_tools_and_plain(&format!(
            "git+https://registry.example.test/plain?x=1#{COMMIT}"
        ))
        .as_bytes(),
    )?;

    let Err(error) = check_without_cargo(fixture.project(), &PackageLocations::new()) else {
        return Err("expected a refusal, got a report".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert!(!error.is_lockfile_not_current(), "{error:?}");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("tools/Cargo.toml: "), "{detail}");
    assert!(
        detail.ends_with(
            "[dependencies] `plain`: `git = \"https://registry.example.test/plain?x=1\"` has a \
             query of its own, which Cargo keeps in the lockfile's source and this reader does \
             not follow"
        ),
        "{detail}"
    );
    Ok(())
}

#[test]
fn a_git_source_with_a_query_in_a_lockfile_is_unsupported_and_names_the_package() -> TestOutcome {
    // Given a manifest that writes the URL plain and a lockfile whose source for it carries
    // `?x=1`, the read is refused as unsupported and not as a lockfile out of date: the detail
    // names the lockfile, the package, its version and the source.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "plain = { git = \"https://registry.example.test/plain\" }\n",
        "",
    )?;
    let source = format!("git+https://registry.example.test/plain?x=1#{COMMIT}");
    fixture.write(
        "Cargo.lock",
        lockfile_of_tools_and_plain(&source).as_bytes(),
    )?;

    let Err(error) = check_without_cargo(fixture.project(), &PackageLocations::new()) else {
        return Err("expected a refusal, got a report".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert!(!error.is_lockfile_not_current(), "{error:?}");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("Cargo.lock: "), "{detail}");
    assert!(
        detail.ends_with(&format!(
            "package `plain` 1.0.0 is locked from `{source}`, whose query is not one `branch`, \
             `tag` or `rev`; Cargo keeps a query of the URL's own in the source, which this \
             reader does not follow"
        )),
        "{detail}"
    );
    Ok(())
}

const LOCATED_EQUALS_CHECK_TEST: &str =
    "a_git_wearing_whose_own_manifest_writes_a_query_is_located_and_gives_the_report_check_gives";

/// The identity cargo wrote for `name` in the project's lockfile.
fn locked(project: &Path, name: &str) -> Result<LockedPackage, Box<dyn Error>> {
    let entry = lock::lock_entry(project, name)?;
    Ok(LockedPackage {
        name: entry.name,
        version: entry.version,
        source: entry.source,
    })
}

#[test]
fn a_git_wearing_whose_own_manifest_writes_a_query_is_located_and_gives_the_report_check_gives()
-> TestOutcome {
    // Given a git wearing whose skeleton's own manifest has a dev-dependency on a git URL
    // written with `?x=1` (Cargo resolves no dev-dependency of a package outside the
    // workspace, so nothing there need exist), and `check_without_cargo` given the directory
    // holding the skeleton: Cargo reads that manifest, so the skeleton is located, not
    // unlocated, and the report equals the one `check` gives and holds the values written here.
    // `check` reads the process's cargo home, so the comparison runs in a child that is born
    // with the fixture's, as for the other git wearing.
    if lock::is_child() {
        let project = lock::handed_path("SKELETONS_LIBRARY_TEST_PROJECT")?;
        let mut locations = PackageLocations::new();
        locations.insert(
            locked(&project, "house-rules")?,
            lock::handed_path("SKELETONS_LIBRARY_TEST_LOCATION")?,
        );

        let expected = check(&project)?;
        let report = check_without_cargo(&project, &locations)?;

        assert_eq!(report, expected);
        let skeleton = worn_once(&report, "house-rules")?;
        assert_eq!(skeleton.version().to_string(), "4.5.6");
        assert_eq!(skeleton.dependency_key(), "rules");
        let states: Vec<FileState> = checked_files(skeleton)?
            .iter()
            .map(ClaimedFile::state)
            .collect();
        assert_eq!(states, [FileState::Matches, FileState::Changed]);
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        return Ok(());
    }
    let fixture = lock::wearing_from_git_with_house_rules_tail(
        "\n[dev-dependencies]\nfoo = { git = \"https://example.test/foo?x=1\" }\n",
    )?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, b"edited\n")?;

    lock::run_in_child(
        LOCATED_EQUALS_CHECK_TEST,
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
