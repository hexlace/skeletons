//! A directive hides behind an invisible character turned into something other than a
//! space, and behind an invisible character sitting inside the marker rather than in
//! front of it — not just in the leading run `acceptance::hidden_directives` covers.
//!
//! A page that turns every space into a no-break space turns the ones inside
//! `# skeletons:partial` too, and a paste can drop a zero-width space or a stray control
//! byte anywhere in that run — after `#`, after `# `, or right before `:` — not only at
//! the very start of the line. Each of these must still be refused rather than rendered
//! literally, and refused as `HiddenDirective` when the culprit is a genuinely invisible
//! character, or as `DirectiveMalformed` when only ordinary ASCII spacing is off (no
//! space between `#` and `skeletons` at all, and nothing invisible anywhere).
//!
//! Alongside those, a handful of shapes render exactly as authored: a
//! control character or a no-break space that never sits in front of `# skeletons:` at all,
//! a directive whose case does not match (`# Skeletons:`), and a comment that merely
//! contains the word `skeletons` without being shaped like the marker (`# not skeletons:`).
//! However wide the set of invisible characters is, and however much of the run around
//! the marker is read, the rule must not refuse these.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

/// Fails the test, with the actual outcome, if `render` did not refuse `skeleton` at
/// `expected_file`/`expected_line` for `Reason::HiddenDirective`.
fn expect_hidden_directive(skeleton: &str, expected_file: &str, expected_line: u32) {
    let error = render(test_skeleton(skeleton), &Choices::new())
        .expect_err("a hidden directive must be refused");
    assert_eq!(error.file(), Some(expected_file));
    assert_eq!(error.line(), Some(expected_line));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

// --- Invisible characters in front of the marker and inside it, together. ---

#[test]
fn two_no_break_spaces_and_a_third_one_inside_the_marker_are_all_hidden() {
    // `files/ci.yml` line 3 reads:
    // `c2 a0 c2 a0 23 c2 a0 73 6b 65 6c 65 74 6f 6e 73 3a 70 61 72 74 69 61 6c c2 a0 73`
    // -- two leading no-break spaces, then `#`, then a third no-break space between `#`
    // and `skeletons:partial`, then a fourth between `partial` and the option name `s`.
    // A rule that looked only at the leading run would let this whole line render
    // literally; every no-break space here, in front of the marker or inside it, hides
    // the directive. Line 4 is a clean `# skeletons:partial s`, so the skeleton's
    // one option `s` is genuinely used elsewhere and this refusal is about line 3 alone.
    expect_hidden_directive(
        "refused/hidden-marker-double-no-break-space-and-inner-gap",
        "files/ci.yml",
        3,
    );
}

#[test]
fn two_leading_spaces_do_not_excuse_a_no_break_space_inside_the_marker() {
    // Line 3 reads `20 20 23 c2 a0 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- two literal,
    // ordinary leading spaces (which alone would just indent a directive), then `#`, then a
    // no-break space between `#` and `skeletons:`. The line is directive-shaped only once
    // that inner no-break space is set aside, so it hides the directive exactly as a
    // leading one would.
    expect_hidden_directive(
        "refused/hidden-marker-leading-spaces-inner-no-break-space",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_leading_no_break_space_hides_a_directive_missing_its_own_space_after_hash() {
    // Line 3 reads `c2 a0 23 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- a leading no-break
    // space, then `#skeletons:` with no space at all after `#`. Once the no-break space is
    // set aside the rest is directive-shaped under normalisation (`#`, zero spaces,
    // `skeletons`, zero spaces, `:`, all within "at most one"), so the no-break space is
    // what hides it -- not the missing space, which alone would only make it malformed (see
    // `a_directive_shaped_line_with_no_space_after_hash_is_malformed_not_hidden` below).
    expect_hidden_directive(
        "refused/hidden-marker-no-break-space-missing-hash-space",
        "files/ci.yml",
        3,
    );
}

// --- An invisible character at each position inside the marker, for three kinds of
// invisible character: a no-break space (`char::is_whitespace`), a zero-width space
// (Cf), and a C0 control byte that is not whitespace (Cc, which the rule counts as
// invisible too). ---

#[test]
fn a_no_break_space_right_after_hash_hides_the_directive() {
    // Line 3: `23 c2 a0 20 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `#`, a no-break space,
    // then the directive's own ordinary space, then `skeletons:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-no-break-space-after-hash",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_zero_width_space_right_after_hash_hides_the_directive() {
    // Line 3: `23 e2 80 8b 20 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `#`, a zero-width space
    // (Cf, not `White_Space`), then the ordinary space, then `skeletons:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-zero-width-space-after-hash",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_control_byte_right_after_hash_hides_the_directive() {
    // Line 3: `23 01 20 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `#`, then `\x01` (Cc, not
    // whitespace, but a member of the invisible set alongside whitespace and Cf), then
    // the ordinary space, then `skeletons:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-control-after-hash",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_no_break_space_right_after_hash_space_hides_the_directive() {
    // Line 3: `23 20 c2 a0 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `# `, then a no-break
    // space, then `skeletons:partial s` with no space of its own before `skeletons`.
    expect_hidden_directive(
        "refused/hidden-marker-no-break-space-after-hash-space",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_zero_width_space_right_after_hash_space_hides_the_directive() {
    // Line 3: `23 20 e2 80 8b 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `# `, then a zero-width
    // space, then `skeletons:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-zero-width-space-after-hash-space",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_control_byte_right_after_hash_space_hides_the_directive() {
    // Line 3: `23 20 01 73 6b 65 6c 65 74 6f 6e 73 3a ...` -- `# `, then `\x01`, then
    // `skeletons:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-control-after-hash-space",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_no_break_space_right_before_the_colon_hides_the_directive() {
    // Line 3: `23 20 73 6b 65 6c 65 74 6f 6e 73 c2 a0 3a ...` -- `# skeletons`, then a
    // no-break space, then `:partial s` with no space of its own before `:`.
    expect_hidden_directive(
        "refused/hidden-marker-no-break-space-before-colon",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_zero_width_space_right_before_the_colon_hides_the_directive() {
    // Line 3: `23 20 73 6b 65 6c 65 74 6f 6e 73 e2 80 8b 3a ...` -- `# skeletons`, then a
    // zero-width space, then `:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-zero-width-space-before-colon",
        "files/ci.yml",
        3,
    );
}

#[test]
fn a_control_byte_right_before_the_colon_hides_the_directive() {
    // Line 3: `23 20 73 6b 65 6c 65 74 6f 6e 73 01 3a ...` -- `# skeletons`, then `\x01`,
    // then `:partial s`.
    expect_hidden_directive(
        "refused/hidden-marker-control-before-colon",
        "files/ci.yml",
        3,
    );
}

// --- A directive-shaped line whose only defect is ordinary ASCII spacing is malformed,
// never hidden: nothing invisible is in play. ---

#[test]
fn a_directive_shaped_line_with_no_space_after_hash_is_malformed_not_hidden() {
    // `files/ci.yml` line 2 reads
    // `23 73 6b 65 6c 65 74 6f 6e 73 3a 70 61 72 74 69 61 6c 20 73` --
    // `#skeletons:partial s`, with no space anywhere between `#` and `skeletons` and
    // nothing invisible on the line at all. Once collapsed under normalisation this is
    // still directive-shaped (`#`, zero spaces, `skeletons`, zero spaces, `:`; "at most
    // one" space allows zero), so it is not ordinary text -- but with no invisible
    // character anywhere in the marker, what is wrong with it is purely ASCII spacing, so
    // it is `DirectiveMalformed`, not `HiddenDirective`.
    let error = render(
        test_skeleton("refused/hidden-marker-no-space-after-hash-malformed"),
        &Choices::new(),
    )
    .expect_err("a directive-shaped line missing its space after # must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveMalformed),
        "expected a malformed-directive refusal, not hidden, got {error:?}"
    );
}

// --- Pass cases: these are not directives, so they render as authored. ---

#[test]
fn invisible_characters_away_from_a_directive_and_near_misses_render_byte_for_byte() {
    // `files/text.txt` holds four lines:
    //   line 1: `01 63 6f 6e 74 72 6f 6c ...` -- a bare control byte (`\x01`, Cc, not
    //     whitespace) leading straight into ordinary text, never `# skeletons:`. A control
    //     character that carries no whitespace property of its own is exactly as inert
    //     here as a no-break space already is.
    //   line 2: `68 65 6c 6c 6f 20 c2 a0 77 6f 72 6c 64 ...` -- a no-break space sitting
    //     in the middle of an ordinary line, nowhere near a marker.
    //   line 3: `23 20 53 6b 65 6c 65 74 6f 6e 73 3a ...` -- `# Skeletons:partial s`, wrong
    //     case; the marker is `# skeletons:` exactly, under normalisation as much as
    //     without it.
    //   line 4: `23 20 6e 6f 74 20 73 6b 65 6c 65 74 6f 6e 73 3a` -- `# not skeletons:`, a
    //     comment that happens to contain the word `skeletons` without ever being shaped
    //     like the marker.
    // None of these is directive-shaped by any reading, with or without invisible
    // characters set aside, so render has nothing to act on and must pass every byte
    // through unchanged.
    let rendering = render(
        test_skeleton("renders/hidden-marker-pass-cases"),
        &Choices::new(),
    )
    .expect("none of these lines names or hides a directive");

    assert_eq!(
        rendering.get("text.txt"),
        Some(
            "\x01control-character-before-text\nhello \u{a0}world-invisible-inside-text\n\
             # Skeletons:partial s\n# not skeletons:\n"
                .as_bytes()
        ),
        "invisible characters away from a directive, a case mismatch, and a comment that \
         merely mentions skeletons must all render unchanged"
    );
}
