//! Fill grammar: scanning one line's content for `{{ … }}` placeholders.

use super::name::OptionName;

/// One piece of a line's content once it has been scanned for placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    /// Text carried through unchanged.
    Literal(String),
    /// A `{{name}}` naming the option to fill in with its chosen value.
    Placeholder(OptionName),
}

/// A line held a `{{` that did not open a well-formed placeholder: not
/// immediately followed, on the same line, by a well-formed option name and
/// a closing `}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlaceholderMalformed;

impl std::fmt::Display for PlaceholderMalformed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("`{{` without a well-formed name and `}}` on the same line")
    }
}

impl std::error::Error for PlaceholderMalformed {}

/// Scans one line's `content` (its bytes, with no terminator) into segments.
///
/// Every `{{` opens a placeholder: it must be followed immediately — nothing
/// in between — by a well-formed [`OptionName`] and then `}}`, all before
/// the line ends. Anything else of that shape (a space inside the braces, an
/// unclosed `{{`, wrong-cased letters, a third opening brace, a GitHub
/// Actions `${{ … }}` expression) is refused. Once a placeholder closes,
/// scanning resumes after it. A `}}` or a lone `{` that no `{{` opened is
/// ordinary text, carried through in a [`Segment::Literal`].
///
/// # Errors
///
/// Returns [`PlaceholderMalformed`] for the first `{{` that does not open a
/// well-formed placeholder.
pub(crate) fn scan(content: &str) -> Result<Vec<Segment>, PlaceholderMalformed> {
    // Precondition: this scans one line at a time; a placeholder can never
    // span two lines, so the content handed in never holds one's terminator.
    assert!(
        !content.contains('\n'),
        "a line's content never holds a newline"
    );

    let mut segments = Vec::new();
    let mut literal_start = 0;
    let mut search_start = 0;

    while let Some(relative_open) = content[search_start..].find("{{") {
        let open = search_start + relative_open;
        let after_open = open + 2;
        let rest = &content[after_open..];

        let Some(relative_close) = rest.find("}}") else {
            return Err(PlaceholderMalformed);
        };
        let candidate = &rest[..relative_close];
        let Some(name) = OptionName::parse(candidate) else {
            return Err(PlaceholderMalformed);
        };

        if open > literal_start {
            segments.push(Segment::Literal(content[literal_start..open].to_owned()));
        }
        segments.push(Segment::Placeholder(name));

        let close_end = after_open + relative_close + 2;
        literal_start = close_end;
        search_start = close_end;
    }

    if literal_start < content.len() {
        segments.push(Segment::Literal(content[literal_start..].to_owned()));
    }

    // Postcondition: the segments account for every byte of `content`, in
    // order — the same "nothing lost, nothing invented" property `lines()`
    // asserts for a whole file, checked here for one scanned line.
    let accounted: usize = segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(text) => text.len(),
            Segment::Placeholder(name) => "{{".len() + name.as_str().len() + "}}".len(),
        })
        .sum();
    assert_eq!(
        accounted,
        content.len(),
        "segments must account for every byte of the line"
    );

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{OptionName, Segment, scan};

    #[test]
    fn a_well_formed_placeholder_becomes_a_placeholder_segment() {
        assert_eq!(
            scan("{{cadence}}").expect("well-formed"),
            vec![Segment::Placeholder(
                OptionName::parse("cadence").expect("valid name")
            )]
        );
    }

    #[test]
    fn text_with_no_braces_is_one_literal_segment() {
        assert_eq!(
            scan("plain text").expect("no braces"),
            vec![Segment::Literal("plain text".to_owned())]
        );
    }

    #[test]
    fn empty_content_has_no_segments() {
        assert_eq!(scan("").expect("empty is well-formed"), Vec::new());
    }

    #[test]
    fn a_lone_closing_brace_is_ordinary_text() {
        // No `{{` ever opened, so `}}` here is just two literal characters.
        assert_eq!(
            scan("a}}b").expect("no `{{`"),
            vec![Segment::Literal("a}}b".to_owned())]
        );
    }

    #[test]
    fn a_placeholder_with_internal_spaces_is_malformed() {
        assert!(scan("{{ cadence }}").is_err());
    }

    #[test]
    fn an_unclosed_placeholder_is_malformed() {
        assert!(scan("{{cadence").is_err());
    }

    #[test]
    fn a_github_actions_expression_is_malformed() {
        // `${{ github.ref }}` collides with the fill grammar: there is no
        // escape syntax, so text that is scanned refuses it rather than passing
        // it through. A file that needs one is declared verbatim and never
        // scanned.
        assert!(scan("${{ github.ref }}").is_err());
    }

    #[test]
    fn a_triple_brace_is_malformed() {
        assert!(scan("{{{cadence}}}").is_err());
    }

    #[test]
    fn text_around_a_placeholder_is_kept_as_literal_segments() {
        assert_eq!(
            scan("before {{cadence}} after").expect("well-formed"),
            vec![
                Segment::Literal("before ".to_owned()),
                Segment::Placeholder(OptionName::parse("cadence").expect("valid name")),
                Segment::Literal(" after".to_owned()),
            ]
        );
    }

    proptest! {
        #[test]
        fn a_line_with_no_double_brace_is_one_literal_segment(content in "[^{}\n]*") {
            let segments = scan(&content).expect("no `{{` cannot be malformed");
            if content.is_empty() {
                prop_assert!(segments.is_empty());
            } else {
                prop_assert_eq!(segments, vec![Segment::Literal(content)]);
            }
        }

        #[test]
        fn a_successful_scan_reassembles_to_the_input(
            content in "[a-z]{0,5}(\\{\\{[a-z][a-z0-9]{0,5}\\}\\}[a-z]{0,5}){0,3}",
        ) {
            // Only well-formed placeholders and plain lowercase text are
            // generated, so every input must scan: an `Err` fails the
            // property rather than skipping the case, which keeps a `scan`
            // that refuses everything from passing it.
            let segments = scan(&content).expect("a well-formed line scans");
            let rebuilt: String = segments
                .into_iter()
                .map(|segment| match segment {
                    Segment::Literal(text) => text,
                    Segment::Placeholder(name) => format!("{{{{{}}}}}", name.as_str()),
                })
                .collect();
            prop_assert_eq!(rebuilt, content);
        }
    }
}
