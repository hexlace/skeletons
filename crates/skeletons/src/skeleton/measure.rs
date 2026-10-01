//! How many bytes a render comes to, and whose words it owes them to.
//!
//! One model of a render's length answers three questions:
//!
//! - the largest render any choice among a skeleton's own declared values
//!   could produce, checked before assembly begins;
//! - the length of one resolved render, which is checked against the limit
//!   before assembly begins and which assembly verifies its own output
//!   against;
//! - which of the wearer's own words a render that is too large owes its
//!   size to.

use super::choices::Resolved;
use super::declarations::{Declarations, Declared, SetOption, TextOption};
use super::error::{Reason, RenderError, SkeletonIdentity};
use super::keyed::{Key, Keyed};
use super::limits::{RENDERED_BYTES_MAX, RenderedByteBudget};
use super::shipped_file::ShippedFile;
use super::template::{Fill, Partial, Piece, Template, TemplateLine, TextLine};
use super::validated::ValidatedSkeleton;

/// Which render a template is being sized for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Sizing<'render> {
    /// The most any choice among the skeleton's own declared values could
    /// produce: every set value selected, every enum at its longest declared
    /// value, every optional line present, and every `text` at its default,
    /// or at nothing where it declares none, since a `text` value has no
    /// length the skeleton could declare. Bounds every [`Self::Chosen`]
    /// size for the same skeleton up to the wearer's own text, since every
    /// other term of a render's length is non-negative and only grows with
    /// the selection and with value length.
    Largest,
    /// What these resolved choices produce.
    Chosen(&'render Resolved),
}

/// Whether `line` is in the render under `sizing`: a line that drops when its
/// optional `text` option is unset is present only when the wearer stated
/// that option, and every line is present when sizing the largest render.
///
/// The one answer sizing and assembly both ask, so a line one of them drops
/// the other never counts.
pub(crate) fn line_is_present(line: &TextLine, sizing: Sizing<'_>) -> bool {
    match (line.optional(), sizing) {
        (None, Sizing::Largest | Sizing::Chosen(_)) | (Some(_), Sizing::Largest) => true,
        (Some(option), Sizing::Chosen(resolved)) => resolved.text_value(option).is_some(),
    }
}

/// One declared `set` option's precomputed contribution to every directive
/// that selects from it: the summed bytes of the relevant partials' own
/// content (every declared value under [`Sizing::Largest`], the selected
/// ones under [`Sizing::Chosen`]), and how many of their lines are
/// non-blank. These are the only two numbers a directive needs to price
/// itself at any indentation — see [`directive_length`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DirectiveTotal {
    /// The relevant partials' own bytes, before any directive's indentation
    /// is applied.
    bytes: u64,
    /// How many of the relevant partials' lines are non-blank — the count a
    /// directive's own indentation length gets multiplied by, since a blank
    /// line never gains indentation.
    non_blank_lines: u64,
}

/// Every declared `set` option's [`DirectiveTotal`], one for each option.
pub(crate) type DirectiveTotals = Keyed<SetOption, DirectiveTotal>;

/// A sum that would not fit in a `u64`: a size no real budget could ever
/// allow, so a caller checking against a budget treats it exactly like
/// exceeding it.
#[derive(Debug, Clone, Copy)]
struct Overflow;

/// Refuses `validated` if the largest render any choice could ever produce
/// would cross [`RENDERED_BYTES_MAX`], summing every file's own largest
/// render, in path order, and stopping at the first one that crosses it.
///
/// The work here is linear in what a render actually read: the directives a
/// file holds, the values every `set` option declares, and the lines of
/// every partial, each counted once — never once per directive that
/// happens to reference them. [`directive_totals`] computes that once,
/// before any file is sized, and it is the resulting rendered length that
/// gets checked against [`RENDERED_BYTES_MAX`].
///
/// Run before a wearer's choices are even looked at: this is a property of
/// the skeleton, checked the same way for every wearer, not of what any one
/// wearer happened to choose.
pub(crate) fn check_largest(
    skeleton: &SkeletonIdentity,
    validated: &ValidatedSkeleton,
) -> Result<(), RenderError> {
    let totals = directive_totals(validated, Sizing::Largest).ok_or_else(|| {
        RenderError::about_manifest(
            skeleton.clone(),
            Reason::TooManyRenderedBytes {
                bytes_max: RENDERED_BYTES_MAX,
            },
        )
    })?;

    let mut budget = RenderedByteBudget::new();
    for (path, file) in validated.files() {
        let too_large = || {
            RenderError::about_file(
                skeleton.clone(),
                format!("files/{path}"),
                Reason::TooManyRenderedBytes {
                    bytes_max: RENDERED_BYTES_MAX,
                },
            )
        };
        let length = rendered_length(file, validated.declarations(), Sizing::Largest, &totals)
            .ok_or_else(too_large)?;
        budget.reserve(length).map_err(|reason| {
            RenderError::about_file(skeleton.clone(), format!("files/{path}"), reason)
        })?;
    }
    Ok(())
}

/// Refuses `validated` if the render `resolved` produces would cross
/// [`RENDERED_BYTES_MAX`], summing every file's own chosen render in path
/// order exactly as [`check_largest`] sums the largest, and naming the
/// option the wearer set that contributes the most bytes to it.
///
/// Run after the wearer's choices resolve and before anything is assembled:
/// [`check_largest`] bounds every render the skeleton's own text could
/// make, but a wearer's `text` has no length the skeleton could declare, so
/// only the chosen render says whether that text fits. The work to name the
/// option runs only once the sum has already failed.
pub(crate) fn check_chosen(
    skeleton: &SkeletonIdentity,
    validated: &ValidatedSkeleton,
    resolved: &Resolved,
) -> Result<(), RenderError> {
    if chosen_render_fits(validated, resolved) {
        return Ok(());
    }
    Err(RenderError::about_choice(
        skeleton.clone(),
        Reason::ChoicesRenderTooLarge {
            bytes_max: RENDERED_BYTES_MAX,
            option: largest_stated_contributor(validated, resolved),
        },
    ))
}

/// Whether every file's render under `resolved` fits the render's size
/// limit together. A sum that overflows a `u64` does not fit.
fn chosen_render_fits(validated: &ValidatedSkeleton, resolved: &Resolved) -> bool {
    let sizing = Sizing::Chosen(resolved);
    let Some(totals) = directive_totals(validated, sizing) else {
        return false;
    };
    let mut budget = RenderedByteBudget::new();
    for file in validated.files().values() {
        let Some(length) = rendered_length(file, validated.declarations(), sizing, &totals) else {
            return false;
        };
        if budget.reserve(length).is_err() {
            return false;
        }
    }
    true
}

/// The name of the `text` option the wearer stated that contributes the most
/// bytes to the chosen render, ties going to the first in option-name order.
///
/// Options the skeleton's own default filled are passed over: a default
/// passed [`check_largest`], and naming it would tell the wearer to shorten
/// something they never wrote.
///
/// # Panics
///
/// If the wearer stated no `text` option, since then the chosen render is no
/// larger than the largest one [`check_largest`] already accepted, and this
/// is only ever asked about a chosen render that is too large.
fn largest_stated_contributor(validated: &ValidatedSkeleton, resolved: &Resolved) -> String {
    let contributions = text_contributions(validated, resolved);
    let mut largest: Option<(&str, u64)> = None;
    for (name, declared) in validated.declarations().names() {
        let Declared::Text(option) = declared else {
            continue;
        };
        if !resolved.text_is_stated(option) {
            continue;
        }
        let bytes = contributions[option];
        // Strictly greater, so a tie keeps the first option by name.
        if largest.is_none_or(|(_name, largest_bytes)| bytes > largest_bytes) {
            largest = Some((name.as_str(), bytes));
        }
    }
    let Some((name, _bytes)) = largest else {
        unreachable!(
            "a chosen render past the limit holds text the wearer stated: without it the render \
             is no larger than the largest one, which was accepted"
        )
    };
    name.to_owned()
}

/// How many bytes each `text` option's value contributes to the chosen
/// render: its value's length, once for every placeholder of it on a line
/// that render keeps. A placeholder in a partial counts once for every
/// directive that inserts the partial, which is once per directive naming
/// the `set` option whose selected value it belongs to.
///
/// Counted the way [`rendering::assemble`](super::rendering::assemble)
/// expands: directives are tallied per `set` option first and each selected
/// partial is walked once, so the work stays linear in the lines of the
/// skeleton however many directives share a partial. The test
/// `every_text_contribution_matches_the_occurrences_assembly_writes` holds
/// the two to each other, by counting what assembly actually wrote.
///
/// Counts and products saturate at `u64::MAX` rather than fail: a saturated
/// contribution only ever ranks its option first, and the render it belongs
/// to has already been refused.
fn text_contributions(
    validated: &ValidatedSkeleton,
    resolved: &Resolved,
) -> Keyed<TextOption, u64> {
    let declarations = validated.declarations();
    let sizing = Sizing::Chosen(resolved);
    let mut placeholders = declarations.text_options().map(|_key, _option| 0_u64);
    let mut directives = declarations.set_options().map(|_key, _option| 0_u64);

    for file in validated.files().values() {
        // A verbatim file is never scanned, so it holds no placeholder or
        // directive to count.
        let template = match file {
            ShippedFile::Templated(template) => template,
            ShippedFile::Verbatim(_bytes) => continue,
        };
        for line in &template.lines {
            match line {
                TemplateLine::Text(text_line) => {
                    count_text_placeholders(text_line, sizing, 1, &mut placeholders);
                }
                TemplateLine::Directive { option, .. } => {
                    directives[*option] = directives[*option].saturating_add(1);
                }
            }
        }
    }
    for (option, set_option) in declarations.set_options().iter() {
        let inserted_times = directives[option];
        for &value in set_option.values() {
            if inserted_times == 0 {
                continue;
            }
            if !resolved.is_selected(value) {
                continue;
            }
            let partial = validated.partial(declarations.set_values()[value].partial());
            for line in &partial.lines {
                count_text_placeholders(line, sizing, inserted_times, &mut placeholders);
            }
        }
    }

    placeholders.map(|option, count| {
        let value_length = resolved
            .text_value(option)
            .map_or(0, |text| text.as_str().len());
        contribution_bytes(value_length, *count)
    })
}

/// The bytes a `text` value of `value_length` bytes contributes when it is
/// written `count` times, saturating at `u64::MAX`.
const fn contribution_bytes(value_length: usize, count: u64) -> u64 {
    (value_length as u64).saturating_mul(count)
}

/// Adds `times` to the count of every `text` option for each placeholder of
/// it on `line`, when `line` is in the render under `sizing`.
fn count_text_placeholders(
    line: &TextLine,
    sizing: Sizing<'_>,
    times: u64,
    placeholders: &mut Keyed<TextOption, u64>,
) {
    if !line_is_present(line, sizing) {
        return;
    }
    for piece in line.pieces() {
        if let Piece::Placeholder(Fill::Text(option)) = piece {
            placeholders[*option] = placeholders[*option].saturating_add(times);
        }
    }
}

/// Precomputes every declared `set` option's [`DirectiveTotal`] under
/// `sizing`, once, before any file is sized. A directive later reads its
/// own option's total at O(1), instead of walking that option's declared
/// values and their partials itself — which is what keeps sizing a whole
/// skeleton linear in (directives + declared values + partial lines) rather
/// than multiplying directives by declared values.
pub(crate) fn directive_totals(
    validated: &ValidatedSkeleton,
    sizing: Sizing<'_>,
) -> Option<DirectiveTotals> {
    validated
        .declarations()
        .set_options()
        .try_map(|_key, set_option| set_option_total(set_option, validated, sizing))
        .ok()
}

/// One `set` option's [`DirectiveTotal`]: the sum, over its relevant
/// declared values, of each selected partial's own bytes and non-blank-line
/// count.
fn set_option_total(
    set_option: &SetOption,
    validated: &ValidatedSkeleton,
    sizing: Sizing<'_>,
) -> Result<DirectiveTotal, Overflow> {
    let declarations = validated.declarations();
    let mut total = DirectiveTotal::default();
    for &value in set_option.values() {
        let selected = match sizing {
            Sizing::Largest => true,
            Sizing::Chosen(resolved) => resolved.is_selected(value),
        };
        if !selected {
            continue;
        }
        let partial = validated.partial(declarations.set_values()[value].partial());
        let (partial_bytes, partial_non_blank_lines) =
            partial_totals(partial, declarations, sizing).ok_or(Overflow)?;
        total.bytes = total.bytes.checked_add(partial_bytes).ok_or(Overflow)?;
        total.non_blank_lines = total
            .non_blank_lines
            .checked_add(partial_non_blank_lines)
            .ok_or(Overflow)?;
    }
    Ok(total)
}

/// The bytes one partial contributes before any directive's indentation is
/// applied, paired with how many of its lines are non-blank: a blank line
/// contributes only its terminator and never gains indentation, so it is
/// excluded from the count [`directive_length`] multiplies indentation by.
/// A line [`line_is_present`] rules out contributes nothing.
fn partial_totals(
    partial: &Partial,
    declarations: &Declarations,
    sizing: Sizing<'_>,
) -> Option<(u64, u64)> {
    let mut bytes: u64 = 0;
    let mut non_blank_lines: u64 = 0;
    for line in &partial.lines {
        if !line_is_present(line, sizing) {
            continue;
        }
        if line.pieces().is_empty() {
            bytes = bytes.checked_add(line.terminator().len() as u64)?;
        } else {
            let pieces_length = pieces_length(line.pieces(), declarations, sizing)?;
            bytes = bytes
                .checked_add(pieces_length)?
                .checked_add(line.terminator().len() as u64)?;
            non_blank_lines = non_blank_lines.checked_add(1)?;
        }
    }
    Some((bytes, non_blank_lines))
}

/// The number of bytes `file` renders to under `sizing`, or `None` when the
/// total overflows `u64` — a size no real budget could ever allow, so a
/// caller checking against a budget treats it exactly like exceeding it.
///
/// A verbatim file renders to its own bytes under every sizing.
pub(crate) fn rendered_length(
    file: &ShippedFile,
    declarations: &Declarations,
    sizing: Sizing<'_>,
    totals: &DirectiveTotals,
) -> Option<u64> {
    match file {
        ShippedFile::Templated(template) => template_length(template, declarations, sizing, totals),
        ShippedFile::Verbatim(bytes) => Some(bytes.len() as u64),
    }
}

/// The number of bytes `template` renders to under `sizing`, or `None` on
/// overflow, as [`rendered_length`].
fn template_length(
    template: &Template,
    declarations: &Declarations,
    sizing: Sizing<'_>,
    totals: &DirectiveTotals,
) -> Option<u64> {
    let mut total: u64 = 0;
    for line in &template.lines {
        let line_length = match line {
            TemplateLine::Text(text_line) if !line_is_present(text_line, sizing) => 0,
            TemplateLine::Text(text_line) => {
                pieces_length(text_line.pieces(), declarations, sizing)?
                    .checked_add(text_line.terminator().len() as u64)?
            }
            TemplateLine::Directive {
                indentation,
                option,
                terminator: _,
            } => directive_length(indentation, *option, totals)?,
        };
        total = total.checked_add(line_length)?;
    }
    Some(total)
}

/// The number of bytes a directive expands to, at O(1): `totals` already
/// sums every selected partial's own bytes and non-blank-line count for
/// this directive's option, so pricing one more directive is just this
/// directive's own indentation, once per non-blank line, exactly as
/// `rendering::emit_directive` assembles it.
fn directive_length(
    indentation: &str,
    option: Key<SetOption>,
    totals: &DirectiveTotals,
) -> Option<u64> {
    let total = totals[option];
    let indentation_length = indentation.len() as u64;
    let indentation_total = indentation_length.checked_mul(total.non_blank_lines)?;
    total.bytes.checked_add(indentation_total)
}

/// The number of bytes one line's pieces expand to: a literal contributes
/// its own length, and a placeholder contributes the length of whichever
/// value it would be filled with under `sizing`.
fn pieces_length(pieces: &[Piece], declarations: &Declarations, sizing: Sizing<'_>) -> Option<u64> {
    let mut total: u64 = 0;
    for piece in pieces {
        let piece_length = match piece {
            Piece::Literal(text) => text.len() as u64,
            Piece::Placeholder(fill) => placeholder_length(*fill, declarations, sizing),
        };
        total = total.checked_add(piece_length)?;
    }
    Some(total)
}

/// The length of the value a placeholder would be filled with under
/// `sizing`.
///
/// For an `enum`, the longest declared value under [`Sizing::Largest`] and the
/// resolved value's own length under [`Sizing::Chosen`]. For a `text`, the
/// default's length, or none, under [`Sizing::Largest`], and the resolved
/// text's length under [`Sizing::Chosen`]; an unset optional has none, and is
/// only ever asked about on a line [`line_is_present`] has already dropped.
fn placeholder_length(fill: Fill, declarations: &Declarations, sizing: Sizing<'_>) -> u64 {
    let length = match (fill, sizing) {
        (Fill::Enum(option), Sizing::Largest) => {
            declarations.enum_options()[option].longest_value_length()
        }
        (Fill::Enum(option), Sizing::Chosen(resolved)) => {
            resolved.enum_value(option).as_str().len()
        }
        (Fill::Text(option), Sizing::Largest) => declarations.text_options()[option]
            .default()
            .map_or(0, |default| default.as_str().len()),
        (Fill::Text(option), Sizing::Chosen(resolved)) => {
            let Some(text) = resolved.text_value(option) else {
                unreachable!(
                    "an optional text is unset only on a line `line_is_present` has already \
                     dropped, and lengths are asked only of the lines it keeps"
                )
            };
            text.as_str().len()
        }
    };
    length as u64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    use super::{
        Sizing, check_largest, contribution_bytes, directive_totals, rendered_length,
        text_contributions,
    };
    use crate::skeleton::choices::{Choice, Choices, resolve};
    use crate::skeleton::declarations::{Declarations, Declared};
    use crate::skeleton::error::{Reason, SkeletonIdentity};
    use crate::skeleton::keyed::KeyedBuilder;
    use crate::skeleton::limits::RENDERED_BYTES_MAX;
    #[cfg(skeletons_checkout)]
    use crate::skeleton::load;
    use crate::skeleton::rendering::assemble;
    use crate::skeleton::shipped_file::ShippedFile;
    use crate::skeleton::template::{Partial, Template, TemplateLine, parse_file, parse_partial};
    use crate::skeleton::validated::ValidatedSkeleton;
    use crate::skeleton::walk::TreePath;

    /// A validated skeleton with one `set` option named `blocks`, declaring
    /// `values` many values, each mapping to its own genuinely empty
    /// partial (`p0`, `p1`, ...), and one file of `directives` many
    /// unindented directive lines all selecting from `blocks` — built
    /// entirely in memory, since what matters here is the shape a directive
    /// sizes against, not a real skeleton directory.
    fn many_directives_over_empty_partials(values: usize, directives: usize) -> ValidatedSkeleton {
        let mut toml_text = String::from("[options.blocks]\ntype = \"set\"\ndefault = []\n");
        let mut partial_names = Vec::with_capacity(values);
        for index in 0..values {
            // `write!` to a `String` never fails; the only `Err` it defines
            // is an allocation failure, which aborts the process before it
            // could ever be returned here.
            let _never_fails = write!(
                toml_text,
                "[[options.blocks.values]]\nvalue = \"v{index}\"\npartial = \"p{index}\"\n"
            );
            partial_names.push(format!("p{index}"));
        }
        let partial_names: Vec<&str> = partial_names.iter().map(String::as_str).collect();
        let (declarations, partial_paths) =
            Declarations::for_test_with_paths(&toml_text, &partial_names);
        let Some(Declared::Set(option)) = declarations.find("blocks") else {
            panic!("`blocks` is declared as a set option above")
        };

        let lines = (0..directives)
            .map(|_| TemplateLine::Directive {
                indentation: String::new(),
                option,
                terminator: "\n".to_owned(),
            })
            .collect();
        let mut files = BTreeMap::new();
        files.insert(
            TreePath::for_test(&["big.txt"]),
            ShippedFile::Templated(Template {
                lines,
                source: String::new(),
            }),
        );
        let partials = partial_paths.map(|_key, _path| Partial { lines: Vec::new() });

        ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            files,
            partials,
        )
        .expect("every directive selects nothing but empty partials; the largest render is tiny")
    }

    /// One optional `text` (`note`) and one with a default (`owner`).
    const TEXT_OPTIONS: &str = r#"
        [options.note]
        type = "text"

        [options.owner]
        type = "text"
        default = "octocat"
        "#;

    /// The length of `source` rendered under `sizing`, for `TEXT_OPTIONS`
    /// resolved against `choices`.
    fn length_of(source: &str, choices: &Choices, largest: bool) -> u64 {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &[]);
        let file =
            ShippedFile::Templated(parse_file(source, &declarations).expect("test source parses"));
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            choices,
        )
        .expect("test choices resolve");
        let sizing = if largest {
            Sizing::Largest
        } else {
            Sizing::Chosen(&resolved)
        };
        let validated = ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            BTreeMap::from([(TreePath::for_test(&["file.txt"]), file.clone())]),
            KeyedBuilder::new().finish(),
        )
        .expect("test skeleton validates");
        let totals = directive_totals(&validated, sizing).expect("no directives to overflow");
        rendered_length(&file, validated.declarations(), sizing, &totals).expect("test lengths fit")
    }

    #[test]
    fn the_largest_render_keeps_every_optional_line_and_prices_a_text_at_its_default() {
        // `note` has no default, so its line is present but its placeholder
        // adds nothing; `owner` adds its default's seven bytes.
        let source = "head\n{{note}}\nowner: {{owner}}\n";
        assert_eq!(
            length_of(source, &Choices::new(), true),
            ("head\n".len() + "\n".len() + "owner: octocat\n".len()) as u64
        );
    }

    #[test]
    fn a_chosen_render_drops_an_unset_optional_line_and_counts_a_set_one_by_its_value() {
        let source = "head\n[{{note}}]\nowner: {{owner}}\n";
        let owner_line = "owner: octocat\n".len();
        assert_eq!(
            length_of(source, &Choices::new(), false),
            ("head\n".len() + owner_line) as u64,
            "an unset optional line contributes nothing, terminator included"
        );

        let mut choices = Choices::new();
        choices.insert("note", Choice::One("hello".to_owned()));
        assert_eq!(
            length_of(source, &choices, false),
            ("head\n".len() + "[hello]\n".len() + owner_line) as u64
        );
    }

    /// A validated skeleton, built in memory, whose one file and one partial
    /// are `file_source` and `partial_source`, under `TEXT_CONTRIBUTION_OPTIONS`.
    fn contribution_skeleton(file_source: &str, partial_source: &str) -> ValidatedSkeleton {
        let (declarations, partial_paths) =
            Declarations::for_test_with_paths(TEXT_CONTRIBUTION_OPTIONS, &["note.yml"]);
        let file = parse_file(file_source, &declarations).expect("test file parses");
        let partials = partial_paths.map(|_key, _path| {
            parse_partial(partial_source, &declarations).expect("test partial parses")
        });
        ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            BTreeMap::from([(
                TreePath::for_test(&["notes.yml"]),
                ShippedFile::Templated(file),
            )]),
            partials,
        )
        .expect("test skeleton validates")
    }

    /// Three `text` options and one `set` option selecting the partial
    /// `note.yml`: `note` and `other` unset by default, `ghost` too.
    const TEXT_CONTRIBUTION_OPTIONS: &str = r#"
        [options.note]
        type = "text"

        [options.other]
        type = "text"

        [options.ghost]
        type = "text"

        [options.blocks]
        type = "set"
        default = ["note"]
        [[options.blocks.values]]
        value = "note"
        partial = "note.yml"
        "#;

    /// The skeleton `a_text_contributes_its_length_once_per_placeholder_the_render_keeps`
    /// counts by hand: two directives inserting one partial of `note`
    /// placeholders, beside file lines of `other` and a dropped one of `ghost`.
    fn shared_partial_skeleton() -> ValidatedSkeleton {
        contribution_skeleton(
            "a: {{other}}\nb: {{other}} {{other}}\ndrop: {{ghost}}\n\
             # skeletons:partial blocks\n# skeletons:partial blocks\n",
            "one: {{note}}\ntwo: {{note}}\nnone: {{ghost}}\nkeep: kept\n",
        )
    }

    /// A skeleton whose only directive inserts a partial holding `note`, which
    /// a render choosing no value of `blocks` never inserts.
    fn uninserted_partial_skeleton() -> ValidatedSkeleton {
        contribution_skeleton(
            "# skeletons:partial blocks\nplain: {{ghost}}\nfill: {{other}}\n",
            "one: {{note}}\n",
        )
    }

    /// The contribution of the `text` option `name` to the render of
    /// `validated` for `choices`.
    fn contribution_of(validated: &ValidatedSkeleton, choices: &Choices, name: &str) -> u64 {
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            validated.declarations(),
            choices,
        )
        .expect("test choices resolve");
        let Some(Declared::Text(option)) = validated.declarations().find(name) else {
            panic!("{name} must be a declared text option")
        };
        text_contributions(validated, &resolved)[option]
    }

    #[test]
    fn a_text_contributes_its_length_once_per_placeholder_the_render_keeps() {
        // Counted by hand from the two sources below. The file holds two
        // directives of `blocks`, each inserting the one selected partial,
        // and two lines of `{{other}}`, one of them holding it twice.
        //
        // - `other` (4 bytes, `wxyz`): 3 placeholders in the file, none in the
        //   partial, so 3 x 4 = 12.
        // - `note` (3 bytes, `abc`): 2 placeholders in the partial, which two
        //   directives insert, so 2 x 2 = 4 placeholders and 4 x 3 = 12. Counted
        //   once per partial it would be 6, and only 4 x 3 tells them apart.
        // - `ghost` (unset): its partial line is dropped and so is its file
        //   line, so it holds no placeholder in the render and contributes 0.
        let validated = shared_partial_skeleton();
        let mut choices = Choices::new();
        choices.insert("note", Choice::One("abc".to_owned()));
        choices.insert("other", Choice::One("wxyz".to_owned()));

        assert_eq!(contribution_of(&validated, &choices, "other"), 12);
        assert_eq!(contribution_of(&validated, &choices, "note"), 12);
        assert_eq!(contribution_of(&validated, &choices, "ghost"), 0);
    }

    #[test]
    fn a_partial_no_directive_inserts_contributes_nothing() {
        // The set option `blocks` is chosen empty, so the partial is inserted
        // by no directive and `note` holds no placeholder in the render.
        // `ghost` and `other` are only there so that every declared option is
        // used.
        let validated = uninserted_partial_skeleton();
        let mut choices = Choices::new();
        choices.insert("note", Choice::One("abc".to_owned()));
        choices.insert("blocks", Choice::Many(Vec::new()));

        assert_eq!(contribution_of(&validated, &choices, "note"), 0);
    }

    /// One optional `text` (`note`) and one with a default (`owner`), both
    /// on lines of their own, for the completeness test below.
    fn optional_lines_skeleton() -> ValidatedSkeleton {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &[]);
        let template = parse_file("head\n[{{note}}]\nowner: {{owner}}\n", &declarations)
            .expect("test source parses");
        ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            BTreeMap::from([(
                TreePath::for_test(&["file.txt"]),
                ShippedFile::Templated(template),
            )]),
            KeyedBuilder::new().finish(),
        )
        .expect("test skeleton validates")
    }

    /// The `text` options `validated` declares, as `(name, has a default)`.
    fn text_options_of(validated: &ValidatedSkeleton) -> Vec<(String, bool)> {
        let declarations = validated.declarations();
        let mut options = Vec::new();
        for (name, declared) in declarations.names() {
            if let Declared::Text(option) = declared {
                options.push((
                    name.as_str().to_owned(),
                    declarations.text_options()[option].default().is_some(),
                ));
            }
        }
        options
    }

    /// The marker the option at `position` is given: `position + 1` copies of
    /// one private-use character, so each option's marker differs from every
    /// other in both spelling and length, and none is a character any test
    /// source writes.
    fn marker(position: usize) -> String {
        let offset = u32::try_from(position).expect("a handful of options");
        let character = char::from_u32(0xE000 + offset).expect("private use");
        character.to_string().repeat(position + 1)
    }

    /// Holds `text_contributions` to what assembly writes: for every way of
    /// stating the options that have no default, and for every choice of
    /// `set` values in `set_choices`, gives each stated option its marker,
    /// assembles the render, and checks each option's contribution against
    /// how many times its marker occurs in the assembled bytes times the
    /// marker's length. An option with a default is always stated, so that
    /// no default text stands in the render where a marker was expected.
    fn assert_contributions_match_assembly(validated: &ValidatedSkeleton, set_choices: &[Choices]) {
        let options = text_options_of(validated);
        let identity = SkeletonIdentity::Named("test".to_owned());
        for base in set_choices {
            for mask in 0..(1_u32 << options.len()) {
                let mut choices = base.clone();
                for (position, (name, has_default)) in options.iter().enumerate() {
                    if *has_default || mask & (1 << position) != 0 {
                        choices.insert(name.clone(), Choice::One(marker(position)));
                    }
                }
                let resolved =
                    resolve(&identity, validated.declarations(), &choices).expect("resolves");
                let mut assembled = String::new();
                for (_path, bytes) in assemble(validated, &resolved).iter() {
                    assembled.push_str(std::str::from_utf8(bytes).expect("assembled text"));
                }
                let contributions = text_contributions(validated, &resolved);
                for (position, (name, _has_default)) in options.iter().enumerate() {
                    let Some(Declared::Text(option)) = validated.declarations().find(name) else {
                        panic!("{name} is a declared text option")
                    };
                    let marker = marker(position);
                    let occurrences = assembled.matches(&marker).count() as u64;
                    assert_eq!(
                        contributions[option],
                        occurrences * marker.len() as u64,
                        "option `{name}` with options {mask:b} stated and {base:?}: its \
                         contribution must be its marker's occurrences in the assembled render \
                         times its length"
                    );
                }
            }
        }
    }

    #[test]
    fn every_text_contribution_matches_the_occurrences_assembly_writes() {
        // The oracle is assembly itself, not a second expansion of the
        // directives: each option's value is a marker found nowhere else, so
        // counting the markers in the assembled bytes says how many times
        // assembly wrote the value. Runs over the in-memory skeletons and
        // over the checked-in ones that put a placeholder in a partial, with
        // each option stated and unstated, and with the `set` option
        // inserting its partial and inserting nothing.
        let no_values = {
            let mut choices = Choices::new();
            choices.insert("blocks", Choice::Many(Vec::new()));
            choices
        };
        assert_contributions_match_assembly(
            &shared_partial_skeleton(),
            &[Choices::new(), no_values.clone()],
        );
        assert_contributions_match_assembly(
            &uninserted_partial_skeleton(),
            &[Choices::new(), no_values],
        );
        assert_contributions_match_assembly(&optional_lines_skeleton(), &[Choices::new()]);

        // The checked-in skeletons are read from `test-skeletons/`, which the
        // published package leaves out, so only a checkout runs this half.
        #[cfg(skeletons_checkout)]
        check_checked_in_skeletons_match_assembly();
    }

    #[cfg(skeletons_checkout)]
    fn check_checked_in_skeletons_match_assembly() {
        let no_ecosystems = {
            let mut choices = Choices::new();
            choices.insert("ecosystems", Choice::Many(Vec::new()));
            choices
        };
        for (relative, set_choices) in [
            (
                "renders/text-partial-in-two-directives",
                vec![Choices::new(), {
                    let mut choices = Choices::new();
                    choices.insert("blocks", Choice::Many(Vec::new()));
                    choices
                }],
            ),
            (
                "renders/text-optional-in-partial",
                vec![Choices::new(), no_ecosystems],
            ),
        ] {
            let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test-skeletons")
                .join(relative);
            let (_identity, validated) = load(&directory).expect("the checked-in skeleton loads");
            assert_contributions_match_assembly(&validated, &set_choices);
        }
    }

    #[test]
    fn a_contribution_saturates_at_the_largest_size_instead_of_wrapping() {
        // Products at the boundary of `u64`: a product that fits is exact, the
        // first that does not is `u64::MAX`, and neither zero factor is
        // disturbed by saturation.
        let half = u64::MAX / 2;
        assert_eq!(
            contribution_bytes(2, half),
            u64::MAX - 1,
            "the last product that fits"
        );
        assert_eq!(
            contribution_bytes(2, half + 1),
            u64::MAX,
            "the first that does not"
        );
        assert_eq!(
            contribution_bytes(1, u64::MAX),
            u64::MAX,
            "a factor of one is exact"
        );
        assert_eq!(contribution_bytes(usize::MAX, u64::MAX), u64::MAX);
        assert_eq!(
            contribution_bytes(0, u64::MAX),
            0,
            "no value contributes nothing"
        );
        assert_eq!(
            contribution_bytes(7, 0),
            0,
            "no placeholder contributes nothing"
        );
    }

    /// How long one `check_largest` of `validated` takes.
    fn time_check_largest(validated: &ValidatedSkeleton) -> Duration {
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let started = Instant::now();
        check_largest(&skeleton, validated).expect(
            "every directive selects nothing but empty partials; the largest render is tiny",
        );
        started.elapsed()
    }

    /// Directives in each timed skeleton: enough that per-directive work is
    /// most of what gets timed.
    const DIRECTIVES: usize = 50_000;

    /// How many times each skeleton is timed. The fastest run of each is the one
    /// least disturbed by anything else the machine was doing: noise only
    /// ever adds time.
    const TIMED_RUNS: u32 = 5;

    /// The most the skeleton declaring a hundred times as many values may take,
    /// as a multiple of the other: linear sizing is about 1, sizing that
    /// walks an option's declared values at every directive about 100.
    const RATIO_MAX: f64 = 10.0;

    #[test]
    fn sizing_a_skeleton_costs_the_same_however_many_values_its_option_declares() {
        // Two skeletons with the same 50,000 directives, all selecting from one
        // `set` option whose partials are empty: one declares 10 values,
        // the other 1,000. Sizing is linear in (directives + declared
        // values + partial lines), so the two take about the same time;
        // sizing that walked the option's declared values at every
        // directive would take about a hundred times longer on the second.
        // What is asserted is the ratio of the two, never a duration, so
        // the bound holds on any machine and in any build profile. Each
        // skeleton is timed as the fastest of several runs, taken in turn so
        // that a slow moment on the machine lands on both, and each has
        // been sized once already by `ValidatedSkeleton::validate`, so neither
        // timing pays for a cold cache the other does not.
        let few_values = many_directives_over_empty_partials(10, DIRECTIVES);
        let many_values = many_directives_over_empty_partials(1_000, DIRECTIVES);

        let mut few = Duration::MAX;
        let mut many = Duration::MAX;
        for _ in 0..TIMED_RUNS {
            few = few.min(time_check_largest(&few_values));
            many = many.min(time_check_largest(&many_values));
        }

        let ratio = many.as_secs_f64() / few.as_secs_f64();
        assert!(
            ratio < RATIO_MAX,
            "sizing with 1,000 declared values took {many:?} against {few:?} with 10, a ratio \
             of {ratio:.1}: directive_length must read its option's precomputed total, not \
             walk the option's declared values"
        );
    }

    #[test]
    fn a_verbatim_files_bytes_count_toward_the_largest_render_limit() {
        // Verifies that a verbatim file is sized by its byte length: one
        // file of exactly the limit fits, and one byte more is refused as a
        // render past the limit, naming that file. Exercised by validating
        // a skeleton whose only file is verbatim, at each length.
        let validate = |length: u64| {
            let declarations = Declarations::for_test_verbatim("", &[], &["big.bin"]);
            let files = BTreeMap::from([(
                TreePath::for_test(&["big.bin"]),
                ShippedFile::Verbatim(vec![0_u8; usize::try_from(length).expect("fits")]),
            )]);
            ValidatedSkeleton::validate(
                &SkeletonIdentity::Named("test".to_owned()),
                declarations,
                files,
                KeyedBuilder::new().finish(),
            )
        };

        validate(RENDERED_BYTES_MAX).expect("a verbatim file of exactly the limit fits");
        let error =
            validate(RENDERED_BYTES_MAX + 1).expect_err("one byte past the limit must be refused");

        assert_eq!(error.file(), Some("files/big.bin"));
        assert!(
            matches!(error.reason(), Reason::TooManyRenderedBytes { .. }),
            "expected a rendered-byte refusal, got {error:?}"
        );
    }

    #[test]
    fn a_verbatim_file_is_its_byte_length_under_every_sizing() {
        // Verifies that the length of a verbatim file does not depend on the
        // sizing or on any choice: it is the byte length under the largest
        // render and under a chosen one alike.
        let file = ShippedFile::Verbatim(vec![0xFF; 7]);
        let validated = ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            Declarations::for_test_verbatim("", &[], &["v.bin"]),
            BTreeMap::from([(TreePath::for_test(&["v.bin"]), file.clone())]),
            KeyedBuilder::new().finish(),
        )
        .expect("test skeleton validates");
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            validated.declarations(),
            &Choices::new(),
        )
        .expect("test choices resolve");

        for sizing in [Sizing::Largest, Sizing::Chosen(&resolved)] {
            let totals = directive_totals(&validated, sizing).expect("no directives to overflow");
            assert_eq!(
                rendered_length(&file, validated.declarations(), sizing, &totals),
                Some(7)
            );
        }
    }
}
