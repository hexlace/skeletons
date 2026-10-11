//! A git object id: exactly the shape git itself always prints one as.

/// A git object id — 40 lowercase hex digits (a SHA-1 repository) or 64 (a
/// SHA-256 one). Nothing else this crate accepts as one: git never prints
/// an object id any other length or case, so a caller that accepted more
/// would be guessing rather than reading what git actually said.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ObjectId(String);

/// How many leading hex digits [`ObjectId::abbreviated`] keeps: the same
/// abbreviation `git log --oneline` shows. Only human text is cut to it;
/// `--json` carries every object id whole.
const ABBREVIATED_HEX_DIGITS: usize = 7;

impl ObjectId {
    /// Parses `text` as an object id.
    ///
    /// # Errors
    ///
    /// Returns `None` when `text` is not exactly 40 or 64 characters, or
    /// holds anything but a lowercase hex digit.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let length_ok = text.len() == 40 || text.len() == 64;
        if !length_ok {
            return None;
        }
        let all_lowercase_hex = text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        if !all_lowercase_hex {
            return None;
        }
        Some(Self(text.to_owned()))
    }

    /// This object id's own text, exactly as parsed — for a caller that
    /// needs to embed it (a `<rev>:<path>` git object spec, a command
    /// argument) rather than merely display it whole.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// This object id's first seven hex digits, as human text shows a
    /// commit.
    ///
    /// Every object id is 40 or 64 ASCII hex digits, so the cut always
    /// falls inside it and on a character boundary.
    pub(crate) fn abbreviated(&self) -> &str {
        &self.0[..ABBREVIATED_HEX_DIGITS]
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::ObjectId;

    const SHA1: &str = "3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39";
    const SHA256: &str = "8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f";

    #[test]
    fn a_forty_character_lowercase_hex_string_is_accepted() {
        assert!(ObjectId::parse(SHA1).is_some());
    }

    #[test]
    fn a_sixty_four_character_lowercase_hex_string_is_accepted() {
        assert!(ObjectId::parse(SHA256).is_some());
    }

    #[test]
    fn thirty_nine_characters_is_rejected() {
        assert!(ObjectId::parse(&SHA1[..39]).is_none());
    }

    #[test]
    fn forty_one_characters_is_rejected() {
        let too_long = format!("{SHA1}0");
        assert!(ObjectId::parse(&too_long).is_none());
    }

    #[test]
    fn sixty_three_characters_is_rejected() {
        assert!(ObjectId::parse(&SHA256[..63]).is_none());
    }

    #[test]
    fn uppercase_is_rejected() {
        assert!(ObjectId::parse(&SHA1.to_uppercase()).is_none());
    }

    #[test]
    fn non_hex_characters_are_rejected() {
        assert!(ObjectId::parse("not hex at all, but forty chars long!!!").is_none());
    }

    // Forty bytes of two-byte characters: the length check alone would pass
    // it, and cutting it at seven bytes would land inside a character. A
    // remote's answer is not ours to trust, so this is refused at the parse.
    #[test]
    fn forty_bytes_of_non_ascii_is_rejected() {
        let text = "\u{e9}".repeat(20);
        assert_eq!(text.len(), 40);
        assert!(ObjectId::parse(&text).is_none());
    }

    #[test]
    fn abbreviated_is_the_first_seven_hex_digits() {
        let sha1 = ObjectId::parse(SHA1).expect("a well-formed object id");
        let sha256 = ObjectId::parse(SHA256).expect("a well-formed object id");
        assert_eq!(sha1.abbreviated(), "3f2a9c1");
        assert_eq!(sha256.abbreviated(), "8c1e2f4");
    }

    proptest! {
        /// However `text` is shaped, `parse` never panics.
        #[test]
        fn never_panics(text in ".*") {
            let _result = ObjectId::parse(&text);
        }
    }
}
