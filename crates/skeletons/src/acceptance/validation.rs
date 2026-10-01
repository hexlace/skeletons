//! Whole-skeleton validation and one-pass fill.
//!
//! A skeleton is validated whole, on every render, whatever the options: a
//! defect in a partial nobody selected is still refused. Fill is one pass
//! over the skeleton's own text, so an error inside a partial names the partial
//! file and its own line number, never a line number in the assembled
//! output.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn a_defect_in_a_partial_the_choice_does_not_select_is_still_refused() {
    // A defect in a partial that the given options do not select is still
    // refused: the skeleton is validated whole. The skeleton's default
    // selects only `keep`; `skip.yml` (mapped, but not selected by the
    // default) carries a placeholder typo. Rendering with the default
    // choice must still refuse, not silently render `keep` and ignore
    // `skip`'s defect.
    let error = render(
        test_skeleton("refused/placeholder-typo-in-unselected-partial"),
        &Choices::new(),
    )
    .expect_err("a defect in an unselected partial must still be refused");

    assert!(
        matches!(error.reason(), Reason::PlaceholderNotFillOption { name, .. } if name == "oops"),
        "expected the unselected partial's own typo to be refused, got {error:?}"
    );
}

#[test]
fn a_placeholder_typo_inside_a_partial_names_the_partials_own_path_and_line() {
    // A placeholder typo inside a partial is refused naming the partial's
    // own path and its own line number. Same fixture as above:
    // the typo is on `partials/skip.yml`'s own line 2, and that is what
    // must be named -- not the main file that references it, and not any
    // position the partial's content would occupy once inserted.
    let error = render(
        test_skeleton("refused/placeholder-typo-in-unselected-partial"),
        &Choices::new(),
    )
    .expect_err("a typo inside a partial must be refused");

    assert_eq!(error.file(), Some("partials/skip.yml"));
    assert_eq!(error.line(), Some(2));
}

#[test]
fn a_partial_that_is_not_valid_utf8_is_refused_naming_it_even_when_unselected() {
    // A skeleton file that cannot be read as text is refused, naming the skeleton
    // and the file -- this covers the `partials/` half (the `files/` half
    // is `tree::a_file_that_cannot_be_read_as_text_...`) -- together with
    // the rule that a defect in a partial the given options do not select
    // is still refused, since the skeleton is validated whole. The skeleton's
    // default selects only `keep`; `skip.yml` (mapped, but not selected) is
    // not valid UTF-8: it contains the bytes `377 376` (0xFF 0xFE).
    let error = render(test_skeleton("refused/partial-not-utf8"), &Choices::new())
        .expect_err("a partial that is not valid UTF-8 must be refused, even when unselected");

    assert_eq!(
        error.skeleton(),
        &crate::skeleton::SkeletonIdentity::Named("partial-not-utf8".to_owned())
    );
    assert_eq!(error.file(), Some("partials/skip.yml"));
    assert!(
        matches!(error.reason(), Reason::NotUtf8),
        "expected a not-UTF-8 refusal naming the partial, got {error:?}"
    );
}

#[test]
fn a_fill_error_inside_a_selected_partial_reports_the_partials_authored_line() {
    // Fill is one pass over the skeleton's own text, so an error inside a
    // partial reports the partial's own authored line
    // number, never a line number in the text as it would appear once
    // assembled into the main file.
    //
    // The skeleton's one set value, `keep`, is selected by default and is
    // inserted at line 6 of `files/ci.yml` (five padding lines precede the
    // directive). `partials/keep.yml`'s own typo is on its line 2. If fill
    // operated over the assembled output, the defect would land at
    // assembled line 7 (5 padding lines + `keep.yml`'s own line 1, then the
    // typo on the next line); one-pass fill over authored text must report
    // line 2 of the partial instead.
    let error = render(
        test_skeleton("refused/fill-reports-authored-line-not-assembled-line"),
        &Choices::new(),
    )
    .expect_err("a typo in a selected, inserted partial must still be refused");

    assert_eq!(error.file(), Some("partials/keep.yml"));
    assert_eq!(
        error.line(),
        Some(2),
        "the error must report the partial's own authored line, never a line of assembled output"
    );
    assert!(
        matches!(error.reason(), Reason::PlaceholderNotFillOption { name, .. } if name == "oops"),
        "expected the partial's own typo to be refused, got {error:?}"
    );
}
