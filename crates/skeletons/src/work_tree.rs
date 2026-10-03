//! Confirming a command that writes may ask git about the repository at
//! all: no repository-redirecting variable is set, and `root` genuinely sits
//! inside a non-bare git work tree — the one thing every other question the
//! writing commands ask git (`clean`, and `sync`'s own `proof`) is asked
//! through.
//!
//! Both `sync` and `wear` ask these questions and refuse on the answers, each
//! in its own words: [`writing_command::WritingCommand`] says which one is asking, and the
//! messages in [`message`] take it.

pub(crate) mod abort;
pub(crate) mod clean;
pub(crate) mod index_entry;
pub(crate) mod index_records;
pub(crate) mod message;
pub(crate) mod writing_command;

use std::path::{Path, PathBuf};
use std::process::Command;

use rituals::Failure;

use crate::git::{self, Locale, RepositoryPrefix};
use crate::subprocess::{Finished, SubprocessError, Truncated};

use abort::{GitQuestion, WorkTreeAbort};
use clean::{CleanWorkTree, Cleanliness};
use message::{abort_message, dirty_summary, report_dirty};
use writing_command::WritingCommand;

/// A git work tree a writing command may ask about: no redirecting variable
/// is set, and git itself confirmed `root` sits inside a non-bare work tree.
/// Only [`open`] constructs one, so every other function that takes a
/// `&WorkTree` — in this module's siblings and in `sync`'s `proof` — can trust
/// both properties already hold.
#[derive(Debug)]
pub(crate) struct WorkTree {
    root: PathBuf,
    prefix: RepositoryPrefix,
}

impl WorkTree {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) const fn prefix(&self) -> &RepositoryPrefix {
        &self.prefix
    }

    /// A git [`Command`] already pointed at this work tree's own root
    /// (`current_dir`), built by [`git::command`]: every pathspec it is given
    /// is read literally. [`Self::git_for_pathspec_magic`] is the variant for
    /// a query that needs pathspec magic.
    pub(crate) fn git(&self, locale: Locale) -> Command {
        let mut command = git::command(locale);
        command.current_dir(&self.root);
        command
    }

    /// [`Self::git`] for a query that needs pathspec magic
    /// ([`git::command_for_pathspec_magic`]): the same command, pointed at
    /// this work tree's own root in the same place, without the global
    /// `--literal-pathspecs`.
    pub(crate) fn git_for_pathspec_magic(&self, locale: Locale) -> Command {
        let mut command = git::command_for_pathspec_magic(locale);
        command.current_dir(&self.root);
        command
    }
}

/// Confirms `root` may be asked about at all: none of the variables in
/// `REDIRECTING_VARIABLES` (`git/command.rs`) is set (checked via `is_set`, so a test
/// can hand this any lookup it likes; production passes
/// `|name| std::env::var_os(name).is_some()`), and git itself reports `root`
/// sits inside a non-bare work tree.
pub(crate) fn open(root: &Path, is_set: impl Fn(&str) -> bool) -> Result<WorkTree, WorkTreeAbort> {
    let redirecting = git::redirecting_variables_set(is_set);
    if !redirecting.is_empty() {
        return Err(WorkTreeAbort::RedirectedGit {
            variables: redirecting,
        });
    }

    let mut command = git::command(Locale::Fixed);
    command
        .current_dir(root)
        .args(["rev-parse", "--is-inside-work-tree", "--show-prefix"]);
    let finished = run_local(command, GitQuestion::WorkTree)?;

    let prefix = classify_rev_parse(
        finished.success(),
        finished.stdout(),
        finished.stderr_head(),
    )?;
    Ok(WorkTree {
        root: root.to_path_buf(),
        prefix,
    })
}

/// Opens the work tree `root` sits in and confirms the whole of it is clean,
/// for `command` to write into, and hands back both the work tree and the
/// witness that it was clean.
///
/// This is the one place the two questions are asked together and the one
/// place their refusals are worded: every git question that could not be asked
/// is the abort's message, and a tree with anything uncommitted in it has each
/// dirty path reported on its own line before the failure that counts them.
/// Both are in `command`'s own words.
pub(crate) fn open_clean(
    root: &Path,
    command: WritingCommand,
) -> Result<(WorkTree, CleanWorkTree), Failure> {
    let aborted = |abort| Failure::new(abort_message(&abort, root, command));
    let opened = open(root, |name| std::env::var_os(name).is_some()).map_err(aborted)?;
    match clean::check_clean(&opened).map_err(aborted)? {
        Cleanliness::Clean(clean) => Ok((opened, clean)),
        Cleanliness::Dirty(dirty) => {
            report_dirty(&dirty);
            Err(Failure::new(dirty_summary(dirty.len(), command)))
        }
    }
}

/// The pure classifier behind [`open`]'s own reading of
/// `rev-parse --is-inside-work-tree --show-prefix`'s answer — split out so a
/// unit test can drive every captured shape directly, with no process to
/// spawn.
fn classify_rev_parse(
    exit_ok: bool,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<RepositoryPrefix, WorkTreeAbort> {
    if !exit_ok {
        let diagnostic = git::diagnostic(stderr);
        if diagnostic.starts_with("detected dubious ownership in repository at ") {
            return Err(WorkTreeAbort::DubiousOwnership { diagnostic });
        }
        if diagnostic.starts_with("not a git repository") {
            return Err(WorkTreeAbort::NotAWorkTree);
        }
        return Err(WorkTreeAbort::GitFailed {
            command: "rev-parse",
            diagnostic,
        });
    }

    let stdout = stdout.map_err(|_truncated| WorkTreeAbort::GitOutputTooLarge {
        command: "rev-parse",
    })?;
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines();

    if lines.next() != Some("true") {
        return Err(WorkTreeAbort::NotAWorkTree);
    }

    let Some(prefix_line) = lines.next() else {
        return Err(WorkTreeAbort::GitFailed {
            command: "rev-parse",
            diagnostic: "printed a prefix `skeletons` cannot read: ".to_owned(),
        });
    };
    RepositoryPrefix::parse(prefix_line).ok_or_else(|| WorkTreeAbort::GitFailed {
        command: "rev-parse",
        diagnostic: format!("printed a prefix `skeletons` cannot read: {prefix_line}"),
    })
}

/// Runs `command`, which is answering `question`, under the shared local
/// bounds ([`git::run_local`]): a command killed for running past them is
/// [`WorkTreeAbort::GitTimedOut`], naming the question, and a failure to run it at
/// all is [`WorkTreeAbort::GitUnavailable`] — the one way this module, its
/// `clean`, and `sync`'s `proof` run a bounded, local git process and report
/// that failure.
pub(crate) fn run_local(
    command: Command,
    question: GitQuestion,
) -> Result<Finished, WorkTreeAbort> {
    git::run_local(command).map_err(|error| abort_for(&error, question))
}

/// What a command that could not produce a [`Finished`] means for the
/// `question` it was answering: killed for running past its bound is a
/// timeout that names the question, and anything else is that git could not
/// be run.
fn abort_for(error: &SubprocessError, question: GitQuestion) -> WorkTreeAbort {
    if error.is_timed_out() {
        WorkTreeAbort::GitTimedOut { question }
    } else {
        WorkTreeAbort::GitUnavailable {
            detail: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use crate::claim::ClaimPath;
    use crate::subprocess::clock::Clock;
    use crate::subprocess::{Limits, TestClock, Truncated, run_with_clock};
    use crate::work_tree::abort::GitQuestion;

    use super::{RepositoryPrefix, WorkTreeAbort, abort_for, classify_rev_parse};

    #[test]
    fn a_command_killed_for_running_too_long_is_a_timeout_naming_the_question() {
        // A real command past a bound measured on a test clock, so the
        // error is the one `subprocess::run_with_clock` produces and not a
        // stand-in for it. The test clock must have advanced by the timeout
        // (the bound was measured on the injected clock), and the call must
        // return in far less real time than the child would have run (the
        // child was killed, not waited out); that real-time bound only fails
        // when the kill is missing, and a passing run never waits on it.
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 60"]);
        let timeout = Duration::from_millis(100);
        let limits = Limits {
            timeout: Some(timeout),
            stdout_bytes_max: u64::MAX,
            stderr_bytes_max: u64::MAX,
        };
        // Half the child's `sleep 60`: a call that returns sooner cannot have
        // waited the child out, and the margin is wide enough for a loaded
        // machine.
        let real_elapsed_max = Duration::from_secs(30);
        let clock = TestClock::new();
        let deadline = clock.now() + timeout;
        let real = std::time::Instant::now();
        let error = run_with_clock(command, &limits, &clock)
            .expect_err("a command past its timeout is killed");
        assert!(
            real.elapsed() < real_elapsed_max,
            "the child was waited out instead of killed"
        );
        // The last pause is cut to what is left of the timeout, so the pauses
        // before the deadline add up to exactly `timeout` on the injected
        // clock. Exact on both sides, so a deadline read from the wall clock
        // while the waits stay injected fails as surely as one measured on
        // the wall clock throughout.
        let measured: Duration = clock.waits_begun_before(deadline).iter().sum();
        assert_eq!(
            measured, timeout,
            "the timeout was not measured on the injected clock"
        );
        let question = GitQuestion::Checkout(
            ClaimPath::from_rendering_path("plain.yml").expect("a well-formed test path"),
        );

        let abort = abort_for(&error, question.clone());

        let WorkTreeAbort::GitTimedOut { question: named } = abort else {
            panic!("expected GitTimedOut, got {abort:?}")
        };
        assert_eq!(named, question);
    }

    #[test]
    fn a_command_that_could_not_be_run_is_git_unavailable_not_a_timeout() {
        let limits = Limits {
            timeout: Some(Duration::from_secs(5)),
            stdout_bytes_max: u64::MAX,
            stderr_bytes_max: u64::MAX,
        };
        let error = run_with_clock(
            Command::new("skeletons-check-nonexistent-program-xyz"),
            &limits,
            &TestClock::new(),
        )
        .expect_err("a missing program fails to spawn");

        let abort = abort_for(&error, GitQuestion::Status);

        assert!(
            matches!(abort, WorkTreeAbort::GitUnavailable { .. }),
            "expected GitUnavailable, got {abort:?}"
        );
    }

    const NOT_A_WORK_TREE: &[u8] =
        b"fatal: not a git repository (or any of the parent directories): .git\n";
    const DUBIOUS_OWNERSHIP: &[u8] = b"\
        fatal: detected dubious ownership in repository at '<repository>'\n\
        To add an exception for this directory, call:\n\
        \n\
        \tgit config --global --add safe.directory <repository>\n";

    /// The captured fixture is written as a line-continued byte string to
    /// stay within 100 columns; this pins that doing so kept every captured
    /// byte, line by line.
    #[test]
    fn the_captured_dubious_ownership_fixture_holds_exactly_the_captured_bytes() {
        assert_eq!(
            DUBIOUS_OWNERSHIP
                .split_inclusive(|&byte| byte == b'\n')
                .collect::<Vec<_>>(),
            [
                &b"fatal: detected dubious ownership in repository at '<repository>'\n"[..],
                b"To add an exception for this directory, call:\n",
                b"\n",
                b"\tgit config --global --add safe.directory <repository>\n",
            ]
        );
    }

    #[test]
    fn a_successful_run_at_the_top_level_reads_the_empty_prefix() {
        let prefix = classify_rev_parse(true, Ok(b"true\n\n"), b"")
            .expect("a well-formed answer must parse");
        assert_eq!(prefix, RepositoryPrefix::parse("").expect("empty prefix"));
    }

    #[test]
    fn a_successful_run_in_a_subdirectory_reads_its_own_prefix() {
        let prefix = classify_rev_parse(true, Ok(b"true\nws/\n"), b"")
            .expect("a well-formed answer must parse");
        assert_eq!(prefix, RepositoryPrefix::parse("ws/").expect("ws/ prefix"));
    }

    #[test]
    fn a_bare_repository_reads_as_not_a_work_tree() {
        let error = classify_rev_parse(true, Ok(b"false\n\n"), b"")
            .expect_err("a bare repository must be refused");
        assert!(matches!(error, WorkTreeAbort::NotAWorkTree));
    }

    #[test]
    fn exit_failure_naming_not_a_git_repository_reads_as_not_a_work_tree() {
        let error = classify_rev_parse(false, Ok(b""), NOT_A_WORK_TREE)
            .expect_err("must be refused as not a work tree");
        assert!(matches!(error, WorkTreeAbort::NotAWorkTree));
    }

    #[test]
    fn exit_failure_naming_dubious_ownership_is_read_and_the_diagnostic_kept() {
        let error = classify_rev_parse(false, Ok(b""), DUBIOUS_OWNERSHIP)
            .expect_err("must be refused as dubious ownership");
        let WorkTreeAbort::DubiousOwnership { diagnostic } = error else {
            panic!("expected DubiousOwnership, got {error:?}")
        };
        assert_eq!(
            diagnostic,
            "detected dubious ownership in repository at '<repository>'"
        );
    }

    #[test]
    fn an_unrecognised_exit_failure_reads_as_git_failed_naming_rev_parse() {
        let error = classify_rev_parse(false, Ok(b""), b"fatal: something else entirely\n")
            .expect_err("must be refused");
        let WorkTreeAbort::GitFailed {
            command,
            diagnostic,
        } = error
        else {
            panic!("expected GitFailed, got {error:?}")
        };
        assert_eq!(command, "rev-parse");
        assert_eq!(diagnostic, "something else entirely");
    }

    #[test]
    fn truncated_stdout_reads_as_git_output_too_large() {
        let error = classify_rev_parse(true, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("truncated stdout must be refused");
        assert!(matches!(
            error,
            WorkTreeAbort::GitOutputTooLarge {
                command: "rev-parse"
            }
        ));
    }

    #[test]
    fn a_prefix_line_that_fails_to_parse_reads_as_git_failed_naming_it() {
        let error = classify_rev_parse(true, Ok(b"true\n/leading-slash/\n"), b"")
            .expect_err("an unparseable prefix must be refused");
        let WorkTreeAbort::GitFailed {
            command,
            diagnostic,
        } = error
        else {
            panic!("expected GitFailed, got {error:?}")
        };
        assert_eq!(command, "rev-parse");
        assert_eq!(
            diagnostic,
            "printed a prefix `skeletons` cannot read: /leading-slash/"
        );
    }

    #[test]
    fn a_missing_prefix_line_entirely_reads_as_git_failed() {
        let error = classify_rev_parse(true, Ok(b"true"), b"")
            .expect_err("an absent prefix line must be refused");
        assert!(matches!(
            error,
            WorkTreeAbort::GitFailed {
                command: "rev-parse",
                ..
            }
        ));
    }
}
