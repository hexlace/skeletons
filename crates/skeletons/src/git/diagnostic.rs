//! Reading the one line a person needs out of git's own, possibly
//! multi-line, stderr.

/// The first line starting `fatal: ` (with that prefix removed) — the
/// clearest single line git's own error output carries. Failing that, the
/// first non-blank line, trimmed. Failing that (nothing but blank lines, or
/// no stderr at all), `"git printed no message"`.
///
/// Lossy UTF-8 (git's own stderr is not guaranteed valid UTF-8, and this is
/// shown to a person, never parsed further), and never more than one
/// line: every caller here shows this text inline in a sentence of its
/// own, so a diagnostic that itself spanned lines would corrupt the
/// sentence around it.
pub(crate) fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("fatal: ") {
            return rest.to_owned();
        }
    }
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    "git printed no message".to_owned()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::diagnostic;

    /// Captured `git clone` of a path that never existed: five lines, the
    /// first two both starting `fatal: `.
    const NOT_A_REPOSITORY: &str = "\
        fatal: '/path/to/gone' does not appear to be a git repository\n\
        fatal: Could not read from remote repository.\n\
        \n\
        Please make sure you have the correct access rights\n\
        and the repository exists.\n";

    /// Captured `GIT_TEST_ASSUME_DIFFERENT_OWNER=1 git rev-parse`, path
    /// scrubbed to `<repository>`.
    const DUBIOUS_OWNERSHIP: &str = "\
        fatal: detected dubious ownership in repository at '<repository>'\n\
        To add an exception for this directory, call:\n\
        \n\
        \tgit config --global --add safe.directory <repository>\n";

    /// Captured a required clean filter that itself fails: an `error:` line
    /// from the filter, then git's own `fatal:` line naming it.
    const FILTER_ERROR_THEN_FATAL: &str =
        "error: external filter 'false' failed 1\nfatal: f.txt: clean filter 'up' failed\n";

    /// Captured `git ls-remote https://nonexistent.invalid/x/` (the
    /// shape `behind`'s git remote queries meet): a single `fatal:` line,
    /// with no generic boilerplate tail to prefer it over.
    const UNREACHABLE_HTTPS: &str = "fatal: unable to access 'https://nonexistent.invalid/x/': \
                                      Could not resolve host: nonexistent.invalid\n";

    /// `NOT_A_REPOSITORY` and `DUBIOUS_OWNERSHIP` are written as
    /// line-continued string constants to stay within 100 columns; this pins
    /// that doing so kept every captured byte, line by line, so the tests
    /// below still read the capture and not an approximation of it.
    #[test]
    fn the_multi_line_fixtures_hold_exactly_the_captured_bytes() {
        assert_eq!(
            NOT_A_REPOSITORY.split_inclusive('\n').collect::<Vec<_>>(),
            [
                "fatal: '/path/to/gone' does not appear to be a git repository\n",
                "fatal: Could not read from remote repository.\n",
                "\n",
                "Please make sure you have the correct access rights\n",
                "and the repository exists.\n",
            ]
        );
        assert_eq!(
            DUBIOUS_OWNERSHIP.split_inclusive('\n').collect::<Vec<_>>(),
            [
                "fatal: detected dubious ownership in repository at '<repository>'\n",
                "To add an exception for this directory, call:\n",
                "\n",
                "\tgit config --global --add safe.directory <repository>\n",
            ]
        );
    }

    #[test]
    fn the_first_fatal_line_is_read_with_its_prefix_stripped() {
        assert_eq!(
            diagnostic(NOT_A_REPOSITORY.as_bytes()),
            "'/path/to/gone' does not appear to be a git repository"
        );
    }

    #[test]
    fn the_dubious_ownership_fixture_reads_its_own_fatal_line() {
        assert_eq!(
            diagnostic(DUBIOUS_OWNERSHIP.as_bytes()),
            "detected dubious ownership in repository at '<repository>'"
        );
    }

    #[test]
    fn a_single_line_https_unreachable_fatal_is_read_whole() {
        assert_eq!(
            diagnostic(UNREACHABLE_HTTPS.as_bytes()),
            "unable to access 'https://nonexistent.invalid/x/': Could not resolve host: \
             nonexistent.invalid"
        );
    }

    #[test]
    fn a_fatal_line_after_error_lines_is_still_found() {
        assert_eq!(
            diagnostic(FILTER_ERROR_THEN_FATAL.as_bytes()),
            "f.txt: clean filter 'up' failed"
        );
    }

    #[test]
    fn only_error_lines_falls_back_to_the_first_one_whole() {
        assert_eq!(
            diagnostic(b"error: external filter 'false' failed 1\n"),
            "error: external filter 'false' failed 1"
        );
    }

    #[test]
    fn empty_stderr_reads_as_no_message() {
        assert_eq!(diagnostic(b""), "git printed no message");
    }

    #[test]
    fn blank_lines_alone_read_as_no_message() {
        assert_eq!(diagnostic(b"\n\n   \n"), "git printed no message");
    }

    proptest! {
        /// However git's stderr is shaped — truncated, adversarial, or not
        /// even from git at all — `diagnostic` never panics and never
        /// returns a string holding a newline.
        #[test]
        fn never_panics_and_never_returns_a_newline(
            bytes in proptest::collection::vec(any::<u8>(), 0..200)
        ) {
            let result = diagnostic(&bytes);
            assert!(!result.contains('\n'));
        }
    }
}
