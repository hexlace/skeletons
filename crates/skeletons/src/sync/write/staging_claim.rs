//! Whether a write's staging name could be taken for a claimed path.
//!
//! A drifted write is staged at `a/.b.skeletons-sync` beside its target `a/b`.
//! If any claim, drifted or not, is that same name to the filesystem, then
//! staging the one file is writing the other: on a filesystem that ignores
//! case or Unicode normalization the bytes of one bone would land on the
//! other's path, and `verify` would then fail reading them back. So the
//! staging name is compared against every claim by the fold the render and
//! overlap detection use ([`FoldedName`], component by component), on every
//! platform alike, and any of three relations refuses before anything is
//! written: the staging name equals a claim, is a directory above one, or
//! sits beneath one.
//!
//! The claims are indexed once, by folded path and by folded proper prefix,
//! so each staging name costs one lookup per component rather than one
//! comparison per claim.

use std::collections::BTreeMap;

use crate::claim::{ClaimPath, folded_components, proper_folded_prefixes};
use crate::skeleton::FoldedName;

/// How a staging name relates to the claim it could be taken for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingRelation {
    /// The staging name is exactly the claimed path.
    Exact,
    /// The staging name is not the claimed path as spelled, but is one name
    /// with it to a filesystem that ignores case or Unicode normalization.
    Folded,
    /// The staging name is a directory above the claimed path, so staging
    /// there would need a file where the claim needs a directory.
    DirectoryAbove,
    /// The staging name sits beneath the claimed path, so staging there
    /// would need a directory where the claim holds a file.
    Beneath,
}

/// Every claim's folded path and folded proper prefixes, for
/// [`ClaimedNames::taking_the_name_of`].
pub(crate) struct ClaimedNames<'a> {
    by_folded_path: BTreeMap<Vec<FoldedName>, &'a ClaimPath>,
    by_folded_proper_prefix: BTreeMap<Vec<FoldedName>, &'a ClaimPath>,
}

impl<'a> ClaimedNames<'a> {
    /// Indexes `claims`, keeping the first claim seen for any folded path
    /// or prefix so the one a refusal names does not depend on map order.
    pub(crate) fn new(claims: impl IntoIterator<Item = &'a ClaimPath>) -> Self {
        let mut by_folded_path = BTreeMap::new();
        let mut by_folded_proper_prefix = BTreeMap::new();
        for claim in claims {
            let components = folded_components(claim);
            for (_, prefix) in proper_folded_prefixes(&components) {
                by_folded_proper_prefix
                    .entry(prefix.to_vec())
                    .or_insert(claim);
            }
            by_folded_path.entry(components).or_insert(claim);
        }
        Self {
            by_folded_path,
            by_folded_proper_prefix,
        }
    }

    /// The claim `write`'s staging name could be taken for, and how, or
    /// `None` when the staging name folds onto nothing claimed.
    ///
    /// Checked in order: the same folded path, a claim above the staging
    /// name, then a claim below it.
    pub(crate) fn taking_the_name_of(
        &self,
        write: &ClaimPath,
    ) -> Option<(&'a ClaimPath, StagingRelation)> {
        let staging_claim = write.staging();
        let staging = staging_claim.as_str();
        let components = folded_components(&staging_claim);

        if let Some(claimed) = self.by_folded_path.get(&components) {
            let relation = if claimed.as_str() == staging {
                StagingRelation::Exact
            } else {
                StagingRelation::Folded
            };
            return Some((claimed, relation));
        }
        for (_, prefix) in proper_folded_prefixes(&components) {
            if let Some(claimed) = self.by_folded_path.get(prefix) {
                return Some((claimed, StagingRelation::Beneath));
            }
        }
        self.by_folded_proper_prefix
            .get(&components)
            .map(|claimed| (*claimed, StagingRelation::DirectoryAbove))
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimedNames, StagingRelation};
    use crate::claim::ClaimPath;

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// Indexes `claimed` and asks what `write`'s staging name could be taken
    /// for, answering with the claimed path as spelled and the relation.
    fn taken_for(write: &str, claimed: &[&str]) -> Option<(String, StagingRelation)> {
        let claims: Vec<ClaimPath> = claimed.iter().map(|path| claim_path(path)).collect();
        ClaimedNames::new(&claims)
            .taking_the_name_of(&claim_path(write))
            .map(|(claim, relation)| (claim.as_str().to_owned(), relation))
    }

    #[test]
    fn a_claim_spelled_exactly_as_the_staging_name_is_exact() {
        // Exercises the equal relation with no fold involved: the staging
        // name of `a/b` is `a/.b.skeletons-sync`, claimed as written.
        assert_eq!(
            taken_for("a/b", &["a/b", "a/.b.skeletons-sync"]),
            Some(("a/.b.skeletons-sync".to_owned(), StagingRelation::Exact))
        );
    }

    #[test]
    fn a_claim_that_differs_from_the_staging_name_only_by_case_is_folded() {
        assert_eq!(
            taken_for("a/b", &["a/b", "a/.B.skeletons-sync"]),
            Some(("a/.B.skeletons-sync".to_owned(), StagingRelation::Folded))
        );
    }

    #[test]
    fn a_claim_that_differs_from_the_staging_name_only_by_normalisation_is_folded() {
        // The staging name of `é` written composed (NFC) is `.é.skeletons-sync`;
        // the claim spells the same name decomposed (NFD).
        let composed = "\u{e9}";
        let decomposed = "e\u{301}";
        assert_eq!(
            taken_for(
                composed,
                &[composed, &format!(".{decomposed}.skeletons-sync")]
            ),
            Some((
                format!(".{decomposed}.skeletons-sync"),
                StagingRelation::Folded
            ))
        );
    }

    #[test]
    fn a_directory_of_another_case_above_a_claim_is_a_directory_above() {
        // Staging `a/b` puts a file at `a/.b.skeletons-sync`, and a claim
        // `A/.B.skeletons-sync/c` needs that name to be a directory.
        assert_eq!(
            taken_for("a/b", &["a/b", "A/.B.skeletons-sync/c"]),
            Some((
                "A/.B.skeletons-sync/c".to_owned(),
                StagingRelation::DirectoryAbove
            ))
        );
    }

    #[test]
    fn a_claim_above_the_staging_name_is_beneath() {
        // The claim `a` is a file to any filesystem that folds it, and the
        // staging name `A/.b.skeletons-sync` would sit inside it.
        assert_eq!(
            taken_for("A/b", &["A/b", "a"]),
            Some(("a".to_owned(), StagingRelation::Beneath))
        );
    }

    #[test]
    fn a_staging_name_that_folds_onto_nothing_claimed_is_free() {
        // The control: every other claim is a different name, and one shares
        // only the directory the staging name sits in.
        assert_eq!(
            taken_for(
                "a/b",
                &[
                    "a/b",
                    "a/c",
                    "a/.c.skeletons-sync",
                    "a/b.skeletons-sync",
                    "z/.b.skeletons-sync"
                ]
            ),
            None
        );
    }

    #[test]
    fn the_first_claim_seen_is_the_one_named() {
        assert_eq!(
            taken_for(
                "a/b",
                &["a/b", "a/.B.skeletons-sync", "a/.b.SKELETONS-sync"]
            ),
            Some(("a/.B.skeletons-sync".to_owned(), StagingRelation::Folded))
        );
    }
}
