//! Test-only observability: appends one line to `SKELETONS_TEST_ONLY_REMOTE_LOG`
//! immediately before each remote query `behind` makes, so a test can prove
//! a query was — or, for `sync` and a non-default registry, was never —
//! attempted, rather than inferring it from a side effect.
//!
//! Entirely `#[cfg(any(test, feature = "test-util"))]`: this module does not
//! exist at all in a production build.

#![cfg(any(test, feature = "test-util"))]

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Reads `SKELETONS_TEST_ONLY_REMOTE_LOG`; `None` when it is unset, meaning "log
/// nothing" — the default even in a test build, so a test that never sets it
/// pays nothing for this seam.
pub(crate) fn from_environment() -> Option<PathBuf> {
    std::env::var_os("SKELETONS_TEST_ONLY_REMOTE_LOG").map(PathBuf::from)
}

/// Appends `line`, plus a newline, to `path`, creating the file on its first
/// call.
///
/// Best-effort: this is diagnostic output for a test, not a fact `behind`
/// itself depends on, and every test that reads it creates `path`'s parent
/// directory first, so a failure here would itself be a test-fixture defect
/// rather than something a production caller could ever see.
pub(crate) fn append(path: &Path, line: &str) {
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _unused = writeln!(file, "{line}");
}

#[cfg(test)]
mod tests {
    use super::append;

    #[test]
    fn append_creates_the_file_on_its_first_call() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("remote.log");
        assert!(!path.exists());

        append(&path, "crates-io semver");
        append(&path, "git https://example.invalid tags");

        let content = std::fs::read_to_string(&path).expect("the log file must now exist");
        assert_eq!(
            content,
            "crates-io semver\ngit https://example.invalid tags\n"
        );
    }
}
