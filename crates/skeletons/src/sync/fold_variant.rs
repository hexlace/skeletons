//! Index entries hidden under another spelling of a path `sync` is about to
//! write: the ones a filesystem that folds names could take for the claim,
//! though git, which folds ASCII case alone, keeps them apart.
//!
//! Git asks nothing of a Unicode difference: `café.yml` spelled precomposed
//! and spelled decomposed are two names to it, and `É` and `é` are too. A
//! filesystem such as APFS takes each pair for one name. So an index entry
//! hidden by skip-worktree or a sparse checkout under the other spelling, and
//! the claim created under this one, are one file on disk and two entries to
//! git: the hidden entry then reads as modified and the new file as
//! untracked, or git shows nothing at all, and the bone is never committed.
//!
//! Whether that can happen is a fact about the filesystem, and only the
//! filesystem can say (`sync::write`'s fold probe asks it, once the staging
//! file exists). What this module decides is which index entries are worth
//! asking about: those that are one name with a claim, or with a directory
//! above it, under [`FoldedName`], the fold the render, overlap detection and
//! the claim walk all use, and that are not spelled the same. It is pure:
//! bytes and paths in, entries out.
//!
//! An entry *beneath* a fold variant of the claim, and a sibling under a
//! fold-variant directory, are deliberately not candidates. Git shows both as
//! untracked files whatever the filesystem does, and the hidden entry never
//! stands in for the claim, so the bone survives.

use std::collections::{BTreeMap, BTreeSet};

use crate::claim::{ClaimPath, folded_components};
use crate::skeleton::FoldedName;

/// One index entry that is one name, to a filesystem that folds names, with
/// a write's claim or with a directory above it, though spelled differently.
/// The fields are private: [`fold_variants`] is what builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoldVariant {
    git_path: String,
    relation: FoldRelation,
}

/// How an index entry relates to a claim it is one name with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FoldRelation {
    /// The entry is one name with the claim itself.
    SamePath,
    /// The entry is one name with `claimed`, a directory above the claim: git
    /// holds a file where the claim needs a directory.
    DirectoryAbove { claimed: ClaimPath },
}

impl FoldVariant {
    /// The entry's path as git's index spells it, workspace-relative.
    pub(crate) fn git_path(&self) -> &str {
        &self.git_path
    }

    /// How the entry relates to the claim it was found for.
    pub(crate) const fn relation(&self) -> &FoldRelation {
        &self.relation
    }

    /// Whether this is still a fold variant of `claim`: the relation
    /// [`fold_variants`] produced, checked again by whoever acts on it so an
    /// entry paired with the wrong claim cannot be probed as though it belonged
    /// to it.
    pub(crate) fn relates_to(&self, claim: &ClaimPath) -> bool {
        let entry: Vec<&str> = self.git_path.split('/').collect();
        let claimed: Vec<&str> = claim.as_str().split('/').collect();
        let expected_length = match &self.relation {
            FoldRelation::SamePath => claimed.len(),
            FoldRelation::DirectoryAbove { claimed: above } => above.depth(),
        };
        entry.len() == expected_length
            && entry.len() <= claimed.len()
            && entry.iter().zip(&claimed).all(|(entry_name, claim_name)| {
                FoldedName::of(entry_name) == FoldedName::of(claim_name)
            })
            && entry != claimed[..entry.len()]
    }
}

/// For each of `writes`, the index entries in `listing` (`ls-files -z`'s own
/// output, paths NUL-separated) that are variants of it.
///
/// An entry `e1..ek` is a variant of the claim `c1..cn` when `k <= n`, every
/// `ei` is one name with `ci` under [`FoldedName`], and `e1..ek` are not
/// byte for byte `c1..ck`. With `k == n` it is the claim's own path spelled
/// another way; with `k < n` it is a directory above the claim spelled
/// another way, and git tracks a file there. Exact matches are left out
/// because git's own questions already ask about them: an entry at the claim
/// is asked about as the claim's index entry, and one at a directory above it
/// as the ancestor question.
///
/// A path that is not valid UTF-8 is skipped, as the claim walk skips such a
/// directory entry: a claim's name is always UTF-8, so no such path is one
/// name with it. One entry listed twice (an unmerged path is listed once per
/// stage) counts once. The result holds one list per write, in `writes`
/// order, each sorted by the entry's path.
///
/// All of `writes` are answered in one pass: each write's folded prefixes are
/// indexed, and every entry is folded once.
pub(crate) fn fold_variants(listing: &[u8], writes: &[&ClaimPath]) -> Vec<Vec<FoldVariant>> {
    let mut wanted: BTreeMap<Vec<FoldedName>, Vec<usize>> = BTreeMap::new();
    for (index, claim) in writes.iter().enumerate() {
        let folded = folded_components(claim);
        for length in 1..=folded.len() {
            wanted
                .entry(folded[..length].to_vec())
                .or_default()
                .push(index);
        }
    }

    let entries: BTreeSet<&str> = listing
        .split(|&byte| byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|path| std::str::from_utf8(path).ok())
        .collect();

    let mut found: Vec<Vec<FoldVariant>> = vec![Vec::new(); writes.len()];
    for entry in entries {
        let components: Vec<&str> = entry.split('/').collect();
        let folded: Vec<FoldedName> = components.iter().map(|name| FoldedName::of(name)).collect();
        for &index in wanted.get(&folded).into_iter().flatten() {
            if let Some(variant) = variant_of(entry, &components, writes[index]) {
                found[index].push(variant);
            }
        }
    }

    // Postcondition: one answer per write, and each is what `relates_to`
    // says it is, so the consumer's check can never fail for a variant this
    // function produced.
    assert_eq!(
        found.len(),
        writes.len(),
        "one list of variants per write, whether or not it has any"
    );
    for (claim, variants) in writes.iter().zip(&found) {
        for variant in variants {
            assert!(
                variant.relates_to(claim),
                "{variant:?} was produced for {claim} and must relate to it"
            );
        }
    }
    found
}

/// The variant `entry` (already known to fold onto the first `components.len()`
/// components of `claim`) makes of `claim`, or `None` when it is spelled
/// exactly as the claim spells that many components.
fn variant_of(entry: &str, components: &[&str], claim: &ClaimPath) -> Option<FoldVariant> {
    let claimed: Vec<&str> = claim.as_str().split('/').collect();
    let length = components.len();
    assert!(
        length <= claimed.len(),
        "an entry only folds onto as many components as the claim has"
    );
    if components == &claimed[..length] {
        return None;
    }
    let relation = if length == claimed.len() {
        FoldRelation::SamePath
    } else {
        FoldRelation::DirectoryAbove {
            claimed: claim.ancestors()[length - 1].clone(),
        }
    };
    Some(FoldVariant {
        git_path: entry.to_owned(),
        relation,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{FoldRelation, FoldVariant, fold_variants};
    use crate::claim::ClaimPath;
    use crate::skeleton::FoldedName;

    fn claim(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// The listing `ls-files -z` prints for `paths`.
    fn listing(paths: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for path in paths {
            bytes.extend_from_slice(path.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    /// Every variant found for the one claim `path`, as `(git path, relation)`.
    fn variants_of(path: &str, paths: &[&str]) -> Vec<(String, FoldRelation)> {
        let claim = claim(path);
        let mut all = fold_variants(&listing(paths), &[&claim]);
        assert_eq!(all.len(), 1);
        all.remove(0)
            .into_iter()
            .map(|variant| (variant.git_path().to_owned(), variant.relation().clone()))
            .collect()
    }

    #[test]
    fn an_entry_one_name_with_the_claim_but_spelled_otherwise_is_the_same_path() {
        // The claim is precomposed and the entry decomposed: two names to
        // git, one to APFS.
        let found = variants_of("caf\u{e9}.yml", &["cafe\u{301}.yml", "other.yml"]);
        assert_eq!(
            found,
            vec![("cafe\u{301}.yml".to_owned(), FoldRelation::SamePath)]
        );
    }

    #[test]
    fn a_non_ascii_case_variant_is_one_name_too() {
        // `É` and `é`: git keeps them apart, the fold and APFS do not.
        let found = variants_of("\u{e9}.yml", &["\u{c9}.yml"]);
        assert_eq!(
            found,
            vec![("\u{c9}.yml".to_owned(), FoldRelation::SamePath)]
        );
    }

    #[test]
    fn an_entry_at_a_directory_above_the_claim_spelled_otherwise_is_a_directory_above() {
        // Git holds a file `cafe\u{301}` where the claim needs the directory
        // `café`.
        let found = variants_of("caf\u{e9}/x.yml", &["cafe\u{301}"]);
        assert_eq!(
            found,
            vec![(
                "cafe\u{301}".to_owned(),
                FoldRelation::DirectoryAbove {
                    claimed: claim("caf\u{e9}")
                }
            )]
        );
    }

    #[test]
    fn a_middle_directory_names_the_prefix_it_stands_for() {
        let found = variants_of("a/caf\u{e9}/x.yml", &["a/cafe\u{301}"]);
        assert_eq!(
            found,
            vec![(
                "a/cafe\u{301}".to_owned(),
                FoldRelation::DirectoryAbove {
                    claimed: claim("a/caf\u{e9}")
                }
            )]
        );
    }

    #[test]
    fn an_exact_spelling_is_left_to_git_s_own_questions() {
        // The claim's own entry, and a file at a directory above it, spelled
        // exactly as claimed: `ls-files` asks about those already, so they
        // are not variants. The control is the decomposed entry beside them,
        // which is one.
        let found = variants_of(
            "caf\u{e9}/x.yml",
            &["caf\u{e9}", "caf\u{e9}/x.yml", "cafe\u{301}/x.yml"],
        );
        assert_eq!(
            found,
            vec![("cafe\u{301}/x.yml".to_owned(), FoldRelation::SamePath)],
            "only the respelled entry is a variant"
        );
    }

    #[test]
    fn an_entry_beneath_a_variant_of_the_claim_is_not_a_candidate() {
        // `CAFÉ.yml/z` hidden, `café.yml` claimed: git shows the new file as
        // untracked and the bone survives whatever the filesystem does.
        let found = variants_of("caf\u{e9}.yml", &["CAF\u{c9}.yml/z"]);
        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn a_sibling_under_a_variant_directory_is_not_a_candidate() {
        // `cafe\u{301}/x.yml` hidden, `café/y.yml` claimed: they share only a
        // directory, and git shows the claim as untracked.
        let found = variants_of("caf\u{e9}/y.yml", &["cafe\u{301}/x.yml"]);
        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn a_name_that_only_looks_alike_is_not_one() {
        // A near miss on each axis: a different letter, an accent apart, and
        // a longer name.
        let found = variants_of("caf\u{e9}.yml", &["cafe.yml", "caf\u{e9}s.yml", "cafx.yml"]);
        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn a_path_that_is_not_utf_8_is_skipped_and_the_rest_are_still_read() {
        let mut bytes = b"cafe\xff.yml\0".to_vec();
        bytes.extend_from_slice("cafe\u{301}.yml".as_bytes());
        bytes.push(0);
        let claim = claim("caf\u{e9}.yml");

        let found = fold_variants(&bytes, &[&claim]);

        assert_eq!(found[0].len(), 1);
        assert_eq!(found[0][0].git_path(), "cafe\u{301}.yml");
    }

    #[test]
    fn an_entry_listed_once_per_stage_counts_once() {
        let found = variants_of("caf\u{e9}.yml", &["cafe\u{301}.yml", "cafe\u{301}.yml"]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn every_write_is_answered_and_a_write_with_no_variants_has_an_empty_list() {
        let with = claim("caf\u{e9}.yml");
        let without = claim("plain.yml");
        let listing = listing(&["cafe\u{301}.yml", "plain.yml"]);

        let found = fold_variants(&listing, &[&without, &with]);

        assert_eq!(found.len(), 2);
        assert!(
            found[0].is_empty(),
            "plain.yml is an exact entry, not a variant"
        );
        assert_eq!(found[1].len(), 1);
    }

    #[test]
    fn an_empty_listing_and_no_writes_are_both_empty_answers() {
        let claim = claim("a.yml");
        assert_eq!(
            fold_variants(b"", &[&claim]),
            vec![Vec::<FoldVariant>::new()]
        );
        assert!(fold_variants(b"a.yml\0", &[]).is_empty());
    }

    #[test]
    fn a_variant_relates_to_its_own_claim_and_to_no_other() {
        let claim_a = claim("caf\u{e9}.yml");
        let found = fold_variants(&listing(&["cafe\u{301}.yml"]), &[&claim_a]);
        let variant = &found[0][0];

        assert!(variant.relates_to(&claim_a));
        assert!(
            !variant.relates_to(&claim("caf\u{e9}.yaml")),
            "another name"
        );
        assert!(
            !variant.relates_to(&claim("caf\u{e9}/x.yml")),
            "the same entry against a claim it is a directory of would be another relation"
        );
        assert!(
            !variant.relates_to(&claim("cafe\u{301}.yml")),
            "an exact spelling is not a variant"
        );
    }

    /// Names drawn from a small alphabet in which every kind of difference
    /// occurs: case, normalization and a plain other letter.
    fn name() -> impl Strategy<Value = String> {
        proptest::sample::select(vec![
            "a", "A", "b", "\u{e9}", "e\u{301}", "\u{c9}", "E\u{301}",
        ])
        .prop_map(str::to_owned)
    }

    fn path(depth: std::ops::RangeInclusive<usize>) -> impl Strategy<Value = String> {
        proptest::collection::vec(name(), depth).prop_map(|names| names.join("/"))
    }

    proptest! {
        /// The relation is exactly the component-wise rule: an entry is a
        /// variant of a claim precisely when it has no more components than
        /// the claim, each folds to the claim's, and the spellings are not
        /// byte for byte the claim's. The oracle compares every entry with
        /// every claim by comparing components as `FoldedName`s, with no
        /// index, so it shares nothing with the map the function is built on
        /// but the fold.
        #[test]
        fn variants_are_exactly_the_component_wise_fold_prefix_rule(
            claims in proptest::collection::vec(path(1..=3), 1..4),
            entries in proptest::collection::vec(path(1..=3), 0..8),
        ) {
            let claims: Vec<ClaimPath> = claims.iter().map(|text| claim(text)).collect();
            let claim_refs: Vec<&ClaimPath> = claims.iter().collect();
            let entry_refs: Vec<&str> = entries.iter().map(String::as_str).collect();

            let found = fold_variants(&listing(&entry_refs), &claim_refs);

            for (claim, variants) in claims.iter().zip(&found) {
                let claimed: Vec<&str> = claim.as_str().split('/').collect();
                let mut expected: Vec<&str> = entries
                    .iter()
                    .map(String::as_str)
                    .filter(|entry| {
                        let components: Vec<&str> = entry.split('/').collect();
                        components.len() <= claimed.len()
                            && components
                                .iter()
                                .zip(&claimed)
                                .all(|(left, right)| FoldedName::of(left) == FoldedName::of(right))
                            && components != claimed[..components.len()]
                    })
                    .collect();
                expected.sort_unstable();
                expected.dedup();
                let actual: Vec<&str> = variants.iter().map(FoldVariant::git_path).collect();
                prop_assert_eq!(actual, expected);
            }
        }
    }
}
