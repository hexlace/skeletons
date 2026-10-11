//! Placeholders: every `{{` in a file that fills opens a placeholder that must
//! be closed by a well-formed name and `}}` on the same line, and the name must
//! exactly name a declared `enum` or `text` option.

use super::test_skeleton;
use crate::skeleton::{Choices, OptionKind, Reason, SkeletonIdentity, render};

#[test]
fn a_placeholder_naming_no_option_is_refused_at_its_line() {
    // A `{{...}}` that does not exactly name a declared `enum` or `text` option is
    // refused as a typo in the skeleton, naming the skeleton, the file, and the
    // line -- including one that names nothing declared at all. The skeleton
    // writes `{{cadense}}` on line 4 of its only file; nothing is
    // declared by that name, so the render refuses rather than passing it
    // through.
    let error = render(
        test_skeleton("refused/placeholder-names-nothing"),
        &Choices::new(),
    )
    .expect_err("a typo in a placeholder must be refused");

    assert_eq!(
        error.skeleton(),
        &SkeletonIdentity::Named("placeholder-names-nothing".to_owned())
    );
    assert_eq!(error.file(), Some("files/settings.yml"));
    assert_eq!(error.line(), Some(4));
    assert!(
        matches!(
            error.reason(),
            Reason::PlaceholderNotFillOption { name, declared_as: None } if name == "cadense"
        ),
        "expected a placeholder typo, got {error:?}"
    );
}

#[test]
fn a_placeholder_naming_a_declared_set_option_is_refused_as_the_wrong_kind() {
    // Same rule as above, including one that names a `set` option
    // instead. The skeleton declares `workflows` as a `set`, and a file
    // fills `{{workflows}}` as though it were an `enum`.
    let error = render(
        test_skeleton("refused/placeholder-names-set-option"),
        &Choices::new(),
    )
    .expect_err("a placeholder naming a set option must be refused, not treated as a fill");

    assert_eq!(error.file(), Some("files/settings.yml"));
    assert_eq!(error.line(), Some(1));
    assert!(
        matches!(
            error.reason(),
            Reason::PlaceholderNotFillOption { name, declared_as: Some(OptionKind::Set) }
                if name == "workflows"
        ),
        "expected a wrong-kind placeholder refusal naming Set, got {error:?}"
    );
}

#[test]
fn an_unclosed_placeholder_is_refused_as_malformed() {
    // Every `{{` in a skeleton file opens a placeholder. One that is not
    // followed on the same line by a well-formed name and `}}` is refused,
    // naming the skeleton, file and line. Line 2 opens `{{name` with
    // no closing `}}` anywhere on the line.
    let error = render(
        test_skeleton("refused/placeholder-malformed-unclosed"),
        &Choices::new(),
    )
    .expect_err("an unclosed `{{` must be refused");

    assert_eq!(error.file(), Some("files/settings.yml"));
    assert_eq!(error.line(), Some(2));
    assert!(
        matches!(error.reason(), Reason::PlaceholderMalformed),
        "expected a malformed-placeholder refusal, got {error:?}"
    );
}

#[test]
fn a_github_actions_expression_is_refused_never_passed_through() {
    // A literal `{{`, including a GitHub Actions `${{ … }}`, is refused in a
    // file that fills, never passed through; only a file declared verbatim
    // may hold one (see `verbatim.rs`). Line 6 contains
    // `OS: "${{ matrix.os }}"`: the `{{` is not followed by a well-formed
    // name (a leading space, then a dotted expression) and `}}`, so it must
    // be refused exactly like any other malformed placeholder rather than
    // silently kept as-is because it "looks like" CI syntax.
    let error = render(
        test_skeleton("refused/github-actions-expression"),
        &Choices::new(),
    )
    .expect_err("a GitHub Actions expression must be refused, not passed through");

    assert_eq!(error.file(), Some("files/workflow.yml"));
    assert_eq!(error.line(), Some(6));
    assert!(
        matches!(error.reason(), Reason::PlaceholderMalformed),
        "expected a malformed-placeholder refusal for the GitHub Actions collision, got {error:?}"
    );
}
