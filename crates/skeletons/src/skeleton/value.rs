//! [`Value`]: the grammar for an option's values, whether an `enum`'s whole
//! value set, a `set`'s, a `text` option's default, or the text a wearer
//! types for a `text` option.

use std::fmt;

/// A validated option value: non-empty, with no control character.
///
/// An `enum` or `text` value is inserted directly into a skeleton's files, so
/// this rule keeps a value from adding a newline, a tab, or any other invisible
/// byte the skeleton's author did not write into the file themselves — and,
/// for a wearer's own text, from adding a line the wearer typed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Value(String);

impl Value {
    /// Parses `candidate` as a value, or returns [`None`] when it is empty
    /// or holds a control character.
    pub(crate) fn parse(candidate: &str) -> Option<Self> {
        if candidate.is_empty() {
            return None;
        }
        if candidate.chars().any(char::is_control) {
            return None;
        }

        // Postcondition: the empty check above already ruled this out.
        assert!(
            !candidate.is_empty(),
            "an empty candidate was already refused above"
        );
        Some(Self(candidate.to_owned()))
    }

    /// The value's own text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for Value {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::Value;

    #[test]
    fn valid_values_are_accepted() {
        for value in ["daily", "github-actions", "a value with spaces", "0"] {
            let parsed = Value::parse(value);
            assert!(parsed.is_some(), "expected {value:?} to be accepted");
            assert_eq!(parsed.expect("checked above").as_str(), value);
        }
    }

    #[test]
    fn invalid_values_are_refused() {
        for value in [
            "",            // nothing to select
            "week\tly",    // a tab is a control character
            "line\nbreak", // a newline is a control character
        ] {
            assert!(
                Value::parse(value).is_none(),
                "expected {value:?} to be refused"
            );
        }
    }

    proptest! {
        #[test]
        fn any_non_empty_string_without_control_characters_is_accepted(
            // Printable ASCII only (space through `~`): `char::is_control`
            // also holds for the Unicode C1 range (U+0080–U+009F), so a
            // wider class here would risk generating a "non-control" string
            // that is in fact one.
            value in "[\\x20-\\x7e]{1,50}",
        ) {
            let parsed = Value::parse(&value).expect("no control characters, and not empty");
            prop_assert_eq!(parsed.as_str(), value);
        }

        #[test]
        fn a_string_containing_a_control_character_is_refused(
            prefix in "[\\x20-\\x7e]{0,10}",
            suffix in "[\\x20-\\x7e]{0,10}",
        ) {
            // U+0007 (BEL) is a control character wherever it lands in the
            // string.
            let mutated = format!("{prefix}\u{0007}{suffix}");
            prop_assert!(Value::parse(&mutated).is_none());
        }
    }
}
