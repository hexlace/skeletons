//! The failures only `check_without_cargo` has, and the ones it shares: each is
//! exactly one predicate, with the fixed phrase `Display` gives it and the
//! detail that names what was wrong.

mod support;

use std::error::Error;

use skeletons_library::{PackageLocations, ReadWorkspaceError, check_without_cargo};
use support::{Fixture, TestOutcome, write_file};

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

/// The error `check_without_cargo` gives for `directory`, or an error saying
/// it gave a report.
fn refusal_of(directory: &std::path::Path) -> Result<ReadWorkspaceError, Box<dyn Error>> {
    match check_without_cargo(directory, &PackageLocations::new()) {
        Ok(report) => Err(format!("expected an error, got {report:?}").into()),
        Err(error) => Ok(error),
    }
}

#[test]
fn a_patch_table_is_unsupported_without_cargo_and_names_the_manifest() -> TestOutcome {
    // Given a workspace whose root manifest has a `[patch]` table, the error
    // is exactly the unsupported one: its phrase is fixed and its detail
    // names the feature and the manifest. A lockfile is made first, so the
    // refusal is for the feature and not for a missing lock.
    let fixture = Fixture::wearing_house_rules()?;
    let manifest = fixture.project().join("Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest)?;
    text.push_str("\n[patch.crates-io]\n");
    write_file(&manifest, text.as_bytes())?;

    let error = refusal_of(&fixture.project())?;

    assert_eq!(
        predicates(&error),
        [false, false, false, false, false, false, true]
    );
    assert_eq!(
        error.to_string(),
        "the workspace uses something only cargo can read"
    );
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.starts_with("`[patch]` in "), "{detail}");
    assert!(detail.ends_with("Cargo.toml"), "{detail}");
    Ok(())
}

#[test]
fn a_manifest_that_is_not_toml_is_unreadable_and_names_the_file() -> TestOutcome {
    // Given a member manifest that is not TOML, the error is exactly the
    // unreadable one, with its fixed phrase and a detail that names the file.
    let fixture = Fixture::wearing_house_rules()?;
    write_file(&fixture.project().join("tools/Cargo.toml"), b"[package\n")?;

    let error = refusal_of(&fixture.project())?;

    assert_eq!(
        predicates(&error),
        [false, false, false, false, false, true, false]
    );
    assert_eq!(
        error.to_string(),
        "a manifest or the lockfile could not be read"
    );
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("tools/Cargo.toml"), "{detail}");
    Ok(())
}

#[test]
fn a_directory_that_is_not_there_is_not_found_with_the_system_words() -> TestOutcome {
    // Given a directory that does not exist, the error is exactly the
    // directory one, and its detail holds the directory asked for.
    let fixture = Fixture::new()?;
    let missing = fixture.project().join("no-such-directory");

    let error = refusal_of(&missing)?;

    assert_eq!(
        predicates(&error),
        [false, false, false, true, false, false, false]
    );
    assert_eq!(error.to_string(), "the directory to read was not found");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("no-such-directory"), "{detail}");
    Ok(())
}
