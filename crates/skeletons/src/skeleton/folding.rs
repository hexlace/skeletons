//! When two entry names are one name: the rule a filesystem that ignores
//! case and Unicode normalization applies, written once.
//!
//! Supported platforms disagree about names. A default macOS volume stores
//! `Dependabot.yml` and `dependabot.yml` as one entry, and an `é` written
//! precomposed (NFC) as the same entry as one written as `e` plus a
//! combining accent (NFD); a Linux filesystem, case-sensitive and
//! byte-exact, stores four. A skeleton that ships two such names renders two
//! files on one machine and one on the other, so the walk refuses them, and
//! this module decides what "such names" means.
//!
//! The rule is the Unicode Standard's canonical caseless match (section
//! 3.13, D145): decompose (NFD), apply full default case folding, and
//! decompose again, since folding can produce text that is no longer
//! decomposed. A case-insensitive APFS volume agrees with it on every pair
//! measured, and Linux's optional case-insensitive directories apply it too,
//! except that those also ignore default-ignorable code points such as the
//! soft hyphen.
//! It is deliberately not compatibility matching (NFKD): `①` and `1` are
//! different names to every supported filesystem.
//! And it applies no language-specific folding: Turkish `İ` and `ı` fold
//! with nothing but themselves, as they do on every supported filesystem.

use caseless::Caseless as _;
use unicode_normalization::UnicodeNormalization as _;

/// An entry name as a filesystem that ignores case and Unicode
/// normalization compares it: two names are one name to such a filesystem
/// exactly when their folded names are equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FoldedName(String);

impl FoldedName {
    /// Folds `name`: canonical decomposition, full default case folding,
    /// then canonical decomposition again.
    ///
    /// An ASCII name takes a shorter road to the same answer: decomposition
    /// changes nothing in ASCII, and full default case folding maps exactly
    /// `A` to `Z` onto `a` to `z` there and nothing else, so folding it is
    /// lowercasing it, with no allocation beyond the result. Most names in a
    /// large directory are ASCII, and this is the road each one takes when a
    /// walk compares every entry of it. The two roads agree on every ASCII
    /// name (`an_ascii_name_folds_as_the_full_rule_folds_it`, below).
    pub(crate) fn of(name: &str) -> Self {
        let folded: String = if name.is_ascii() {
            name.to_ascii_lowercase()
        } else {
            name.chars().nfd().default_case_fold().nfd().collect()
        };

        // Postconditions: folding never turns a name into nothing or
        // something out of nothing, and what it returns is itself fully
        // decomposed — the last step of the rule, checked rather than
        // trusted.
        assert_eq!(
            name.is_empty(),
            folded.is_empty(),
            "folding {name:?} must neither empty it nor fill it"
        );
        assert!(
            unicode_normalization::is_nfd(&folded),
            "a folded name is always in canonical decomposition: {folded:?}"
        );
        Self(folded)
    }
}

/// Whether `first` and `second` are one name to a filesystem that ignores
/// case and Unicode normalization.
pub(crate) fn names_collide(first: &str, second: &str) -> bool {
    ascii_names_collide(first, second)
        .unwrap_or_else(|| FoldedName::of(first) == FoldedName::of(second))
}

/// The answer to [`names_collide`] for two ASCII names, without folding
/// either, or `None` when either is not ASCII and only a fold can say.
///
/// Two ASCII names are one name exactly when they are equal apart from ASCII
/// case, which is what folding both would decide. This is the one place that
/// road is decided: [`names_collide`] takes it, and so does a directory scan
/// that has folded its component once and compares every entry against it.
pub(crate) fn ascii_names_collide(first: &str, second: &str) -> Option<bool> {
    (first.is_ascii() && second.is_ascii()).then(|| first.eq_ignore_ascii_case(second))
}

/// The rule as written out in full, with no shortcut: the oracle the ASCII
/// fast paths, here and in the claim walk's directory scan, are held to.
#[cfg(test)]
pub(crate) fn slow_fold(name: &str) -> String {
    name.chars().nfd().default_case_fold().nfd().collect()
}

/// A name of `lengths` characters built from ones that fold interestingly:
/// ASCII in both cases, the letters the special folds land on, and the
/// singleton, the ligature and the accent that stand in for them.
#[cfg(test)]
pub(crate) fn tricky_name(
    lengths: std::ops::Range<usize>,
) -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::prelude::*;
    proptest::collection::vec(
        prop_oneof![
            Just('a'),
            Just('A'),
            Just('k'),
            Just('K'),
            Just('s'),
            Just('S'),
            Just('f'),
            Just('i'),
            Just('.'),
            Just('e'),
            Just('E'),
            Just('\u{e9}'),
            Just('\u{301}'),
            Just('\u{212a}'),
            Just('\u{17f}'),
            Just('\u{fb01}'),
            Just('\u{df}'),
        ],
        lengths,
    )
    .prop_map(|characters| characters.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use unicode_normalization::UnicodeNormalization as _;

    use super::{FoldedName, names_collide, slow_fold, tricky_name};

    /// `(first, second, collide)`: pairs of names, and whether a default
    /// macOS volume — case-insensitive and normalization-insensitive —
    /// stores them as one entry. Every row was written to such a volume to
    /// confirm it; `folding_agrees_with_this_filesystem` below repeats that
    /// against whatever filesystem the tests run on.
    const PAIRS: &[(&str, &str, bool)] = &[
        // Case alone.
        ("Dependabot.yml", "dependabot.yml", true),
        // NFC `é` against NFD `e` + U+0301.
        ("\u{e9}.txt", "e\u{301}.txt", true),
        // Case and normalization at once: NFD `A` + ring against NFC `å`.
        ("A\u{30a}", "\u{e5}", true),
        // A canonical singleton: the Angstrom sign decomposes to `Å`.
        ("\u{c5}", "\u{212b}", true),
        // Full folding, not simple: `ß` folds to `ss`.
        ("stra\u{df}e", "strasse", true),
        ("\u{df}", "\u{1e9e}", true),
        ("\u{3c2}", "\u{3c3}", true),
        ("\u{17f}", "s", true),
        ("\u{212a}", "k", true),
        ("\u{fb01}", "fi", true),
        ("\u{130}", "i\u{307}", true),
        // Folding produces text needing decomposition again.
        ("\u{1f80}", "\u{1f08}\u{345}", true),
        ("\u{13a0}", "\u{ab70}", true),
        // Different names on every supported filesystem.
        ("dependabot.yml", "dependabot.yaml", false),
        ("\u{e9}.txt", "e.txt", false),
        ("\u{2460}", "1", false),
        ("\u{130}", "i", false),
        ("\u{131}", "i", false),
    ];

    #[test]
    fn nfc_and_nfd_spellings_of_one_name_collide() {
        // The same visible name, `é.txt`, written precomposed and
        // decomposed: two byte strings a Linux filesystem keeps apart and a
        // default macOS volume stores as one entry.
        assert!(names_collide("\u{e9}.txt", "e\u{301}.txt"));
    }

    #[test]
    fn names_differing_only_in_case_collide() {
        assert!(names_collide("Dependabot.yml", "dependabot.yml"));
        assert!(names_collide("CARGO.TOML", "Cargo.toml"));
    }

    #[test]
    fn names_differing_in_more_than_case_and_normalization_do_not_collide() {
        // One letter apart, an accent apart, and a compatibility-only
        // equivalence (`①` against `1`): every supported filesystem keeps
        // each pair apart, so the render must too.
        assert!(!names_collide("dependabot.yml", "dependabot.yaml"));
        assert!(!names_collide("\u{e9}.txt", "e.txt"));
        assert!(!names_collide("\u{2460}", "1"));
    }

    #[test]
    fn every_recorded_pair_folds_as_a_default_macos_volume_stores_it() {
        for &(first, second, collide) in PAIRS {
            assert_eq!(
                names_collide(first, second),
                collide,
                "{first:?} and {second:?} must {} under the folding rule",
                if collide { "collide" } else { "stay apart" }
            );
        }
    }

    /// A fresh directory for one probe of the running filesystem, removed
    /// by the caller.
    fn scratch_directory() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "skeletons-folding-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        directory
    }

    /// Whether the filesystem holding `directory` stores `first` and
    /// `second` as one entry: `second` is written after `first`, and one
    /// entry is left where two names were written.
    fn filesystem_stores_as_one(directory: &std::path::Path, first: &str, second: &str) -> bool {
        std::fs::write(directory.join(first), b"first").expect("write the first name");
        std::fs::write(directory.join(second), b"second").expect("write the second name");
        let entries = std::fs::read_dir(directory)
            .expect("list the probe directory")
            .count();
        entries == 1
    }

    #[test]
    fn folding_agrees_with_this_filesystem() {
        // The filesystem the tests run on is an oracle for the rule. Each
        // pair is written into a directory of its own; wherever the
        // filesystem stores the two names as one entry, the rule must say
        // they collide — a render there would otherwise ship one file where
        // the skeleton meant two. On a filesystem that ignores both case and
        // normalization (a default macOS volume), the converse holds too:
        // the rule must never refuse a pair that filesystem keeps apart. On
        // a case-sensitive, byte-exact filesystem nothing is stored as one,
        // and the recorded table above carries the check instead.
        let ignores_case_and_normalization = {
            let case = scratch_directory();
            let normalization = scratch_directory();
            let ignores = filesystem_stores_as_one(&case, "A", "a")
                && filesystem_stores_as_one(&normalization, "\u{e9}", "e\u{301}");
            std::fs::remove_dir_all(&case).expect("clean up");
            std::fs::remove_dir_all(&normalization).expect("clean up");
            ignores
        };

        for &(first, second, _recorded) in PAIRS {
            let directory = scratch_directory();
            let stored_as_one = filesystem_stores_as_one(&directory, first, second);
            std::fs::remove_dir_all(&directory).expect("clean up");

            if stored_as_one {
                assert!(
                    names_collide(first, second),
                    "this filesystem stores {first:?} and {second:?} as one entry, \
                     so the rule must say they collide"
                );
            }
            if ignores_case_and_normalization {
                assert_eq!(
                    names_collide(first, second),
                    stored_as_one,
                    "{first:?} and {second:?}: the rule must match a filesystem that ignores \
                     case and normalization exactly"
                );
            }
        }
    }

    proptest! {
        #[test]
        fn an_ascii_name_folds_as_the_full_rule_folds_it(name in "[\\x00-\\x7f]{0,24}") {
            // Property: the ASCII road is an optimisation and never an
            // opinion. Over every ASCII string, control characters included,
            // it gives what decomposing, case folding and decomposing again
            // gives.
            prop_assert_eq!(FoldedName::of(&name).0, slow_fold(&name));
        }

        #[test]
        fn colliding_gives_the_answer_folding_both_names_gives(
            first in tricky_name(0..6),
            second in tricky_name(0..6),
        ) {
            // Property: over names mixing ASCII with the characters the
            // recorded pairs turn on, `names_collide` agrees with comparing
            // the two folds in full, whichever road each name took.
            prop_assert_eq!(
                names_collide(&first, &second),
                slow_fold(&first) == slow_fold(&second),
                "{:?} and {:?}", first, second
            );
        }

        #[test]
        fn folding_a_folded_name_changes_nothing(name in any::<String>()) {
            // Property: the rule is a projection. A folded name is already
            // one of the names the rule sorts others into, so folding it
            // again gives it back unchanged — otherwise two names could
            // collide with a third without colliding with each other.
            let folded = FoldedName::of(&name);
            prop_assert_eq!(FoldedName::of(&folded.0), folded);
        }

        #[test]
        fn a_name_and_its_normalized_forms_collide(name in any::<String>()) {
            // Property: whichever canonical form a name is written in, it
            // is the same name.
            let composed: String = name.nfc().collect();
            let decomposed: String = name.nfd().collect();
            prop_assert!(names_collide(&name, &composed));
            prop_assert!(names_collide(&name, &decomposed));
        }

        #[test]
        fn a_name_and_its_ascii_case_variants_collide(name in "[A-Za-z0-9._-]{1,24}") {
            prop_assert!(names_collide(&name, &name.to_ascii_uppercase()));
            prop_assert!(names_collide(&name, &name.to_ascii_lowercase()));
        }
    }
}
