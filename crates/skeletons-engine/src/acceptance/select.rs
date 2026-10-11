//! Select: a `set` option's directive is replaced by the partials its
//! chosen values name, in the skeleton's declared order, and by nothing at all
//! when the selection is empty.

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, render};

#[test]
fn a_set_directive_selects_the_declared_default_when_nothing_is_chosen() {
    // When the wearer gives no value for a set option, its default applies.
    // The skeleton declares `workflows` as set(lint/test/release), default
    // ["lint"], so an empty `Choices` must insert only `lint.yml`.
    let rendering = render(test_skeleton("renders/set-select"), &Choices::new())
        .expect("the default selection must render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\nlint-job: shellcheck\ndone: true\n".as_slice()),
        "only the default value's partial should be inserted"
    );
}

#[test]
fn choosing_more_than_one_value_orders_partials_by_the_skeletons_declaration_never_the_choice() {
    // Choosing more than one value renders their partials one after another
    // at the directive's position, in the order the skeleton declared them --
    // never the order the values were supplied in. The skeleton
    // declares lint, test, release in that order; the wearer supplies
    // ["release", "lint"] (test excluded, order reversed relative to
    // declaration), and the output must still read lint-then-release.
    let mut choices = Choices::new();
    choices.insert(
        "workflows",
        Choice::Many(vec!["release".to_owned(), "lint".to_owned()]),
    );

    let rendering = render(test_skeleton("renders/set-select"), &choices)
        .expect("choosing two declared values must render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\nlint-job: shellcheck\nrelease-job: publish\ndone: true\n".as_slice()),
        "partials must appear in the skeleton's declared order, not the order the wearer supplied"
    );
}

#[test]
fn choosing_nothing_removes_the_directive_line_and_leaves_no_blank_line() {
    // When the chosen values collectively select nothing, the directive
    // line is removed and nothing takes its place -- not even a blank line.
    // The wearer explicitly overrides the (non-empty) default
    // with an empty set of values.
    let mut choices = Choices::new();
    choices.insert("workflows", Choice::Many(vec![]));

    let rendering = render(test_skeleton("renders/set-select"), &choices)
        .expect("selecting nothing must still render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\ndone: true\n".as_slice()),
        "the directive line must vanish with nothing left in its place, not even a blank line"
    );
}

#[test]
fn an_empty_partial_inserts_nothing() {
    // An empty partial inserts nothing. The skeleton's one set value
    // maps to a zero-byte partial file and is selected by default; the
    // directive line must still be removed cleanly, exactly as if nothing
    // had been chosen.
    let rendering = render(test_skeleton("renders/set-select-empty"), &Choices::new())
        .expect("a selected but empty partial must still render");

    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\ndone: true\n".as_slice()),
        "an empty partial must contribute zero lines, leaving no trace of the directive"
    );
}
