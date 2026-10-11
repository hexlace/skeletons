//! Whether `wear` can write the two files it changes, in place.
//!
//! `cargo add` writes through a temporary file and a rename, so it succeeds
//! on a read-only manifest. `wear`'s own table write and `rollback`'s restore
//! write in place, and both fail on one, which would leave the dependency
//! added and neither a worn project nor the files as they were. So both files
//! are asked, before anything is written.
//!
//! The question put to the operating system is the one `wear` is about to
//! need answered: can this file be opened for writing, without creating it
//! and without truncating it. Opening for append writes no byte and leaves
//! the modification time alone, and the answer comes from the operating system
//! itself, so an access control list, an immutable flag and a read-only mount
//! all answer as they would for the real write, which reading the permission
//! bits would not. A process that can write any file, a superuser, passes,
//! which is correct: it can then also restore what it wrote.

use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;

use rituals::Failure;

use super::{CommandLineCrate, WRITING};
use crate::skeleton::Escaped;

/// Whether a file has to exist for `wear` to change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// The file is there, or `wear` has nothing to change: the manifest.
    Required,
    /// The file may not be there yet: `Cargo.lock`. A missing one is not
    /// this check's to refuse, since reading the workspace `--locked` already
    /// refuses a lockfile that is missing.
    MayBeAbsent,
}

/// Refuses, before anything is written, a manifest or an existing
/// `Cargo.lock` that cannot be written in place.
///
/// Each file is named as `wear` shows it, relative to the workspace root.
pub(super) fn check(command_line_crate: &CommandLineCrate) -> Result<(), Failure> {
    for (path, shown, presence) in [
        (
            &command_line_crate.manifest_path,
            &command_line_crate.manifest_shown,
            Presence::Required,
        ),
        (
            &command_line_crate.lockfile_path,
            &command_line_crate.lockfile_shown,
            Presence::MayBeAbsent,
        ),
    ] {
        check_file(path, shown, presence)?;
    }
    Ok(())
}

/// Opens `path` for appending and drops it at once, which writes nothing.
fn check_file(path: &Path, shown: &str, presence: Presence) -> Result<(), Failure> {
    assert!(!shown.is_empty(), "a file `wear` changes has a name");
    match (
        OpenOptions::new()
            .append(true)
            .create(false)
            .truncate(false)
            .open(path),
        presence,
    ) {
        (Ok(_file), Presence::Required | Presence::MayBeAbsent) => Ok(()),
        (Err(error), Presence::MayBeAbsent) if error.kind() == ErrorKind::NotFound => Ok(()),
        (Err(error), Presence::Required | Presence::MayBeAbsent) => {
            Err(Failure::new(line(shown, &error)))
        }
    }
}

/// `` {path} cannot be written in place, so wear wrote nothing: wear changes
/// it in place, and the operating system refused to open it for writing:
/// {error}; make it writable, then run the `wear` task again ``.
fn line(shown: &str, error: &std::io::Error) -> String {
    let shown = Escaped(shown);
    let reason = error.to_string();
    let reason = Escaped(&reason);
    format!(
        "{shown} cannot be written in place, so {} wrote nothing: {} changes it in place, and \
         the operating system refused to open it for writing: {reason}; make it writable, then \
         {}",
        WRITING.name(),
        WRITING.name(),
        WRITING.run_again(),
    )
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::{Presence, check_file, line};

    #[test]
    fn the_refusal_names_the_file_says_nothing_was_written_and_ends_in_a_remedy() {
        let error = std::io::Error::from(ErrorKind::PermissionDenied);

        assert_eq!(
            line("crates/cli/Cargo.toml", &error),
            "crates/cli/Cargo.toml cannot be written in place, so wear wrote nothing: wear \
             changes it in place, and the operating system refused to open it for writing: \
             permission denied; make it writable, then run the `wear` task again"
        );
    }

    #[test]
    fn a_file_that_can_be_opened_for_writing_passes_and_is_left_untouched() {
        // The probe must write nothing: the bytes and the modification time
        // are read before and after.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("Cargo.toml");
        std::fs::write(&path, "[package]\n").expect("write the file");
        let modified = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .expect("read the modification time");

        let outcome = check_file(&path, "Cargo.toml", Presence::Required);

        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(std::fs::read(&path).expect("read it back"), b"[package]\n");
        assert_eq!(
            std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .expect("read the modification time"),
            modified
        );
    }

    #[test]
    fn a_missing_lockfile_passes_and_is_not_created() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("Cargo.lock");

        let outcome = check_file(&path, "Cargo.lock", Presence::MayBeAbsent);

        assert!(outcome.is_ok(), "{outcome:?}");
        assert!(!path.exists(), "the probe must not create the file");
    }

    #[test]
    fn a_missing_manifest_is_refused_with_the_operating_systems_answer() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("Cargo.toml");

        let failure = check_file(&path, "Cargo.toml", Presence::Required)
            .expect_err("a manifest that is not there cannot be changed");

        assert!(
            failure
                .to_string()
                .starts_with("Cargo.toml cannot be written in place, so wear wrote nothing:"),
            "{failure}"
        );
        assert!(!path.exists(), "the probe must not create the file");
    }

    #[test]
    fn a_path_that_cannot_be_opened_for_writing_is_refused_for_either_file_whoever_runs_it() {
        // A directory is never opened for writing, by anyone, so unlike a
        // read-only file it refuses a superuser too. It stands in for every
        // reason the operating system can give.
        let directory = tempfile::tempdir().expect("a temporary directory");

        for presence in [Presence::Required, Presence::MayBeAbsent] {
            let failure = check_file(directory.path(), "Cargo.lock", presence)
                .expect_err("a directory cannot be written as a file");

            assert!(
                failure
                    .to_string()
                    .ends_with("then run the `wear` task again"),
                "{presence:?}: {failure}"
            );
        }
    }
}
