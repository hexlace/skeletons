//! Determinism: the same skeleton, rendered with the same option values, always
//! produces byte-identical output -- and the order an unordered choice
//! happens to be stored or supplied in must never leak into the result.

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, render};

/// `files/ci.yml` of `renders/determinism`, with all three of its set
/// option's values selected, in the skeleton's declared order (a, b, c). Every
/// test in this module renders against this one literal.
const EXPECTED: &[u8] = b"jobs:\na-job: one\nb-job: two\nc-job: three\ndone: true\n";

/// All six orderings of three elements, written out rather than computed:
/// this suite takes no dependency to generate permutations for one fixed,
/// small case.
const PERMUTATIONS_OF_THREE_VALUES: [[&str; 3]; 6] = [
    ["a", "b", "c"],
    ["a", "c", "b"],
    ["b", "a", "c"],
    ["b", "c", "a"],
    ["c", "a", "b"],
    ["c", "b", "a"],
];

#[test]
fn choosing_all_three_values_in_every_supplied_order_always_renders_the_same_bytes() {
    // Rendering the same skeleton with the same option values always
    // produces byte-identical output -- even when a set value is supplied as
    // an unordered collection, where the order values happen to be stored in
    // must never leak into the result.
    // The same three values are supplied in every one of their six possible
    // orders; the skeleton's own declared order (a, b, c) must win every time,
    // regardless of how the wearer happened to list them.
    for ordering in PERMUTATIONS_OF_THREE_VALUES {
        let mut choices = Choices::new();
        choices.insert(
            "workflows",
            Choice::Many(ordering.iter().map(|value| (*value).to_owned()).collect()),
        );

        let rendering = render(test_skeleton("renders/determinism"), &choices)
            .unwrap_or_else(|error| panic!("ordering {ordering:?} must render, got {error:?}"));

        assert_eq!(
            rendering.get("ci.yml"),
            Some(EXPECTED),
            "ordering {ordering:?} must produce the same bytes as every other ordering"
        );
    }
}

#[test]
fn rendering_the_same_choices_dozens_of_times_in_one_process_always_matches() {
    // Same determinism guarantee, the "repeatedly, in one process" half: a
    // single `Choices` value, rendered fifty times over in one process,
    // must produce the identical literal every time. This is the guard
    // against any internal reliance on hash-map iteration order or similar
    // non-determinism that a single render, or even a handful, would not
    // surface.
    let mut choices = Choices::new();
    choices.insert(
        "workflows",
        Choice::Many(vec!["c".to_owned(), "a".to_owned(), "b".to_owned()]),
    );

    for attempt in 0..50 {
        let rendering = render(test_skeleton("renders/determinism"), &choices)
            .unwrap_or_else(|error| panic!("attempt {attempt} must render, got {error:?}"));

        assert_eq!(
            rendering.get("ci.yml"),
            Some(EXPECTED),
            "attempt {attempt} must match the same committed literal as every other attempt"
        );
    }
}
