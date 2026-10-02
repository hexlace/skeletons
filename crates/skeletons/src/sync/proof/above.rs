//! The question a claim's own index entry cannot answer: does git's index
//! track something at a directory *above* the claim?
//!
//! A claim `a/b.yml` needs `a` to be a directory. If the index tracks `a` as a
//! file, a symbolic link or a submodule's gitlink, hidden from the work tree
//! by skip-worktree or a sparse checkout (or, for a gitlink, simply not
//! checked out), `sync` creating `a/` and `a/b.yml` makes a tree git reads as
//! `a` deleted and `a/b.yml` untracked, and `git checkout -- a` puts the file
//! back over the bone. A question about entries *at or beneath the claim*
//! never sees it, since `a` is above.
//!
//! The answer is asked of git, with git's own case fold, for exactly the
//! directory's own path and nothing beneath it
//! ([`crate::git::pathspec_exactly_ignoring_case`]). Any record refuses every
//! write under that directory. A gitlink reads as a submodule: another
//! repository, which the walk refuses on disk once it is checked out
//! (`InsideAnotherRepository`) and this refuses while it is not.

use std::collections::{BTreeMap, BTreeSet};

use crate::claim::ClaimPath;
use crate::git::{self, Locale};

use super::{AboveEntry, Why, run_ls_files};
use crate::work_tree::WorkTree;
use crate::work_tree::abort::{GitQuestion, WorkTreeAbort};

/// For each of `writes`, the refusal that follows from an index entry at a
/// directory above it, or `None` when git tracks nothing at any of them.
///
/// One `ls-files` runs per *distinct* directory above any write, so two
/// writes under `.github` share one question, and an exclusion cannot leak
/// from one directory's question into another's (it applies to a whole
/// command). Every question runs before any answer is used, so a git that
/// cannot be run, or runs past its bound, aborts the whole proof rather than
/// refusing some writes and not others; a git that runs and answers badly
/// refuses only the writes under that directory.
/// The number of commands is at most the sum of the writes' depths.
///
/// A write under several refused directories carries the topmost one, since
/// that is the first place the path stops being a directory.
pub(super) fn tracked_above(
    work_tree: &WorkTree,
    writes: &[&ClaimPath],
) -> Result<Vec<Option<Why>>, WorkTreeAbort> {
    let directories: BTreeSet<ClaimPath> =
        writes.iter().flat_map(|claim| claim.ancestors()).collect();

    let mut answers: BTreeMap<ClaimPath, Why> = BTreeMap::new();
    for directory in directories {
        if let Some(why) = ask_about(work_tree, &directory)? {
            answers.insert(directory, why);
        }
    }

    Ok(writes
        .iter()
        .map(|claim| {
            claim
                .ancestors()
                .iter()
                .find_map(|directory| answers.get(directory).cloned())
        })
        .collect())
}

/// Asks git what its index tracks at exactly `directory`, and turns the first
/// record into the refusal it means: a file or link where a directory is
/// needed, a submodule, or a mode this crate does not know. A git failure or
/// an unreadable answer is the refusal itself, so it reaches every write
/// under `directory`.
fn ask_about(work_tree: &WorkTree, directory: &ClaimPath) -> Result<Option<Why>, WorkTreeAbort> {
    let mut ls_files = work_tree.git_for_pathspec_magic(Locale::Fixed);
    ls_files.args(["ls-files", "-v", "--stage", "-z", "--"]);
    ls_files.args(git::pathspec_exactly_ignoring_case(directory));
    Ok(
        match run_ls_files(ls_files, GitQuestion::IndexAbove(directory.clone()))? {
            Err(why) => Some(why),
            Ok(records) => records.first().map(|record| Why::TrackedAbove {
                git_path: String::from_utf8_lossy(&record.path).into_owned(),
                entry: AboveEntry::from_mode(&record.mode),
            }),
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::claim::ClaimPath;
    use crate::sync::test_repository::TestRepository;

    use super::super::{AboveEntry, Why};
    use super::tracked_above;

    fn claim(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// `tracked_above` over `paths`, against `repository`.
    fn asked(repository: &TestRepository, paths: &[&str]) -> Vec<Option<Why>> {
        let claims: Vec<ClaimPath> = paths.iter().map(|path| claim(path)).collect();
        let refs: Vec<&ClaimPath> = claims.iter().collect();
        tracked_above(&repository.work_tree(), &refs).expect("git must answer")
    }

    /// Commits `path` as a file and hides it from the work tree the way a
    /// sparse checkout does: skip-worktree, and removed from disk.
    fn commit_hidden(repository: &TestRepository, path: &str) {
        repository.write(path, b"tracked\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", path])
            .run_ok();
        std::fs::remove_file(repository.path().join(path)).expect("remove the file");
    }

    fn tracked_above_entry(answer: Option<&Why>) -> (&str, &AboveEntry) {
        let Some(Why::TrackedAbove { git_path, entry }) = answer else {
            panic!("expected TrackedAbove, got {answer:?}")
        };
        (git_path, entry)
    }

    #[test]
    fn a_file_the_index_tracks_at_a_directory_above_the_claim_refuses_it() {
        // The scenario: `a` is tracked, hidden and absent, and the claim
        // is `a/b.yml`. A question about entries at or beneath the claim
        // finds nothing; this one asks at `a` itself.
        let repository = TestRepository::new();
        commit_hidden(&repository, "a");

        let answers = asked(&repository, &["a/b.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("a", &AboveEntry::File)
        );
    }

    #[test]
    fn a_symbolic_link_at_a_directory_above_is_named_as_one() {
        let repository = TestRepository::new();
        std::os::unix::fs::symlink("elsewhere", repository.path().join("a")).expect("a link");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "a"])
            .run_ok();
        std::fs::remove_file(repository.path().join("a")).expect("remove the link");

        let answers = asked(&repository, &["a/b.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("a", &AboveEntry::SymbolicLink)
        );
    }

    #[test]
    fn a_gitlink_at_a_directory_above_is_a_submodule_even_when_nothing_is_checked_out() {
        // An uninitialised submodule leaves an empty directory and no `.git`
        // for the claim walk to find, so the index is the only witness.
        let repository = TestRepository::new();
        repository.write(".gitkeep", b"");
        repository.commit_all("initial");
        let object = repository.git(&["rev-parse", "HEAD"]).output_ok();
        repository
            .git(&[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{object},sub"),
            ])
            .run_ok();

        let answers = asked(&repository, &["sub/x.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("sub", &AboveEntry::Submodule)
        );
    }

    #[test]
    fn an_entry_of_another_case_at_a_directory_above_is_named_as_git_spells_it() {
        // Git's own fold: `A` and `a` are one name to it under
        // `core.ignorecase`, and the answer names the entry as the index has
        // it.
        let repository = TestRepository::new();
        commit_hidden(&repository, "A");

        let answers = asked(&repository, &["a/b.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("A", &AboveEntry::File)
        );
    }

    #[test]
    fn a_directory_git_tracks_files_beneath_is_not_an_entry_above_the_claim() {
        // The exclusion in the pathspec pair is what makes this `None`:
        // `d` matched alone would match every file under it. The control is
        // the file case above, with the same claim shape.
        let repository = TestRepository::new();
        repository.write("d/tracked.yml", b"tracked\n");
        repository.commit_all("initial");

        let answers = asked(&repository, &["d/new.yml"]);

        assert!(answers[0].is_none(), "got {:?}", answers[0]);
    }

    #[test]
    fn a_name_that_only_starts_like_the_directory_is_not_it() {
        // `d` is a hidden file; `dx/f.yml` is under a different directory.
        let repository = TestRepository::new();
        commit_hidden(&repository, "d");

        let answers = asked(&repository, &["dx/f.yml", "d/f.yml"]);

        assert!(answers[0].is_none(), "dx is not d: {:?}", answers[0]);
        assert_eq!(
            tracked_above_entry(answers[1].as_ref()),
            ("d", &AboveEntry::File)
        );
    }

    #[test]
    fn a_glob_character_in_a_directory_stays_a_literal_character() {
        // `*` and `[a]` would match the hidden file `a` as a glob.
        let repository = TestRepository::new();
        commit_hidden(&repository, "a");

        let answers = asked(&repository, &["*/x.yml", "[a]/x.yml", "?/x.yml"]);

        for answer in &answers {
            assert!(answer.is_none(), "not a glob, got {answer:?}");
        }
    }

    #[test]
    fn tracked_files_beneath_directories_named_like_globs_are_not_entries_above() {
        // The exclusion is what leaves only an entry at the directory's own
        // path, and it is a glob: unescaped, `[ab]/**` would match beneath
        // `a` and `b` and nothing beneath the directory `[ab]`, so the
        // tracked file under it would be reported as an entry above the
        // claim. Real git, real tracked files, one directory for each
        // character a glob reads: none is an entry above.
        let repository = TestRepository::new();
        repository.write("[ab]/f.yml", b"tracked\n");
        repository.write("a?/f.yml", b"tracked\n");
        repository.write("x\\y/f.yml", b"tracked\n");
        repository.commit_all("initial");

        let answers = asked(&repository, &["[ab]/x.yml", "a?/x.yml", "x\\y/x.yml"]);

        for answer in &answers {
            assert!(
                answer.is_none(),
                "a file beneath a directory is not an entry at it, got {answer:?}"
            );
        }
    }

    #[test]
    fn a_claim_at_the_root_has_no_directory_above_it() {
        let repository = TestRepository::new();
        commit_hidden(&repository, "a");

        let answers = asked(&repository, &["b.yml"]);

        assert!(answers[0].is_none());
    }

    #[test]
    fn the_entry_is_found_at_any_depth_and_only_at_its_own() {
        // `a` is a directory git tracks files under, and `a/b` is a hidden
        // file: the claim `a/b/c/x.yml` needs `a/b` to be a directory, and
        // `a` is one. Each ancestor is asked about exactly.
        let repository = TestRepository::new();
        repository.write("a/keep.yml", b"kept\n");
        commit_hidden(&repository, "a/b");

        let answers = asked(&repository, &["a/b/c/x.yml", "a/other/x.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("a/b", &AboveEntry::File)
        );
        assert!(answers[1].is_none(), "got {:?}", answers[1]);
    }

    #[test]
    fn the_topmost_refused_directory_is_the_one_named() {
        // `A` (a file, another case of `a`) is tracked, and so is `a/b` (a
        // file); the claim `a/b/c.yml` is under both. The first place the
        // path stops being a directory is `a`, which git spells `A`.
        let repository = TestRepository::new();
        repository.write("a/b", b"beneath\n");
        repository.commit_all("initial");
        let object = repository.git(&["hash-object", "a/b"]).output_ok();
        repository
            .git(&[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{object},A"),
            ])
            .run_ok();

        let answers = asked(&repository, &["a/b/c.yml"]);

        assert_eq!(
            tracked_above_entry(answers[0].as_ref()),
            ("A", &AboveEntry::File)
        );
    }

    #[test]
    fn writes_sharing_a_directory_are_each_answered_in_their_own_order() {
        let repository = TestRepository::new();
        commit_hidden(&repository, "a");

        let answers = asked(&repository, &["z.yml", "a/x.yml", "b/y.yml", "a/y.yml"]);

        assert_eq!(answers.len(), 4);
        assert!(answers[0].is_none());
        assert_eq!(
            tracked_above_entry(answers[1].as_ref()),
            ("a", &AboveEntry::File)
        );
        assert!(answers[2].is_none());
        assert_eq!(
            tracked_above_entry(answers[3].as_ref()),
            ("a", &AboveEntry::File)
        );
    }

    #[test]
    fn no_writes_ask_nothing_and_answer_nothing() {
        let repository = TestRepository::new();
        assert!(asked(&repository, &[]).is_empty());
    }
}
