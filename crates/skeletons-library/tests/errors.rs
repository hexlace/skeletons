//! The ways a workspace cannot be read at all: each failure answers exactly one
//! predicate, shows a fixed phrase, and carries what came from outside in
//! `detail()`.

mod support;

use std::error::Error;

use skeletons_library::ReadWorkspaceError;
use support::{Fixture, TestOutcome};

/// Which of the four predicates are true of `error`, in the order
/// lockfile-not-current, metadata-failed, not-started, metadata-unreadable.
fn predicates(error: &ReadWorkspaceError) -> [bool; 4] {
    [
        error.is_lockfile_not_current(),
        error.is_cargo_metadata_failed(),
        error.is_cargo_not_started(),
        error.is_metadata_unreadable(),
    ]
}

/// Asserts what is true of every error: it shows as `phrase` alone, has no
/// source, and converts into a boxed error that crosses threads.
fn assert_plain(error: ReadWorkspaceError, phrase: &str) {
    assert_eq!(error.to_string(), phrase);
    assert!(error.source().is_none(), "{error:?}");
    let boxed: Box<dyn Error + Send + Sync> = error.into();
    assert_eq!(boxed.to_string(), phrase);
}

#[test]
fn a_workspace_without_a_lockfile_is_not_current_and_carries_no_detail() -> TestOutcome {
    // Given a workspace with no `Cargo.lock`, which `check` never writes: only
    // the lockfile predicate is true, there is no detail, and the error shows
    // as its fixed phrase.
    let fixture = Fixture::new()?;

    let Err(error) = skeletons_library::check(fixture.project()) else {
        return Err("a workspace without a lockfile was read".into());
    };

    assert_eq!(predicates(&error), [true, false, false, false]);
    assert_eq!(error.detail(), None);
    assert_plain(error, "the lockfile is missing or out of date");
    Ok(())
}

#[test]
fn a_directory_with_no_manifest_is_a_cargo_failure_with_cargos_words() -> TestOutcome {
    // Given an empty directory, cargo runs and fails: only the cargo-failed
    // predicate is true, `detail()` is what cargo printed, and `Display` does
    // not repeat it.
    let directory = tempfile::tempdir()?;

    let Err(error) = skeletons_library::check(directory.path()) else {
        return Err("an empty directory was read".into());
    };

    assert_eq!(predicates(&error), [false, true, false, false]);
    let detail = error.detail().ok_or("cargo failed with nothing to say")?;
    assert!(detail.contains("Cargo.toml"), "cargo said: {detail}");
    assert_plain(error, "cargo metadata failed");
    Ok(())
}

#[test]
fn a_directory_that_does_not_exist_is_reported_as_cargo_not_started() -> TestOutcome {
    // Given a path under a temporary directory that was never made, cargo
    // cannot be started in it: only the not-started predicate is true, and
    // `detail()` holds the operating system's words.
    let directory = tempfile::tempdir()?;

    let Err(error) = skeletons_library::check(directory.path().join("never-made")) else {
        return Err("a directory that does not exist was read".into());
    };

    assert_eq!(predicates(&error), [false, false, true, false]);
    let detail = error.detail().ok_or("the operating system said nothing")?;
    assert!(
        detail.contains("No such file or directory"),
        "it said: {detail}"
    );
    assert_plain(error, "cargo metadata could not be started");
    Ok(())
}
