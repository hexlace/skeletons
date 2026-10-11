//! Splitting a skeleton's text into lines, exactly as it was written.
//!
//! A skeleton's files that are filled and selected are read as UTF-8 text (a
//! file declared verbatim is held as bytes and never split into lines), and
//! every other rule in this module tree — fill, select, indentation — is
//! stated a line at a time: a placeholder or a directive never spans more than
//! one line. [`lines`]
//! turns a whole file's text into that per-line view without losing or
//! inventing a single byte: each line keeps its own terminator, so the
//! original text is always exactly `content` followed by `terminator`, for
//! every line, in order.

/// One line of a skeleton's text: the bytes before its line terminator, and the
/// terminator itself.
///
/// `terminator` is `"\n"`, or `""` — empty only for a file's last line when
/// the file does not end with `\n`. `\n` is the only byte that ends a line;
/// every other byte is `content`.
///
/// "Blank" means a line whose `content` is empty — not one holding only
/// spaces or tabs, which is content like any other and is not stripped
/// here or anywhere downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Line<'text> {
    /// The line's own bytes, not including its terminator.
    pub(crate) content: &'text str,
    /// `"\n"`, or `""` for an unterminated final line.
    pub(crate) terminator: &'text str,
}

/// Splits `text` into its lines, each with its own terminator.
///
/// An empty `text` splits into zero lines. Otherwise every byte of `text`
/// belongs to exactly one line, either as its `content` or as its
/// `terminator` — a property enforced below, not merely hoped for.
pub(crate) fn lines(text: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let Some(newline_index) = rest.find('\n') else {
            // No `\n` left at all: the remainder is one unterminated line.
            lines.push(Line {
                content: rest,
                terminator: "",
            });
            break;
        };

        let terminator_end = newline_index + 1;

        lines.push(Line {
            content: &rest[..newline_index],
            terminator: &rest[newline_index..terminator_end],
        });
        rest = &rest[terminator_end..];
    }

    // Postcondition: every byte of `text` was claimed by exactly one line,
    // and no line's terminator is any shape but the two this type allows.
    let accounted: usize = lines
        .iter()
        .map(|line| line.content.len() + line.terminator.len())
        .sum();
    assert_eq!(
        accounted,
        text.len(),
        "lines() must account for every byte of its input"
    );
    assert!(
        lines
            .iter()
            .all(|line| matches!(line.terminator, "" | "\n")),
        "every line's terminator must be empty or `\\n`"
    );
    // Paired with the preconditions of `directive::scan` and
    // `placeholder::scan`, which take one line's content and rely on it
    // holding no `\n`.
    assert!(
        lines.iter().all(|line| !line.content.contains('\n')),
        "no line's content may hold `\\n`, the only byte that ends a line"
    );

    lines
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Line, lines};

    #[test]
    fn empty_text_has_no_lines() {
        // "" holds no bytes at all, so there is nothing to split into a line.
        assert_eq!(lines(""), Vec::new());
    }

    #[test]
    fn a_line_with_a_trailing_newline_keeps_it_as_the_terminator() {
        assert_eq!(
            lines("a\n"),
            vec![Line {
                content: "a",
                terminator: "\n"
            }]
        );
    }

    #[test]
    fn a_final_line_without_a_newline_has_an_empty_terminator() {
        // No trailing `\n` at all: the whole text is one line, unterminated.
        assert_eq!(
            lines("a"),
            vec![Line {
                content: "a",
                terminator: ""
            }]
        );
    }

    #[test]
    fn only_a_newline_ends_a_line() {
        // A spread of control bytes around `\n`: none of them, `\r` included,
        // ends a line or joins a terminator, so each stays in its line's
        // content exactly where it was written.
        assert_eq!(
            lines("a\r\nb\x0b\n\x0cc\r"),
            vec![
                Line {
                    content: "a\r",
                    terminator: "\n"
                },
                Line {
                    content: "b\x0b",
                    terminator: "\n"
                },
                Line {
                    content: "\x0cc\r",
                    terminator: ""
                },
            ]
        );
    }

    #[test]
    fn a_blank_line_has_empty_content() {
        assert_eq!(
            lines("\n\na"),
            vec![
                Line {
                    content: "",
                    terminator: "\n"
                },
                Line {
                    content: "",
                    terminator: "\n"
                },
                Line {
                    content: "a",
                    terminator: ""
                },
            ]
        );
    }

    proptest! {
        #[test]
        fn splitting_and_rejoining_lines_is_the_identity(text in any::<String>()) {
            // Property: for any string at all — arbitrary Unicode, arbitrary
            // placement of `\n` — concatenating every line's
            // content and terminator, in order, reproduces the input
            // exactly. This is the same invariant `lines()` asserts
            // internally, checked here from outside against inputs no one
            // would think to write by hand.
            let rejoined = lines(&text).into_iter().fold(String::new(), |mut buffer, line| {
                buffer.push_str(line.content);
                buffer.push_str(line.terminator);
                buffer
            });
            prop_assert_eq!(rejoined, text);
        }
    }
}
