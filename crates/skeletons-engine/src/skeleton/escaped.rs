//! [`Escaped`] and [`Echoed`]: how every message prints text that came from
//! outside the tool.

use std::fmt::{self, Write as _};

/// Text from outside the tool (a skeleton author's, a wearer's, a file
/// system's), printed so that no control or invisible character in it can split
/// the line a message is reported on.
///
/// The characters escaped are the ones [`str::escape_debug`] escapes (controls,
/// invisible and non-printable characters, and a grapheme extender at the start
/// of the text) plus the backslash. A quote is written as typed: escaping one
/// would show `\'` in a message where nothing was wrong with the text.
///
/// The notation is one `$'…'` in bash 4.3 or later, or zsh, reads the same way
/// under a UTF-8 locale, except that bash cannot hold a NUL and cuts the text
/// off at `\u0000` where zsh keeps it. A backslash is `\\`, and a newline, tab
/// and carriage return are `\n`, `\t` and `\r`.
/// Every other escaped character is `\uXXXX` (four uppercase hex digits) inside
/// the Basic Multilingual Plane and `\UXXXXXXXX` (eight) above it, so a NUL is
/// `\u0000`. Only the escapes read that way: a whole message is not a `$'…'`
/// literal.
///
/// The backslash is always doubled, so a typed backslash and `n` cannot pass
/// for a newline, and two different texts never print the same. This is the one
/// place text from outside is escaped; every message and the survey's rendering
/// of a refusal's file go through it.
pub(crate) struct Escaped<'text>(pub(crate) &'text str);

impl<'text> Escaped<'text> {
    /// The same text with every non-ASCII character escaped too, in the same
    /// notation, so the result is ASCII: `é` is `\u00E9`. ASCII is written
    /// exactly as [`Escaped`] writes it.
    ///
    /// Two spellings that display identically differ here, which is what
    /// [`told_apart`](crate::survey::told_apart) uses it for.
    pub(crate) const fn code_points(self) -> CodePoints<'text> {
        CodePoints(self.0)
    }
}

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_escape_debug(formatter, self.0.escape_debug())
    }
}

/// [`Escaped`] text with every non-ASCII character written as a code point;
/// made by [`Escaped::code_points`].
pub(crate) struct CodePoints<'text>(&'text str);

impl fmt::Display for CodePoints<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            if character.is_ascii() {
                // An ASCII character is escaped on its own by `escape_debug`
                // exactly as it is inside a longer text: the one rule that
                // looks at neighbours, the grapheme extender at the start,
                // names no ASCII character.
                write_escape_debug(formatter, character.escape_debug())?;
            } else {
                write_code_point(formatter, u32::from(character))?;
            }
        }
        Ok(())
    }
}

/// Writes what `escape_debug` produced, in this module's notation.
///
/// In `escape_debug`'s output every backslash opens an escape, a typed one
/// included (it is written `\\`), so the character after one is always part of
/// that escape: `\'` and `\"` become the bare quote, `\\`, `\n`, `\r` and `\t`
/// stay, `\0` becomes `\u0000`, and `\u{h}` becomes its hex digits in full.
fn write_escape_debug(
    formatter: &mut fmt::Formatter<'_>,
    mut debug: impl Iterator<Item = char>,
) -> fmt::Result {
    while let Some(character) = debug.next() {
        if character != '\\' {
            formatter.write_char(character)?;
            continue;
        }
        match debug.next() {
            Some(quote @ ('\'' | '"')) => formatter.write_char(quote)?,
            Some(short @ ('\\' | 'n' | 'r' | 't')) => {
                formatter.write_char('\\')?;
                formatter.write_char(short)?;
            }
            Some('0') => write_code_point(formatter, 0)?,
            Some('u') => {
                assert_eq!(
                    debug.next(),
                    Some('{'),
                    "`escape_debug` opens `\\u` with a brace"
                );
                write_code_point(formatter, read_braced_hex(&mut debug))?;
            }
            Some(other) => unreachable!("`escape_debug` wrote an unknown escape `\\{other}`"),
            None => unreachable!("`escape_debug` never ends on a lone backslash"),
        }
    }
    Ok(())
}

/// The most hex digits a code point takes: `10FFFF`. It bounds the digits
/// [`read_braced_hex`] accumulates, so the value cannot overflow a `u32`.
const CODE_POINT_HEX_DIGITS_MAX: usize = 6;

/// Reads the hex digits of an `escape_debug` `\u{…}` escape, after its opening
/// brace, up to and including the closing brace, and returns their value.
///
/// # Panics
///
/// Panics if the digits are missing, are not hex, or are more than a code
/// point takes, or if the closing brace never comes: `escape_debug` writes
/// none of those.
fn read_braced_hex(debug: &mut impl Iterator<Item = char>) -> u32 {
    let mut value: u32 = 0;
    // One more than the digits a code point takes: the brace that closes them.
    // Every character before the brace is a digit, so its position is the
    // number of digits read so far.
    for (digits_read, character) in debug.take(CODE_POINT_HEX_DIGITS_MAX + 1).enumerate() {
        if character == '}' {
            assert!(
                digits_read > 0,
                "`escape_debug` writes a digit inside `\\u{{}}`"
            );
            return value;
        }
        let Some(digit_value) = character.to_digit(16) else {
            unreachable!("`escape_debug` writes a code point in hex, got `{character}`")
        };
        assert!(
            digits_read < CODE_POINT_HEX_DIGITS_MAX,
            "`escape_debug` wrote more than {CODE_POINT_HEX_DIGITS_MAX} hex digits"
        );
        value = value * 16 + digit_value;
    }
    unreachable!(
        "`escape_debug` closes `\\u{{` with a brace within {CODE_POINT_HEX_DIGITS_MAX} digits"
    )
}

/// Writes `value` as `\uXXXX` inside the Basic Multilingual Plane and as
/// `\UXXXXXXXX` above it, in uppercase hex.
fn write_code_point(formatter: &mut fmt::Formatter<'_>, value: u32) -> fmt::Result {
    assert!(
        value <= 0x0010_FFFF,
        "{value:#X} is not a Unicode code point"
    );
    if value <= 0xFFFF {
        write!(formatter, "\\u{value:04X}")
    } else {
        write!(formatter, "\\U{value:08X}")
    }
}

/// [`Escaped`] text inside backticks, the way a refusal shows a name or a value
/// someone typed or authored.
pub(super) struct Echoed<'text>(pub(super) &'text str);

impl fmt::Display for Echoed<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "`{}`", Escaped(self.0))
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{Echoed, Escaped, read_braced_hex};

    #[test]
    fn quotes_pass_through_escaped_text_and_a_backslash_is_doubled() {
        assert_eq!(Escaped("it's \"q\"").to_string(), "it's \"q\"");
        assert_eq!(Escaped("a\\nb").to_string(), "a\\\\nb");
    }

    #[test]
    fn the_short_controls_are_written_as_a_shell_reads_them() {
        assert_eq!(Escaped("a\nb\tc\rd").to_string(), "a\\nb\\tc\\rd");
    }

    #[test]
    fn every_other_escaped_character_is_four_or_eight_uppercase_hex_digits() {
        assert_eq!(Escaped("a\0b").to_string(), "a\\u0000b");
        assert_eq!(Escaped("a\u{1}b").to_string(), "a\\u0001b");
        assert_eq!(Escaped("a\u{85}b").to_string(), "a\\u0085b");
        assert_eq!(Escaped("a\u{200b}b").to_string(), "a\\u200Bb");
        assert_eq!(Escaped("a\u{e0001}b").to_string(), "a\\U000E0001b");
    }

    #[test]
    fn braced_hex_reads_one_to_six_digits_and_stops_at_the_brace() {
        assert_eq!(read_braced_hex(&mut "0}".chars()), 0);
        assert_eq!(read_braced_hex(&mut "1F600}".chars()), 0x1F600);
        assert_eq!(read_braced_hex(&mut "10FFFF}".chars()), 0x0010_FFFF);
        let mut rest = "41}tail".chars();
        assert_eq!(read_braced_hex(&mut rest), 0x41);
        assert_eq!(rest.as_str(), "tail", "the brace is consumed and no more");
    }

    // The closing brace is asserted as the opening one is: a digit run with
    // no brace, more digits than a code point takes, no digits, and a
    // character that is not hex each stop the program rather than write a
    // wrong escape.
    #[test]
    #[should_panic(expected = "closes `\\u{` with a brace")]
    fn braced_hex_without_a_closing_brace_panics() {
        let _ = read_braced_hex(&mut "41".chars());
    }

    #[test]
    #[should_panic(expected = "closes `\\u{` with a brace")]
    fn braced_hex_never_closed_after_six_digits_panics() {
        let _ = read_braced_hex(&mut "10FFFF".chars());
    }

    #[test]
    #[should_panic(expected = "more than 6 hex digits")]
    fn braced_hex_of_seven_digits_panics_instead_of_overflowing() {
        let _ = read_braced_hex(&mut "FFFFFFFF}".chars());
    }

    #[test]
    #[should_panic(expected = "writes a digit inside")]
    fn braced_hex_with_no_digits_panics() {
        let _ = read_braced_hex(&mut "}".chars());
    }

    #[test]
    #[should_panic(expected = "writes a code point in hex")]
    fn braced_hex_with_a_character_that_is_not_hex_panics() {
        let _ = read_braced_hex(&mut "4g}".chars());
    }

    #[test]
    fn a_combining_accent_is_escaped_only_at_the_start() {
        assert_eq!(Escaped("e\u{301}").to_string(), "e\u{301}");
        assert_eq!(Escaped("\u{301}e").to_string(), "\\u0301e");
        assert_eq!(Escaped("'\u{301}").to_string(), "'\u{301}");
    }

    #[test]
    fn printable_non_ascii_characters_are_left_alone() {
        assert_eq!(Escaped("caf\u{e9}").to_string(), "caf\u{e9}");
        assert_eq!(Escaped("\u{1d15e}").to_string(), "\u{1d15e}");
    }

    #[test]
    fn code_points_writes_every_non_ascii_character_and_escapes_ascii_as_escaped_does() {
        assert_eq!(Escaped("caf\u{e9}").code_points().to_string(), "caf\\u00E9");
        assert_eq!(
            Escaped("\u{1d15e}").code_points().to_string(),
            "\\U0001D15E"
        );
        assert_eq!(
            Escaped("a\nb\\c'd").code_points().to_string(),
            "a\\nb\\\\c'd"
        );
        assert_eq!(Escaped("e\u{301}").code_points().to_string(), "e\\u0301");
        assert_eq!(Escaped("\u{301}e").code_points().to_string(), "\\u0301e");
    }

    #[test]
    fn an_echoed_text_is_escaped_and_set_in_backticks() {
        assert_eq!(Echoed("a\nb").to_string(), "`a\\nb`");
        assert_eq!(Echoed("plain").to_string(), "`plain`");
    }

    /// `escape_debug`'s own account of `text`, rewritten into the notation
    /// [`Escaped`] promises: quotes as typed, `\0` as `\u0000`, and every
    /// `\u{h}` as four (inside the BMP) or eight (above it) uppercase hex
    /// digits. Written with `replace`, so it shares no code with the writer.
    fn reference(text: &str) -> String {
        let debug = text
            .escape_debug()
            .to_string()
            .replace("\\'", "'")
            .replace("\\\"", "\"");
        let mut rewritten = String::new();
        let mut rest = debug.as_str();
        while let Some(at) = rest.find('\\') {
            rewritten.push_str(&rest[..at]);
            rest = &rest[at + 1..];
            if let Some(after) = rest.strip_prefix("u{") {
                let close = after.find('}').expect("a closing brace");
                let value = u32::from_str_radix(&after[..close], 16).expect("hex digits");
                if value <= 0xFFFF {
                    write!(rewritten, "\\u{value:04X}").expect("writing to a String");
                } else {
                    write!(rewritten, "\\U{value:08X}").expect("writing to a String");
                }
                rest = &after[close + 1..];
            } else if let Some(after) = rest.strip_prefix('0') {
                rewritten.push_str("\\u0000");
                rest = after;
            } else {
                // `\\`, `\n`, `\r` or `\t`: the two characters as they are.
                let mut characters = rest.chars();
                let escaped = characters.next().expect("a character after a backslash");
                rewritten.push('\\');
                rewritten.push(escaped);
                rest = characters.as_str();
            }
        }
        rewritten.push_str(rest);
        rewritten
    }

    proptest::proptest! {
        // Escaped must equal the reference for any string: the set of
        // characters escaped is `escape_debug`'s, and only the notation and
        // the quotes differ.
        #[test]
        fn escaped_matches_the_reference_for_any_text(text in ".*") {
            proptest::prop_assert_eq!(Escaped(&text).to_string(), reference(&text));
        }

        // The code-point form is pure ASCII for any text.
        #[test]
        fn code_points_are_ascii(text in ".*") {
            proptest::prop_assert!(Escaped(&text).code_points().to_string().is_ascii());
        }

        // Two distinct strings never print the same, in either form: a
        // backslash is always doubled, so an escape cannot be forged by
        // typing one.
        #[test]
        fn neither_form_merges_two_strings(first in ".{0,8}", second in ".{0,8}") {
            proptest::prop_assume!(first != second);
            proptest::prop_assert_ne!(
                Escaped(&first).to_string(),
                Escaped(&second).to_string()
            );
            proptest::prop_assert_ne!(
                Escaped(&first).code_points().to_string(),
                Escaped(&second).code_points().to_string()
            );
        }

        // On ASCII text the two forms are the same.
        #[test]
        fn code_points_equal_escaped_on_ascii(text in "[\\x00-\\x7f]*") {
            proptest::prop_assert_eq!(
                Escaped(&text).code_points().to_string(),
                Escaped(&text).to_string()
            );
        }
    }
}
