//! A directive hidden behind invisible characters is refused, not rendered
//! literally: strip a line's leading characters that are `char::is_whitespace`
//! or Unicode general category Cf, and if what remains begins `# skeletons:`
//! while the same line with only leading spaces and tabs stripped does not,
//! the line reads as a directive only once those invisible characters are
//! set aside -- and is refused, naming the file and the line, in a file
//! under `files/` and in a partial alike. A byte-order mark on a file's
//! first line is one instance of this rule, already covered by
//! `acceptance::byte_order_mark`; this module covers the rest of the class:
//! a no-break space, a zero-width space, a word joiner, an ideographic
//! space, a vertical tab, a form feed, and a byte-order mark on a line
//! other than the first -- plus the same rule reaching into a partial.
//!
//! Two shapes are outside this rule, and are pinned here: leading spaces and
//! tabs alone in front of `# skeletons:` are an ordinary directive (or
//! malformed), and any of these same invisible characters in front of
//! ordinary text -- never `# skeletons:` -- is not a directive at all, and
//! passes through byte for byte.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn a_directive_hidden_behind_a_no_break_space_is_refused_at_its_line() {
    // `files/ci.yml` reads: `jobs:`, `  stage: build`,
    // `<U+00A0># skeletons:partial workflows`, `# skeletons:partial workflows` --
    // the hidden directive on line 3, and a clean, visible copy of the same
    // directive on line 4 so the skeleton's declared `workflows` option is
    // genuinely used elsewhere and this refusal is about the no-break space
    // alone. Line 3 opens `c2 a0` (U+00A0), immediately followed by
    // `23 20 73 6b 65 6c 65 74 6f 6e 73 3a` (`# skeletons:`), with nothing
    // between them. A no-break space is the realistic case: it is what
    // arrives when YAML is pasted from a web page. Line 3, not line 1, so a
    // check that only special-cases a file's first line cannot pass this
    // fixture by accident.
    let error = render(
        test_skeleton("refused/hidden-directive-no-break-space"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind a no-break space must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_a_zero_width_space_is_refused_at_its_line() {
    // Same shape as the no-break-space test above; only the invisible
    // prefix differs. Line 3 of `files/ci.yml` opens `e2 80 8b` (U+200B),
    // immediately followed by `# skeletons:partial workflows`. U+200B has no
    // `White_Space` property -- it is caught only because it is Unicode
    // general category Cf.
    let error = render(
        test_skeleton("refused/hidden-directive-zero-width-space"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind a zero-width space must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_a_word_joiner_is_refused_at_its_line() {
    // Same shape again. Line 3 of `files/ci.yml` opens `e2 81 a0` (U+2060),
    // immediately followed by `# skeletons:partial workflows`. Like the
    // zero-width space, U+2060 is not `White_Space` and is caught only as Cf.
    let error = render(
        test_skeleton("refused/hidden-directive-word-joiner"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind a word joiner must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_an_ideographic_space_is_refused_at_its_line() {
    // Same shape again. Line 3 of `files/ci.yml` opens `e3 80 80` (U+3000),
    // immediately followed by `# skeletons:partial workflows`. U+3000 is
    // `White_Space`, unlike the two tests above, so it is caught by the
    // `char::is_whitespace` half of the rule rather than the Cf half.
    let error = render(
        test_skeleton("refused/hidden-directive-ideographic-space"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind an ideographic space must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_a_vertical_tab_is_refused_at_its_line() {
    // Same shape again. Line 3 of `files/ci.yml` opens `0b` (`\x0b`, line
    // tabulation), immediately followed by `# skeletons:partial workflows`.
    // `\x0b` is `White_Space` but is neither a space nor a tab, so
    // leading-whitespace stripping (which only removes spaces and tabs)
    // leaves it in place while the wider `char::is_whitespace` strip removes
    // it.
    let error = render(
        test_skeleton("refused/hidden-directive-vertical-tab"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind a vertical tab must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_a_form_feed_is_refused_at_its_line() {
    // Same shape again. Line 3 of `files/ci.yml` opens `0c` (`\x0c`, form
    // feed), immediately followed by `# skeletons:partial workflows`. Same
    // reasoning as the vertical tab above: `White_Space`, but not a space or
    // a tab.
    let error = render(
        test_skeleton("refused/hidden-directive-form-feed"),
        &Choices::new(),
    )
    .expect_err("a directive hidden behind a form feed must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_byte_order_mark_hides_a_directive_on_a_line_other_than_the_first() {
    // The same rule reaches a byte-order mark wherever it sits, not only a
    // file's first line: `acceptance::byte_order_mark` already covers line
    // 1, so this fixture puts it on line 3 instead. Line 3 of `files/ci.yml`
    // opens `ef bb bf` (U+FEFF), immediately followed by
    // `# skeletons:partial workflows`.
    let error = render(
        test_skeleton("refused/hidden-directive-byte-order-mark-not-line-one"),
        &Choices::new(),
    )
    .expect_err("a byte-order mark hiding a directive past line 1 must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_hidden_behind_an_invisible_character_inside_a_partial_is_refused() {
    // The skeleton declares `workflows` as set(keep/hidden), both selected by
    // default, so `partials/hidden.yml` is scanned. `files/ci.yml` carries
    // a clean `# skeletons:partial workflows`, using the option so this
    // refusal is about the partial alone. `partials/hidden.yml` reads
    // `before: true`, `still: fine`,
    // `<U+00A0># skeletons:partial workflows` -- a line that begins `# skeletons:`
    // only once the no-break space before it is set aside, so it is never
    // directive-shaped: `Reason::DirectiveInPartial`, which is for a line
    // that is directive-shaped inside a partial, cannot arise for it. It is
    // refused as `HiddenDirective` naming the partial's own line, the same
    // way a byte-order mark already does, in the
    // `a_byte_order_mark_before_skeletons_text_in_a_partial_is_refused_at_line_one`
    // test of `acceptance::byte_order_mark`.
    // Line 3 of `partials/hidden.yml` opens `c2 a0`, immediately followed by
    // `# skeletons:partial workflows`.
    let error = render(
        test_skeleton("refused/hidden-directive-in-partial"),
        &Choices::new(),
    )
    .expect_err("a directive hidden inside a partial must be refused as hidden, not as nested");

    assert_eq!(error.file(), Some("partials/hidden.yml"));
    assert_eq!(error.line(), Some(3));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, not DirectiveInPartial, got {error:?}"
    );
}

#[test]
fn leading_spaces_and_a_tab_before_a_directive_render_it_as_a_directive() {
    // Spaces and tabs in front of `# skeletons:` make an ordinary directive,
    // not a hidden one: `char::is_whitespace`/Cf stripping is a strict
    // superset of spaces-and-tabs stripping, so a line that reads as a
    // directive with only spaces and tabs set aside is already an ordinary
    // directive, and the hidden-directive rule has nothing to refuse.
    // `files/ci.yml`'s directive is indented with two literal spaces and
    // one literal tab. Line 2 reads `20 20 09 23 20 73 6b 65 6c 65 74 6f 6e
    // 73 3a ...` (two spaces, a tab, then `# skeletons:...`). The declared
    // set option `workflows` defaults to `keep`, whose partial is one line,
    // `keep-job: true`.
    let rendering = render(
        test_skeleton("renders/hidden-directive-leading-whitespace-still-directive"),
        &Choices::new(),
    )
    .expect("a directive indented with spaces and a tab must still render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\n  \tkeep-job: true\ndone: true\n".as_slice()),
        "the indented directive must insert its partial exactly as an ordinary directive would, \
         carrying its own leading whitespace onto the inserted line"
    );
}

#[test]
fn an_invisible_character_before_ordinary_text_is_not_a_directive_and_renders_byte_for_byte() {
    // Preservation, the other direction: none of these invisible
    // characters is special on a line that is not shaped like a directive
    // once they are set aside. `files/text.txt` has one line per prefix in
    // the class, each character leading straight into ordinary text --
    // never `# skeletons:` -- so no amount of stripping ever exposes a
    // directive, and render has nothing to act on. Each line opens with
    // exactly the named prefix's bytes (no-break space `c2 a0`, zero-width
    // space `e2 80 8b`, word joiner `e2 81 a0`, ideographic space
    // `e3 80 80`, vertical tab `0b`, form feed `0c`, byte-order mark
    // `ef bb bf`), immediately followed by that line's plain-text label --
    // nothing else on the line.
    let rendering = render(
        test_skeleton("renders/hidden-directive-invisible-character-then-text"),
        &Choices::new(),
    )
    .expect("invisible characters ahead of ordinary text must still render");

    assert_eq!(
        rendering.get("text.txt"),
        Some(
            "\u{a0}no-break-space line\n\
             \u{200b}zero-width-space line\n\
             \u{2060}word-joiner line\n\
             \u{3000}ideographic-space line\n\
             \x0bvertical-tab line\n\
             \x0cform-feed line\n\
             \u{feff}byte-order-mark line\n"
                .as_bytes()
        ),
        "a line led by an invisible character but not `# skeletons:` must pass through unchanged"
    );
}
