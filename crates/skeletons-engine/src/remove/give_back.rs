//! Whether git could give a file back, which is what decides that `remove`
//! may delete it.
//!
//! A tree git reports clean is not enough. An untracked file that git's ignore
//! rules hide, a file the index flags so that git does not read it from the
//! work tree, and a file under a content filter that does not write back what
//! it was handed all report clean while holding bytes no commit has. So each
//! file in the work tree is put to the proof `sync` requires before it
//! overwrites one ([`prove_held`]): the file holds exactly what git would check
//! out for its index entry. A file that passes is one the run may delete; one
//! that does not is left where it is, and the reason is the one the success
//! line names.

use super::files::{Fate, LeftBecause};
use crate::claim::ClaimPath;
use crate::sync::{Why, prove_held};
use crate::work_tree::WorkTree;
use crate::work_tree::abort::WorkTreeAbort;
use crate::work_tree::clean::CleanWorkTree;

/// What becomes of the claimed file at `path`, which the survey found in the
/// work tree.
///
/// `_clean` is never read: holding it is what shows the whole work tree was
/// clean when the question was put, which the proof relies on, as `sync`'s
/// does. A file that is not a regular file on disk, or that has gone, is left,
/// because there is nothing git could give back.
///
/// # Errors
///
/// Returns [`WorkTreeAbort`] when git could not be asked.
pub(crate) fn fate(
    work_tree: &WorkTree,
    _clean: &CleanWorkTree,
    path: &ClaimPath,
) -> Result<Fate, WorkTreeAbort> {
    let target = path.to_path(work_tree.root());
    // A file that cannot even be examined is one nothing can be shown about, so
    // the error itself is no more to the wearer than the file being left.
    let is_regular_file =
        std::fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_file());
    if !is_regular_file {
        return Ok(Fate::Left(LeftBecause::CannotShowItHolds));
    }
    Ok(match prove_held(work_tree, path)? {
        Ok(_held) => Fate::Deleted,
        Err(why) => Fate::Left(left_because(&why)),
    })
}

/// The reason a file is left for each way the proof can refuse it.
///
/// Every refusal is placed by name: a reason the proof gains later has to be
/// given a place here before the crate builds.
const fn left_because(why: &Why) -> LeftBecause {
    match why {
        Why::NotInIndex => LeftBecause::NotTracked,
        Why::HiddenFromWorkTree { flag } => LeftBecause::Hidden(*flag),
        Why::NotWhatGitChecksOut | Why::CheckoutNotReproducible => LeftBecause::NotWhatGitChecksOut,
        Why::TrackedButAbsent { .. }
        | Why::TrackedAbove { .. }
        | Why::IgnoredByGit { .. }
        | Why::IgnoreCheckFailed { .. }
        | Why::ListedAs { .. }
        | Why::Conflicted
        | Why::SymbolicLinkInIndex
        | Why::SubmoduleInIndex
        | Why::UnexpectedMode { .. }
        | Why::IndexEntryUnreadable { .. }
        | Why::CheckoutFailed { .. }
        | Why::OutputTooLarge { .. }
        | Why::NotARegularFile
        | Why::Unreadable { .. }
        | Why::ChangedSinceSurvey => LeftBecause::CannotShowItHolds,
    }
}

#[cfg(test)]
mod tests {
    use super::{fate, left_because};
    use crate::claim::ClaimPath;
    use crate::remove::files::{Fate, LeftBecause};
    use crate::sync::Why;
    use crate::sync::test_repository::TestRepository;
    use crate::work_tree::clean::{CleanWorkTree, Cleanliness, check_clean};
    use crate::work_tree::index_entry::HiddenFlag;

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// The witness that `repository`'s work tree is clean, as the command
    /// asks for it before it deletes anything.
    fn clean_witness(repository: &TestRepository) -> CleanWorkTree {
        match check_clean(&repository.work_tree()).expect("status must run") {
            Cleanliness::Clean(clean) => clean,
            Cleanliness::Dirty(dirty) => {
                panic!("the fixture must be clean going in: {dirty:?}")
            }
        }
    }

    /// What `remove` would do with `path` in `repository`.
    fn fate_of(repository: &TestRepository, path: &str) -> Fate {
        let clean = clean_witness(repository);
        fate(&repository.work_tree(), &clean, &claim_path(path)).expect("git must be askable")
    }

    #[test]
    fn a_clean_tracked_file_is_deleted() {
        // The control for every case below: the same repository shape with
        // nothing to hide the file from git, so what leaves the others is the
        // reason each is about, never the setup.
        let repository = TestRepository::new();
        repository.write("ci/a.yml", b"committed\n");
        repository.commit_all("initial");

        assert_eq!(fate_of(&repository, "ci/a.yml"), Fate::Deleted);
    }

    #[test]
    fn an_ignored_untracked_file_is_left_as_not_tracked() {
        // Git reports a tree clean while an ignored file sits in it, and no
        // commit holds the file's bytes.
        let repository = TestRepository::new();
        repository.write(".gitignore", b"local/\n");
        repository.write("kept.yml", b"committed\n");
        repository.commit_all("initial");
        repository.write("local/notes.yml", b"only here\n");

        assert_eq!(
            fate_of(&repository, "local/notes.yml"),
            Fate::Left(LeftBecause::NotTracked)
        );
    }

    #[test]
    fn a_file_that_has_gone_is_left_as_one_git_cannot_show_it_holds() {
        let repository = TestRepository::new();
        repository.write("a.yml", b"committed\n");
        repository.commit_all("initial");
        let clean = clean_witness(&repository);
        std::fs::remove_file(repository.path().join("a.yml")).expect("remove the file");

        assert_eq!(
            fate(&repository.work_tree(), &clean, &claim_path("a.yml"))
                .expect("git must be askable"),
            Fate::Left(LeftBecause::CannotShowItHolds)
        );
    }

    #[test]
    fn a_symbolic_link_is_left_as_one_git_cannot_show_it_holds() {
        let repository = TestRepository::new();
        repository.write("target.yml", b"committed\n");
        std::os::unix::fs::symlink("target.yml", repository.path().join("link.yml"))
            .expect("create the link");
        repository.commit_all("initial");

        assert_eq!(
            fate_of(&repository, "link.yml"),
            Fate::Left(LeftBecause::CannotShowItHolds)
        );
    }

    #[test]
    fn a_directory_is_left_as_one_git_cannot_show_it_holds() {
        let repository = TestRepository::new();
        repository.write("dir/inner.yml", b"committed\n");
        repository.commit_all("initial");

        assert_eq!(
            fate_of(&repository, "dir"),
            Fate::Left(LeftBecause::CannotShowItHolds)
        );
    }

    #[test]
    fn every_reason_the_proof_gives_is_placed() {
        // The mapping is an exhaustive match, so a reason the proof gains
        // fails to build until it is placed; this pins where each existing one
        // goes, the four that name a cause and a sample of the rest.
        assert_eq!(left_because(&Why::NotInIndex), LeftBecause::NotTracked);
        assert_eq!(
            left_because(&Why::HiddenFromWorkTree {
                flag: HiddenFlag::Both
            }),
            LeftBecause::Hidden(HiddenFlag::Both)
        );
        assert_eq!(
            left_because(&Why::NotWhatGitChecksOut),
            LeftBecause::NotWhatGitChecksOut
        );
        assert_eq!(
            left_because(&Why::CheckoutNotReproducible),
            LeftBecause::NotWhatGitChecksOut
        );
        for why in [
            Why::Conflicted,
            Why::SymbolicLinkInIndex,
            Why::SubmoduleInIndex,
            Why::NotARegularFile,
            Why::ChangedSinceSurvey,
            Why::CheckoutFailed {
                diagnostic: "git said no".to_owned(),
            },
            Why::OutputTooLarge {
                command: "cat-file",
            },
            Why::Unreadable {
                detail: "denied".to_owned(),
            },
        ] {
            assert_eq!(
                left_because(&why),
                LeftBecause::CannotShowItHolds,
                "{why:?}"
            );
        }
    }
}
