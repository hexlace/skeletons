//! Indentation: an inserted partial's lines each carry the directive line's
//! own leading whitespace, except a truly blank line, which stays blank.

use super::test_skeleton;
use crate::skeleton::{Choices, render};

#[test]
fn every_inserted_line_carries_the_directives_leading_whitespace_and_blank_lines_stay_blank() {
    // Every line of an inserted partial carries the directive line's own
    // leading whitespace. A blank line inside a partial stays blank:
    // rendering never adds whitespace to a blank line. "Blank" means a line
    // with no bytes before its terminator -- a partial line holding only
    // spaces or tabs is not blank: it gets the indentation like any other
    // line, exactly as written.
    //
    // The directive in `files/ci.yml` is indented with one literal tab. The
    // partial it inserts has three lines: an ordinary line, a truly empty
    // line (0 bytes before its `\n`), and a line holding only three spaces.
    // Only the truly empty line must come out untouched; the spaces-only
    // line must gain the tab like every other line.
    let rendering = render(test_skeleton("renders/indentation"), &Choices::new())
        .expect("a well-formed indentation skeleton must render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\n\tnormal-line: yes\n\n\t   \ndone: true\n".as_slice()),
        "ordinary and whitespace-only lines gain the tab; the truly blank line does not"
    );
}

#[test]
fn a_partial_line_holding_only_a_control_byte_is_content_and_gets_the_directives_indentation() {
    // "Blank" means no bytes before the terminator. A partial line holding
    // one control byte holds a byte, so it is content like a line holding
    // only a space, and gains the directive's two-space indentation. The
    // truly empty line beside it stays empty, and the control byte ending
    // the first line is kept. The partial is `a: 1`, a line of just the
    // control byte, an empty line, then `b: 2`.
    let rendering = render(
        test_skeleton("renders/indentation-control-byte-line"),
        &Choices::new(),
    )
    .expect("a well-formed skeleton with a control byte in a partial must render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\n  a: 1\r\n  \r\n\n  b: 2\ndone: true\n".as_slice()),
        "the control-byte line gains the indentation; the empty line does not"
    );
}
