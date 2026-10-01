//! Schema: `[package.metadata.skeletons]` is a closed schema. Every way to
//! break it -- an unknown key, a missing default, a default or a value
//! outside its declared set, a value declared twice, a partial that does
//! not exist, a partial nothing selects -- is refused, naming the skeleton,
//! `Cargo.toml`, and what specifically is wrong, never silently accepted
//! or silently corrected.
//!
//! `UnknownKey` and `MissingKey` carry a dotted key path "from the manifest
//! root", such as `package.metadata.skeletons.options.cadence.defualt`.
//! These tests check that the option name and the offending field both
//! appear in that path, rather than pinning the whole dotted string: one
//! example shows the shape, not a full grammar for the path, and asserting
//! more than that would test a guess rather than the behaviour.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn an_unknown_key_in_an_option_table_is_refused_naming_it() {
    // The `[package.metadata.skeletons]` table is a closed schema: an unknown
    // key is refused, naming the skeleton, `Cargo.toml` and what is wrong.
    // `options.cadence` declares an extra `surprising` key.
    let error = render(test_skeleton("refused/unknown-key"), &Choices::new())
        .expect_err("an unknown key in the schema must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::UnknownKey { key } if key.contains("cadence") && key.contains("surprising")
        ),
        "expected an unknown-key refusal naming the option and the stray key, got {error:?}"
    );
}

#[test]
fn an_enum_option_with_no_default_is_refused_naming_it() {
    // An enum option with no default is refused, naming the skeleton,
    // `Cargo.toml` and what is wrong. `options.cadence` declares `type` and
    // `values` but no `default`.
    let error = render(
        test_skeleton("refused/option-missing-default"),
        &Choices::new(),
    )
    .expect_err("an enum option with no default must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::MissingKey { key } if key.contains("cadence") && key.contains("default")
        ),
        "expected a missing-key refusal naming the option and the missing default, got {error:?}"
    );
}

#[test]
fn an_enum_default_outside_its_values_is_refused_naming_it() {
    // An enum default outside its values is refused, naming the skeleton,
    // `Cargo.toml` and what is wrong. `cadence` declares values
    // daily/weekly but defaults to "monthly".
    let error = render(
        test_skeleton("refused/enum-default-not-declared"),
        &Choices::new(),
    )
    .expect_err("an enum default outside its declared values must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::DefaultNotDeclared { option, value }
                if option == "cadence" && value == "monthly"
        ),
        "expected a default-not-declared refusal naming the option and value, got {error:?}"
    );
}

#[test]
fn a_set_default_outside_its_values_is_refused_naming_it() {
    // A set default naming a value outside its values is refused, naming
    // the skeleton, `Cargo.toml` and what is wrong. `workflows` declares
    // lint/test but defaults to `["release"]`.
    let error = render(
        test_skeleton("refused/set-default-not-declared"),
        &Choices::new(),
    )
    .expect_err("a set default naming an undeclared value must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::DefaultNotDeclared { option, value }
                if option == "workflows" && value == "release"
        ),
        "expected a default-not-declared refusal naming the option and value, got {error:?}"
    );
}

#[test]
fn a_value_declared_twice_is_refused_naming_it() {
    // A value declared twice is refused, naming the skeleton, `Cargo.toml` and
    // what is wrong. `cadence` declares `["daily", "daily", "weekly"]`.
    let error = render(
        test_skeleton("refused/value-declared-twice"),
        &Choices::new(),
    )
    .expect_err("a value declared twice in one option's values must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::ValueDeclaredTwice { option, value }
                if option == "cadence" && value == "daily"
        ),
        "expected a value-declared-twice refusal naming the option and value, got {error:?}"
    );
}

#[test]
fn a_value_mapped_to_a_partial_that_does_not_exist_is_refused_naming_it() {
    // A value mapped to a partial file that does not exist is refused,
    // naming the skeleton, `Cargo.toml` and what is wrong. `workflows` maps
    // "release" to `release.yml`, which is not shipped under `partials/`.
    let error = render(test_skeleton("refused/partial-not-found"), &Choices::new())
        .expect_err("a value mapped to a missing partial must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::PartialNotFound { option, value, partial }
                if option == "workflows" && value == "release" && partial == "release.yml"
        ),
        "expected a partial-not-found refusal naming the option, value and partial, got {error:?}"
    );
}

#[test]
fn a_file_under_partials_that_no_value_selects_is_refused_naming_it() {
    // A file under `partials/` that no value selects is refused, naming the
    // skeleton, `Cargo.toml` and what is wrong. `partials/stray.yml` exists but
    // no declared value maps to it.
    let error = render(
        test_skeleton("refused/partial-selected-by-nothing"),
        &Choices::new(),
    )
    .expect_err("a partial file that nothing selects must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::PartialSelectedByNothing { partial } if partial == "stray.yml"
        ),
        "expected a partial-selected-by-nothing refusal naming the stray partial, got {error:?}"
    );
}

#[test]
fn an_option_type_that_is_not_enum_set_or_text_is_refused_naming_it() {
    // The only option types are `enum` and `text` (both fill) and `set` (selects).
    // `cadence` declares `type = "flag"`.
    let error = render(
        test_skeleton("refused/option-type-unknown"),
        &Choices::new(),
    )
    .expect_err("an option type other than enum, set or text must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(
            error.reason(),
            Reason::OptionTypeUnknown { option, given }
                if option == "cadence" && given == "flag"
        ),
        "expected an option-type-unknown refusal naming the option and the given type, got {error:?}"
    );
}

#[test]
fn an_enum_option_used_by_no_placeholder_is_refused_naming_it() {
    // An option the skeleton declares but never uses -- an `enum` named by no
    // placeholder in any file or partial -- is refused, naming the skeleton,
    // `Cargo.toml` and the option. `cadence` is declared but no file
    // contains `{{cadence}}`.
    let error = render(test_skeleton("refused/enum-option-unused"), &Choices::new())
        .expect_err("an enum option named by no placeholder must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::OptionUnused { option } if option == "cadence"),
        "expected an option-unused refusal naming the enum option, got {error:?}"
    );
}

#[test]
fn a_set_option_used_by_no_directive_is_refused_naming_it() {
    // A `set` that no directive names: `workflows` is declared, its partials
    // all exist and are all mapped, but no file contains a
    // `# skeletons:partial workflows` directive.
    let error = render(test_skeleton("refused/set-option-unused"), &Choices::new())
        .expect_err("a set option named by no directive must be refused");

    assert_eq!(error.file(), Some("Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::OptionUnused { option } if option == "workflows"),
        "expected an option-unused refusal naming the set option, got {error:?}"
    );
}
