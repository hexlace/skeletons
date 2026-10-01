//! Two or more claims that cannot both hold, because a filesystem that
//! ignores case or Unicode normalization would hold one name where the
//! claims name two. Paths are compared one `/`-separated component at a
//! time, under the render's own fold, [`FoldedName`]. Claims collide when
//! their folded paths are identical, when one claim's path is a folded
//! ancestor directory of another's, or when two claims sit under one folded
//! directory that they spell two ways (`a/x` and `A/y`).

use std::collections::BTreeMap;

use crate::skeleton::FoldedName;

use super::{Claim, ClaimPath, Claimant};

/// A group of two or more claims that collide, naming every path and every
/// claimant involved.
pub(crate) struct Overlap {
    /// Every distinct path in the group, sorted.
    pub(crate) paths: Vec<ClaimPath>,
    /// Every claimant in the group, in the order their claims were given.
    pub(crate) claimants: Vec<Claimant>,
}

/// `path`, split on `/` and each component folded with [`FoldedName`]: the
/// render's own rule for when two entry names are one name, applied here
/// component by component so a proper prefix of the returned sequence names
/// a folded ancestor directory.
///
/// # Panics
///
/// If `path` is empty, which a [`ClaimPath`] never is
/// (`ClaimPath::from_rendering_path` refuses it).
pub(crate) fn folded_components(path: &ClaimPath) -> Vec<FoldedName> {
    let raw = path.as_str();
    assert!(
        !raw.is_empty(),
        "a claim path is never empty (ClaimPath::from_rendering_path)"
    );
    let components: Vec<FoldedName> = raw.split('/').map(FoldedName::of).collect();
    assert!(
        !components.is_empty(),
        "splitting a non-empty path on '/' always yields at least one component"
    );
    components
}

/// Every proper prefix of `components`, shortest first, each with its own
/// length: the folded ancestor directories of a path, and the one place
/// that walk is written. The whole sequence is never yielded, so a prefix
/// is never the path's own last component.
///
/// The length is a component count, so `spelled_prefix` can name the same
/// prefix as its path spells it.
pub(crate) fn proper_folded_prefixes(
    components: &[FoldedName],
) -> impl Iterator<Item = (usize, &[FoldedName])> {
    (1..components.len()).map(move |length| (length, &components[..length]))
}

/// How two colliding claim paths relate under the fold [`find_overlaps`]
/// used to group them, so a reported overlap's own message can word each
/// differently instead of guessing from a narrower fold of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FoldedRelation {
    /// The two paths fold to the same components: identical, or apart only
    /// by case or Unicode normalization.
    SamePath,
    /// One path is a folded ancestor directory of the other.
    Nested,
    /// The two paths share a folded proper prefix, a directory, that each
    /// spells differently: `a/x` and `A/y` share the directory `a`, spelled
    /// `a` by the first and `A` by the second. Each field is that directory
    /// as its own path spells it, from the workspace root, at the shortest
    /// prefix where the two spellings differ.
    SharedDirectorySpelledDifferently {
        /// The shared directory as the first path spells it.
        first_at: String,
        /// The shared directory as the second path spells it.
        second_at: String,
    },
}

/// `raw`'s first `component_count` components, `/`-joined as `raw` spells
/// them: the exact-spelling counterpart of a folded prefix.
///
/// # Panics
///
/// If `raw` has `component_count` components or fewer, since this only ever
/// names a proper prefix and so needs the `/` that follows it.
fn spelled_prefix(raw: &str, component_count: usize) -> &str {
    assert!(component_count > 0, "a proper prefix has a component");
    assert!(
        raw.split('/').count() > component_count,
        "{raw:?} has no proper prefix of {component_count} components"
    );
    let Some((end, _)) = raw.match_indices('/').nth(component_count - 1) else {
        unreachable!("more than {component_count} components means at least that many slashes")
    };
    &raw[..end]
}

/// Whether `shorter`'s components are a proper prefix of `longer`'s — the
/// folded-ancestor-directory relationship [`find_overlaps`] unions two
/// claims by, named here so [`folded_relation`] can tell it apart from an
/// equal-path collision.
fn is_folded_prefix(shorter: &[FoldedName], longer: &[FoldedName]) -> bool {
    shorter.len() < longer.len() && shorter == &longer[..shorter.len()]
}

/// How `first` and `second` collide: the same relations [`find_overlaps`]
/// unions two claims by, consulted again here so a reported overlap's own
/// message words each one from the same fold that grouped it.
///
/// Checked in order: a same folded path, then one path a folded ancestor of
/// the other, then a shared folded directory spelled two ways.
///
/// # Panics
///
/// If `first` and `second` do not collide by this fold at all: this is only
/// ever asked of two paths [`find_overlaps`] already reported as one
/// overlap, so one of the relationships must hold.
pub(crate) fn folded_relation(first: &ClaimPath, second: &ClaimPath) -> FoldedRelation {
    let first_components = folded_components(first);
    let second_components = folded_components(second);
    if first_components == second_components {
        return FoldedRelation::SamePath;
    }
    if is_folded_prefix(&first_components, &second_components)
        || is_folded_prefix(&second_components, &first_components)
    {
        return FoldedRelation::Nested;
    }

    // A proper prefix of both, so the shared directory is never a claim's
    // own last component.
    let shared_prefixes =
        proper_folded_prefixes(&first_components).zip(proper_folded_prefixes(&second_components));
    for ((length, first_prefix), (_, second_prefix)) in shared_prefixes {
        if first_prefix != second_prefix {
            break;
        }
        let first_at = spelled_prefix(first.as_str(), length);
        let second_at = spelled_prefix(second.as_str(), length);
        if first_at != second_at {
            return FoldedRelation::SharedDirectorySpelledDifferently {
                first_at: first_at.to_owned(),
                second_at: second_at.to_owned(),
            };
        }
    }
    unreachable!(
        "folded_relation is only asked about two paths find_overlaps already reported as one \
         overlap, so they must fold to the same components, one must be a folded ancestor of \
         the other, or they must share a folded directory they spell two ways"
    );
}

/// Splits `claims` into the ones that collide with nothing else (returned
/// for normal resolution) and the groups that do.
///
/// Two claims collide when their folded paths are equal — identical, or one
/// name apart only by case or Unicode normalization, the same fold the
/// render itself refuses two of a skeleton's own names by — or when one
/// claim's folded path is a proper prefix of another's, naming a folded
/// ancestor directory — or when two claims sit under one folded directory
/// that they spell two ways (`a/x` and `A/y`), which no case-insensitive
/// filesystem can hold as both. Collision is grouped transitively
/// (union-find): if `a` collides with `b` and `b` with `c`, all three are
/// reported as one overlap even when `a` and `c` do not collide directly — a
/// fold-only clash, a nesting clash and a shared-directory clash can chain
/// through a shared path.
///
/// Every claim's folded path is computed once and grouped in a
/// [`BTreeMap`] keyed on it, so an equal-path collision is a map lookup and
/// a nesting collision is one lookup per proper prefix. A second map, keyed
/// on each folded proper prefix, records the exact spellings claims give
/// it, so a shared directory spelled two ways is found by reading one map
/// entry per prefix rather than comparing every pair of claims:
/// `O(n · depth · log n)` altogether, for `n` claims at most `depth` path
/// components deep.
pub(crate) fn find_overlaps(claims: Vec<Claim>) -> (Vec<Claim>, Vec<Overlap>) {
    let (kept, overlaps, _map_operations) = find_overlaps_and_count_map_operations(claims);
    (kept, overlaps)
}

/// [`find_overlaps`]'s own algorithm, also returning how many [`BTreeMap`]
/// insertions and lookups it performed. The count itself changes nothing
/// about the result; it exists so a test can assert the count grows
/// linearly with the number of claims rather than timing a wall clock,
/// which would not run the same on every machine.
fn find_overlaps_and_count_map_operations(claims: Vec<Claim>) -> (Vec<Claim>, Vec<Overlap>, usize) {
    let total_claims = claims.len();
    let folded_paths: Vec<Vec<FoldedName>> = claims
        .iter()
        .map(|claim| folded_components(&claim.path))
        .collect();
    assert_eq!(
        folded_paths.len(),
        total_claims,
        "one folded path per claim"
    );

    let (by_folded_path, insert_operations) = index_by_folded_path(&folded_paths);
    assert_eq!(
        insert_operations, total_claims,
        "one map insertion per claim"
    );

    let spelled_paths: Vec<&str> = claims.iter().map(|claim| claim.path.as_str()).collect();
    let (by_folded_prefix, prefix_operations) =
        index_by_folded_prefix(&spelled_paths, &folded_paths);

    let mut parent: Vec<usize> = (0..total_claims).collect();
    let lookup_operations = union_collisions(
        &folded_paths,
        &by_folded_path,
        &by_folded_prefix,
        &mut parent,
    );

    let (kept, overlaps) = partition_by_group(claims, parent);
    (
        kept,
        overlaps,
        insert_operations + prefix_operations + lookup_operations,
    )
}

/// Every claim that sits under a folded proper prefix, and how it spells
/// that prefix, keyed by the folded prefix.
type PrefixSpellings<'a> = BTreeMap<Vec<FoldedName>, Vec<(&'a str, usize)>>;

/// Groups, for every folded proper prefix of every claim, the claim's index
/// and the exact spelling it gives that prefix, so a prefix spelled two
/// ways is one map entry holding two spellings. Returns the map alongside
/// how many entries it recorded: one per proper prefix of each claim.
fn index_by_folded_prefix<'a>(
    spelled_paths: &[&'a str],
    folded_paths: &[Vec<FoldedName>],
) -> (PrefixSpellings<'a>, usize) {
    assert_eq!(
        spelled_paths.len(),
        folded_paths.len(),
        "one spelled path per folded path"
    );
    let mut by_folded_prefix: PrefixSpellings<'a> = BTreeMap::new();
    let mut prefix_operations = 0usize;
    for (index, (spelled, components)) in spelled_paths.iter().zip(folded_paths).enumerate() {
        for (prefix_length, prefix) in proper_folded_prefixes(components) {
            prefix_operations += 1;
            by_folded_prefix
                .entry(prefix.to_vec())
                .or_default()
                .push((spelled_prefix(spelled, prefix_length), index));
        }
    }
    let expected_operations: usize = folded_paths
        .iter()
        .map(|components| components.len().saturating_sub(1))
        .sum();
    assert_eq!(
        prefix_operations, expected_operations,
        "one entry per proper prefix of each claim's folded path"
    );
    assert_eq!(
        by_folded_prefix.values().map(Vec::len).sum::<usize>(),
        prefix_operations,
        "every recorded prefix entry is kept"
    );
    (by_folded_prefix, prefix_operations)
}

/// Groups `folded_paths`' own indices by their folded path, so a later
/// equal-path collision is one map lookup rather than a pairwise
/// comparison. Returns the map alongside how many insertions it took.
fn index_by_folded_path(
    folded_paths: &[Vec<FoldedName>],
) -> (BTreeMap<Vec<FoldedName>, Vec<usize>>, usize) {
    let mut by_folded_path: BTreeMap<Vec<FoldedName>, Vec<usize>> = BTreeMap::new();
    for (index, components) in folded_paths.iter().enumerate() {
        by_folded_path
            .entry(components.clone())
            .or_default()
            .push(index);
    }
    assert_eq!(
        by_folded_path.values().map(Vec::len).sum::<usize>(),
        folded_paths.len(),
        "every claim is grouped under exactly one folded path"
    );
    assert!(
        by_folded_path.len() <= folded_paths.len(),
        "grouping can only merge claims together, never invent new ones"
    );
    (by_folded_path, folded_paths.len())
}

/// Unions every pair of claims that collide — equal folded paths directly,
/// a folded ancestor relationship through a prefix lookup, and a shared
/// directory spelled two ways through the prefix spellings — into
/// `parent`'s union-find forest. Returns how many map lookups it performed.
fn union_collisions(
    folded_paths: &[Vec<FoldedName>],
    by_folded_path: &BTreeMap<Vec<FoldedName>, Vec<usize>>,
    by_folded_prefix: &PrefixSpellings<'_>,
    parent: &mut [usize],
) -> usize {
    assert_eq!(
        folded_paths.len(),
        parent.len(),
        "the union-find forest has one slot per claim"
    );

    // Two or more claims whose folded paths are equal collide directly —
    // this covers an identical path, a case-only difference and a
    // Unicode-normalization-only difference all at once, since all three
    // fold to the same sequence of components.
    for indices in by_folded_path.values() {
        for pair in indices.windows(2) {
            union(parent, pair[0], pair[1]);
        }
    }

    // A claim whose folded path has a proper prefix that is itself another
    // claim's whole folded path collides with that claim: the shorter path
    // names a directory the longer one sits inside.
    let mut lookup_operations = 0usize;
    for (index, components) in folded_paths.iter().enumerate() {
        for (_, prefix) in proper_folded_prefixes(components) {
            lookup_operations += 1;
            if let Some(ancestors) = by_folded_path.get(prefix) {
                for &ancestor_index in ancestors {
                    union(parent, index, ancestor_index);
                }
            }
        }
    }
    let expected_lookup_operations: usize = folded_paths
        .iter()
        .map(|components| components.len().saturating_sub(1))
        .sum();
    assert_eq!(
        lookup_operations, expected_lookup_operations,
        "one lookup per proper prefix of each claim's folded path"
    );

    union_shared_directories(by_folded_prefix, parent);
    lookup_operations
}

/// Unions every claim under a folded directory that two or more claims
/// spell differently. A directory every claim under it spells the same way
/// is one directory on any filesystem, so it unions nothing: two worn
/// skeletons claiming `a/x` and `a/y` stay independent.
fn union_shared_directories(by_folded_prefix: &PrefixSpellings<'_>, parent: &mut [usize]) {
    for entries in by_folded_prefix.values() {
        let Some(&(first_spelling, first_index)) = entries.first() else {
            unreachable!("a prefix entry is only created by pushing a claim onto it")
        };
        let spelled_two_ways = entries
            .iter()
            .any(|&(spelling, _)| spelling != first_spelling);
        if spelled_two_ways {
            for &(_, index) in entries {
                union(parent, first_index, index);
            }
        }
    }
}

/// Splits `claims` into the ones whose union-find root is unique (`kept`)
/// and the groups that share a root with at least one other claim
/// (`overlaps`), using the forest [`union_collisions`] already built.
fn partition_by_group(claims: Vec<Claim>, mut parent: Vec<usize>) -> (Vec<Claim>, Vec<Overlap>) {
    let total_claims = claims.len();
    assert_eq!(
        parent.len(),
        total_claims,
        "the union-find forest has one slot per claim"
    );

    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); total_claims];
    for index in 0..total_claims {
        let root = find_root(&mut parent, index);
        grouped[root].push(index);
    }

    let mut kept = Vec::new();
    let mut overlaps = Vec::new();
    let mut slots: Vec<Option<Claim>> = claims.into_iter().map(Some).collect();
    for indices in grouped {
        match indices.as_slice() {
            [] => {}
            [only] => {
                if let Some(claim) = slots[*only].take() {
                    kept.push(claim);
                }
            }
            many => {
                let mut paths = Vec::new();
                let mut claimants = Vec::new();
                for &index in many {
                    if let Some(claim) = slots[index].take() {
                        paths.push(claim.path);
                        claimants.push(claim.claimant);
                    }
                }
                paths.sort();
                paths.dedup();
                overlaps.push(Overlap { paths, claimants });
            }
        }
    }
    assert_eq!(
        kept.len()
            + overlaps
                .iter()
                .map(|overlap| overlap.claimants.len())
                .sum::<usize>(),
        total_claims,
        "every claim ends up kept exactly once or inside exactly one overlap"
    );
    (kept, overlaps)
}

/// Finds `index`'s own root in the union-find forest `parent` encodes,
/// flattening every node visited along the way onto its own grandparent (path
/// halving) as it goes, so the forest stays close to flat across repeated calls
/// without a second compression pass. Iterative, not recursive: the forest
/// holds one entry per bone, and nothing bounds how many bones a skeleton or a
/// workspace's worn skeletons together may claim, so an unbounded call depth
/// here would be an unbounded stack depth too.
fn find_root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let left_root = find_root(parent, left);
    let right_root = find_root(parent, right);
    if left_root != right_root {
        parent[left_root] = right_root;
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        Claim, ClaimPath, Claimant, FoldedRelation, find_overlaps,
        find_overlaps_and_count_map_operations, folded_relation,
    };
    use crate::skeleton::FoldedName;

    fn claimant(dependency: &str) -> Claimant {
        Claimant {
            manifest: "Cargo.toml".to_owned(),
            dependency: dependency.to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        }
    }

    fn claim(path: &str, dependency: &str) -> Claim {
        Claim {
            path: ClaimPath::from_rendering_path(path).expect("valid test path"),
            claimant: claimant(dependency),
            rendered: Vec::new(),
        }
    }

    #[test]
    fn claims_with_distinct_unrelated_paths_never_overlap() {
        let (kept, overlaps) = find_overlaps(vec![claim("a.txt", "one"), claim("b.txt", "two")]);
        assert_eq!(kept.len(), 2);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn two_claims_at_the_same_exact_path_overlap() {
        let (kept, overlaps) = find_overlaps(vec![claim("a.txt", "one"), claim("a.txt", "two")]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].paths.len(), 1);
        assert_eq!(overlaps[0].claimants.len(), 2);
    }

    #[test]
    fn two_claims_differing_only_in_case_overlap() {
        let (kept, overlaps) = find_overlaps(vec![
            claim(".github/Dependabot.yml", "one"),
            claim(".github/dependabot.yml", "two"),
        ]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(
            overlaps[0].paths.len(),
            2,
            "the two distinct spellings are both named"
        );
    }

    #[test]
    fn a_claim_that_is_an_ancestor_directory_of_another_overlaps() {
        let (kept, overlaps) = find_overlaps(vec![claim("ci", "one"), claim("ci/x.yml", "two")]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn a_sibling_sharing_a_string_prefix_does_not_overlap() {
        let (kept, overlaps) = find_overlaps(vec![claim("ci", "one"), claim("cix", "two")]);
        assert_eq!(kept.len(), 2);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn three_claims_at_one_path_are_one_overlap_naming_all_three() {
        let (kept, overlaps) = find_overlaps(vec![
            claim("a.txt", "one"),
            claim("a.txt", "two"),
            claim("a.txt", "three"),
        ]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].claimants.len(), 3);
    }

    #[test]
    fn an_overlapping_pair_does_not_hide_an_unrelated_claim() {
        let (kept, overlaps) = find_overlaps(vec![
            claim("a.txt", "one"),
            claim("a.txt", "two"),
            claim("unrelated.txt", "three"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path.as_str(), "unrelated.txt");
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn nfc_and_nfd_spellings_of_one_path_overlap() {
        // "café.txt" written precomposed (NFC) against the same visible name
        // written as "cafe" followed by a combining acute accent (NFD): two
        // distinct byte strings that a case-insensitive, normalization-
        // insensitive filesystem (a default macOS volume) stores as the one
        // file, exactly the collision `skeleton::folding` already refuses
        // between two names inside a single skeleton. Neither differs from
        // the other in case, and neither is an ancestor of the other, so
        // this exercises the fold itself rather than a comparison of case or
        // of ancestor directories.
        let (kept, overlaps) = find_overlaps(vec![
            claim("caf\u{e9}.txt", "one"),
            claim("cafe\u{301}.txt", "two"),
        ]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(
            overlaps[0].paths.len(),
            2,
            "the two distinct byte spellings are both named"
        );
    }

    fn path(text: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(text).expect("valid test path")
    }

    #[test]
    fn folded_relation_is_same_path_for_a_case_only_difference() {
        assert_eq!(
            folded_relation(
                &path(".github/Dependabot.yml"),
                &path(".github/dependabot.yml")
            ),
            FoldedRelation::SamePath
        );
    }

    #[test]
    fn folded_relation_is_same_path_for_an_nfc_nfd_difference() {
        // Same NFC/NFD pair as `nfc_and_nfd_spellings_of_one_path_overlap`:
        // two byte strings that fold to the same components without either
        // differing from the other in ASCII+Unicode case.
        assert_eq!(
            folded_relation(&path("caf\u{e9}.txt"), &path("cafe\u{301}.txt")),
            FoldedRelation::SamePath
        );
    }

    #[test]
    fn folded_relation_is_nested_for_a_folded_ancestor_directory() {
        assert_eq!(
            folded_relation(&path("ci"), &path("ci/x.yml")),
            FoldedRelation::Nested
        );
        assert_eq!(
            folded_relation(&path("CI/x.yml"), &path("ci")),
            FoldedRelation::Nested
        );
    }

    #[test]
    fn folded_relation_names_a_shared_directory_each_path_spells_differently() {
        assert_eq!(
            folded_relation(&path("a/x"), &path("A/y")),
            FoldedRelation::SharedDirectorySpelledDifferently {
                first_at: "a".to_owned(),
                second_at: "A".to_owned(),
            }
        );
    }

    #[test]
    fn folded_relation_names_the_shortest_prefix_where_the_spellings_differ() {
        // `a/b` is spelled the same by both, so the first difference is one
        // level down, at `a/b/c` against `a/b/C`, and not at the deeper
        // `a/b/c/d` against `a/b/C/d` that also differs.
        assert_eq!(
            folded_relation(&path("a/b/c/d/x"), &path("a/b/C/d/y")),
            FoldedRelation::SharedDirectorySpelledDifferently {
                first_at: "a/b/c".to_owned(),
                second_at: "a/b/C".to_owned(),
            }
        );
    }

    #[test]
    #[should_panic(expected = "already reported as one overlap")]
    fn folded_relation_panics_on_two_paths_that_do_not_collide_at_all() {
        folded_relation(&path("a.txt"), &path("b.txt"));
    }

    #[test]
    #[should_panic(expected = "already reported as one overlap")]
    fn folded_relation_panics_on_two_paths_sharing_a_directory_spelled_the_same_way() {
        folded_relation(&path("a/x"), &path("a/y"));
    }

    #[test]
    fn two_claims_under_one_directory_spelled_two_ways_overlap() {
        let (kept, overlaps) = find_overlaps(vec![claim("a/x", "one"), claim("A/y", "two")]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].paths.len(), 2, "both spellings are named");
        assert_eq!(overlaps[0].claimants.len(), 2);
    }

    #[test]
    fn two_claims_under_one_identically_spelled_directory_do_not_overlap() {
        let (kept, overlaps) = find_overlaps(vec![claim("a/x", "one"), claim("a/y", "two")]);
        assert_eq!(kept.len(), 2);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn a_shared_directory_spelled_two_ways_chains_a_third_claim_into_the_group() {
        // `a/x` and `A/y` collide through the directory `a`. `a/q/z` spells
        // that directory the same way as `a/x` does, and would collide
        // with nothing on its own; it joins the group because the one
        // folded directory it sits under is spelled two ways by the others.
        let (kept, overlaps) = find_overlaps(vec![
            claim("a/x", "one"),
            claim("A/y", "two"),
            claim("a/q/z", "three"),
            claim("unrelated/x", "four"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path.as_str(), "unrelated/x");
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].claimants.len(), 3);
        assert_eq!(overlaps[0].paths.len(), 3);
    }

    #[test]
    fn two_directory_spellings_joined_through_a_deeper_clash_are_one_group() {
        // `a/b/x` against `a/B/y` share `a/b`; `a/B/y` against `A/c/z`
        // share `a`. Neither of the outer two collides with the other
        // directly at the same level, yet all three are one group.
        let (kept, overlaps) = find_overlaps(vec![
            claim("a/b/x", "one"),
            claim("a/B/y", "two"),
            claim("A/c/z", "three"),
        ]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].claimants.len(), 3);
    }

    #[test]
    fn a_deep_shared_directory_spelled_two_ways_overlaps() {
        let (kept, overlaps) = find_overlaps(vec![claim("a/b/x", "one"), claim("a/B/y", "two")]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn a_normalization_only_difference_in_a_shared_directory_overlaps() {
        // The directory `café`, precomposed (NFC) for one claim and
        // decomposed (NFD) for the other: no case differs, so this proves
        // the shared-directory relation uses the render's own fold and not
        // a case-only one.
        let (kept, overlaps) = find_overlaps(vec![
            claim("caf\u{e9}/x", "one"),
            claim("cafe\u{301}/y", "two"),
        ]);
        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn a_directory_spelled_two_ways_beside_an_unrelated_sibling_does_not_hide_it() {
        let (kept, overlaps) = find_overlaps(vec![
            claim("ci/x", "one"),
            claim("CI/y", "two"),
            claim("cix/z", "three"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path.as_str(), "cix/z");
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn map_operations_grow_linearly_with_claim_count() {
        // At depth 1 (a flat file per claim, no `/`), the algorithm performs
        // exactly one `BTreeMap` insertion per claim and no prefix work at
        // all — so the operation count is exactly `n`. At depth 3 every
        // claim also has two proper prefixes, each one prefix-spelling
        // insertion and one ancestor lookup, so the count is exactly `5n`.
        // Quadrupling the claim count quadruples the count either way; a
        // pairwise comparison of claims would make it sixteen times as large
        // instead. This is a count, not a wall-clock timing, so the
        // assertion holds on any machine.
        fn flat_claims(count: usize) -> Vec<Claim> {
            (0..count)
                .map(|index| claim(&format!("file-{index}.txt"), &format!("dep-{index}")))
                .collect()
        }

        // Every claim shares the directories `d` and `d/e`, spelled the
        // same way, so the prefix maps hold a single entry of `n` claims
        // each and nothing collides.
        fn deep_claims(count: usize) -> Vec<Claim> {
            (0..count)
                .map(|index| claim(&format!("d/e/file-{index}.txt"), &format!("dep-{index}")))
                .collect()
        }

        let (_, _, flat_small) = find_overlaps_and_count_map_operations(flat_claims(1_000));
        let (_, _, flat_large) = find_overlaps_and_count_map_operations(flat_claims(4_000));
        assert_eq!(flat_small, 1_000, "one insertion per claim at depth 1");
        assert_eq!(flat_large, 4_000, "one insertion per claim at depth 1");
        assert_eq!(
            flat_large,
            flat_small * 4,
            "four times the claims is four times the map operations, not sixteen"
        );

        let (kept_small, overlaps_small, deep_small) =
            find_overlaps_and_count_map_operations(deep_claims(1_000));
        let (kept_large, overlaps_large, deep_large) =
            find_overlaps_and_count_map_operations(deep_claims(4_000));
        assert_eq!(
            kept_small.len(),
            1_000,
            "one spelling collides with nothing"
        );
        assert_eq!(
            kept_large.len(),
            4_000,
            "one spelling collides with nothing"
        );
        assert!(overlaps_small.is_empty());
        assert!(overlaps_large.is_empty());
        assert_eq!(
            deep_small, 5_000,
            "one insertion, two prefix entries and two lookups per depth-3 claim"
        );
        assert_eq!(
            deep_large, 20_000,
            "one insertion, two prefix entries and two lookups per depth-3 claim"
        );
        assert_eq!(
            deep_large,
            deep_small * 4,
            "four times the claims is four times the map operations, not sixteen"
        );
    }

    #[test]
    fn a_shared_directory_spelled_two_ways_costs_no_more_than_one_spelling_check_per_prefix() {
        // Every claim under `d` spells it the same way but one, so the whole
        // set unions through one prefix entry. The count is the same `5n`
        // as when nothing collides, and the result is one group of `n`, so
        // the shared-directory pass is not a pairwise comparison.
        let mut claims: Vec<Claim> = (0..1_000)
            .map(|index| claim(&format!("d/e/file-{index}.txt"), &format!("dep-{index}")))
            .collect();
        claims.push(claim("D/e/other.txt", "odd-one-out"));

        let (kept, overlaps, operations) = find_overlaps_and_count_map_operations(claims);

        assert!(kept.is_empty());
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].claimants.len(), 1_001);
        assert_eq!(operations, 1_001 * 5);
    }

    proptest! {
        /// Overlap is symmetric: shuffling the input claims never changes
        /// how many end up kept versus grouped into an overlap, which would
        /// not hold if the folded-path grouping were accidentally
        /// order-sensitive. Each claim keeps its own claimant through the
        /// shuffle, so the two runs are given the same claims in a different
        /// order. The four paths are case variants of two files under one
        /// directory, so the folded grouping and the shared-directory
        /// grouping are both exercised, not only byte-equal paths.
        #[test]
        fn overlap_count_is_independent_of_input_order(
            (given, shuffled) in prop::collection::vec(0u8..4, 2..8).prop_flat_map(|seed| {
                let buckets: Vec<(usize, u8)> = seed.into_iter().enumerate().collect();
                (Just(buckets.clone()), Just(buckets).prop_shuffle())
            })
        ) {
            const PATHS: [&str; 4] = ["a/x", "A/x", "a/y", "A/y"];
            let claims_of = |buckets: &[(usize, u8)]| -> Vec<Claim> {
                buckets
                    .iter()
                    .map(|(index, bucket)| {
                        claim(PATHS[usize::from(*bucket)], &format!("dep-{index}"))
                    })
                    .collect()
            };

            let (given_kept, given_overlaps) = find_overlaps(claims_of(&given));
            let (shuffled_kept, shuffled_overlaps) = find_overlaps(claims_of(&shuffled));

            prop_assert_eq!(given_kept.len(), shuffled_kept.len());
            prop_assert_eq!(given_overlaps.len(), shuffled_overlaps.len());
        }

        /// Removing any one claim never leaves a dangling reference: every
        /// remaining claim is accounted for, in `kept` or in an overlap that
        /// names both its path and its claimant, and no path or claimant
        /// comes back that the remaining claims did not give.
        #[test]
        fn removing_any_claimant_leaves_a_consistent_result(
            bucket_count in 1usize..4,
            claims_per_bucket in 1usize..4,
            remove_index in 0usize..12,
        ) {
            let mut claims = Vec::new();
            for bucket in 0..bucket_count {
                for copy in 0..claims_per_bucket {
                    claims.push(claim(&format!("path-{bucket}"), &format!("dep-{bucket}-{copy}")));
                }
            }
            if claims.is_empty() {
                return Ok(());
            }
            let remove_index = remove_index % claims.len();
            claims.remove(remove_index);

            let given: Vec<(ClaimPath, Claimant)> = claims
                .iter()
                .map(|claim| (claim.path.clone(), claim.claimant.clone()))
                .collect();
            let (kept, overlaps) = find_overlaps(claims);

            let overlapped: usize = overlaps.iter().map(|overlap| overlap.claimants.len()).sum();
            prop_assert_eq!(given.len(), kept.len() + overlapped);
            for (path, claimant) in &given {
                let is_kept = kept
                    .iter()
                    .any(|claim| &claim.path == path && &claim.claimant == claimant);
                let is_overlapped = overlaps.iter().any(|overlap| {
                    overlap.paths.contains(path) && overlap.claimants.contains(claimant)
                });
                prop_assert!(
                    is_kept || is_overlapped,
                    "{path:?} claimed by {claimant:?} is in neither `kept` nor an overlap"
                );
            }
            for overlap in &overlaps {
                for path in &overlap.paths {
                    prop_assert!(
                        given.iter().any(|(given_path, _)| given_path == path),
                        "an overlap names {path:?}, which no remaining claim gave"
                    );
                }
                for claimant in &overlap.claimants {
                    prop_assert!(
                        given.iter().any(|(_, given_claimant)| given_claimant == claimant),
                        "an overlap names {claimant:?}, which no remaining claim gave"
                    );
                }
            }
        }
    }

    /// `path`'s components under the render's own fold, from the raw
    /// spelling, so the checks below never lean on `folded_components`.
    fn folded(path: &str) -> Vec<FoldedName> {
        path.split('/').map(FoldedName::of).collect()
    }

    /// `path`'s first `count` components as `path` spells them, `/`-joined.
    fn spelled(path: &str, count: usize) -> String {
        path.split('/').take(count).collect::<Vec<_>>().join("/")
    }

    /// Whether the shorter of two folded paths is a proper prefix of the
    /// longer.
    fn one_folds_above_the_other(first: &[FoldedName], second: &[FoldedName]) -> bool {
        let (shorter, longer) = if first.len() <= second.len() {
            (first, second)
        } else {
            (second, first)
        };
        shorter.len() < longer.len() && longer.starts_with(shorter)
    }

    /// Whether two claim paths collide, decided straight from the
    /// definition: equal folded components, one a proper folded prefix of
    /// the other, or a folded proper prefix of both that they spell two
    /// ways.
    fn collides_by_definition(first: &str, second: &str) -> bool {
        let (first_folded, second_folded) = (folded(first), folded(second));
        if first_folded == second_folded {
            return true;
        }
        if one_folds_above_the_other(&first_folded, &second_folded) {
            return true;
        }
        let shared_limit = first_folded.len().min(second_folded.len());
        (1..shared_limit).any(|count| {
            first_folded[..count] == second_folded[..count]
                && spelled(first, count) != spelled(second, count)
        })
    }

    /// Whether `relation` is true of `first` and `second`, checked from the
    /// raw spellings alone.
    fn relation_holds(relation: &FoldedRelation, first: &str, second: &str) -> bool {
        let (first_folded, second_folded) = (folded(first), folded(second));
        let is_nested = one_folds_above_the_other(&first_folded, &second_folded);
        match relation {
            FoldedRelation::SamePath => first_folded == second_folded,
            FoldedRelation::Nested => is_nested,
            FoldedRelation::SharedDirectorySpelledDifferently {
                first_at,
                second_at,
            } => {
                first_folded != second_folded
                    && !is_nested
                    && first_at != second_at
                    && folded(first_at) == folded(second_at)
                    && first.starts_with(&format!("{first_at}/"))
                    && second.starts_with(&format!("{second_at}/"))
            }
        }
    }

    /// A path of one to three components drawn from an alphabet chosen to
    /// collide: two cases of one letter, a second letter, and one letter
    /// spelled precomposed and decomposed.
    fn colliding_path() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec!["a", "A", "b", "\u{e9}", "e\u{301}"]),
            1..=3,
        )
        .prop_map(|components| components.join("/"))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// For two claims over paths built to collide (nested, case-varied
        /// and normalisation-varied), `find_overlaps` groups them exactly
        /// when they collide by the definition, and when the group holds two
        /// distinct paths `folded_relation` neither panics nor names a
        /// relation that does not hold. The same run checks the converse:
        /// two claims left apart collide by no relation at all.
        #[test]
        fn find_overlaps_and_folded_relation_agree_on_every_pair(
            first in colliding_path(),
            second in colliding_path(),
        ) {
            let (kept, overlaps) = find_overlaps(vec![claim(&first, "one"), claim(&second, "two")]);
            let collides = collides_by_definition(&first, &second);

            prop_assert_eq!(!overlaps.is_empty(), collides, "{:?} and {:?}", first, second);
            prop_assert_eq!(kept.is_empty(), collides);
            let distinct_pair = match overlaps.as_slice() {
                [overlap] => match overlap.paths.as_slice() {
                    [left, right] => Some((left, right)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((left, right)) = distinct_pair {
                let relation = folded_relation(left, right);
                prop_assert!(
                    relation_holds(&relation, left.as_str(), right.as_str()),
                    "{:?} does not hold of {:?} and {:?}",
                    relation,
                    left,
                    right
                );
            }
        }
    }
}
