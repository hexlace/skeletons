//! Text fidelity: a file with nothing for render to act on comes out
//! byte-for-byte as authored. A control byte is an ordinary byte, and render
//! adds no final newline, strips no byte-order mark, and changes no other
//! byte it was not asked to replace.

use super::test_skeleton;
use crate::skeleton::{Choices, render};

#[test]
fn a_file_with_no_placeholders_or_directives_renders_byte_for_byte_identical() {
    // A file with no placeholders and no directive renders byte-for-byte
    // identical to its source.
    let rendering = render(test_skeleton("renders/passthrough-plain"), &Choices::new())
        .expect("a plain file with nothing to render must still render");

    assert_eq!(
        rendering.get("plain.yml"),
        Some(b"name: unchanged\nvalue: 42\n".as_slice()),
        "a file with no placeholders or directives must pass through unchanged"
    );
}

#[test]
fn a_file_holding_control_bytes_renders_byte_for_byte_identical() {
    // A control byte is an ordinary byte of a line's content, so a file
    // holding them, with nothing to render, must come out exactly as
    // authored: none refused, none stripped, none moved. Two files hold
    // them in different places: one has a control byte before each of its
    // newlines, the other scatters them mid-line, doubled, and at the very
    // end with no newline after.
    let rendering = render(test_skeleton("renders/text-control-bytes"), &Choices::new())
        .expect("a file holding control bytes and nothing to render must still render");

    assert_eq!(
        rendering.get("control.txt"),
        Some(b"first\r\nsecond\r\n".as_slice()),
        "a control byte before a newline must survive render unchanged"
    );
    assert_eq!(
        rendering.get("scattered.txt"),
        Some(b"a\rb\r\r\nc\r".as_slice()),
        "control bytes inside a line, doubled, and at the end of the file must survive"
    );
}

#[test]
fn a_missing_final_newline_is_preserved() {
    // A file whose source ends its last line without a newline renders the
    // same way: render adds no terminator that was not already there, and
    // removes none beyond what replacing a directive line requires. The
    // fixture's last byte is `e`, not `\n`.
    let rendering = render(
        test_skeleton("renders/text-no-final-newline"),
        &Choices::new(),
    )
    .expect("a file missing its final newline must still render");

    assert_eq!(
        rendering.get("no-newline.txt"),
        Some(b"first\nsecond-no-newline".as_slice()),
        "render must not add a final newline that was never authored"
    );
}

#[test]
fn a_byte_order_mark_is_preserved_as_ordinary_bytes() {
    // A byte-order mark is not special -- it is bytes like any other and is
    // preserved. The fixture's first three bytes are `357 273 277` (0xEF
    // 0xBB 0xBF).
    let rendering = render(test_skeleton("renders/text-bom"), &Choices::new())
        .expect("a file with a BOM must still render");

    assert_eq!(
        rendering.get("bom.txt"),
        Some(b"\xEF\xBB\xBFhello\n".as_slice()),
        "the byte-order mark must be preserved, not stripped or treated specially"
    );
}

#[test]
fn a_lone_closing_brace_pair_is_ordinary_text() {
    // A lone `}}` is ordinary text. With no preceding unmatched
    // `{{` on the line, this must render unchanged rather than being
    // treated as a syntax error.
    let rendering = render(
        test_skeleton("renders/lone-closing-braces"),
        &Choices::new(),
    )
    .expect("a lone `}}` with no opening `{{` must render, not refuse");

    assert_eq!(
        rendering.get("text.txt"),
        Some(b"result }} tail\n".as_slice()),
        "a lone `}}` must pass through as ordinary text"
    );
}
