//! Whether git can hand back the two files `wear` changes.
//!
//! `wear` changes the command line crate's manifest and `Cargo.lock` in place
//! and tells the wearer to commit both, so each has to be a file git reads
//! from the work tree: a clean tree says nothing about a file git is told not
//! to look at, so a hidden manifest with a local edit reads as clean, and
//! what `wear` wrote there would never be committed. `sync` closes the same
//! gap for every path it writes, with a per-path proof; this asks the same
//! question of git's index, through the same code, for these two files.
//!
//! The manifest has to be tracked. A `Cargo.lock` git does not track is
//! allowed: git never held it, Cargo regenerates it, and refusing it would
//! shut out every project that ignores its lockfile.

use rituals::Failure;

use super::{CommandLineCrate, WRITING};
use crate::skeleton::Escaped;
use crate::work_tree::WorkTree;
use crate::work_tree::abort::WorkTreeAbort;
use crate::work_tree::clean::CleanWorkTree;
use crate::work_tree::index_entry::{HiddenFlag, IndexRecord, IndexTag};
use crate::work_tree::index_records::{self, UnusableIndexAnswer};
use crate::work_tree::message::{abort_message, hidden_from_work_tree_line};

/// One of the two files `wear` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum File {
    Manifest,
    Lockfile,
}

/// Why git could not hand a file back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GitCannotHandBack {
    /// The index holds the file under a flag that makes git not read it from
    /// the work tree.
    Hidden(HiddenFlag),
    /// Git does not track the file, and the file has to be tracked.
    Untracked,
    /// The index's answer is not one record for exactly this path, so what
    /// git holds for it cannot be said. `detail` is one line.
    Unreadable { detail: String },
}

/// Refuses, before anything is written, a manifest or a tracked `Cargo.lock`
/// that git cannot hand back.
///
/// `clean` is never read: its presence is what says the work tree was found
/// clean first, which is why an untracked `Cargo.lock` that is not ignored
/// never reaches here (git status lists it, and the clean check refuses it as
/// an uncommitted change). What reaches here as untracked is a file git
/// ignores, and for a lockfile that is allowed.
///
/// Each path is asked of git as `wear` shows it, relative to the workspace
/// root the work tree was opened at.
pub(super) fn check(
    work_tree: &WorkTree,
    _clean: &CleanWorkTree,
    command_line_crate: &CommandLineCrate,
) -> Result<(), Failure> {
    for (file, shown) in [
        (File::Manifest, &command_line_crate.manifest_shown),
        (File::Lockfile, &command_line_crate.lockfile_shown),
    ] {
        let aborted = |abort| Failure::new(abort_message(&abort, work_tree.root(), WRITING));
        let records = index_records::records_for(work_tree, shown)
            .map_err(aborted)?
            .map_err(|unusable| aborted(abort_for(unusable)))?;
        classify(file, shown, &records).map_err(|cannot| Failure::new(line(shown, &cannot)))?;
    }
    Ok(())
}

/// The abort an `ls-files` answer that cannot be used amounts to.
fn abort_for(unusable: UnusableIndexAnswer) -> WorkTreeAbort {
    match unusable {
        UnusableIndexAnswer::Failed { detail } => WorkTreeAbort::GitFailed {
            command: "ls-files",
            diagnostic: detail,
        },
        UnusableIndexAnswer::TooLarge => WorkTreeAbort::GitOutputTooLarge {
            command: "ls-files",
        },
    }
}

/// Reads what git's index holds for `path` (`records`, every record it listed
/// at or under it) as whether git can hand `file` back.
///
/// A file is fine when its one record is at stage 0, spelled exactly as asked
/// and tagged `H`. No record is fine for a lockfile and a refusal for a
/// manifest. Anything that is not one such record is refused as unreadable
/// and not guessed at: the clean check has found no conflict, but this reads
/// what git says rather than what that implies.
fn classify(file: File, path: &str, records: &[IndexRecord]) -> Result<(), GitCannotHandBack> {
    assert!(!path.is_empty(), "a file `wear` changes has a path");
    let [record] = records else {
        return match (records.len(), file) {
            (0, File::Lockfile) => Ok(()),
            (0, File::Manifest) => Err(GitCannotHandBack::Untracked),
            (listed, File::Manifest | File::Lockfile) => Err(GitCannotHandBack::Unreadable {
                detail: format!("git listed {listed} index entries for it"),
            }),
        };
    };
    if record.path != path.as_bytes() {
        return Err(GitCannotHandBack::Unreadable {
            detail: format!("git lists it as {}", String::from_utf8_lossy(&record.path)),
        });
    }
    if record.stage != 0 {
        return Err(GitCannotHandBack::Unreadable {
            detail: "git lists it as conflicted".to_owned(),
        });
    }
    if let Some(flag) = record.tag.hiding_flag() {
        return Err(GitCannotHandBack::Hidden(flag));
    }
    match record.tag {
        IndexTag::Tracked => Ok(()),
        IndexTag::Unmerged => Err(GitCannotHandBack::Unreadable {
            detail: "git lists it as conflicted".to_owned(),
        }),
        IndexTag::SkipWorktree
        | IndexTag::AssumeUnchanged
        | IndexTag::SkipWorktreeAndAssumeUnchanged => {
            unreachable!("hiding_flag named a flag for every tag that hides a file: {record:?}")
        }
    }
}

/// The message for a file git cannot hand back, as one line that ends in what
/// to do. An unreadable answer is worded as the abort it is.
fn line(shown: &str, cannot: &GitCannotHandBack) -> String {
    match cannot {
        GitCannotHandBack::Hidden(flag) => hidden_from_work_tree_line(shown, *flag, WRITING),
        GitCannotHandBack::Untracked => untracked_line(shown),
        GitCannotHandBack::Unreadable { detail } => {
            let detail = Escaped(detail);
            let shown = Escaped(shown);
            format!(
                "{shown}'s entry in git's index could not be read, so {} wrote nothing: {detail}",
                WRITING.name()
            )
        }
    }
}

/// `` {path} is not tracked by git, so wear wrote nothing: git could not give
/// back what wear changes in it; run `git add -- {path}` (`git add -f --
/// {path}` if a .gitignore rule matches it) and commit, then run the `wear`
/// task again ``.
fn untracked_line(shown: &str) -> String {
    let shown = Escaped(shown);
    format!(
        "{shown} is not tracked by git, so {} wrote nothing: git could not give back what {} \
         changes in it; run `git add -- {shown}` (`git add -f -- {shown}` if a .gitignore rule \
         matches it) and commit, then {}",
        WRITING.name(),
        WRITING.name(),
        WRITING.run_again(),
    )
}

#[cfg(test)]
mod tests {
    use super::{File, GitCannotHandBack, classify, line};
    use crate::work_tree::index_entry::{HiddenFlag, IndexRecord, parse_ls_files_tagged};

    const OBJECT: &str = "7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a";

    /// The records `ls-files -v --stage -z` prints for `entries`, each a
    /// `(tag, stage, path)`.
    fn listed(entries: &[(&str, u8, &str)]) -> Vec<IndexRecord> {
        let mut bytes = Vec::new();
        for (tag, stage, path) in entries {
            bytes.extend_from_slice(format!("{tag} 100644 {OBJECT} {stage}\t{path}\0").as_bytes());
        }
        parse_ls_files_tagged(&bytes).expect("well-formed test records")
    }

    fn classified(file: File, path: &str, tag: &str) -> Result<(), GitCannotHandBack> {
        classify(file, path, &listed(&[(tag, 0, path)]))
    }

    #[test]
    fn a_tracked_file_git_reads_is_fine_for_the_manifest_and_the_lockfile() {
        assert_eq!(classified(File::Manifest, "Cargo.toml", "H"), Ok(()));
        assert_eq!(classified(File::Lockfile, "Cargo.lock", "H"), Ok(()));
    }

    #[test]
    fn a_file_flagged_so_git_does_not_read_it_is_refused_for_each_flag_and_each_file() {
        for (file, path) in [
            (File::Manifest, "Cargo.toml"),
            (File::Lockfile, "Cargo.lock"),
        ] {
            for (tag, flag) in [
                ("S", HiddenFlag::SkipWorktree),
                ("h", HiddenFlag::AssumeUnchanged),
                ("s", HiddenFlag::Both),
            ] {
                assert_eq!(
                    classified(file, path, tag),
                    Err(GitCannotHandBack::Hidden(flag)),
                    "{path} tagged {tag}"
                );
            }
        }
    }

    #[test]
    fn a_manifest_git_holds_nothing_for_is_refused_and_a_lockfile_is_allowed() {
        assert_eq!(
            classify(File::Manifest, "Cargo.toml", &[]),
            Err(GitCannotHandBack::Untracked)
        );
        assert_eq!(classify(File::Lockfile, "Cargo.lock", &[]), Ok(()));
    }

    #[test]
    fn an_answer_that_is_not_one_record_for_the_path_is_unreadable_for_either_file() {
        // The clean check has found no conflict, but the classifier reads what
        // git printed: a record at another stage, an unmerged tag, another
        // spelling, or several records, is never read as fine.
        for file in [File::Manifest, File::Lockfile] {
            let path = if file == File::Manifest {
                "Cargo.toml"
            } else {
                "Cargo.lock"
            };
            for records in [
                listed(&[("H", 2, path)]),
                listed(&[("M", 0, path)]),
                listed(&[("H", 0, "cargo.toml")]),
                listed(&[("H", 0, path), ("H", 0, path)]),
            ] {
                assert!(
                    matches!(
                        classify(file, path, &records),
                        Err(GitCannotHandBack::Unreadable { .. })
                    ),
                    "{file:?} {records:?}"
                );
            }
        }
    }

    #[test]
    fn the_untracked_manifest_line_names_the_file_says_nothing_was_written_and_ends_in_a_remedy() {
        assert_eq!(
            line("crates/cli/Cargo.toml", &GitCannotHandBack::Untracked),
            "crates/cli/Cargo.toml is not tracked by git, so wear wrote nothing: git could not \
             give back what wear changes in it; run `git add -- crates/cli/Cargo.toml` (`git add \
             -f -- crates/cli/Cargo.toml` if a .gitignore rule matches it) and commit, then run \
             the `wear` task again"
        );
    }

    #[test]
    fn the_hidden_manifest_line_is_the_shared_one_worded_for_wear() {
        assert_eq!(
            line(
                "Cargo.toml",
                &GitCannotHandBack::Hidden(HiddenFlag::SkipWorktree)
            ),
            "Cargo.toml is marked skip-worktree in git's index, so git does not read its bytes \
             from the work tree and would ignore what wear wrote there: run `git update-index \
             --no-skip-worktree -- Cargo.toml`, then run the `wear` task again"
        );
    }

    #[test]
    fn an_unreadable_entry_is_one_line_naming_the_file_and_what_git_said() {
        assert_eq!(
            line(
                "Cargo.lock",
                &GitCannotHandBack::Unreadable {
                    detail: "git listed 2 index entries for it".to_owned()
                }
            ),
            "Cargo.lock's entry in git's index could not be read, so wear wrote nothing: git \
             listed 2 index entries for it"
        );
    }
}
