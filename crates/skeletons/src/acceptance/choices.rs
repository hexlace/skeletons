//! Choices: the wearer's input has its own closed schema, checked against
//! what the skeleton actually declares -- an undeclared option, a value outside
//! the declared set, and a value listed twice are each refused by name
//! rather than silently accepted.
//!
//! These use `renders/set-select`, a skeleton with no defect of its own, so
//! whatever is refused here is refused because of the `Choices` given to
//! `render`, not because of anything wrong with the skeleton.

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, Reason, render};

#[test]
fn setting_a_value_for_an_undeclared_option_is_refused_naming_it() {
    // Setting a value for an option the skeleton did not declare is refused,
    // naming the skeleton and the option.
    let mut choices = Choices::new();
    choices.insert("nonexistent-option", Choice::One("x".to_owned()));

    let error = render(test_skeleton("renders/set-select"), &choices)
        .expect_err("an undeclared option must be refused");

    assert_eq!(
        error.skeleton(),
        &crate::skeleton::SkeletonIdentity::Named("set-select".to_owned())
    );
    assert!(
        matches!(
            error.reason(),
            Reason::UndeclaredOption { option } if option == "nonexistent-option"
        ),
        "expected an undeclared-option refusal, got {error:?}"
    );
}

#[test]
fn setting_a_declared_option_to_an_undeclared_value_is_refused_naming_it() {
    // Setting a declared option to a value outside that option's declared
    // set is refused, naming the skeleton, the option, and the value given.
    // `workflows` declares lint/test/release; "bogus" is none of them.
    let mut choices = Choices::new();
    choices.insert("workflows", Choice::Many(vec!["bogus".to_owned()]));

    let error = render(test_skeleton("renders/set-select"), &choices)
        .expect_err("a value outside the declared set must be refused");

    assert!(
        matches!(
            error.reason(),
            Reason::ValueNotDeclared { option, value }
                if option == "workflows" && value == "bogus"
        ),
        "expected a value-not-declared refusal naming the option and value, got {error:?}"
    );
}

#[test]
fn a_set_value_listed_twice_by_the_wearer_is_refused_naming_it() {
    // A wearer's set value listed twice is refused, naming the skeleton, option
    // and value, rather than silently de-duplicated.
    let mut choices = Choices::new();
    choices.insert(
        "workflows",
        Choice::Many(vec!["lint".to_owned(), "lint".to_owned()]),
    );

    let error = render(test_skeleton("renders/set-select"), &choices)
        .expect_err("a value listed twice by the wearer must be refused, not de-duplicated");

    assert!(
        matches!(
            error.reason(),
            Reason::ValueChosenTwice { option, value }
                if option == "workflows" && value == "lint"
        ),
        "expected a value-chosen-twice refusal naming the option and value, got {error:?}"
    );
}
