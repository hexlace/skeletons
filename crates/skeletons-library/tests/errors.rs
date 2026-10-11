//! The ways a workspace cannot be read at all: each failure answers exactly one
//! predicate, shows a fixed phrase, and carries what came from outside in
//! `detail()`. The predicate `is_metadata_unreadable` is reached by a cargo
//! that prints a document of a format version the library does not read, which
//! needs a process of its own: see the last test.

mod support;

use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

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

/// The name of the test that runs its body in a process of its own, as the
/// test harness lists it. The parent starts the child with `--exact` and this
/// name, so the two must agree: a child that matches no test prints that it ran
/// none, which the parent refuses.
const UNKNOWN_FORMAT_TEST: &str = "a_cargo_printing_an_unknown_format_version_is_unreadable";

/// Set, to anything, only in the process the parent test starts to run the
/// body. Its presence is what makes a run of the test the child.
const UNKNOWN_FORMAT_CHILD_MARKER: &str = "SKELETONS_LIBRARY_TEST_UNKNOWN_FORMAT_CHILD";

/// A `cargo metadata` document that is valid JSON with every field the reader
/// requires, whose format version is one the reader does not know, so that the
/// only thing wrong with it is the version.
const UNKNOWN_FORMAT_DOCUMENT: &str = "{\"version\":2,\"workspace_root\":\"/\",\
     \"workspace_members\":[],\"packages\":[],\"resolve\":{\"nodes\":[]}}";

/// Writes an executable `sh` script at `path` that prints
/// [`UNKNOWN_FORMAT_DOCUMENT`] whatever it is asked, and exits 0.
///
/// The file is complete and closed before this returns, so this process no
/// longer holds it open for writing when the child is started.
fn write_unknown_format_cargo(path: &Path) -> TestOutcome {
    // Deviation from RS-SINGLE-TOOLCHAIN (integration tests use
    // `std::process::Command`, not shell scripts): the library runs the
    // program `CARGO` names, which has to be an executable file, and a
    // one-line POSIX `sh` that prints a document is the narrowest one. A Rust
    // stand-in would need a binary target added to the crate for this one test.
    let mut script = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(path)?;
    script
        .write_all(format!("#!/bin/sh\nprintf '%s\\n' '{UNKNOWN_FORMAT_DOCUMENT}'\n").as_bytes())?;
    script.sync_all()?;
    drop(script);
    Ok(())
}

/// Starts this test binary again, to run only [`UNKNOWN_FORMAT_TEST`], with
/// `cargo` as its `CARGO` and the marker set, and requires that the child ran
/// that one test and it passed.
///
/// The library reads `CARGO` from the environment of the process that calls it,
/// and the tests never change their own environment, so the call has to be made
/// by a process that was born with the stand-in in its environment.
fn run_unknown_format_in_a_child(cargo: &Path) -> TestOutcome {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", UNKNOWN_FORMAT_TEST])
        .env("CARGO", cargo)
        .env(UNKNOWN_FORMAT_CHILD_MARKER, "1")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // A child that matched no test exits 0, so success alone proves nothing:
    // only a body that ran and passed prints these two lines.
    let ran_and_passed = stdout.contains(&format!("test {UNKNOWN_FORMAT_TEST} ... ok"))
        && stdout.contains("1 passed");
    if output.status.success() && ran_and_passed {
        Ok(())
    } else {
        Err(format!(
            "the child did not run {UNKNOWN_FORMAT_TEST} to a pass ({status}); \
             stdout: {stdout}; stderr: {stderr}",
            status = output.status,
        )
        .into())
    }
}

/// The body of the test, run by the child: asks about a directory whose cargo
/// prints a document of an unknown format version.
fn read_with_an_unknown_format_cargo() -> TestOutcome {
    let directory = tempfile::tempdir()?;

    let Err(error) = skeletons_library::check(directory.path()) else {
        return Err("a document of an unknown format version was read".into());
    };

    assert_eq!(predicates(&error), [false, false, false, false, true]);
    let detail = error.detail().ok_or("the reader said nothing")?;
    assert!(detail.contains("format version 2"), "it said: {detail}");
    assert_plain(error, "cargo metadata's output could not be read");
    Ok(())
}

#[test]
fn a_cargo_printing_an_unknown_format_version_is_unreadable() -> TestOutcome {
    // Given a cargo that prints a well-formed document whose format version
    // the reader does not know: only the metadata-unreadable predicate is true,
    // `detail()` names the format version rather than a parse failure, and the
    // error shows as its fixed phrase. The library takes cargo from the
    // environment of the process that calls it, so the test starts itself again
    // with the stand-in as `CARGO`. Run without the marker it is the parent: it
    // writes the stand-in, starts the child and requires that the child ran
    // this test to a pass. Run with the marker it is the child: it makes the
    // assertions above.
    if std::env::var_os(UNKNOWN_FORMAT_CHILD_MARKER).is_some() {
        return read_with_an_unknown_format_cargo();
    }
    let directory = tempfile::tempdir()?;
    let cargo = directory.path().join("cargo");
    write_unknown_format_cargo(&cargo)?;

    run_unknown_format_in_a_child(&cargo)
}
