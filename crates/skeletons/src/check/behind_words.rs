//! The words `check` prints for why a skeleton is behind or why that is
//! undetermined: the parenthetical after the state word in a block's header,
//! and the exact text `--json`'s `behind.detail` carries. One function, so the
//! two outputs cannot word the same fact two different ways, and one place the
//! text from outside it holds (a tag, a branch, the words of a remote's
//! failure) is escaped.

use crate::behind::{self, Newer, Undetermined};
use crate::skeleton::Escaped;
use crate::workspace::Pin;

/// The detail of a skeleton a remote reports a newer `newer` for. A skeleton
/// that is current or pinned has none, and both outputs show it with no detail
/// at all.
///
/// [`behind::newer_detail`] words the fact around text a remote, a tag or a
/// branch supplied, and returns it as it came. The whole detail is escaped
/// here, once: the words around that text hold nothing to escape.
pub(super) fn newer_detail(pin: &Pin, newer: &Newer) -> String {
    Escaped(&behind::newer_detail(pin, newer)).to_string()
}

/// The detail of a skeleton whose state could not be determined: the words the
/// check stored, which quote a remote's or the system's own, escaped once.
pub(super) fn undetermined_detail(undetermined: &Undetermined) -> String {
    Escaped(&undetermined.detail).to_string()
}

#[cfg(test)]
mod tests {
    use super::{newer_detail, undetermined_detail};
    use crate::behind::{Behind, Newer, Undetermined, UndeterminedReason};
    use crate::git::ObjectId;
    use crate::survey::poison::{POISON, assert_escaped_once, assert_every_kind, assert_one_line};
    use crate::workspace::Pin;

    /// A commit as a remote names it. The detail cuts it to its first seven
    /// digits, so the text from outside goes in the fields around it.
    fn commit() -> ObjectId {
        ObjectId::parse("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39")
            .expect("a well-formed object id")
    }

    /// How many kinds of [`Behind`] there are. The match in [`kind`] has no
    /// wildcard, so a kind added without an arm does not compile, and the
    /// samples must then cover it.
    const BEHIND_KINDS: usize = 4;

    const fn kind(fact: &Behind) -> usize {
        match fact {
            Behind::Current => 0,
            Behind::Behind(_) => 1,
            Behind::Pinned => 2,
            Behind::Undetermined(_) => 3,
        }
    }

    /// How many kinds of [`Newer`] there are; see [`BEHIND_KINDS`].
    const NEWER_KINDS: usize = 3;

    const fn newer_kind(newer: &Newer) -> usize {
        match newer {
            Newer::Version(_) => 0,
            Newer::Tag(_) => 1,
            Newer::Commit { .. } => 2,
        }
    }

    fn samples() -> Vec<(Pin, Behind)> {
        let branch_pin = || Pin::Branch {
            url: POISON.to_owned(),
            branch: POISON.to_owned(),
            commit: commit(),
        };
        vec![
            (Pin::CratesIo, Behind::Current),
            (Pin::CratesIo, Behind::Pinned),
            (
                Pin::CratesIo,
                Behind::Behind(Newer::Version(semver::Version::new(0, 2, 0))),
            ),
            (Pin::CratesIo, Behind::Behind(Newer::Tag(POISON.to_owned()))),
            (
                branch_pin(),
                Behind::Behind(Newer::Commit {
                    sha: commit(),
                    branch: None,
                }),
            ),
            (
                Pin::DefaultBranch {
                    url: POISON.to_owned(),
                    commit: commit(),
                },
                Behind::Behind(Newer::Commit {
                    sha: commit(),
                    branch: Some(POISON.to_owned()),
                }),
            ),
            (
                Pin::CratesIo,
                Behind::Undetermined(Undetermined {
                    reason: UndeterminedReason::Unreachable,
                    detail: format!("could not reach {POISON}: {POISON}"),
                }),
            ),
        ]
    }

    #[test]
    fn every_kind_of_behind_fact_prints_its_outside_text_escaped_once() {
        // Each sample puts the poison into every piece of text a fact
        // carries: a tag, a commit, a branch, the words of a failure. The
        // detail is one line and shows each as the escape, once. The kinds
        // are counted through exhaustive matches, so a new one cannot go
        // untested.
        let samples = samples();
        assert_every_kind(
            samples.iter().map(|(_, fact)| kind(fact)),
            BEHIND_KINDS,
            "Behind",
        );
        let newers = samples.iter().filter_map(|(_, fact)| match fact {
            Behind::Behind(newer) => Some(newer_kind(newer)),
            Behind::Current | Behind::Pinned | Behind::Undetermined(_) => None,
        });
        assert_every_kind(newers, NEWER_KINDS, "Newer");

        for (pin, fact) in &samples {
            let detail = match fact {
                Behind::Current | Behind::Pinned => continue,
                Behind::Behind(newer) => newer_detail(pin, newer),
                Behind::Undetermined(undetermined) => undetermined_detail(undetermined),
            };
            let what = format!("{fact:?}");
            // A version is a typed value and carries no text from outside.
            if matches!(fact, Behind::Behind(Newer::Version(_))) {
                assert_one_line(&detail, &what);
            } else {
                assert_escaped_once(&detail, &what);
            }
        }
    }

    #[test]
    fn an_undetermined_fact_reads_the_words_it_stored() {
        let undetermined = Undetermined {
            reason: UndeterminedReason::OtherRegistry,
            detail: "`skeletons` asks only crates.io whether a skeleton is behind".to_owned(),
        };
        assert_eq!(
            undetermined_detail(&undetermined),
            "`skeletons` asks only crates.io whether a skeleton is behind"
        );
    }
}
