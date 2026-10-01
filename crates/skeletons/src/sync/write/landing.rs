//! The order writes land in, and what a failed link means.
//!
//! `sync` creates a missing file by hard-linking it into place and replaces a
//! changed one by renaming over it. A rename is a write that cannot be taken
//! back, and a link is the one step that a filesystem may refuse for every
//! file at once (FAT, exFAT and some network mounts have no hard links). So
//! every link lands before any rename: the first link that fails does so with
//! nothing written, where landing in path order could rename a file first and
//! leave the tree half written.

use super::PreparedWrite;
use super::failure::{CommitCause, TargetChange};
use crate::sync::proof::Evidence;

/// The positions of `writes`, in the order they land: every write that
/// creates a file (its evidence is [`Evidence::Absent`], so it links), then
/// every write that replaces one ([`Evidence::Held`], so it renames), each
/// group in the order `writes` already has, which is path order.
pub(super) fn landing_order(writes: &[PreparedWrite]) -> Vec<usize> {
    let creating = writes
        .iter()
        .enumerate()
        .filter(|(_, write)| matches!(write.evidence, Evidence::Absent));
    let replacing = writes
        .iter()
        .enumerate()
        .filter(|(_, write)| matches!(write.evidence, Evidence::Held(_)));
    let order: Vec<usize> = creating
        .chain(replacing)
        .map(|(position, _)| position)
        .collect();
    assert_eq!(
        order.len(),
        writes.len(),
        "every write lands exactly once, links first"
    );
    // The ordering this function exists for, asserted apart from the count:
    // once a replacement lands, no creation follows it, since a rename
    // cannot be undone and a link that then failed would leave it written.
    let first_replacement = order
        .iter()
        .position(|&position| matches!(writes[position].evidence, Evidence::Held(_)))
        .unwrap_or(order.len());
    assert!(
        order[first_replacement..]
            .iter()
            .all(|&position| matches!(writes[position].evidence, Evidence::Held(_))),
        "no file sync creates lands after a file it replaces"
    );
    order
}

/// What a failed `hard_link` means for the write it was for: a path that is
/// already taken is a file that appeared since the last look, and anything
/// else is one failure, reported as the operating system gave it.
///
/// The error number is not read. A filesystem's lack of hard links answers
/// with a different number on each platform, and reading it would claim to
/// know why the link failed, which `sync` does not: the operating system's
/// own words say it exactly, and the message says only that `sync` creates
/// new files with hard links.
pub(super) fn link_failure(error: &std::io::Error) -> CommitCause {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => CommitCause::Changed(TargetChange::Appeared),
        _ => CommitCause::Create {
            detail: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io::Error;

    use tempfile::TempDir;

    use super::{landing_order, link_failure};
    use crate::claim::DriftReason;
    use crate::git::ObjectId;
    use crate::sync::proof::{Evidence, HeldFile, IndexMode, ProvenWrite};
    use crate::sync::write::failure::{CommitCause, TargetChange};
    use crate::sync::write::{Write, prepare};

    fn proven(path: &str, reason: DriftReason) -> ProvenWrite {
        let evidence = match reason {
            DriftReason::Missing => Evidence::Absent,
            DriftReason::Changed => Evidence::Held(HeldFile::for_test(
                ObjectId::parse("0000000000000000000000000000000000000000")
                    .expect("well-formed test object id"),
                IndexMode::Regular,
                b"old".to_vec(),
            )),
        };
        ProvenWrite::for_test(
            Write {
                path: crate::claim::ClaimPath::from_rendering_path(path)
                    .expect("a well-formed test path"),
                skeleton: "a-skeleton".to_owned(),
                version: semver::Version::new(0, 1, 0),
                rendered: b"new".to_vec(),
                reason,
            },
            evidence,
        )
    }

    #[test]
    fn landing_order_puts_every_absent_write_before_any_held_one() {
        // Path order is `a` (replaced), `b` (created), `c` (replaced), `d`
        // (created). Creations land first, then replacements, each group
        // still in path order: 1, 3, 0, 2. The writes are staged for real so
        // the order is read from what `prepare` produced.
        let root = TempDir::new().expect("scratch directory");
        for held in ["a.yml", "c.yml"] {
            std::fs::write(root.path().join(held), b"old").expect("a file to replace");
        }
        let prepared = prepare(
            root.path(),
            vec![
                proven("a.yml", DriftReason::Changed),
                proven("b.yml", DriftReason::Missing),
                proven("c.yml", DriftReason::Changed),
                proven("d.yml", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");

        let order = landing_order(&prepared.writes);

        assert_eq!(order, vec![1, 3, 0, 2]);
        let is_absent =
            |position: &usize| matches!(prepared.writes[*position].evidence, Evidence::Absent);
        let last_absent = order.iter().rposition(is_absent).expect("a creation lands");
        let first_held = order
            .iter()
            .position(|position| !is_absent(position))
            .expect("a replacement lands");
        assert!(
            last_absent < first_held,
            "every link must land before the first rename: {order:?}"
        );
    }

    #[test]
    fn landing_order_of_nothing_is_nothing_and_of_one_kind_is_path_order() {
        let root = TempDir::new().expect("scratch directory");
        let prepared = prepare(
            root.path(),
            vec![
                proven("a.yml", DriftReason::Missing),
                proven("b.yml", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");

        assert_eq!(landing_order(&prepared.writes), vec![0, 1]);
        assert!(landing_order(&[]).is_empty());
    }

    #[test]
    fn every_link_failure_but_a_taken_path_is_one_create_failure_carrying_the_os_error() {
        // Whatever number the operating system answers with, the cause is the
        // same one and carries the error's own text. The numbers are the
        // ones a filesystem without hard links gives and the ones an
        // unrelated failure gives: none of them is told apart.
        for code in [1, 13, 18, 28, 38, 45, 95] {
            let error = Error::from_raw_os_error(code);
            let cause = link_failure(&error);
            assert_eq!(
                cause,
                CommitCause::Create {
                    detail: error.to_string()
                },
                "errno {code} must be the one create failure"
            );
        }
    }

    #[test]
    fn a_path_that_is_taken_is_a_change_not_a_create_failure() {
        let taken = Error::from(std::io::ErrorKind::AlreadyExists);

        assert_eq!(
            link_failure(&taken),
            CommitCause::Changed(TargetChange::Appeared)
        );
    }

    #[test]
    fn an_error_with_no_errno_is_the_same_create_failure() {
        let cause = link_failure(&Error::other("no number behind this one"));

        assert_eq!(
            cause,
            CommitCause::Create {
                detail: "no number behind this one".to_owned()
            }
        );
    }
}
