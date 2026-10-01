//! [`OptionName`]: the one name grammar `skeletons` uses for an option, wherever
//! one is written — a manifest key, a `{{name}}` placeholder, or a
//! `# skeletons:partial name` directive.

use std::borrow::Borrow;
use std::fmt;

/// A validated option name: a lowercase ASCII letter, then zero or more
/// lowercase ASCII letters, digits and `-`, never ending in `-`.
///
/// `cadence`, `github-actions` and `v2` are names; `Cadence` (uppercase),
/// `2fa` (starts with a digit), `x-` (ends in `-`) and `snake_case`
/// (underscore) are not. The same grammar names an option everywhere it is
/// written, so a validated name is its own type: an unchecked `&str` can
/// never be looked up as though it had already been checked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OptionName(String);

impl OptionName {
    /// Parses `candidate` as an option name, or returns [`None`] when it
    /// does not match the grammar.
    pub(crate) fn parse(candidate: &str) -> Option<Self> {
        let mut characters = candidate.chars();
        let first = characters.next()?;
        if !first.is_ascii_lowercase() {
            return None;
        }
        for character in characters {
            let allowed =
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-';
            if !allowed {
                return None;
            }
        }
        if candidate.ends_with('-') {
            return None;
        }

        // Postcondition: every path above that reaches here started from a
        // real first character, so `candidate` was never empty.
        assert!(
            !candidate.is_empty(),
            "a candidate with a first character is not empty"
        );
        Some(Self(candidate.to_owned()))
    }

    /// The name's own text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OptionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for OptionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// Sound: `Eq`, `Ord` and `Hash` are all derived from the single `String`
// field, so they agree exactly with `str`'s own — the property `Borrow`
// requires of any type that implements it for more than one target.
impl Borrow<str> for OptionName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::OptionName;

    #[test]
    fn valid_names_are_accepted() {
        for name in ["cadence", "github-actions", "v2", "a", "a-b-c", "a1"] {
            let parsed = OptionName::parse(name);
            assert!(parsed.is_some(), "expected {name:?} to be accepted");
            assert_eq!(parsed.expect("checked above").as_str(), name);
        }
    }

    #[test]
    fn invalid_names_are_refused() {
        for name in [
            "",           // nothing to name an option with
            "Cadence",    // uppercase
            "2fa",        // starts with a digit
            "x-",         // ends in a hyphen
            "snake_case", // underscore is not in the grammar
            "-leading",   // starts with a hyphen, not a letter
        ] {
            assert!(
                OptionName::parse(name).is_none(),
                "expected {name:?} to be refused"
            );
        }
    }

    proptest! {
        #[test]
        fn any_string_matching_the_grammar_is_accepted(name in "[a-z][a-z0-9]{0,10}") {
            // A generated name built only from lowercase letters and digits
            // can never end in `-`, so every string this strategy produces
            // is unconditionally valid: the property covers names no
            // hand-written table would think to try.
            let parsed = OptionName::parse(&name).expect("matches the grammar by construction");
            prop_assert_eq!(parsed.as_str(), name);
        }

        #[test]
        fn a_name_containing_an_uppercase_letter_is_refused(
            prefix in "[a-z][a-z0-9]{0,5}",
            upper in "[A-Z]",
            suffix in "[a-z0-9]{0,5}",
        ) {
            let mutated = format!("{prefix}{upper}{suffix}");
            prop_assert!(OptionName::parse(&mutated).is_none());
        }
    }
}
