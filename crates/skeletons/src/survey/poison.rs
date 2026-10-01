//! What every message test shares: text from outside with a real line break
//! in it, as a name in the wearer's repository can hold one, and the questions
//! a message is asked about it.
//!
//! The two halves of the poison are words that appear in no message, so an
//! occurrence of either in a message's output came from the poisoned text and
//! nowhere else. That is what lets one assertion count occurrences instead of
//! only looking for one.

use crate::claim::ClaimPath;

/// The first half of [`POISON`]: a word no message contains.
pub(crate) const POISON_HEAD: &str = "poison-head";

/// The second half of [`POISON`]: a word no message contains.
pub(crate) const POISON_TAIL: &str = "poison-tail";

/// Text from outside with a line break in it.
pub(crate) const POISON: &str = "poison-head\npoison-tail";

/// [`POISON`] as a message shows it: the line break written as the two
/// characters `\n`.
pub(crate) const POISON_ESCAPED: &str = "poison-head\\npoison-tail";

/// [`POISON`] spelled with its first half in capitals, as an index entry that
/// differs from a claimed name only in case.
pub(crate) const POISON_FOLDED: &str = "POISON-HEAD\npoison-tail";

/// [`POISON`], owned, for a field that holds a `String`.
pub(crate) fn poison() -> String {
    POISON.to_owned()
}

/// A claimed path holding `text`, which must be a well-formed path.
pub(crate) fn claim(text: &str) -> ClaimPath {
    ClaimPath::from_rendering_path(text).expect("a well-formed test path")
}

/// Asserts `text` is one line that shows the poisoned name escaped exactly
/// once wherever it shows it, and shows it at least once:
///
/// - no control character;
/// - [`POISON_ESCAPED`] is present;
/// - no doubled backslash, so nothing was escaped a second time;
/// - every occurrence of [`POISON_HEAD`] is followed by the rest of
///   [`POISON_ESCAPED`], so no occurrence shows the name raw or cut short.
pub(crate) fn assert_escaped_once(text: &str, what: &str) {
    assert_one_line(text, what);
    assert!(
        text.contains(POISON_ESCAPED),
        "{what} must show the name with its newline escaped: {text:?}"
    );
    assert!(
        !text.contains("\\\\"),
        "{what} must not escape the name twice: {text:?}"
    );
    for (position, _) in text.match_indices(POISON_HEAD) {
        assert!(
            text[position..].starts_with(POISON_ESCAPED),
            "{what} must show every occurrence of the name escaped: {text:?}"
        );
    }
}

/// Asserts `text` holds no control character, for a message that names no
/// outside text of its own.
pub(crate) fn assert_one_line(text: &str, what: &str) {
    assert!(
        !text.chars().any(char::is_control),
        "{what} must hold no control character: {text:?}"
    );
}

/// Asserts `kinds` covers every kind `0..count` of `what` and names none
/// outside it.
///
/// A message test lists a sample of every kind of an enum and maps each
/// sample to its kind through a `match` with no wildcard, so an enum that
/// gains a variant does not compile until it has a kind, and the sample list
/// must then reach it.
pub(crate) fn assert_every_kind(kinds: impl IntoIterator<Item = usize>, count: usize, what: &str) {
    let mut kinds: Vec<usize> = kinds.into_iter().collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(
        kinds,
        (0..count).collect::<Vec<_>>(),
        "the samples must cover every kind of {what}"
    );
}

#[cfg(test)]
mod tests {
    use super::{
        POISON, POISON_ESCAPED, POISON_FOLDED, POISON_HEAD, POISON_TAIL, assert_escaped_once,
        assert_every_kind, assert_one_line,
    };

    // The halves, the whole and its escaped form are spelled once each, so
    // the three must be checked against one another.
    #[test]
    fn the_poison_and_its_forms_are_built_from_the_same_two_halves() {
        assert_eq!(POISON, format!("{POISON_HEAD}\n{POISON_TAIL}"));
        assert_eq!(POISON_ESCAPED, format!("{POISON_HEAD}\\n{POISON_TAIL}"));
        assert_eq!(
            POISON_FOLDED,
            format!("{}\n{POISON_TAIL}", POISON_HEAD.to_uppercase())
        );
    }

    #[test]
    fn a_line_showing_the_name_escaped_once_or_several_times_passes() {
        assert_escaped_once("names poison-head\\npoison-tail", "one occurrence");
        assert_escaped_once(
            "poison-head\\npoison-tail then $'poison-head\\npoison-tail'",
            "two occurrences",
        );
    }

    // Each of the four properties is broken on its own, and the last two
    // also by leaving the name out (the one thing a search cannot find
    // cannot be counted either).
    #[test]
    #[should_panic(expected = "must hold no control character")]
    fn a_raw_line_break_fails() {
        assert_escaped_once("poison-head\npoison-tail poison-head\\npoison-tail", "raw");
    }

    #[test]
    #[should_panic(expected = "must show the name with its newline escaped")]
    fn a_message_without_the_name_fails() {
        assert_escaped_once("names nothing from outside", "absent");
    }

    #[test]
    #[should_panic(expected = "must not escape the name twice")]
    fn a_doubled_backslash_fails() {
        assert_escaped_once(
            "poison-head\\npoison-tail and poison-head\\\\npoison-tail",
            "doubled",
        );
    }

    #[test]
    #[should_panic(expected = "must show every occurrence of the name escaped")]
    fn one_occurrence_cut_short_fails() {
        assert_escaped_once(
            "poison-head\\npoison-tail and poison-head alone",
            "cut short",
        );
    }

    #[test]
    #[should_panic(expected = "must hold no control character")]
    fn a_control_character_fails_the_one_line_check() {
        assert_one_line("a\tb", "tab");
    }

    #[test]
    fn a_line_with_no_control_character_passes_the_one_line_check() {
        assert_one_line("a b", "space");
    }

    #[test]
    fn every_kind_in_any_order_and_repeated_is_covered() {
        assert_every_kind([2, 0, 1, 1, 0], 3, "test");
    }

    #[test]
    #[should_panic(expected = "the samples must cover every kind of test")]
    fn a_missing_kind_fails() {
        assert_every_kind([0, 2], 3, "test");
    }

    #[test]
    #[should_panic(expected = "the samples must cover every kind of test")]
    fn a_kind_beyond_the_count_fails() {
        assert_every_kind([0, 1, 3], 3, "test");
    }

    #[test]
    #[should_panic(expected = "the samples must cover every kind of test")]
    fn no_kinds_at_all_fails() {
        assert_every_kind([], 3, "test");
    }
}
