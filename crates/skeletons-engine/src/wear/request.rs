//! What the wearer asked `wear` for, checked before anything is read or
//! written: the skeleton's crate and the version asked for, the key it is
//! worn under, and where it comes from.
//!
//! The key names a dependency in the command line's manifest and a table
//! under `[package.metadata.skeletons]`. Cargo accepts far more in a name
//! than a person means to type there, and it checks a rename only after it
//! has written it, so [`Key`] holds the narrower grammar `wear` is willing to
//! hand to `cargo add`. The crate name goes through the same grammar as a
//! [`CrateName`], which is also what keeps a crate spec from ever reaching
//! `cargo add` as an option.

use super::refusal::WearRefusal;
use super::source::Source;
use crate::workspace::{RESERVED_WEARING_KEYS, underscored};

/// The longest a [`Key`] or a [`CrateName`] may be, in bytes: crates.io's
/// limit on a crate name, which is the longest a key that defaults to a
/// crate's name can be.
pub(crate) const KEY_BYTES_MAX: usize = 64;

/// The crates `rustc` provides to every build, as it names them (with `_`).
///
/// A dependency under one of these keys shadows the compiler's crate in the
/// command line's test build: `std` replaces the standard library's prelude and
/// `test` the harness's `test_main_static`, and the build then fails in
/// `rustc`'s words, which never mention skeletons. `wear` refuses the key
/// before writing anything instead.
const COMPILER_CRATES: [&str; 5] = ["std", "core", "alloc", "proc_macro", "test"];

/// A dependency key `wear` will hand to `cargo add`: an ASCII letter or `_`,
/// then ASCII letters, digits, `-` and `_`, at most [`KEY_BYTES_MAX`] bytes.
///
/// Narrower than Cargo's own rule, which lets a key hold Unicode identifier
/// characters and refuses a bad rename only after writing it. The key is
/// refused here, before anything is written, so nothing is written under a key
/// Cargo would then refuse. A key outside this grammar can still be written by
/// hand.
///
/// A key and a [`CrateName`] are held to the same grammar and are still two
/// types, so one cannot be passed where the other is meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key(String);

/// The text given to [`Key::new`] is not a [`Key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidKey;

impl std::fmt::Display for InvalidKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not a key wear can add")
    }
}

impl std::error::Error for InvalidKey {}

/// The name of a skeleton's crate, held to the grammar a [`Key`] is held to,
/// which is also what keeps a crate spec from ever reaching `cargo add` as an
/// option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrateName(String);

/// The text given to [`CrateName::new`] is not a [`CrateName`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidCrateName;

impl std::fmt::Display for InvalidCrateName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not a crate name wear can add")
    }
}

impl std::error::Error for InvalidCrateName {}

/// Whether `character` may begin a [`Key`] or a [`CrateName`].
const fn opens_a_name(character: char) -> bool {
    matches!(character, 'a'..='z' | 'A'..='Z' | '_')
}

/// Whether `character` may follow the first in a [`Key`] or a [`CrateName`].
const fn continues_a_name(character: char) -> bool {
    matches!(character, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_')
}

/// Whether `text` is in the grammar [`Key`] and [`CrateName`] share: not
/// longer than [`KEY_BYTES_MAX`], an ASCII letter or `_` first, and only ASCII
/// letters, digits, `-` and `_` after it.
fn in_the_grammar(text: &str) -> bool {
    if text.len() > KEY_BYTES_MAX {
        return false;
    }
    let mut characters = text.chars();
    match characters.next() {
        Some(first) if opens_a_name(first) => characters.all(continues_a_name),
        Some(_) | None => false,
    }
}

impl Key {
    /// Reads `text` as a key.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidKey`] when `text` is empty, longer than
    /// [`KEY_BYTES_MAX`], does not begin with an ASCII letter or `_`, or holds
    /// any other character after it than an ASCII letter, digit, `-` or `_`.
    pub(crate) fn new(text: &str) -> Result<Self, InvalidKey> {
        if in_the_grammar(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidKey)
        }
    }

    /// The key a skeleton is worn under when none is given: its crate's name.
    pub(crate) fn named_for(crate_name: &CrateName) -> Self {
        Self(crate_name.as_str().to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The key as `rustc` names the dependency under it: with every `-` turned
    /// into `_`, so `a-x` and `a_x` are one name.
    pub(crate) fn underscored(&self) -> String {
        underscored(self.as_str())
    }
}

impl CrateName {
    /// Reads `text` as a crate name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCrateName`] for the text [`Key::new`] refuses.
    pub(crate) fn new(text: &str) -> Result<Self, InvalidCrateName> {
        if in_the_grammar(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidCrateName)
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A checked request to wear one skeleton: the crate, the version asked for if
/// any, the key it is worn under, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    crate_name: CrateName,
    version: Option<String>,
    key: Key,
    source: Source,
}

impl Request {
    /// Checks what the wearer typed.
    ///
    /// `skeleton` is `<crate>` or `<crate>@<version>`, split at the first `@`.
    /// The version is kept exactly as typed, because Cargo owns that grammar
    /// and refuses a bad one before it writes anything. `key` defaults to the
    /// crate's name.
    ///
    /// # Errors
    ///
    /// Returns [`WearRefusal`] when the crate name is not a [`CrateName`], an
    /// explicit key is not a [`Key`], or the key is one
    /// `[package.metadata.skeletons]` keeps for a skeleton's own declarations,
    /// or names, as `rustc` does, a crate the compiler provides.
    pub(crate) fn new(
        skeleton: &str,
        key: Option<&str>,
        source: Source,
    ) -> Result<Self, WearRefusal> {
        let (name, version) = match skeleton.split_once('@') {
            Some((name, version)) => (name, Some(version.to_owned())),
            None => (skeleton, None),
        };
        let crate_name =
            CrateName::new(name).map_err(|InvalidCrateName| WearRefusal::CrateNameInvalid {
                crate_name: name.to_owned(),
            })?;
        let key = match key {
            Some(text) => Key::new(text).map_err(|InvalidKey| WearRefusal::KeyInvalid {
                key: text.to_owned(),
            })?,
            None => Key::named_for(&crate_name),
        };
        if RESERVED_WEARING_KEYS.contains(&key.as_str()) {
            return Err(WearRefusal::KeyReserved {
                key: key.as_str().to_owned(),
            });
        }
        if COMPILER_CRATES.contains(&key.underscored().as_str()) {
            return Err(WearRefusal::KeyShadowsCompiler {
                key: key.as_str().to_owned(),
            });
        }
        Ok(Self {
            crate_name,
            version,
            key,
            source,
        })
    }

    pub(crate) const fn crate_name(&self) -> &CrateName {
        &self.crate_name
    }

    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub(crate) const fn key(&self) -> &Key {
        &self.key
    }

    pub(crate) const fn source(&self) -> &Source {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{CrateName, InvalidCrateName, InvalidKey, KEY_BYTES_MAX, Key, Request};
    use crate::wear::refusal::WearRefusal;
    use crate::wear::source::Source;

    fn accepted(text: &str) -> bool {
        Key::new(text).is_ok()
    }

    #[test]
    fn a_key_made_of_letters_digits_hyphens_and_underscores_is_accepted() {
        // The accepted side of the grammar, one case per character class the
        // grammar names, and the longest length it allows.
        let longest = "a".repeat(KEY_BYTES_MAX);
        for text in ["a", "A", "_a", "a-b", "a_b", "a1", "Z9-_", longest.as_str()] {
            assert!(accepted(text), "{text:?} must be a key");
        }
    }

    #[test]
    fn a_keys_underscored_form_turns_every_hyphen_into_an_underscore_and_nothing_else() {
        // The form `rustc` names a dependency by, which is what makes `a-x`
        // and `a_x` collide: hyphens change, and case and underscores stay.
        for (text, expected) in [
            ("a", "a"),
            ("a-b", "a_b"),
            ("a_b", "a_b"),
            ("a-b-c_d", "a_b_c_d"),
            ("A-B", "A_B"),
        ] {
            let key = Key::new(text).expect("a test passes only keys it knows are valid");
            assert_eq!(key.underscored(), expected, "{text:?}");
        }
    }

    #[test]
    fn the_underscored_form_of_text_that_is_no_key_is_made_the_same_way() {
        // Declared names are read from a manifest and need not be keys `wear`
        // would accept, but still collide under the same rule.
        assert_eq!(super::underscored("fo-o.b-ar"), "fo_o.b_ar");
        assert_eq!(super::underscored(""), "");
    }

    #[test]
    fn a_key_outside_the_grammar_is_refused() {
        // The refused side, one case per way out of the grammar: empty, a
        // digit or hyphen first, a character outside the classes, a non-ASCII
        // letter, and one byte too long.
        let too_long = "a".repeat(KEY_BYTES_MAX + 1);
        for text in ["", "1a", "-a", "a.b", "a b", "føo", too_long.as_str()] {
            assert_eq!(Key::new(text), Err(InvalidKey), "{text:?} must be refused");
        }
    }

    #[test]
    fn the_length_limit_is_the_maximum_in_bytes_exactly() {
        // The limit is on the ASCII side only, since a non-ASCII letter is
        // refused by the character rule whatever the length: a key of the
        // maximum length passes and one byte more does not.
        assert!(accepted(&"a".repeat(KEY_BYTES_MAX)));
        assert!(!accepted(&"a".repeat(KEY_BYTES_MAX + 1)));
    }

    #[test]
    fn a_crate_name_is_held_to_the_grammar_a_key_is_and_is_refused_as_a_crate_name() {
        // The two types share a grammar, and each says which of the two it
        // refused, so a message built from the error cannot name the wrong one.
        let too_long = "a".repeat(KEY_BYTES_MAX + 1);
        for text in ["", "1a", "-a", "a.b", "a b", "føo", too_long.as_str()] {
            assert_eq!(CrateName::new(text), Err(InvalidCrateName), "{text:?}");
        }
        for text in ["a", "_a", "a-b", "Z9-_"] {
            assert!(CrateName::new(text).is_ok(), "{text:?}");
        }
        assert_eq!(InvalidKey.to_string(), "not a key wear can add");
        assert_eq!(
            InvalidCrateName.to_string(),
            "not a crate name wear can add"
        );
    }

    fn request(skeleton: &str, key: Option<&str>) -> Result<Request, WearRefusal> {
        Request::new(skeleton, key, Source::Registry)
    }

    #[test]
    fn a_crate_with_no_version_has_none_and_the_key_defaults_to_its_name() {
        let request = request("tidy", None).expect("a plain crate name is a request");

        assert_eq!(request.crate_name().as_str(), "tidy");
        assert_eq!(request.version(), None);
        assert_eq!(request.key().as_str(), "tidy");
    }

    #[test]
    fn a_version_after_the_at_sign_is_kept_exactly_as_typed() {
        let request = request("tidy@0.1.0", None).expect("a crate with a version is a request");

        assert_eq!(request.crate_name().as_str(), "tidy");
        assert_eq!(request.version(), Some("0.1.0"));
    }

    #[test]
    fn an_empty_version_is_kept_for_cargo_to_refuse() {
        // `wear` does not own the version grammar. `tidy@` reaches `cargo add`
        // as typed, which refuses it before it writes anything.
        let request = request("tidy@", None).expect("the version is not wear's to judge");

        assert_eq!(request.version(), Some(""));
    }

    #[test]
    fn the_split_is_at_the_first_at_sign() {
        let request = request("tidy@>=1@2", None).expect("the rest is the version");

        assert_eq!(request.crate_name().as_str(), "tidy");
        assert_eq!(request.version(), Some(">=1@2"));
    }

    #[test]
    fn an_explicit_key_replaces_the_default() {
        let request = request("tidy@1", Some("neat")).expect("an explicit key is a request");

        assert_eq!(request.crate_name().as_str(), "tidy");
        assert_eq!(request.key().as_str(), "neat");
    }

    #[test]
    fn a_crate_name_outside_the_grammar_is_refused_naming_it() {
        // This is also what keeps a crate spec from reaching `cargo add` as an
        // option: a name cannot begin with `-`.
        for skeleton in ["--git", "-x@1", "a.b", "", "@1.0.0"] {
            let name = skeleton.split('@').next().unwrap_or_default();
            assert_eq!(
                request(skeleton, None),
                Err(WearRefusal::CrateNameInvalid {
                    crate_name: name.to_owned()
                }),
                "{skeleton:?} must be refused"
            );
        }
    }

    #[test]
    fn an_explicit_key_outside_the_grammar_is_refused_naming_it() {
        assert_eq!(
            request("tidy", Some("1neat")),
            Err(WearRefusal::KeyInvalid {
                key: "1neat".to_owned()
            })
        );
    }

    #[test]
    fn the_keys_a_skeleton_keeps_for_itself_are_refused_explicit_or_defaulted() {
        // `options` and `verbatim` under `[package.metadata.skeletons]` belong
        // to a skeleton's own declaration. A crate called `options` would
        // default to one, so the default is held to the same rule.
        for reserved in ["options", "verbatim"] {
            let refusal = WearRefusal::KeyReserved {
                key: reserved.to_owned(),
            };
            assert_eq!(request("tidy", Some(reserved)), Err(refusal.clone()));
            assert_eq!(request(reserved, None), Err(refusal));
        }
    }

    #[test]
    fn the_crates_the_compiler_provides_are_refused_as_keys_explicit_or_defaulted() {
        // A dependency under one of these keys shadows the compiler's crate in
        // the test build. The refusal reads the key as `rustc` does, so
        // `proc-macro` is `proc_macro`, and a crate called `std` defaults to
        // the key `std`.
        for (typed, shown) in [
            ("std", "std"),
            ("core", "core"),
            ("alloc", "alloc"),
            ("proc_macro", "proc_macro"),
            ("proc-macro", "proc-macro"),
            ("test", "test"),
        ] {
            let refusal = WearRefusal::KeyShadowsCompiler {
                key: shown.to_owned(),
            };
            assert_eq!(request("tidy", Some(typed)), Err(refusal.clone()));
            assert_eq!(request(typed, None), Err(refusal));
        }
    }

    #[test]
    fn a_crate_named_for_the_compiler_is_a_fine_crate_under_another_key() {
        for name in ["std", "core", "alloc", "proc-macro", "test"] {
            assert!(request(name, Some("tidy")).is_ok(), "{name}");
        }
    }

    #[test]
    fn a_near_miss_of_a_compiler_crate_is_accepted_as_a_key() {
        for near in [
            "stdx",
            "tests",
            "cores",
            "allocs",
            "proc_macros",
            "procmacro",
            "_std",
        ] {
            assert!(request("tidy", Some(near)).is_ok(), "{near}");
        }
    }

    #[test]
    fn a_reserved_word_is_a_fine_crate_name_under_another_key() {
        assert!(request("options", Some("tidy")).is_ok());
    }

    /// The grammar written out a second way, as a character-class matcher over
    /// bytes, so the proptest compares `Key::new` with something that shares
    /// none of its code.
    fn grammar_oracle(text: &str) -> bool {
        let bytes = text.as_bytes();
        let Some((first, rest)) = bytes.split_first() else {
            return false;
        };
        let first_ok = first.is_ascii_alphabetic() || *first == b'_';
        let rest_ok = rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_');
        first_ok && rest_ok && bytes.len() <= KEY_BYTES_MAX
    }

    proptest! {
        // Property: `Key::new` and `CrateName::new` accept exactly the strings
        // the independent oracle accepts, and never panic. The input space is arbitrary
        // strings, so non-ASCII and control characters are covered, plus
        // strings drawn from the grammar's own alphabet so the accepted side
        // is reached often, up to past the length limit.
        #[test]
        fn key_new_agrees_with_an_independent_grammar_and_never_panics(
            arbitrary in any::<String>(),
            from_alphabet in "[a-zA-Z0-9_-]{0,70}",
        ) {
            prop_assert_eq!(
                Key::new(&arbitrary).is_ok(),
                grammar_oracle(&arbitrary),
                "{:?}",
                arbitrary
            );
            prop_assert_eq!(
                Key::new(&from_alphabet).is_ok(),
                grammar_oracle(&from_alphabet),
                "{:?}",
                from_alphabet
            );
            prop_assert_eq!(
                CrateName::new(&from_alphabet).is_ok(),
                grammar_oracle(&from_alphabet),
                "{:?}",
                from_alphabet
            );
        }
    }
}
