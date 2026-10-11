//! Parsing a file's or a partial's text into a [`Template`] or [`Partial`],
//! checking every placeholder and directive against a skeleton's declarations as
//! it goes.
//!
//! Fill is one pass over the skeleton's own authored text: a placeholder's
//! replacement value is decided later, in
//! [`super::rendering`], and is never itself scanned here or anywhere else.
//! An error found while parsing always names the line of the file actually
//! being parsed — a partial's own line, never a position the text would
//! occupy once assembled into a main file.

use std::num::NonZeroU32;

use super::declarations::{Declarations, Declared, EnumOption, SetOption, TextOption};
use super::directive::{self, DirectiveLine};
use super::error::{Reason, one_based_line};
use super::keyed::Key;
use super::lines::{Line, lines};
use super::placeholder::{self, Segment};

/// The option a placeholder fills from: an `enum`, which fills one of its
/// declared values, or a `text`, which fills the wearer's own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fill {
    Enum(Key<EnumOption>),
    Text(Key<TextOption>),
}

/// One piece of a line's text once its placeholders are checked: text
/// carried through unchanged, or a placeholder holding the option it fills
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Literal(String),
    Placeholder(Fill),
}

/// One line of text: its pieces, its terminator, and the optional `text`
/// option whose being unset drops the whole line, terminator included.
///
/// The optional option is the only thing a line that can be dropped
/// depends on, which is what keeps dropping it from dropping anything else:
/// [`TextLine::new`] asserts that every placeholder on such a line fills
/// from that one option, so no other value is lost with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextLine {
    pieces: Vec<Piece>,
    terminator: String,
    optional: Option<Key<TextOption>>,
}

impl TextLine {
    /// A line of `pieces` ending in `terminator`, dropped when `optional` is
    /// set and that option is unset.
    ///
    /// # Panics
    ///
    /// If `optional` is set and some placeholder among `pieces` fills from
    /// anything but that option. Parsing refuses such a line as
    /// [`Reason::OptionalPlaceholderNotAlone`] before one is ever built.
    pub(crate) fn new(
        pieces: Vec<Piece>,
        terminator: String,
        optional: Option<Key<TextOption>>,
    ) -> Self {
        if let Some(option) = optional {
            for fill in fills(&pieces) {
                assert_eq!(
                    fill,
                    Fill::Text(option),
                    "every placeholder on a droppable line fills from its one optional option"
                );
            }
        }
        Self {
            pieces,
            terminator,
            optional,
        }
    }

    pub(crate) fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub(crate) fn terminator(&self) -> &str {
        &self.terminator
    }

    /// The optional `text` option whose being unset drops this line, or
    /// `None` for a line that is always present.
    pub(crate) const fn optional(&self) -> Option<Key<TextOption>> {
        self.optional
    }
}

/// One line of a main file, once it is known to be either ordinary text or a
/// well-formed, correctly-kinded directive. A directive line in a partial is
/// [`Reason::DirectiveInPartial`] rather than a variant here — the type
/// itself makes a second level of selection unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TemplateLine {
    Text(TextLine),
    Directive {
        indentation: String,
        /// The `set` option the directive selects from.
        option: Key<SetOption>,
        terminator: String,
    },
}

/// A parsed main file: every line, and the original source it was parsed
/// from, kept so assembly can assert a template with no placeholder and no
/// directive renders equal to its own source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) lines: Vec<TemplateLine>,
    pub(crate) source: String,
}

impl Template {
    /// Every option a placeholder in this file fills from, duplicates
    /// included — used only to decide whether a declared option is used by
    /// anything at all.
    pub(crate) fn used_fill_options(&self) -> Vec<Fill> {
        let mut used = Vec::new();
        for line in &self.lines {
            if let TemplateLine::Text(text_line) = line {
                used.extend(fills(text_line.pieces()));
            }
        }
        used
    }

    /// Every set option a directive in this file selects from, duplicates
    /// included.
    pub(crate) fn used_set_options(&self) -> Vec<Key<SetOption>> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                TemplateLine::Directive { option, .. } => Some(*option),
                TemplateLine::Text(_) => None,
            })
            .collect()
    }
}

/// A parsed partial: its lines alone, which are text only since a partial
/// holds no directives of its own, and which nothing compares back to the
/// partial's own source the way a main file's fidelity is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Partial {
    pub(crate) lines: Vec<TextLine>,
}

impl Partial {
    /// Every option a placeholder in this partial fills from, duplicates
    /// included.
    pub(crate) fn used_fill_options(&self) -> Vec<Fill> {
        let mut used = Vec::new();
        for line in &self.lines {
            used.extend(fills(line.pieces()));
        }
        used
    }
}

/// The option of every placeholder among `pieces`, in order.
fn fills(pieces: &[Piece]) -> impl Iterator<Item = Fill> + '_ {
    pieces.iter().filter_map(|piece| match piece {
        Piece::Placeholder(fill) => Some(*fill),
        Piece::Literal(_) => None,
    })
}

/// One template-parsing refusal: the 1-based line it was found at, and why.
/// The callers (`parse_files` and `parse_partials` in `skeleton`) already know
/// which skeleton and which file this is about; this carries only what
/// parsing itself discovers.
#[derive(Debug)]
pub(crate) struct TemplateFailure {
    pub(crate) line: NonZeroU32,
    pub(crate) reason: Reason,
}

impl TemplateFailure {
    fn at(index: usize, reason: Reason) -> Self {
        Self {
            line: one_based_line(index),
            reason,
        }
    }
}

/// Parses a main file's `source` into a [`Template`], checking every
/// placeholder names a declared `enum` or `text` option, an optional `text`
/// placeholder has its line to itself, and every directive names a
/// declared `set` option and ends its line with a terminator. A line that
/// reaches the marker's shape only once an invisible character other than a
/// space or a tab — before it or inside it — is set aside is refused as
/// [`Reason::HiddenDirective`].
pub(crate) fn parse_file(
    source: &str,
    declarations: &Declarations,
) -> Result<Template, TemplateFailure> {
    let raw_lines = lines(source);
    let mut template_lines = Vec::with_capacity(raw_lines.len());

    for (index, line) in raw_lines.iter().enumerate() {
        match directive::scan(line.content) {
            DirectiveLine::Text => {
                template_lines.push(TemplateLine::Text(scan_line(line, declarations, index)?));
            }
            DirectiveLine::Hidden => {
                return Err(TemplateFailure::at(index, Reason::HiddenDirective));
            }
            DirectiveLine::Malformed => {
                return Err(TemplateFailure::at(index, Reason::DirectiveMalformed));
            }
            DirectiveLine::Directive(found) => {
                if line.terminator.is_empty() {
                    return Err(TemplateFailure::at(index, Reason::DirectiveUnterminated));
                }
                let option = match declarations.find(found.option.as_str()) {
                    Some(Declared::Set(option)) => option,
                    other => {
                        return Err(TemplateFailure::at(
                            index,
                            Reason::DirectiveNotSetOption {
                                name: found.option.as_str().to_owned(),
                                declared_as: other.map(Declared::kind),
                            },
                        ));
                    }
                };
                template_lines.push(TemplateLine::Directive {
                    indentation: found.indentation,
                    option,
                    terminator: line.terminator.to_owned(),
                });
            }
        }
    }

    Ok(Template {
        lines: template_lines,
        source: source.to_owned(),
    })
}

/// Parses a partial's `source` into a [`Partial`]: any line shaped like a
/// directive — well-formed or not — is [`Reason::DirectiveInPartial`], and a
/// non-empty partial's last line must be terminated. A line that reaches the
/// marker's shape only once an invisible character other than a space or a
/// tab — before it or inside it — is set aside is [`Reason::HiddenDirective`],
/// as it is in a file: what it hides is the question, whether or not a
/// directive could stand there.
pub(crate) fn parse_partial(
    source: &str,
    declarations: &Declarations,
) -> Result<Partial, TemplateFailure> {
    let raw_lines = lines(source);
    let mut partial_lines = Vec::with_capacity(raw_lines.len());

    for (index, line) in raw_lines.iter().enumerate() {
        match directive::scan(line.content) {
            DirectiveLine::Text => {
                partial_lines.push(scan_line(line, declarations, index).map_err(in_a_partial)?);
            }
            DirectiveLine::Hidden => {
                return Err(TemplateFailure::at(index, Reason::HiddenDirective));
            }
            DirectiveLine::Malformed | DirectiveLine::Directive(_) => {
                return Err(TemplateFailure::at(index, Reason::DirectiveInPartial));
            }
        }
    }

    if let Some(last) = raw_lines.last() {
        if last.terminator.is_empty() {
            return Err(TemplateFailure::at(
                raw_lines.len() - 1,
                Reason::PartialUnterminated,
            ));
        }
    }

    Ok(Partial {
        lines: partial_lines,
    })
}

/// Restates a line's refusal for a partial: a malformed placeholder there is
/// [`Reason::PlaceholderMalformedInPartial`], whose message names the fix a
/// partial can take, since a partial can never be declared verbatim. Every
/// other refusal reads the same in a partial as in a file.
fn in_a_partial(failure: TemplateFailure) -> TemplateFailure {
    if matches!(failure.reason, Reason::PlaceholderMalformed) {
        TemplateFailure {
            line: failure.line,
            reason: Reason::PlaceholderMalformedInPartial,
        }
    } else {
        failure
    }
}

/// Scans one line for placeholders, checking each names a declared `enum` or
/// `text` option, and finds the optional `text` option that drops the line.
///
/// Every placeholder is resolved before any is judged, so a malformed or
/// undeclared one later on the line is the refusal, not an optional's
/// company. A line holding an optional placeholder may then hold placeholders
/// for that option alone, since dropping it would otherwise drop a value the
/// wearer stated, or one the skeleton requires, without saying so.
fn scan_line(
    line: &Line<'_>,
    declarations: &Declarations,
    index: usize,
) -> Result<TextLine, TemplateFailure> {
    let segments = placeholder::scan(line.content)
        .map_err(|_malformed| TemplateFailure::at(index, Reason::PlaceholderMalformed))?;
    let mut pieces = Vec::with_capacity(segments.len());
    let mut named = Vec::new();
    for segment in segments {
        match segment {
            Segment::Literal(text) => pieces.push(Piece::Literal(text)),
            Segment::Placeholder(name) => {
                let fill = match declarations.find(name.as_str()) {
                    Some(Declared::Enum(option)) => Fill::Enum(option),
                    Some(Declared::Text(option)) => Fill::Text(option),
                    other @ (Some(Declared::Set(_)) | None) => {
                        return Err(TemplateFailure::at(
                            index,
                            Reason::PlaceholderNotFillOption {
                                name: name.as_str().to_owned(),
                                declared_as: other.map(Declared::kind),
                            },
                        ));
                    }
                };
                pieces.push(Piece::Placeholder(fill));
                named.push((name, fill));
            }
        }
    }

    let optional = named.iter().find_map(|(name, fill)| match fill {
        Fill::Text(option) if declarations.text_options()[*option].is_optional() => {
            Some((name, *option))
        }
        Fill::Text(_) | Fill::Enum(_) => None,
    });
    let Some((option_name, option)) = optional else {
        return Ok(TextLine::new(pieces, line.terminator.to_owned(), None));
    };
    if let Some((beside, _fill)) = named
        .iter()
        .find(|(_name, fill)| *fill != Fill::Text(option))
    {
        return Err(TemplateFailure::at(
            index,
            Reason::OptionalPlaceholderNotAlone {
                option: option_name.as_str().to_owned(),
                beside: beside.as_str().to_owned(),
            },
        ));
    }
    Ok(TextLine::new(
        pieces,
        line.terminator.to_owned(),
        Some(option),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        Declarations, Declared, Fill, Piece, Reason, TemplateLine, TextLine, parse_file,
        parse_partial,
    };
    use crate::skeleton::error::OptionKind;

    #[test]
    fn a_file_with_no_placeholder_or_directive_is_one_text_line_per_source_line() {
        let declarations = Declarations::for_test("", &[]);
        let template = parse_file("plain text\n", &declarations).expect("plain text must parse");
        assert_eq!(template.lines.len(), 1);
        assert!(
            matches!(&template.lines[0], TemplateLine::Text(line) if line.terminator() == "\n")
        );
    }

    #[test]
    fn a_directive_naming_an_undeclared_option_is_refused_at_its_line() {
        let declarations = Declarations::for_test("", &[]);
        let error = parse_file("before\n# skeletons:partial nope\n", &declarations)
            .expect_err("an undeclared directive option must be refused");
        assert_eq!(error.line.get(), 2);
        assert!(matches!(
            error.reason,
            Reason::DirectiveNotSetOption { name, declared_as: None } if name == "nope"
        ));
    }

    #[test]
    fn a_directive_on_an_unterminated_final_line_is_refused() {
        let declarations = Declarations::for_test(
            r#"
            [options.workflows]
            type = "set"
            default = []
            [[options.workflows.values]]
            value = "lint"
            partial = "lint.yml"
            "#,
            &["lint.yml"],
        );
        let error = parse_file("before\n# skeletons:partial workflows", &declarations)
            .expect_err("an unterminated directive must be refused");
        assert_eq!(error.line.get(), 2);
        assert!(matches!(error.reason, Reason::DirectiveUnterminated));
    }

    #[test]
    fn a_partial_with_a_directive_shaped_line_is_refused_even_when_malformed() {
        let declarations = Declarations::for_test("", &[]);
        let error = parse_partial("normal\n# skeletons:partail typo\n", &declarations).expect_err(
            "a directive-shaped line inside a partial must be refused, well-formed or not",
        );
        assert_eq!(error.line.get(), 2);
        assert!(matches!(error.reason, Reason::DirectiveInPartial));
    }

    #[test]
    fn an_empty_partial_parses_with_no_lines_and_no_refusal() {
        let declarations = Declarations::for_test("", &[]);
        let partial = parse_partial("", &declarations).expect("an empty partial is valid");
        assert!(partial.lines.is_empty());
    }

    #[test]
    fn a_non_empty_partial_missing_its_final_terminator_is_refused() {
        let declarations = Declarations::for_test("", &[]);
        let error = parse_partial("a\nb", &declarations)
            .expect_err("an unterminated non-empty partial must be refused");
        assert_eq!(error.line.get(), 2);
        assert!(matches!(error.reason, Reason::PartialUnterminated));
    }

    #[test]
    fn a_directive_behind_a_byte_order_mark_is_refused_at_line_one() {
        let declarations = Declarations::for_test("", &[]);
        let error = parse_file("\u{feff}# skeletons:partial nope\nrest\n", &declarations)
            .expect_err("a directive behind a byte-order mark on line 1 must be refused");
        assert_eq!(error.line.get(), 1);
        assert!(matches!(error.reason, Reason::HiddenDirective));
    }

    #[test]
    fn a_hidden_directive_in_a_partial_is_refused_as_hidden_not_as_in_a_partial() {
        // A partial can hold no directive, so a directive-shaped line there
        // is `DirectiveInPartial`; a hidden one is not directive-shaped, and
        // is refused for what it hides, the same as in a file.
        let declarations = Declarations::for_test("", &[]);
        let error = parse_partial("rest\n\u{a0}# skeletons:partial nope\n", &declarations)
            .expect_err("a hidden directive in a partial must be refused");
        assert_eq!(error.line.get(), 2);
        assert!(matches!(error.reason, Reason::HiddenDirective));
    }

    #[test]
    fn a_byte_order_mark_before_ordinary_text_parses_and_keeps_its_bytes() {
        let declarations = Declarations::for_test("", &[]);
        let source = "\u{feff}hello\n";
        let template = parse_file(source, &declarations)
            .expect("a byte-order mark before ordinary text must parse");
        assert_eq!(template.source, source);
    }

    #[test]
    fn a_hidden_directive_on_a_later_line_is_refused_at_that_line() {
        // Any line, not only the first: a byte-order mark in the middle of
        // a file, or a no-break space pasted in front of a directive.
        let declarations = Declarations::for_test("", &[]);
        for (source, line) in [
            ("a\n\u{feff}# skeletons:partial nope\n", 2),
            ("a\nb\n  \u{a0}# skeletons:partial nope\n", 3),
        ] {
            let error = parse_file(source, &declarations)
                .expect_err("a hidden directive on a later line must be refused");
            assert_eq!(error.line.get(), line, "{source:?}");
            assert!(
                matches!(error.reason, Reason::HiddenDirective),
                "{source:?}"
            );
        }
    }

    #[test]
    fn the_first_defective_line_is_the_one_refused() {
        // Hidden directives are found in the same pass as every other line
        // refusal, so whichever defect comes first is the one reported.
        let declarations = Declarations::for_test("", &[]);
        let error = parse_file("{{nope\n\u{a0}# skeletons:partial x\n", &declarations)
            .expect_err("a malformed placeholder on line 1 is refused first");
        assert_eq!(error.line.get(), 1);
        assert!(matches!(error.reason, Reason::PlaceholderMalformed));
        let error = parse_partial(
            "\u{a0}# skeletons:partial x\n# skeletons:partial y\n",
            &declarations,
        )
        .expect_err("a hidden directive on line 1 is refused first");
        assert_eq!(error.line.get(), 1);
        assert!(matches!(error.reason, Reason::HiddenDirective));
    }

    #[test]
    fn a_placeholder_naming_a_set_option_is_refused_as_the_wrong_kind() {
        let declarations = Declarations::for_test(
            r#"
            [options.workflows]
            type = "set"
            default = []
            [[options.workflows.values]]
            value = "lint"
            partial = "lint.yml"
            "#,
            &["lint.yml"],
        );
        let error = parse_file("{{workflows}}\n", &declarations)
            .expect_err("a placeholder naming a set option must be refused");
        let Reason::PlaceholderNotFillOption { name, declared_as } = error.reason else {
            panic!("expected a placeholder that fills nothing, got {error:?}")
        };
        assert_eq!(name, "workflows");
        assert_eq!(declared_as, Some(OptionKind::Set));
    }

    /// Declares two `text` options with no default, `assignee` and `reviewer`,
    /// beside a `text` with a default, an `enum` with one, and a `set`,
    /// `workflows`, that names the partial `lint.yml`.
    const TEXT_OPTIONS: &str = r#"
        [options.assignee]
        type = "text"

        [options.reviewer]
        type = "text"

        [options.owner]
        type = "text"
        default = "octocat"

        [options.cadence]
        type = "enum"
        values = ["daily", "weekly"]
        default = "weekly"

        [options.workflows]
        type = "set"
        default = []
        [[options.workflows.values]]
        value = "lint"
        partial = "lint.yml"
        "#;

    /// The refusal `source` earns as a file, as `(line, option, beside)`.
    fn not_alone(source: &str) -> (u32, String, String) {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let error = parse_file(source, &declarations)
            .expect_err("an optional placeholder beside another must be refused");
        let Reason::OptionalPlaceholderNotAlone { option, beside } = error.reason else {
            panic!(
                "expected an optional-not-alone refusal, got {:?}",
                error.reason
            )
        };
        (error.line.get(), option, beside)
    }

    #[test]
    fn an_optional_placeholder_beside_a_required_one_is_refused_whichever_comes_first() {
        // Both orders name the optional as `option` and the other as `beside`:
        // which one is written first is not what decides the roles.
        assert_eq!(
            not_alone("a\n{{assignee}} {{cadence}}\n"),
            (2, "assignee".to_owned(), "cadence".to_owned())
        );
        assert_eq!(
            not_alone("a\n{{cadence}} {{assignee}}\n"),
            (2, "assignee".to_owned(), "cadence".to_owned())
        );
        assert_eq!(
            not_alone("{{owner}}: {{assignee}}\n"),
            (1, "assignee".to_owned(), "owner".to_owned()),
            "a text with a default is required, so it is beside the optional too"
        );
    }

    #[test]
    fn two_different_optionals_on_one_line_are_named_in_reading_order() {
        assert_eq!(
            not_alone("{{reviewer}} {{assignee}}\n"),
            (1, "reviewer".to_owned(), "assignee".to_owned())
        );
        assert_eq!(
            not_alone("{{assignee}} {{reviewer}} {{assignee}}\n"),
            (1, "assignee".to_owned(), "reviewer".to_owned())
        );
    }

    #[test]
    fn an_optional_placeholder_beside_another_is_refused_in_a_partial_too() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let error = parse_partial("ok\n{{assignee}} {{cadence}}\n", &declarations)
            .expect_err("the rule holds inside a partial");
        assert_eq!(error.line.get(), 2);
        assert!(matches!(
            error.reason,
            Reason::OptionalPlaceholderNotAlone { option, beside }
                if option == "assignee" && beside == "cadence"
        ));
    }

    #[test]
    fn the_same_optional_twice_on_one_line_is_accepted_and_recorded_on_the_line() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let template = parse_file("[\"{{assignee}}\", \"{{assignee}}\"]\n", &declarations)
            .expect("one optional written twice is still one optional");
        let TemplateLine::Text(line) = &template.lines[0] else {
            panic!("a placeholder line is a text line")
        };
        let Some(Declared::Text(assignee)) = declarations.find("assignee") else {
            panic!("`assignee` is a declared text option")
        };
        assert_eq!(line.optional(), Some(assignee));
    }

    #[test]
    fn a_text_with_a_default_beside_an_enum_is_accepted_and_droppable_by_nothing() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let template = parse_file("{{owner}} runs {{cadence}}\n", &declarations)
            .expect("a defaulted text is required, so it may share a line");
        let TemplateLine::Text(line) = &template.lines[0] else {
            panic!("a placeholder line is a text line")
        };
        assert_eq!(line.optional(), None);
    }

    #[test]
    fn a_malformed_placeholder_on_the_line_of_an_optional_is_the_refusal() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let error = parse_file("{{assignee}} {{nope\n", &declarations)
            .expect_err("a malformed placeholder must be refused");
        assert!(matches!(error.reason, Reason::PlaceholderMalformed));
    }

    #[test]
    fn a_malformed_placeholder_is_its_own_reason_in_a_partial() {
        // The same line refuses as `PlaceholderMalformed` in a file and as
        // `PlaceholderMalformedInPartial` in a partial: the two messages name
        // different fixes, and only a file can be declared verbatim.
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let in_file = parse_file("ok\n${{ bad }}\n", &declarations)
            .expect_err("a malformed placeholder in a file must be refused");
        let in_partial = parse_partial("ok\n${{ bad }}\n", &declarations)
            .expect_err("a malformed placeholder in a partial must be refused");
        assert!(matches!(in_file.reason, Reason::PlaceholderMalformed));
        assert!(matches!(
            in_partial.reason,
            Reason::PlaceholderMalformedInPartial
        ));
        assert_eq!(in_file.line, in_partial.line, "both name the same line");
    }

    #[test]
    fn an_undeclared_placeholder_on_the_line_of_an_optional_is_the_refusal() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let error = parse_file("{{assignee}} {{nope}}\n", &declarations)
            .expect_err("an undeclared placeholder must be refused");
        assert!(matches!(
            error.reason,
            Reason::PlaceholderNotFillOption { name, declared_as: None } if name == "nope"
        ));
    }

    #[test]
    fn a_directive_naming_a_text_option_is_refused_as_the_wrong_kind() {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let error = parse_file("# skeletons:partial assignee\n", &declarations)
            .expect_err("a directive naming a text option must be refused");
        assert!(matches!(
            error.reason,
            Reason::DirectiveNotSetOption { name, declared_as: Some(OptionKind::Text) }
                if name == "assignee"
        ));
    }

    #[test]
    #[should_panic(expected = "every placeholder on a droppable line fills from its one optional")]
    fn building_a_droppable_line_holding_another_placeholder_panics() {
        // Parsing refuses such a line, so building one is a bug in the caller.
        let declarations = Declarations::for_test(TEXT_OPTIONS, &["lint.yml"]);
        let Some(Declared::Text(assignee)) = declarations.find("assignee") else {
            panic!("`assignee` is a declared text option")
        };
        let Some(Declared::Enum(cadence)) = declarations.find("cadence") else {
            panic!("`cadence` is a declared enum option")
        };
        let _never_built = TextLine::new(
            vec![
                Piece::Placeholder(Fill::Text(assignee)),
                Piece::Placeholder(Fill::Enum(cadence)),
            ],
            "\n".to_owned(),
            Some(assignee),
        );
    }
}
