//! Directives: a `# skeletons:partial <option-name>` line must exactly name a
//! declared `set` option, must be well-formed, must never appear inside a
//! partial, and must end in a line terminator like any other line.

use super::test_skeleton;
use crate::skeleton::{Choices, OptionKind, Reason, render};

#[test]
fn a_directive_naming_no_option_is_refused_at_its_line() {
    // A directive line that does not exactly name a declared `set` option
    // is refused as a typo in the skeleton, naming the skeleton, the file, and the
    // line -- including one that names nothing declared at all.
    let error = render(
        test_skeleton("refused/directive-names-nothing"),
        &Choices::new(),
    )
    .expect_err("a directive naming an undeclared option must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(
            error.reason(),
            Reason::DirectiveNotSetOption { name, declared_as: None } if name == "nope"
        ),
        "expected a directive typo naming nothing declared, got {error:?}"
    );
}

#[test]
fn a_directive_naming_a_declared_enum_option_is_refused_as_the_wrong_kind() {
    // Same rule as above, including one that names an `enum` option
    // instead. `cadence` is declared as `enum`, and is used
    // correctly elsewhere in the skeleton (a real fill), so the only defect is
    // the directive in `files/ci.yml` naming it as though it selected
    // partials.
    let error = render(
        test_skeleton("refused/directive-names-enum-option"),
        &Choices::new(),
    )
    .expect_err("a directive naming an enum option must be refused, not treated as a select");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(
            error.reason(),
            Reason::DirectiveNotSetOption { name, declared_as: Some(OptionKind::Enum) }
                if name == "cadence"
        ),
        "expected a wrong-kind directive refusal naming Enum, got {error:?}"
    );
}

#[test]
fn a_directive_missing_its_option_name_is_refused_as_malformed() {
    // A directive line must be exactly `# skeletons:partial <option-name>` with
    // nothing after the name except its line terminator. Anything else of
    // that shape -- a misspelt keyword, a missing or extra argument,
    // trailing text -- is refused, never treated as an ordinary comment.
    // Line 2 is `# skeletons:partial` with no argument at all.
    let error = render(
        test_skeleton("refused/directive-malformed-missing-name"),
        &Choices::new(),
    )
    .expect_err("a directive with no option name must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveMalformed),
        "expected a malformed-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_with_trailing_text_after_the_name_is_refused_as_malformed() {
    // Same rule, the "trailing text" case: line 2 is
    // `# skeletons:partial workflows extra`, which names a real option but adds
    // text after it. `workflows` is also used correctly in a second file,
    // `files/correct.yml`, so the only defect present is the trailing text.
    let error = render(
        test_skeleton("refused/directive-malformed-trailing-text"),
        &Choices::new(),
    )
    .expect_err("a directive with trailing text must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveMalformed),
        "expected a malformed-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_with_a_control_byte_after_its_option_name_is_refused_as_malformed() {
    // A directive is exactly `# skeletons:partial <option-name>` and a line ends
    // only at a newline, so a control byte written straight after the
    // option name is part of the line's content: the line is not exactly a
    // directive, and is refused as malformed rather than accepted and
    // replaced. Two skeletons, one defect each: the directive at the start
    // of line 2, and the directive indented two spaces on line 3. Each
    // names a real, declared option, so the control byte is the only
    // defect. The byte after `workflows` is `\r`, then `\n`.
    for (fixture, line) in [
        ("refused/directive-malformed-control-byte-after-name", 2),
        (
            "refused/directive-malformed-control-byte-after-name-indented",
            3,
        ),
    ] {
        let error = render(test_skeleton(fixture), &Choices::new())
            .expect_err("a directive with a control byte after its name must be refused");

        assert_eq!(error.file(), Some("files/ci.yml"), "{fixture}");
        assert_eq!(error.line(), Some(line), "{fixture}");
        assert!(
            matches!(error.reason(), Reason::DirectiveMalformed),
            "{fixture}: expected a malformed-directive refusal, got {error:?}"
        );
    }
}

#[test]
fn a_directive_with_a_misspelled_keyword_is_refused_as_malformed() {
    // Same rule as the malformed-directive tests above, the misspelt-keyword
    // case.
    // Line 2 of `files/ci.yml` is `# skeletons:partail workflows` -- it opens a
    // directive line (its text after leading whitespace begins `# skeletons:`)
    // but misspells `partial`. `workflows` is used correctly in a second
    // file, `files/correct.yml`, so the only defect present is the
    // misspelling.
    let error = render(
        test_skeleton("refused/directive-malformed-misspelled-keyword"),
        &Choices::new(),
    )
    .expect_err("a directive with a misspelled keyword must be refused");

    assert_eq!(error.file(), Some("files/ci.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveMalformed),
        "expected a malformed-directive refusal, got {error:?}"
    );
}

#[test]
fn a_directive_line_found_inside_a_partial_is_refused() {
    // A directive line found inside a partial file is refused, naming the
    // skeleton, that file, and the line, rather than being expanded a second
    // level deep. `partials/test.yml` line 2 is
    // `# skeletons:partial lint`, a directive nested inside a partial.
    let error = render(
        test_skeleton("refused/directive-in-partial"),
        &Choices::new(),
    )
    .expect_err("a directive nested inside a partial must be refused");

    assert_eq!(error.file(), Some("partials/test.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveInPartial),
        "expected a directive-in-partial refusal, got {error:?}"
    );
}

#[test]
fn a_partial_whose_last_line_has_no_terminator_is_refused() {
    // A partial whose last line has no line terminator is refused, naming
    // the skeleton and the partial: inserted anywhere but the end of a file it
    // would join the following line. `partials/test.yml`'s last line, line
    // 2, has no trailing newline.
    let error = render(
        test_skeleton("refused/partial-unterminated"),
        &Choices::new(),
    )
    .expect_err("an unterminated partial must be refused");

    assert_eq!(error.file(), Some("partials/test.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::PartialUnterminated),
        "expected a partial-unterminated refusal, got {error:?}"
    );
}

#[test]
fn a_directive_that_is_a_files_final_line_with_no_terminator_is_refused() {
    // A directive line on a file's final line with no line terminator is
    // refused, naming the skeleton, file and line, the same as any other
    // unterminated directive. The skeleton uses `workflows` correctly in
    // `files/correct.yml`; the only defect is `files/broken.yml`, whose
    // last line (line 2) is the directive itself with no trailing newline.
    let error = render(
        test_skeleton("refused/directive-unterminated"),
        &Choices::new(),
    )
    .expect_err("a directive with no trailing line terminator must be refused");

    assert_eq!(error.file(), Some("files/broken.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::DirectiveUnterminated),
        "expected a directive-unterminated refusal, got {error:?}"
    );
}
