//! The branch-directory phase of the behind question: what a `branch =` or
//! unqualified `git =` pin's own remote head settles immediately, and, for a
//! head that differs from what is locked, the directory-scoped comparison
//! that decides whether the difference is inside this skeleton's own
//! directory at all.
//!
//! [`resolve_branch`] and [`resolve_default_branch`] read the cheap head
//! answer each pin kind shares; a differing head becomes a
//! [`DirectoryCheck`], and [`resolve_directory_checks`] finishes every one
//! by reading the locked directory from Cargo's checkout
//! (`cargo_checkout`) and comparing it with the snapshot fetched by
//! `snapshot`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::cargo_checkout::{CheckoutFailure, LockedDirectory, locked_directory};
use super::git_remote::{BranchAnswer, DefaultBranchAnswer};
use super::snapshot::{SnapshotAnswer, directory_trees};
use super::{
    Behind, Newer, REMOTE_QUERIES_IN_FLIGHT_MAX, Remotes, UndeterminedReason, insert_once,
    log_query, undetermined, unexpected_detail, unreachable_detail,
};
use crate::git::{ObjectId, RepositoryPrefix};
use crate::workspace::WornId;

/// What a branch or default-branch pin's own cheap head query settles
/// immediately, or leaves for the directory-comparison phase.
pub(super) enum BranchIntent {
    Answered(Behind),
    Pending(DirectoryCheck),
}

/// One worn dependency's own directory-scoped comparison, once its remote
/// head is known to differ from what is locked — everything
/// [`resolve_directory_checks`] needs to finish deciding it.
pub(super) struct DirectoryCheck {
    url: String,
    head: ObjectId,
    skeleton_directory: PathBuf,
    locked_commit: ObjectId,
    /// What `Newer::Commit`'s own `branch` field should hold if this ends up
    /// `Behind`: always `None` for an explicit `branch =` pin — hardcoded at
    /// `resolve_branch`'s own construction site, not computed — or the
    /// server's own named default branch (or `None`) for an unqualified
    /// `git =` pin, carried straight from `resolve_default_branch`'s answer
    /// (tested by `resolve_branch_leaves_a_differing_head_pending` and
    /// `resolve_default_branch_carries_the_servers_own_named_branch_into_the_pending_check`).
    newer_branch_field: Option<String>,
    /// The pin's own name for its branch, shown in a `directory-missing`
    /// detail — the explicit `branch =` name, or the server-named default
    /// branch, or `"the default branch"` when neither is known.
    label: String,
}

/// An explicit `branch =` pin's own cheap head query, resolved.
pub(super) fn resolve_branch(
    url: &str,
    branch: &str,
    locked_commit: &ObjectId,
    skeleton_directory: PathBuf,
    answer: &BranchAnswer,
) -> BranchIntent {
    match answer {
        BranchAnswer::Unreachable { detail } => BranchIntent::Answered(undetermined(
            UndeterminedReason::Unreachable,
            unreachable_detail(url, detail),
        )),
        BranchAnswer::Malformed { detail } => BranchIntent::Answered(undetermined(
            UndeterminedReason::UnexpectedResponse,
            unexpected_detail(url, detail),
        )),
        BranchAnswer::Missing => BranchIntent::Answered(undetermined(
            UndeterminedReason::BranchMissing,
            format!("branch {branch} no longer exists on {url}"),
        )),
        BranchAnswer::Head(head) if head == locked_commit => {
            BranchIntent::Answered(Behind::Current)
        }
        BranchAnswer::Head(head) => BranchIntent::Pending(DirectoryCheck {
            url: url.to_owned(),
            head: head.clone(),
            skeleton_directory,
            locked_commit: locked_commit.clone(),
            newer_branch_field: None,
            label: branch.to_owned(),
        }),
    }
}

/// An unqualified `git =` pin's own cheap default-branch head query,
/// resolved.
pub(super) fn resolve_default_branch(
    url: &str,
    locked_commit: &ObjectId,
    skeleton_directory: PathBuf,
    answer: &DefaultBranchAnswer,
) -> BranchIntent {
    match answer {
        DefaultBranchAnswer::Unreachable { detail } => BranchIntent::Answered(undetermined(
            UndeterminedReason::Unreachable,
            unreachable_detail(url, detail),
        )),
        DefaultBranchAnswer::Malformed { detail } => BranchIntent::Answered(undetermined(
            UndeterminedReason::UnexpectedResponse,
            unexpected_detail(url, detail),
        )),
        DefaultBranchAnswer::Head { sha, .. } if sha == locked_commit => {
            BranchIntent::Answered(Behind::Current)
        }
        DefaultBranchAnswer::Head { sha, branch } => BranchIntent::Pending(DirectoryCheck {
            url: url.to_owned(),
            head: sha.clone(),
            skeleton_directory,
            locked_commit: locked_commit.clone(),
            newer_branch_field: branch.clone(),
            label: branch
                .clone()
                .unwrap_or_else(|| "the default branch".to_owned()),
        }),
    }
}

/// The second, directory-snapshot phase: for every pending check, reads its
/// own locked directory from Cargo's checkout (local, no network); groups
/// the ones that succeeded by `(url, head)`, since two skeletons can share a
/// remote and a head while living in different directories; fetches each
/// distinct `(url, head)` snapshot at most once, concurrently; then settles
/// every pending check against its own directory's answer.
pub(super) fn resolve_directory_checks(
    pending: Vec<(WornId, DirectoryCheck)>,
    remotes: &Remotes,
    results: &mut BTreeMap<WornId, Behind>,
) {
    let mut checked: Vec<(WornId, DirectoryCheck, LockedDirectory)> =
        Vec::with_capacity(pending.len());
    for (id, check) in pending {
        match locked_directory(&check.skeleton_directory, &check.locked_commit) {
            Ok(directory) => checked.push((id, check, directory)),
            Err(failure) => {
                let behind = checkout_failure_behind(&check.url, &check.locked_commit, &failure);
                insert_once(results, id, behind);
            }
        }
    }

    let mut groups: BTreeMap<(String, ObjectId), BTreeSet<RepositoryPrefix>> = BTreeMap::new();
    for (_id, check, directory) in &checked {
        groups
            .entry((check.url.clone(), check.head.clone()))
            .or_default()
            .insert(directory.prefix().clone());
    }
    let snapshots = run_snapshot_queries(&groups, remotes);

    for (id, check, directory) in checked {
        let key = (check.url.clone(), check.head.clone());
        let Some(answer) = snapshots.get(&key) else {
            unreachable!("every snapshot query built from `checked` is run before this runs")
        };
        let behind = finalize_branch_directory(
            &check.head,
            check.newer_branch_field,
            &check.label,
            &directory,
            answer,
        );
        insert_once(results, id, behind);
    }
}

/// Runs every distinct `(url, head)` snapshot query in `groups`, in chunks
/// of at most [`REMOTE_QUERIES_IN_FLIGHT_MAX`], each on its own thread —
/// logging one `git {url} snapshot {head}` line immediately before each,
/// the same convention [`super::answer_one`] follows for every other remote
/// question.
fn run_snapshot_queries(
    groups: &BTreeMap<(String, ObjectId), BTreeSet<RepositoryPrefix>>,
    remotes: &Remotes,
) -> BTreeMap<(String, ObjectId), SnapshotAnswer> {
    let parent = std::env::temp_dir();
    let parent: &std::path::Path = &parent;
    let jobs: Vec<(&(String, ObjectId), &BTreeSet<RepositoryPrefix>)> = groups.iter().collect();
    let mut answers = BTreeMap::new();
    for (chunk_index, chunk) in jobs.chunks(REMOTE_QUERIES_IN_FLIGHT_MAX).enumerate() {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .enumerate()
                .map(|(index_in_chunk, &(key, prefixes))| {
                    let slot = chunk_index * REMOTE_QUERIES_IN_FLIGHT_MAX + index_in_chunk;
                    let (url, head) = key;
                    log_query(remotes, || format!("git {url} snapshot {head}"));
                    scope.spawn(move || {
                        (
                            key.clone(),
                            directory_trees(url, head, prefixes, parent, slot),
                        )
                    })
                })
                .collect();
            for handle in handles {
                let (key, answer) = match handle.join() {
                    Ok(result) => result,
                    Err(payload) => std::panic::resume_unwind(payload),
                };
                answers.insert(key, answer);
            }
        });
    }
    assert_eq!(
        answers.len(),
        groups.len(),
        "every distinct (url, head) snapshot query is answered exactly once"
    );
    answers
}

fn checkout_failure_behind(
    url: &str,
    locked_commit: &ObjectId,
    failure: &CheckoutFailure,
) -> Behind {
    let locked7 = locked_commit.abbreviated();
    let detail = match failure {
        CheckoutFailure::Unreadable { diagnostic } => {
            format!("cargo's checkout of {url} at {locked7} could not be read: {diagnostic}")
        }
        CheckoutFailure::NotAtLockedCommit { actual } => format!(
            "cargo's checkout of {url} is at {}, not the locked {locked7}",
            actual.abbreviated()
        ),
    };
    undetermined(UndeterminedReason::CheckoutUnreadable, detail)
}

/// `behind` for one branch or default-branch pin once its own directory
/// snapshot has been answered.
fn finalize_branch_directory(
    head: &ObjectId,
    newer_branch_field: Option<String>,
    label: &str,
    locked: &LockedDirectory,
    answer: &SnapshotAnswer,
) -> Behind {
    match answer {
        SnapshotAnswer::LocalFailure { detail } => {
            undetermined(UndeterminedReason::LocalFailure, detail.clone())
        }
        SnapshotAnswer::Unreachable { detail } => {
            undetermined(UndeterminedReason::Unreachable, detail.clone())
        }
        SnapshotAnswer::Malformed { detail } => {
            undetermined(UndeterminedReason::UnexpectedResponse, detail.clone())
        }
        SnapshotAnswer::Trees(trees) => {
            let Some(tree_at_head) = trees.get(locked.prefix()) else {
                unreachable!("every locked prefix requested is answered by its own snapshot query")
            };
            match tree_at_head {
                None => undetermined(
                    UndeterminedReason::DirectoryMissing,
                    directory_missing_detail(locked.prefix(), label, head),
                ),
                Some(head_tree) if head_tree == locked.tree() => Behind::Current,
                Some(_head_tree) => Behind::Behind(Newer::Commit {
                    sha: head.clone(),
                    branch: newer_branch_field,
                }),
            }
        }
    }
}

fn directory_missing_detail(prefix: &RepositoryPrefix, label: &str, head: &ObjectId) -> String {
    let shown = if prefix.as_str().is_empty() {
        "."
    } else {
        prefix.as_str().trim_end_matches('/')
    };
    format!(
        "{shown} does not exist on {label} at {}",
        head.abbreviated()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Behind, BranchAnswer, BranchIntent, CheckoutFailure, DefaultBranchAnswer, LockedDirectory,
        Newer, SnapshotAnswer, UndeterminedReason, checkout_failure_behind,
        directory_missing_detail, finalize_branch_directory, resolve_branch,
        resolve_default_branch,
    };
    use crate::behind::Undetermined;
    use crate::git::{ObjectId, RepositoryPrefix};

    fn oid(number: u8) -> ObjectId {
        ObjectId::parse(&format!(
            "{number:0>2}c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2"
        ))
        .expect("a well-formed test object id")
    }

    fn prefix(text: &str) -> RepositoryPrefix {
        RepositoryPrefix::parse(text).expect("a well-formed test prefix")
    }

    const URL: &str = "https://example.invalid/skeleton";

    /// [`resolve_branch`] for `main` locked at commit 1, against `answer`.
    fn branch_intent(answer: &BranchAnswer) -> BranchIntent {
        resolve_branch(URL, "main", &oid(1), "/skeleton".into(), answer)
    }

    /// [`resolve_branch`]'s own table, one test per [`BranchAnswer`] shape:
    /// an unreachable remote settles as `Unreachable`.
    #[test]
    fn resolve_branch_settles_an_unreachable_remote_as_undetermined() {
        let intent = branch_intent(&BranchAnswer::Unreachable {
            detail: "boom".to_owned(),
        });
        assert!(matches!(
            intent,
            BranchIntent::Answered(Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::Unreachable,
                ..
            }))
        ));
    }

    /// A malformed ref listing settles as `UnexpectedResponse`.
    #[test]
    fn resolve_branch_settles_a_malformed_answer_as_undetermined() {
        let intent = branch_intent(&BranchAnswer::Malformed {
            detail: "garbage".to_owned(),
        });
        assert!(matches!(
            intent,
            BranchIntent::Answered(Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::UnexpectedResponse,
                ..
            }))
        ));
    }

    /// A branch the remote no longer has settles as `BranchMissing`, naming
    /// the branch and the remote.
    #[test]
    fn resolve_branch_settles_a_missing_branch_naming_it() {
        let BranchIntent::Answered(Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::BranchMissing,
            detail,
        })) = branch_intent(&BranchAnswer::Missing)
        else {
            panic!("expected BranchMissing")
        };
        assert_eq!(
            detail,
            "branch main no longer exists on https://example.invalid/skeleton"
        );
    }

    /// A head equal to the locked commit settles as `Current`.
    #[test]
    fn resolve_branch_settles_a_head_equal_to_the_locked_commit_as_current() {
        let intent = branch_intent(&BranchAnswer::Head(oid(1)));
        assert!(matches!(intent, BranchIntent::Answered(Behind::Current)));
    }

    /// A differing head leaves a pending directory check carrying the head,
    /// no server-named branch, and the pin's own branch name as its label.
    #[test]
    fn resolve_branch_leaves_a_differing_head_pending() {
        let differing_head = oid(2);
        let BranchIntent::Pending(check) =
            branch_intent(&BranchAnswer::Head(differing_head.clone()))
        else {
            panic!("expected Pending")
        };
        assert_eq!(check.head, differing_head);
        assert_eq!(check.newer_branch_field, None);
        assert_eq!(check.label, "main");
    }

    #[test]
    fn resolve_default_branch_carries_the_servers_own_named_branch_into_the_pending_check() {
        let locked = oid(1);
        let pending = resolve_default_branch(
            "https://example.invalid/skeleton",
            &locked,
            "/skeleton".into(),
            &DefaultBranchAnswer::Head {
                branch: Some("trunk".to_owned()),
                sha: oid(2),
            },
        );
        let BranchIntent::Pending(check) = pending else {
            panic!("expected Pending")
        };
        assert_eq!(check.newer_branch_field, Some("trunk".to_owned()));
        assert_eq!(check.label, "trunk");
    }

    #[test]
    fn resolve_default_branch_falls_back_to_the_default_branch_wording_when_unnamed() {
        let locked = oid(1);
        let differing_head = oid(2);
        let pending = resolve_default_branch(
            "https://example.invalid/skeleton",
            &locked,
            "/skeleton".into(),
            &DefaultBranchAnswer::Head {
                branch: None,
                sha: differing_head,
            },
        );
        let BranchIntent::Pending(check) = pending else {
            panic!("expected Pending")
        };
        assert_eq!(check.label, "the default branch");
    }

    #[test]
    fn finalize_branch_directory_reads_current_when_the_trees_are_equal() {
        let prefix = prefix("crates/alpha/");
        let tree = oid(3);
        let directory = LockedDirectory::for_test(prefix.clone(), tree.clone());
        let mut trees = std::collections::BTreeMap::new();
        trees.insert(prefix, Some(tree));
        let answer = SnapshotAnswer::Trees(trees);
        let behind = finalize_branch_directory(&oid(2), None, "main", &directory, &answer);
        assert_eq!(behind, Behind::Current);
    }

    #[test]
    fn finalize_branch_directory_reads_behind_naming_the_head_when_the_trees_differ() {
        let prefix = prefix("crates/alpha/");
        let directory = LockedDirectory::for_test(prefix.clone(), oid(3));
        let mut trees = std::collections::BTreeMap::new();
        trees.insert(prefix, Some(oid(4)));
        let answer = SnapshotAnswer::Trees(trees);
        let head = oid(2);
        let behind = finalize_branch_directory(
            &head,
            Some("trunk".to_owned()),
            "trunk",
            &directory,
            &answer,
        );
        assert_eq!(
            behind,
            Behind::Behind(Newer::Commit {
                sha: head,
                branch: Some("trunk".to_owned()),
            })
        );
    }

    #[test]
    fn finalize_branch_directory_reads_directory_missing_when_the_prefix_is_absent_at_head() {
        let prefix = prefix("crates/alpha/");
        let directory = LockedDirectory::for_test(prefix.clone(), oid(3));
        let mut trees = std::collections::BTreeMap::new();
        trees.insert(prefix, None);
        let answer = SnapshotAnswer::Trees(trees);
        let head = oid(2);
        let behind = finalize_branch_directory(&head, None, "main", &directory, &answer);
        let Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::DirectoryMissing,
            detail,
        }) = behind
        else {
            panic!("expected DirectoryMissing")
        };
        assert!(detail.contains("crates/alpha"));
        assert!(detail.contains("main"));
    }

    #[test]
    fn finalize_branch_directory_reads_each_snapshot_failure_as_its_own_undetermined_reason() {
        let directory = LockedDirectory::for_test(prefix("crates/alpha/"), oid(3));
        let head = oid(2);

        let local_failure = finalize_branch_directory(
            &head,
            None,
            "main",
            &directory,
            &SnapshotAnswer::LocalFailure {
                detail: "no space left".to_owned(),
            },
        );
        assert!(matches!(
            local_failure,
            Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::LocalFailure,
                ..
            })
        ));

        let unreachable = finalize_branch_directory(
            &head,
            None,
            "main",
            &directory,
            &SnapshotAnswer::Unreachable {
                detail: "timed out".to_owned(),
            },
        );
        assert!(matches!(
            unreachable,
            Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::Unreachable,
                ..
            })
        ));

        let malformed = finalize_branch_directory(
            &head,
            None,
            "main",
            &directory,
            &SnapshotAnswer::Malformed {
                detail: "garbage".to_owned(),
            },
        );
        assert!(matches!(
            malformed,
            Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::UnexpectedResponse,
                ..
            })
        ));
    }

    /// A checkout whose `HEAD` is not the locked commit reads
    /// `checkout-unreadable`, and its detail names the remote, the commit
    /// the checkout is at and the commit the pin locked, each cut to seven
    /// hex digits. Drives [`checkout_failure_behind`] with the failure
    /// `locked_directory` returns for that checkout.
    #[test]
    fn a_checkout_at_another_commit_reads_checkout_unreadable_naming_both_commits() {
        let behind = checkout_failure_behind(
            URL,
            &oid(1),
            &CheckoutFailure::NotAtLockedCommit { actual: oid(2) },
        );

        let Behind::Undetermined(Undetermined { reason, detail }) = behind else {
            panic!("expected undetermined, got {behind:?}");
        };
        assert_eq!(reason, UndeterminedReason::CheckoutUnreadable);
        assert_eq!(
            detail,
            format!("cargo's checkout of {URL} is at 02c1e2f, not the locked 01c1e2f")
        );
    }

    /// A checkout git could not read reads `checkout-unreadable` too, and
    /// its detail carries git's own diagnostic beside the locked commit.
    #[test]
    fn an_unreadable_checkout_reads_checkout_unreadable_carrying_the_diagnostic() {
        let behind = checkout_failure_behind(
            URL,
            &oid(1),
            &CheckoutFailure::Unreadable {
                diagnostic: "not a git repository".to_owned(),
            },
        );

        let Behind::Undetermined(Undetermined { reason, detail }) = behind else {
            panic!("expected undetermined, got {behind:?}");
        };
        assert_eq!(reason, UndeterminedReason::CheckoutUnreadable);
        assert_eq!(
            detail,
            format!("cargo's checkout of {URL} at 01c1e2f could not be read: not a git repository")
        );
    }

    #[test]
    fn directory_missing_detail_shows_the_repository_root_as_a_dot() {
        let text = directory_missing_detail(&prefix(""), "main", &oid(2));
        assert!(text.starts_with(". does not exist on main at "));
    }

    #[test]
    fn directory_missing_detail_strips_the_trailing_slash_from_a_nested_prefix() {
        let text = directory_missing_detail(&prefix("crates/alpha/"), "main", &oid(2));
        assert!(text.starts_with("crates/alpha does not exist on main at "));
    }
}
