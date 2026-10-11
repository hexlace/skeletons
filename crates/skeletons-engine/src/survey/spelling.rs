//! Showing two spellings of one name so they can be told apart.
//!
//! `café.yml` written precomposed (NFC) and written as `e` plus a combining
//! accent (NFD) are two byte strings that display identically, so a message
//! that names both reads "rename café.yml to café.yml" and gives the reader
//! nothing to act on. When a message names two spellings that are canonically
//! equivalent but differ in bytes, every spelling it names is shown escaped as
//! any outside text is, followed by the same name with its non-ASCII characters
//! as `\uXXXX` code points in parentheses, which bash 4.3 or later, or zsh,
//! reads back as the same bytes in `$'…'` under a UTF-8 locale. The name
//! stays, so the reader recognises the file, and the code points are what tell
//! it from its twin. A name shown when nothing is confusable is only escaped.
//!
//! Only canonical equivalence looks identical, so only it escapes: a case
//! variant (`É` and `é`) and a full-fold variant (`ß` and `ss`) are visibly
//! different and stay readable. The Kelvin sign and `K`, and the Ångström sign
//! and `Å`, are canonically equivalent and do escape.
//!
//! `--json` never goes through this: it carries the exact strings.

use std::ops::Index;

use unicode_normalization::UnicodeNormalization as _;

use crate::skeleton::Escaped;

/// Why the code points are there, worded to follow the sentence that names
/// the spellings.
const NOTE: &str = "the two spellings differ only in Unicode normalization, so each is followed \
                    by its characters as \\uXXXX code points, as bash 4.3 or later, or zsh, \
                    reads them in $'…' under a UTF-8 locale";

/// The spellings a message names, ready to be shown: each [`Escaped`], and
/// each followed by its code points when two can be mistaken for one another,
/// except a name whose code points read the same as it does, which is shown as
/// escaped with nothing after it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Spellings {
    shown: Vec<String>,
    /// Whether two of the spellings can be mistaken for one another, so the
    /// names that read differently from their code points are followed by them.
    code_points_follow: bool,
}

impl Spellings {
    /// The note that says why the spellings are shown as code points, in
    /// parentheses and led by a space so it follows a clause directly: ``
    /// (the two spellings differ only in …)``. Empty when no spelling is
    /// followed by its code points.
    pub(crate) fn note_in_parentheses(&self) -> String {
        if self.code_points_follow {
            format!(" ({NOTE})")
        } else {
            String::new()
        }
    }

    /// The same note, led by a semicolon, for a clause that is itself inside
    /// parentheses. Empty when no spelling is followed by its code points.
    pub(crate) fn note_after_semicolon(&self) -> String {
        if self.code_points_follow {
            format!("; {NOTE}")
        } else {
            String::new()
        }
    }
}

impl Index<usize> for Spellings {
    type Output = str;

    /// The `index`th spelling handed to [`told_apart`], as it is to be shown.
    fn index(&self, index: usize) -> &str {
        &self.shown[index]
    }
}

/// Prepares `spellings`, every name one message shows, for showing.
///
/// Every name is returned [`Escaped`], so a message that names it stays on one
/// line. When any two are canonically equivalent (equal after NFC) but differ
/// in bytes, each is also followed, in parentheses, by the same name with each
/// non-ASCII character written as `\uXXXX` (`\UXXXXXXXX` above U+FFFF): the
/// name as escaped, then its code points. A name whose code points read the
/// same as it does is returned as escaped, with nothing after it.
pub(crate) fn told_apart(spellings: &[&str]) -> Spellings {
    let composed: Vec<String> = spellings.iter().map(|name| name.nfc().collect()).collect();
    let mut confusable = false;
    for (first, first_composed) in composed.iter().enumerate() {
        for (second, second_composed) in composed.iter().enumerate().skip(first + 1) {
            let equivalent = first_composed == second_composed;
            let same_bytes = spellings[first] == spellings[second];
            confusable |= equivalent && !same_bytes;
        }
    }
    Spellings {
        shown: spellings
            .iter()
            .map(|name| shown(name, confusable))
            .collect(),
        code_points_follow: confusable,
    }
}

/// `name` escaped, followed by its code points in parentheses when `confusable`
/// and they read differently from the escaped name.
fn shown(name: &str, confusable: bool) -> String {
    let escaped = Escaped(name).to_string();
    if !confusable {
        return escaped;
    }
    let code_points = Escaped(name).code_points().to_string();
    if code_points == escaped {
        escaped
    } else {
        format!("{escaped} ({code_points})")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use unicode_normalization::UnicodeNormalization as _;

    use super::{Escaped, NOTE, told_apart};

    #[test]
    fn a_composed_and_a_decomposed_spelling_are_each_followed_by_their_code_points() {
        let composed = "caf\u{e9}.yml";
        let decomposed = "cafe\u{301}.yml";

        let shown = told_apart(&[composed, decomposed]);

        assert_eq!(&shown[0], "caf\u{e9}.yml (caf\\u00E9.yml)");
        assert_eq!(&shown[1], "cafe\u{301}.yml (cafe\\u0301.yml)");
        assert!(shown.note_in_parentheses().contains("\\uXXXX"));
    }

    #[test]
    fn every_spelling_of_the_message_is_followed_when_any_two_are_confusable() {
        // The third name is not confusable with either, and is written out
        // too, so the message reads in one notation throughout. A name with
        // nothing to write out (here the ASCII one) is left alone.
        let shown = told_apart(&["caf\u{e9}.yml", "cafe\u{301}.yml", "d/\u{e9}", "d/x"]);

        assert_eq!(&shown[2], "d/\u{e9} (d/\\u00E9)");
        assert_eq!(&shown[3], "d/x");
    }

    #[test]
    fn the_kelvin_and_angstrom_signs_look_like_their_letters_and_are_written_out() {
        // U+212A and U+212B are canonical singletons: NFC turns them into `K`
        // and `Å`, so each pair is one name that looks like one.
        let kelvin = told_apart(&["\u{212a}.yml", "K.yml"]);
        assert_eq!(&kelvin[0], "\u{212a}.yml (\\u212A.yml)");
        assert_eq!(&kelvin[1], "K.yml");

        let angstrom = told_apart(&["\u{212b}", "\u{c5}"]);
        assert_eq!(&angstrom[0], "\u{212b} (\\u212B)");
        assert_eq!(&angstrom[1], "\u{c5} (\\u00C5)");
    }

    #[test]
    fn case_and_full_fold_variants_are_visibly_different_and_stay_readable() {
        for pair in [["\u{c9}.yml", "\u{e9}.yml"], ["stra\u{df}e", "strasse"]] {
            let shown = told_apart(&pair);
            assert_eq!(&shown[0], pair[0]);
            assert_eq!(&shown[1], pair[1]);
            assert_eq!(shown.note_in_parentheses(), "");
            assert_eq!(shown.note_after_semicolon(), "");
        }
    }

    #[test]
    fn identical_spellings_are_not_confusable_with_themselves() {
        let shown = told_apart(&["caf\u{e9}.yml", "caf\u{e9}.yml"]);

        assert_eq!(&shown[0], "caf\u{e9}.yml");
    }

    #[test]
    fn a_backslash_is_doubled_when_writing_out_so_a_code_point_is_never_ambiguous() {
        let shown = told_apart(&["a\\b\u{e9}", "a\\be\u{301}"]);

        assert_eq!(&shown[0], "a\\\\b\u{e9} (a\\\\b\\u00E9)");
        assert_eq!(&shown[1], "a\\\\be\u{301} (a\\\\be\\u0301)");
    }

    #[test]
    fn a_backslash_alone_is_left_as_it_is_when_nothing_is_confusable() {
        let shown = told_apart(&["a\\b", "a/b"]);

        assert_eq!(&shown[0], "a\\\\b");
    }

    #[test]
    fn a_character_above_the_basic_plane_uses_eight_digits() {
        // U+1D15E (a musical symbol) decomposes canonically and is excluded
        // from composition, so its NFC form is the two-character sequence.
        let shown = told_apart(&["\u{1d15e}", "\u{1d157}\u{1d165}"]);

        assert_eq!(&shown[0], "\u{1d15e} (\\U0001D15E)");
        assert_eq!(&shown[1], "\u{1d157}\u{1d165} (\\U0001D157\\U0001D165)");
    }

    #[test]
    fn an_ascii_newline_is_escaped_in_both_parts_of_a_confusable_pair() {
        let shown = told_apart(&["a\n\u{e9}", "a\ne\u{301}"]);

        assert_eq!(&shown[0], "a\\n\u{e9} (a\\n\\u00E9)");
        assert_eq!(&shown[1], "a\\ne\u{301} (a\\ne\\u0301)");
    }

    #[test]
    fn a_newline_in_a_pair_that_is_not_confusable_is_still_escaped() {
        let shown = told_apart(&["a\nb", "a/b"]);

        assert_eq!(&shown[0], "a\\nb");
        assert_eq!(&shown[1], "a/b");
        assert_eq!(shown.note_in_parentheses(), "");
    }

    #[test]
    fn a_leading_combining_accent_is_escaped_as_it_is_anywhere_else() {
        // An accent at the start of a name has nothing to combine with and
        // could be lost against what precedes the name in a message, so it is
        // escaped; the code-point form writes it the same way, and adds nothing.
        let shown = told_apart(&["\u{301}e", "x"]);

        assert_eq!(&shown[0], "\\u0301e");
        assert_eq!(&shown[1], "x");
    }

    #[test]
    fn the_two_notes_carry_the_same_words() {
        let shown = told_apart(&["\u{e9}", "e\u{301}"]);

        assert_eq!(shown.note_in_parentheses(), format!(" ({NOTE})"));
        assert_eq!(shown.note_after_semicolon(), format!("; {NOTE}"));
    }

    /// The property the proptest below states, as a function so inputs found
    /// to break it can be pinned as ordinary tests.
    ///
    /// A name set against its own decomposition is confusable exactly when
    /// the two differ in bytes. Then each is shown followed by its code
    /// points in parentheses, which are pure ASCII and differ between the
    /// two, since writing out never merges two names (a backslash is
    /// doubled, so an escape cannot be forged). A spelling with nothing to
    /// write out, one that is already ASCII, is shown escaped and is its own
    /// code points. When the two do not differ the name is shown escaped.
    fn assert_a_confusable_name_is_told_from_its_twin(name: &str) -> Result<(), TestCaseError> {
        let decomposed: String = name.nfd().collect();
        let shown = told_apart(&[name, decomposed.as_str()]);
        if name == decomposed {
            prop_assert_eq!(&shown[0], &Escaped(name).to_string());
            return Ok(());
        }
        let code_points = |spelling: &str, shown: &str| -> Option<String> {
            let escaped = Escaped(spelling).to_string();
            if shown == escaped {
                return Some(escaped);
            }
            shown
                .strip_prefix(&format!("{escaped} ("))
                .and_then(|rest| rest.strip_suffix(')'))
                .map(str::to_owned)
        };
        let first = code_points(name, &shown[0]).expect("the name, then its code points");
        let second = code_points(&decomposed, &shown[1]).expect("the name, then its code points");
        prop_assert!(first.is_ascii(), "{}", first);
        prop_assert!(second.is_ascii(), "{}", second);
        prop_assert_ne!(first, second);
        Ok(())
    }

    #[test]
    fn a_name_whose_decomposition_is_ascii_is_told_from_it() {
        // U+1FEF (GREEK VARIA) and U+037E (GREEK QUESTION MARK) are canonical
        // singletons of the ASCII "`" and ";", so the ASCII twin has no code
        // points to write out and is shown escaped, with nothing after it,
        // beside the other's.
        for (name, twin) in [("\u{1fef}", "`"), ("\u{37e}", ";")] {
            let shown = told_apart(&[name, twin]);

            assert_eq!(&shown[0], format!("{name} ({})", escape_of(name)));
            assert_eq!(&shown[1], twin);
            assert!(assert_a_confusable_name_is_told_from_its_twin(name).is_ok());
        }
    }

    /// `name`'s code points as `\uXXXX`, written out by hand for the two
    /// singletons above.
    fn escape_of(name: &str) -> String {
        format!(
            "\\u{:04X}",
            u32::from(name.chars().next().expect("one character"))
        )
    }

    proptest! {
        #[test]
        fn a_confusable_name_is_followed_by_ascii_code_points_that_tell_it_from_its_twin(
            name in "\\PC{0,12}",
        ) {
            // Property: see `assert_a_confusable_name_is_told_from_its_twin`.
            assert_a_confusable_name_is_told_from_its_twin(&name)?;
        }
    }
}
