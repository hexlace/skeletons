//! A directive hidden behind a byte-order mark on a file's first line is
//! refused rather than rendered literally: a leading U+FEFF, then any
//! leading spaces and tabs, then `# skeletons:` is treated exactly like `#
//! skeletons:` at the true start of the line -- in a file under `files/` and in
//! a partial alike. A byte-order mark followed by anything else is ordinary
//! bytes, preserved as authored; that pass-case is already exercised by
//! `acceptance::text::a_byte_order_mark_is_preserved_as_ordinary_bytes` and
//! is not repeated here.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn a_byte_order_mark_before_a_directive_in_a_file_is_refused_at_line_one() {
    // `files/ci.yml`'s first line is `\xEF\xBB\xBF# skeletons:partial
    // workflows`; its second line is the same directive without a BOM, so
    // the `workflows` option is genuinely used elsewhere in the skeleton and
    // this test's refusal is about the byte-order mark alone, not an
    // otherwise-unused option. The file's first three bytes are `357 273
    // 277` (0xEF 0xBB 0xBF), immediately followed by
    // `# skeletons:partial workflows\n`.
    let error = render(
        test_skeleton("refused/directive-after-bom-in-file"),
        &Choices::new(),
    )
    .expect_err("a directive behind a byte-order mark on line 1 must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(1));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}

#[test]
fn a_byte_order_mark_before_skeletons_text_in_a_partial_is_refused_at_line_one() {
    // `partials/keep.yml`'s first line is `\xEF\xBB\xBF# skeletons:partial
    // workflows` -- the same shape as the file case above, but inside a
    // partial, where a directive-shaped line (well-formed or not) is
    // ordinarily `Reason::DirectiveInPartial`. A byte-order mark ahead of it
    // takes precedence over that: it is refused as
    // `HiddenDirective` instead, naming the partial's own line 1. The
    // partial's first three bytes are `357 273 277` (0xEF 0xBB 0xBF).
    let error = render(
        test_skeleton("refused/directive-after-bom-in-partial"),
        &Choices::new(),
    )
    .expect_err("a byte-order mark before skeletons: text in a partial must be refused");

    assert_eq!(error.file(), Some("partials/keep.yml"));
    assert_eq!(error.line(), Some(1));
    assert!(
        matches!(error.reason(), Reason::HiddenDirective),
        "expected a hidden-directive refusal, got {error:?}"
    );
}
