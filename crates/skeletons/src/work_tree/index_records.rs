//! Asking git what its index holds for a path: `git ls-files -v --stage -z`,
//! run under the shared local bounds and read through
//! [`index_entry`](super::index_entry).
//!
//! `sync` asks it of every path it would write, and `wear` of the two files it
//! changes, so the command, its bounds and the reading of its answer live here
//! once. What a record then means for a command is that command's to say.

use std::process::Command;

use crate::git::{self, Locale};
use crate::subprocess::Truncated;

use super::abort::{GitQuestion, WorkTreeAbort};
use super::index_entry::{IndexRecord, parse_ls_files_tagged};
use super::{WorkTree, run_local};

/// Why an `ls-files` answer cannot be used, short of git itself not being
/// askable at all ([`WorkTreeAbort`]): the command ran and its answer is
/// either a failure or not one to read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnusableIndexAnswer {
    /// `ls-files` exited non-zero, or printed a record `skeletons` cannot
    /// read. `detail` is one line: git's own diagnostic, or the record.
    Failed { detail: String },
    /// `ls-files` printed more than the cap `skeletons` reads.
    TooLarge,
}

/// Asks git for every index record at or under `path`, spelled exactly as
/// given, relative to `work_tree`'s root: the pathspec is read literally.
///
/// `path` is shown as it is when the question times out, so it is the path a
/// message would name.
pub(crate) fn records_for(
    work_tree: &WorkTree,
    path: &str,
) -> Result<Result<Vec<IndexRecord>, UnusableIndexAnswer>, WorkTreeAbort> {
    let mut ls_files = work_tree.git(Locale::Fixed);
    ls_files.args(["ls-files", "-v", "--stage", "-z", "--", path]);
    run_ls_files(ls_files, GitQuestion::IndexEntry(path.to_owned()))
}

/// Runs a built `ls-files -v --stage -z` command under [`run_local`]'s bounds
/// and reads its records through [`classify_ls_files`]: the one place each
/// question about an index entry, at a path or above it, is run and read.
pub(crate) fn run_ls_files(
    ls_files: Command,
    question: GitQuestion,
) -> Result<Result<Vec<IndexRecord>, UnusableIndexAnswer>, WorkTreeAbort> {
    let finished = run_local(ls_files, question)?;
    Ok(classify_ls_files(
        finished.success(),
        finished.stdout(),
        finished.stderr_head(),
    ))
}

/// The pure classifier behind [`run_ls_files`]: `ls-files`'s exit,
/// stdout and stderr in, its records (or why they cannot be used) out —
/// split out so a unit test drives a truncated stream through the very code
/// production runs, with no process to spawn.
pub(crate) fn classify_ls_files(
    exit_ok: bool,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<Vec<IndexRecord>, UnusableIndexAnswer> {
    if !exit_ok {
        return Err(UnusableIndexAnswer::Failed {
            detail: git::diagnostic(stderr),
        });
    }
    let stdout = stdout.map_err(|_truncated| UnusableIndexAnswer::TooLarge)?;
    parse_ls_files_tagged(stdout).map_err(|error| UnusableIndexAnswer::Failed {
        detail: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use crate::subprocess::Truncated;
    use crate::sync::test_repository::TestRepository;

    use super::{UnusableIndexAnswer, classify_ls_files, records_for};
    use crate::work_tree::index_entry::IndexTag;

    const ONE_INDEX_RECORD: &[u8] = b"H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";

    #[test]
    fn truncated_ls_files_stdout_reads_as_too_large() {
        // Every ls-files answer, for a path on disk or not, is read through
        // `classify_ls_files`; this drives a stream past its cap through it.
        // The control hands it the same well-formed record uncut, which must
        // parse, so the refusal is the truncation, never the bytes.
        let refused = classify_ls_files(true, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("a truncated ls-files stream must be refused");
        assert_eq!(refused, UnusableIndexAnswer::TooLarge);

        let records = classify_ls_files(true, Ok(ONE_INDEX_RECORD), b"")
            .expect("the same record, uncut, must parse");
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn a_failed_ls_files_is_git_s_own_diagnostic() {
        let refused = classify_ls_files(false, Ok(b""), b"fatal: something else entirely\n")
            .expect_err("a failed ls-files must be refused");
        assert_eq!(
            refused,
            UnusableIndexAnswer::Failed {
                detail: "something else entirely".to_owned()
            }
        );
    }

    #[test]
    fn a_record_that_cannot_be_read_is_named_whole() {
        let refused = classify_ls_files(true, Ok(b"H 100644 not-an-oid 0\tf.txt\0"), b"")
            .expect_err("an unreadable record must be refused");
        assert_eq!(
            refused,
            UnusableIndexAnswer::Failed {
                detail: "git listed an index entry `skeletons` cannot read: H 100644 not-an-oid \
                         0\tf.txt"
                    .to_owned()
            }
        );
    }

    #[test]
    fn records_for_reads_the_path_literally_and_reports_what_git_holds() {
        // A tracked file is listed with the tag git reads it under; a path
        // git holds nothing for lists nothing; and a glob character in the
        // path stays a literal character, so it matches no other file.
        let repository = TestRepository::new();
        repository.write("f.txt", b"committed\n");
        repository.commit_all("initial");
        let work_tree = repository.work_tree();

        let tracked = records_for(&work_tree, "f.txt")
            .expect("git must answer")
            .expect("the answer must be usable");
        let absent = records_for(&work_tree, "g.txt")
            .expect("git must answer")
            .expect("the answer must be usable");
        let glob = records_for(&work_tree, "*.txt")
            .expect("git must answer")
            .expect("the answer must be usable");

        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].tag, IndexTag::Tracked);
        assert_eq!(tracked[0].path, b"f.txt");
        assert!(absent.is_empty(), "{absent:?}");
        assert!(glob.is_empty(), "{glob:?}");
    }
}
