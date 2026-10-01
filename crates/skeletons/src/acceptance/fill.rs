//! Fill: an `enum` option's placeholder is replaced by the chosen value, or
//! by the declared default when the wearer chose nothing. A `text` option fills
//! the same way, and its own scenarios are in `optional_text`.

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, render};

#[test]
fn an_enum_placeholder_is_filled_with_the_declared_default_when_nothing_is_chosen() {
    // A skeleton declares an option of type `enum` with a closed set of values
    // and a default, and any `{{name}}` written in one of its files is
    // replaced with whichever value was chosen for that option, or with the
    // default when none was chosen. This exercises the "default" half: the
    // skeleton declares `cadence` as enum(daily/weekly/monthly, default weekly)
    // and the wearer chooses nothing at all.
    let rendering = render(test_skeleton("renders/enum-fill"), &Choices::new())
        .expect("a skeleton with one enum option and no choices must render");

    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: weekly\n".as_slice()),
        "the placeholder must be filled with the option's declared default"
    );
}

#[test]
fn an_enum_placeholder_is_filled_with_the_wearers_chosen_value() {
    // Same rule as above, the "chosen value" half: the wearer picks
    // "daily" for `cadence`, and the placeholder must carry that value
    // rather than the default.
    let mut choices = Choices::new();
    choices.insert("cadence", Choice::One("daily".to_owned()));

    let rendering = render(test_skeleton("renders/enum-fill"), &choices)
        .expect("a valid chosen value must render");

    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: daily\n".as_slice()),
        "the placeholder must be filled with the wearer's chosen value, not the default"
    );
}

#[test]
fn a_fill_replacement_value_is_never_itself_rescanned_for_placeholders() {
    // Fill is one pass over the skeleton's own text: the replacement value is
    // never itself scanned for placeholders or directives. The skeleton's
    // declared default for `literal` is the literal string `{{surprise}}`
    // -- a value that looks exactly like a fill placeholder. If render
    // performed a second pass over its own
    // output, this would either be expanded again (and fail, since
    // `surprise` names no option) or otherwise mutated; one-pass fill
    // leaves it untouched.
    let rendering = render(
        test_skeleton("renders/fill-replacement-not-rescanned"),
        &Choices::new(),
    )
    .expect("a replacement value that merely looks like a placeholder must not be rescanned");

    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"value: {{surprise}}\n".as_slice()),
        "the chosen value must appear verbatim, never re-expanded"
    );
}

#[test]
fn a_placeholder_followed_by_a_control_byte_is_filled_and_the_byte_kept() {
    // A placeholder is filled wherever it sits in a line, and a control byte
    // after it is ordinary content that stays after the value. The skeleton
    // declares `cadence` (default weekly) and its file is
    // `schedule: {{cadence}}` followed by a control byte and a newline. This
    // also guards against a render that refuses or strips the byte only on a
    // line it fills.
    let rendering = render(
        test_skeleton("renders/fill-then-control-byte"),
        &Choices::new(),
    )
    .expect("a placeholder followed by a control byte must render");

    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: weekly\r\n".as_slice()),
        "the placeholder must be filled and the control byte kept after the value"
    );
}
