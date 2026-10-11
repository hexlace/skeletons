//! Optional `text` options: a `text` option takes the wearer's own words, and
//! when it declares no default, leaving it unset drops every line holding its
//! placeholder. A line holding an optional placeholder holds nothing else
//! that is a placeholder, and a value the wearer typed never reopens the
//! grammar it was filled into.
//!
//! Every scenario reads a real skeleton crate under `test-skeletons/`.
//! `renders/text-optional` declares one option, `assignee`, of type `text`
//! with no default, and its only file, `dependabot.yml`, holds:
//!
//! ```text
//! version: 2
//! assignees: ["{{assignee}}"]
//! open-pull-requests-limit: 5
//! reviewers: ["{{assignee}}"]
//! ```

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, OptionKind, Reason, SkeletonIdentity, render};

fn assignee(value: &str) -> Choices {
    let mut choices = Choices::new();
    choices.insert("assignee", Choice::One(value.to_owned()));
    choices
}

#[test]
fn a_set_optional_text_value_fills_every_line_holding_its_placeholder() {
    // The wearer states `octocat` for an option that declares no values and no
    // default; both lines holding the placeholder carry it, verbatim.
    let rendering = render(test_skeleton("renders/text-optional"), &assignee("octocat"))
        .expect("a wearer's non-empty text without control characters must render");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(
            b"version: 2\nassignees: [\"octocat\"]\nopen-pull-requests-limit: 5\n\
              reviewers: [\"octocat\"]\n"
                .as_slice()
        ),
        "the wearer's text must fill every placeholder, exactly as an enum's value does"
    );
}

#[test]
fn a_text_value_is_taken_verbatim_whatever_printable_characters_it_holds() {
    // No patterns, no length bounds: spaces, quotes, non-ASCII text and a long
    // value all fill the placeholder unchanged. Escaping for the target format
    // is the skeleton author's business, not the tool's.
    let long = "x".repeat(4096);
    for value in [
        "two words",
        "quote\"inside",
        "back\\slash",
        "  padded  ",
        "h\u{e9}l\u{e8}ne \u{2603}",
        long.as_str(),
    ] {
        let rendering = render(test_skeleton("renders/text-optional"), &assignee(value))
            .unwrap_or_else(|error| panic!("`{value}` must be accepted as text: {error:?}"));

        let expected = format!(
            "version: 2\nassignees: [\"{value}\"]\nopen-pull-requests-limit: 5\n\
             reviewers: [\"{value}\"]\n"
        );
        assert_eq!(
            rendering.get("dependabot.yml"),
            Some(expected.as_bytes()),
            "`{value}` must be filled in verbatim"
        );
    }
}

#[test]
fn an_unset_optional_text_drops_every_line_holding_it_terminator_included() {
    // The wearer sets nothing. Both lines holding `{{assignee}}` vanish whole,
    // leaving exactly the lines that never held it: no blank line, no
    // `assignees:` husk.
    let rendering = render(test_skeleton("renders/text-optional"), &Choices::new())
        .expect("an optional text left unset must render, not be refused");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\nopen-pull-requests-limit: 5\n".as_slice()),
        "both lines holding the unset optional must be gone, terminators with them"
    );
}

#[test]
fn an_unset_optional_inside_a_selected_partial_drops_its_line_there() {
    // The placeholder lives in `partials/cargo.yml`, inserted by the file's
    // directive. Unset, that one line leaves the inserted block; the rest of
    // the block, and the file around it, are untouched.
    let rendering = render(
        test_skeleton("renders/text-optional-in-partial"),
        &Choices::new(),
    )
    .expect("an unset optional inside a partial must render");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(
            b"updates:\n  - package-ecosystem: \"cargo\"\n    directory: \"/\"\nend: true\n"
                .as_slice()
        ),
        "the partial's optional line must be dropped, indentation of the rest kept"
    );
}

#[test]
fn a_set_optional_inside_a_selected_partial_fills_at_the_directives_indentation() {
    // The same partial, with the option set: the line stays, filled, and takes
    // the directive's indentation like every other line of the partial.
    let rendering = render(
        test_skeleton("renders/text-optional-in-partial"),
        &assignee("octocat"),
    )
    .expect("a set optional inside a partial must render");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(
            b"updates:\n  - package-ecosystem: \"cargo\"\n    assignees: [\"octocat\"]\n    \
              directory: \"/\"\nend: true\n"
                .as_slice()
        ),
        "the partial's optional line must be filled and indented"
    );
}

#[test]
fn the_same_optional_twice_on_one_line_is_accepted_and_drops_the_line_once() {
    // One optional, written twice on one line, is still one optional: dropping
    // the line can only ever drop that one value. Unset, the line goes once,
    // and the lines around it stay.
    let rendering = render(
        test_skeleton("renders/text-optional-twice-on-one-line"),
        &Choices::new(),
    )
    .expect("one optional written twice on a line must be accepted");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\nopen-pull-requests-limit: 5\n".as_slice()),
        "the line must be dropped exactly once, neighbours intact"
    );
}

#[test]
fn the_same_optional_twice_on_one_line_fills_both_places_when_set() {
    // The accepting half of the rule above: set, both placeholders on the line
    // are filled.
    let rendering = render(
        test_skeleton("renders/text-optional-twice-on-one-line"),
        &assignee("octocat"),
    )
    .expect("one optional written twice on a line must be accepted");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(
            b"version: 2\nassignees: [\"octocat\", \"octocat\"]\nopen-pull-requests-limit: 5\n"
                .as_slice()
        ),
        "both placeholders on the line must be filled"
    );
}

#[test]
fn an_unset_optional_on_an_unterminated_last_line_drops_cleanly() {
    // The file's last line holds the placeholder and has no terminator. Dropping
    // it leaves the preceding line, terminated as it was, and nothing else: no
    // stray newline, no half-line.
    let rendering = render(
        test_skeleton("renders/text-optional-on-unterminated-last-line"),
        &Choices::new(),
    )
    .expect("an unset optional on an unterminated last line must render");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\n".as_slice()),
        "the unterminated last line must vanish and leave the file cleanly terminated"
    );
}

#[test]
fn a_set_optional_on_an_unterminated_last_line_keeps_the_missing_terminator() {
    // Set, the line is filled and stays unterminated, as its author wrote it.
    let rendering = render(
        test_skeleton("renders/text-optional-on-unterminated-last-line"),
        &assignee("octocat"),
    )
    .expect("a set optional on an unterminated last line must render");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\nassignees: [\"octocat\"]".as_slice()),
        "a filled last line must stay unterminated"
    );
}

#[test]
fn a_text_with_a_default_fills_from_it_when_the_wearer_sets_nothing() {
    // A `text` that declares a default is a required option, not an optional
    // one: its line is never dropped, and the default fills it.
    let rendering = render(test_skeleton("renders/text-with-default"), &Choices::new())
        .expect("a text with a default must render with no choice");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\nassignees: [\"octocat\"]\nopen-pull-requests-limit: 5\n".as_slice()),
        "the default must fill the placeholder and the line must stay"
    );
}

#[test]
fn a_text_with_a_default_fills_from_the_wearers_value_when_set() {
    // The wearer's own text wins over the declared default.
    let rendering = render(
        test_skeleton("renders/text-with-default"),
        &assignee("hubot"),
    )
    .expect("a text with a default must render with the wearer's value");

    assert_eq!(
        rendering.get("dependabot.yml"),
        Some(b"version: 2\nassignees: [\"hubot\"]\nopen-pull-requests-limit: 5\n".as_slice()),
        "the wearer's value must fill the placeholder, not the default"
    );
}

#[test]
fn a_text_value_holding_a_placeholder_is_written_verbatim_not_filled_again() {
    // `cadence` is a real enum option (default weekly). The wearer's note is
    // the text `{{cadence}}`, and the skeleton's `{{note}}` line must carry
    // those eleven characters as they are, while the skeleton's own
    // `{{cadence}}` line is filled as usual.
    let mut choices = Choices::new();
    choices.insert("note", Choice::One("{{cadence}}".to_owned()));

    let rendering = render(test_skeleton("renders/text-value-not-rescanned"), &choices)
        .expect("a text value that looks like a placeholder must not be rescanned");

    assert_eq!(
        rendering.get("notes.yml"),
        Some(b"{{cadence}}\ncadence: weekly\njobs:\ncargo-job: build\n".as_slice()),
        "the wearer's `{{cadence}}` must appear verbatim, never expanded"
    );
}

#[test]
fn a_text_value_that_is_a_directive_is_written_verbatim_not_selected() {
    // The wearer's note is `# skeletons:partial ecosystems`, naming a real set
    // option, alone on its line in the rendered output. It must stay text: the
    // partial is inserted once, by the skeleton's own directive, and never a
    // second time by the wearer's text.
    let mut choices = Choices::new();
    choices.insert(
        "note",
        Choice::One("# skeletons:partial ecosystems".to_owned()),
    );

    let rendering = render(test_skeleton("renders/text-value-not-rescanned"), &choices)
        .expect("a text value that looks like a directive must not be rescanned");

    assert_eq!(
        rendering.get("notes.yml"),
        Some(
            b"# skeletons:partial ecosystems\ncadence: weekly\njobs:\ncargo-job: build\n"
                .as_slice()
        ),
        "the wearer's directive-shaped text must appear verbatim, never acted on"
    );
}

#[test]
fn a_wearer_value_for_a_text_option_that_is_empty_is_refused_naming_the_option() {
    // Empty is not a value: it is refused as the wearer's choice, so names the
    // skeleton and the option but no file and no line. The same skeleton renders
    // with a real value, so what is refused is the empty string and nothing else.
    render(test_skeleton("renders/text-optional"), &assignee("octocat"))
        .expect("the skeleton itself must render with a real value");

    let error = render(test_skeleton("renders/text-optional"), &assignee(""))
        .expect_err("an empty value for a text option must be refused");

    assert_eq!(
        error.skeleton(),
        &SkeletonIdentity::Named("text-optional".to_owned())
    );
    assert_eq!(error.file(), None, "a wearer's choice names no file");
    assert_eq!(error.line(), None, "a wearer's choice names no line");
    assert!(
        matches!(
            error.reason(),
            Reason::TextChoiceInvalid { option, value } if option == "assignee" && value.is_empty()
        ),
        "expected a text-choice refusal naming `assignee` and the empty value, got {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("option `assignee` was given an empty value"),
        "the message must say the option was given an empty value, got {error}"
    );
}

#[test]
fn a_wearer_value_holding_a_control_character_is_refused_naming_the_option() {
    // A newline would break the one-line guarantee; a tab and a stray `\x01`
    // are control characters too. Each is refused as the wearer's choice,
    // naming the option and the value, and neither a file nor a line.
    render(test_skeleton("renders/text-optional"), &assignee("octocat"))
        .expect("the skeleton itself must render with a real value");

    for (label, value) in [
        ("newline", "octo\ncat"),
        ("tab", "octo\tcat"),
        ("start of heading", "octo\u{1}cat"),
    ] {
        let error = render(test_skeleton("renders/text-optional"), &assignee(value))
            .expect_err(&format!("a value holding a {label} must be refused"));

        assert_eq!(
            error.file(),
            None,
            "{label}: a wearer's choice names no file"
        );
        assert_eq!(
            error.line(),
            None,
            "{label}: a wearer's choice names no line"
        );
        assert!(
            matches!(
                error.reason(),
                Reason::TextChoiceInvalid { option, value: refused }
                    if option == "assignee" && refused == value
            ),
            "{label}: expected a text-choice refusal naming `assignee` and the value, got {error:?}"
        );
    }

    let error = render(
        test_skeleton("renders/text-optional"),
        &assignee("octo\ncat"),
    )
    .expect_err("a value holding a newline must be refused");
    assert!(
        error
            .to_string()
            .contains("option `assignee` was given `octo\\ncat`, which holds a control character"),
        "the message must show the value escaped, got {error}"
    );
}

#[test]
fn a_wearer_giving_a_text_option_an_array_is_refused_as_the_wrong_shape() {
    // A `text` option is set with one string. A list is the wrong shape for
    // it, refused the way an array given to an `enum` is: naming the option.
    let mut choices = Choices::new();
    choices.insert("assignee", Choice::Many(vec!["octocat".to_owned()]));

    let error = render(test_skeleton("renders/text-optional"), &choices)
        .expect_err("an array for a text option must be refused");

    assert_eq!(error.file(), None, "a wearer's choice names no file");
    assert!(
        matches!(
            error.reason(),
            Reason::ChoiceShapeMismatch { option, declared: OptionKind::Text }
                if option == "assignee"
        ),
        "expected a wrong-shape refusal naming the option as declared `text`, got {error:?}"
    );
}

#[test]
fn a_text_option_declaring_values_is_refused_naming_the_stray_key() {
    // A `text` option has no `values`: the manifest is a closed schema, and
    // `values` is a key `text` does not take. Refused as an unknown key, at its
    // dotted path, like any other key the schema does not recognise.
    let error = render(
        test_skeleton("refused/text-declares-values"),
        &Choices::new(),
    )
    .expect_err("a text option that declares values must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::UnknownKey { key } if key.contains("assignee") && key.contains("values")
        ),
        "expected an unknown-key refusal naming the option and `values`, got {error:?}"
    );
}

#[test]
fn an_enum_requires_its_default_beside_an_optional_text() {
    // Optionality is `text`-only. The skeleton declares an optional `text`
    // alongside an `enum` that omits its default; the enum is refused for the
    // missing default, whatever the `text` beside it does.
    let error = render(
        test_skeleton("refused/enum-without-default-beside-text"),
        &Choices::new(),
    )
    .expect_err("an enum with no default must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::MissingKey { key } if key.contains("cadence") && key.contains("default")
        ),
        "expected a missing-key refusal naming the enum and its default, got {error:?}"
    );
}

#[test]
fn a_text_option_used_by_no_placeholder_is_refused_naming_it() {
    // Every declared option must be used by something; a `text` is used by a
    // placeholder, the same as an `enum`.
    let error = render(test_skeleton("refused/text-option-unused"), &Choices::new())
        .expect_err("a text option that nothing uses must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::OptionUnused { option } if option == "assignee"),
        "expected an option-unused refusal naming the text option, got {error:?}"
    );
}

/// Asserts that rendering the refused skeleton `name` with no choices is
/// refused at `file`, line `line`, as an optional placeholder that is not
/// alone on its line, naming `(option, beside)`.
///
/// `option` is the first optional placeholder on the line in reading order,
/// and `beside` the first placeholder on it naming anything else, so a line
/// holding two optionals names them in the order its author wrote them.
fn assert_refused_at(name: &str, file: &str, line: u32, pair: (&str, &str)) {
    let error = render(test_skeleton(&format!("refused/{name}")), &Choices::new())
        .expect_err("a line holding an optional placeholder beside another must be refused");

    assert_eq!(
        error.skeleton(),
        &SkeletonIdentity::Named(name.to_owned()),
        "the refusal must name the skeleton"
    );
    assert_eq!(error.file(), Some(file), "the refusal must name the file");
    assert_eq!(
        error.line(),
        Some(line),
        "the refusal must name the line the author wrote"
    );
    assert!(
        matches!(
            error.reason(),
            Reason::OptionalPlaceholderNotAlone { option, beside }
                if option == pair.0 && beside == pair.1
        ),
        "expected an optional-not-alone refusal naming {pair:?}, got {error:?}"
    );
}

#[test]
fn an_optional_placeholder_beside_a_required_one_is_refused_at_its_line() {
    // Dropping the line would drop the required value with it, unannounced.
    assert_refused_at(
        "optional-beside-required",
        "files/settings.yml",
        3,
        ("assignee", "cadence"),
    );
}

#[test]
fn an_optional_placeholder_beside_another_optional_is_refused_at_its_line() {
    // Dropping the line for one unset optional would drop a set one's value.
    assert_refused_at(
        "optional-beside-optional",
        "files/dependabot.yml",
        2,
        ("assignee", "reviewer"),
    );
}

#[test]
fn an_optional_placeholder_beside_a_required_one_in_a_partial_is_refused_at_its_line() {
    // The rule holds inside partials, and names the partial and the line the
    // author wrote there, not a line of the assembled output.
    assert_refused_at(
        "optional-beside-required-in-partial",
        "partials/cargo.yml",
        2,
        ("assignee", "cadence"),
    );
}

#[test]
fn an_optional_placeholder_beside_another_optional_in_a_partial_is_refused_at_its_line() {
    assert_refused_at(
        "optional-beside-optional-in-partial",
        "partials/cargo.yml",
        2,
        ("assignee", "reviewer"),
    );
}

#[test]
fn a_text_default_that_is_empty_is_refused_as_an_invalid_value() {
    // An empty default would fill a placeholder with nothing. Refused in the
    // manifest, naming the option and the empty value, with no line.
    let error = render(test_skeleton("refused/text-default-empty"), &Choices::new())
        .expect_err("an empty text default must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert_eq!(error.line(), None, "a manifest value names no line");
    assert!(
        matches!(
            error.reason(),
            Reason::ValueInvalid { option, value } if option == "assignee" && value.is_empty()
        ),
        "expected an invalid-value refusal naming `assignee` and the empty value, got {error:?}"
    );
}

#[test]
fn a_text_default_holding_a_control_character_is_refused_as_an_invalid_value() {
    // The default is a newline in the middle of a name; it would break the
    // one-line guarantee the same way a wearer's value would.
    let error = render(
        test_skeleton("refused/text-default-control-character"),
        &Choices::new(),
    )
    .expect_err("a text default holding a control character must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert_eq!(error.line(), None, "a manifest value names no line");
    assert!(
        matches!(
            error.reason(),
            Reason::ValueInvalid { option, value }
                if option == "assignee" && value == "octo\ncat"
        ),
        "expected an invalid-value refusal naming `assignee` and its default, got {error:?}"
    );
}

/// One mebibyte, the unit the render size limit and these values are counted in.
const MEBIBYTE: usize = 1024 * 1024;

/// A choice set giving each `(option, length)` a value of that many `x` bytes.
fn text_choices(values: &[(&str, usize)]) -> Choices {
    let mut choices = Choices::new();
    for (option, length) in values {
        choices.insert(*option, Choice::One("x".repeat(*length)));
    }
    choices
}

/// Asserts that `error` is the refusal of a render past the size limit, as the
/// wearer's choice, naming `option`.
fn assert_render_too_large_naming(error: &crate::skeleton::RenderError, option: &str) {
    assert_eq!(error.file(), None, "a wearer's choice names no file");
    assert_eq!(error.line(), None, "a wearer's choice names no line");
    assert!(
        matches!(
            error.reason(),
            Reason::ChoicesRenderTooLarge { bytes_max, option: named }
                if *bytes_max == 8 * 1024 * 1024 && named == option
        ),
        "expected a render-too-large refusal naming `{option}` against 8 MiB, got {error:?}"
    );
    assert!(
        error.to_string().contains(&format!("`{option}`")),
        "the message must name the option, got {error}"
    );
}

#[test]
fn a_render_made_too_large_by_the_wearers_text_names_the_option_contributing_most() {
    // `wide` fills four placeholders and `narrow` one. The wearer gives `wide`
    // 1.5 MiB (6 MiB of the render) and `narrow` the longer value, 2.5 MiB
    // (2.5 MiB of the render): 8.5 MiB in all, past the 8 MiB limit. The option
    // named is `wide`, though its own value is the shorter one. With short
    // values the same skeleton renders, so the length is what is refused.
    let skeleton = test_skeleton("renders/text-render-size-by-placeholders");
    render(&skeleton, &text_choices(&[("narrow", 4), ("wide", 4)]))
        .expect("the skeleton itself must render with short values");

    let error = render(
        &skeleton,
        &text_choices(&[("narrow", 5 * MEBIBYTE / 2), ("wide", 3 * MEBIBYTE / 2)]),
    )
    .expect_err("a render past the size limit must be refused");

    assert_render_too_large_naming(&error, "wide");
}

#[test]
fn a_tie_for_the_most_bytes_in_a_too_large_render_names_the_first_option_by_name() {
    // `alpha` and `beta` each fill one placeholder and each is given 4.5 MiB:
    // 9 MiB, past the limit, and neither contributes more. `beta` is declared
    // and placed first in the skeleton, so the tie is broken by name, not by
    // position: `alpha` is named.
    let skeleton = test_skeleton("renders/text-render-size-tie");
    render(&skeleton, &text_choices(&[("alpha", 4), ("beta", 4)]))
        .expect("the skeleton itself must render with short values");

    let error = render(
        &skeleton,
        &text_choices(&[("alpha", 9 * MEBIBYTE / 2), ("beta", 9 * MEBIBYTE / 2)]),
    )
    .expect_err("a render past the size limit must be refused");

    assert_render_too_large_naming(&error, "alpha");
}

#[test]
fn a_long_text_value_that_keeps_the_render_under_the_limit_renders() {
    // The limit is on the whole render, not on a value. `assignee` fills two
    // lines of `text-optional`, so 3 MiB of text renders as 6 MiB and more,
    // still under 8 MiB, and every byte arrives.
    let value = "x".repeat(3 * MEBIBYTE);
    let rendering = render(test_skeleton("renders/text-optional"), &assignee(&value))
        .expect("a long value keeping the render under the limit must render");

    let expected = format!(
        "version: 2\nassignees: [\"{value}\"]\nopen-pull-requests-limit: 5\n\
         reviewers: [\"{value}\"]\n"
    );
    assert_eq!(
        rendering.get("dependabot.yml").map(<[u8]>::len),
        Some(expected.len()),
        "the whole long value must be filled in, both times"
    );
    assert!(
        rendering.get("dependabot.yml") == Some(expected.as_bytes()),
        "the rendered bytes must be the template with the value filled in verbatim"
    );
}
