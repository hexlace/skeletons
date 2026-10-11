//! The ways a workspace cannot be read at all: each failure answers exactly one
//! predicate, shows a fixed phrase, and carries what came from outside in
//! `detail()`. The predicate `is_metadata_unreadable` is not tested here.

mod support;

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use skeletons_library::ReadWorkspaceError;
use support::{Fixture, TestOutcome};

/// Which of the five predicates are true of `error`, in the order
/// lockfile-not-current, metadata-failed, not-started, directory-not-found,
/// metadata-unreadable.
fn predicates(error: &ReadWorkspaceError) -> [bool; 5] {
    [
        error.is_lockfile_not_current(),
        error.is_cargo_metadata_failed(),
        error.is_cargo_not_started(),
        error.is_directory_not_found(),
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

    assert_eq!(predicates(&error), [true, false, false, false, false]);
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

    assert_eq!(predicates(&error), [false, true, false, false, false]);
    let detail = error.detail().ok_or("cargo failed with nothing to say")?;
    assert!(detail.contains("Cargo.toml"), "cargo said: {detail}");
    assert_plain(error, "cargo metadata failed");
    Ok(())
}

#[test]
fn a_directory_that_does_not_exist_is_not_found() -> TestOutcome {
    // Given a path under a temporary directory that was never made, cargo
    // cannot be started in it and nothing is there: only the not-found
    // predicate is true, `detail()` holds the operating system's words, and
    // the error shows as its fixed phrase.
    let directory = tempfile::tempdir()?;

    let Err(error) = skeletons_library::check(directory.path().join("never-made")) else {
        return Err("a directory that does not exist was read".into());
    };

    assert_eq!(predicates(&error), [false, false, false, true, false]);
    let detail = error.detail().ok_or("the operating system said nothing")?;
    assert!(
        detail.contains("No such file or directory"),
        "it said: {detail}"
    );
    assert_plain(error, "the directory to read was not found");
    Ok(())
}

#[test]
fn a_file_given_as_the_directory_is_not_found() -> TestOutcome {
    // Given a regular file where a directory is asked for, cargo cannot be
    // started in it and no directory is there: only the not-found predicate is
    // true, and `detail()` holds the operating system's words for it.
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("not-a-directory");
    std::fs::write(&file, b"")?;

    let Err(error) = skeletons_library::check(&file) else {
        return Err("a file was read as a workspace".into());
    };

    assert_eq!(predicates(&error), [false, false, false, true, false]);
    let detail = error.detail().ok_or("the operating system said nothing")?;
    assert!(detail.contains("Not a directory"), "it said: {detail}");
    assert_plain(error, "the directory to read was not found");
    Ok(())
}

/// Gives a directory permissions it can be removed with when it goes out of
/// scope, on every exit path, a panic included.
struct Unlocked<'directory>(&'directory Path);

impl Drop for Unlocked<'_> {
    fn drop(&mut self) {
        // A failure here leaves a directory the temporary directory cannot
        // remove, which is only litter; a panic in `drop` would hide the
        // failure the test is reporting.
        drop(std::fs::set_permissions(
            self.0,
            std::fs::Permissions::from_mode(0o755),
        ));
    }
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn a_directory_that_cannot_be_entered_is_a_cargo_that_could_not_be_started() -> TestOutcome {
    // Given a directory with no permissions at all, it exists but cargo cannot
    // be started in it: only the not-started predicate is true. Permissions are
    // given back before anything is asserted, so the temporary directory can be
    // removed whatever the outcome. A process that bypasses permission checks
    // can still list the directory, so there is nothing to exercise and the
    // test says so.
    let base = tempfile::tempdir()?;
    let locked = base.path().join("locked");
    std::fs::create_dir(&locked)?;
    let unlock = Unlocked(&locked);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))?;
    if std::fs::read_dir(&locked).is_ok() {
        eprintln!("skipped: this process can read a mode-000 directory");
        return Ok(());
    }

    let result = skeletons_library::check(&locked);
    drop(unlock);

    let Err(error) = result else {
        return Err("a directory that cannot be entered was read".into());
    };
    assert_eq!(predicates(&error), [false, false, true, false, false]);
    assert_plain(error, "cargo metadata could not be started");
    Ok(())
}
