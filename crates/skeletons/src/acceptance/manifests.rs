//! An entry named `Cargo.toml` -- file or directory, at any depth, under
//! `files/` or `partials/` -- is refused, naming it: Cargo leaves such a
//! directory out of the package it builds, in git and outside it, so a skeleton
//! shipped that way would render one way from its author's checkout and
//! another from the registry. An entry whose name merely folds to
//! `Cargo.toml` (lowercase `cargo.toml`) is refused for the same reason: a
//! case-insensitive filesystem finds it as the manifest just as readily.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn a_cargo_toml_file_nested_under_files_is_refused_naming_it() {
    // `files/sub/Cargo.toml`: a file, one directory deep under `files/`.
    let error = render(
        test_skeleton("refused/nested-manifest-in-files-file"),
        &Choices::new(),
    )
    .expect_err("a nested Cargo.toml file under files/ must be refused");

    assert_eq!(error.file(), Some("files/sub/Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::NestedManifest),
        "expected a nested-manifest refusal, got {error:?}"
    );
}

#[test]
fn a_cargo_toml_directory_nested_under_files_is_refused_naming_it() {
    // `files/a/b/Cargo.toml/`: a directory named `Cargo.toml`, two levels
    // deep under `files/`, holding `inner.txt`. The refusal must name the
    // directory itself -- `files/a/b/Cargo.toml` -- not the file inside it:
    // whatever is under a directory Cargo would already exclude is beside
    // the point.
    let error = render(
        test_skeleton("refused/nested-manifest-in-files-directory"),
        &Choices::new(),
    )
    .expect_err("a nested Cargo.toml directory under files/ must be refused");

    assert_eq!(error.file(), Some("files/a/b/Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::NestedManifest),
        "expected a nested-manifest refusal, got {error:?}"
    );
}

#[test]
fn a_cargo_toml_file_at_the_top_of_partials_is_refused_naming_it() {
    // `partials/Cargo.toml`: a file, at the top of `partials/` rather than
    // nested -- the `partials/` half of the same rule, at a different depth
    // than the `files/` cases above.
    let error = render(
        test_skeleton("refused/nested-manifest-in-partials-file"),
        &Choices::new(),
    )
    .expect_err("a Cargo.toml file at the top of partials/ must be refused");

    assert_eq!(error.file(), Some("partials/Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::NestedManifest),
        "expected a nested-manifest refusal, got {error:?}"
    );
}

#[test]
fn a_cargo_toml_directory_nested_under_partials_is_refused_naming_it() {
    // `partials/nested/Cargo.toml/`: a directory named `Cargo.toml`, one
    // level deep under `partials/`, holding `x.txt`.
    let error = render(
        test_skeleton("refused/nested-manifest-in-partials-directory"),
        &Choices::new(),
    )
    .expect_err("a nested Cargo.toml directory under partials/ must be refused");

    assert_eq!(error.file(), Some("partials/nested/Cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::NestedManifest),
        "expected a nested-manifest refusal, got {error:?}"
    );
}

#[test]
fn a_name_that_folds_to_cargo_toml_is_refused_the_same_way() {
    // `files/cargo.toml`, all lowercase: on a case-insensitive filesystem,
    // Cargo finds this file as the crate's own manifest exactly as it would
    // find `Cargo.toml` -- so it is refused for the same reason, not merely
    // because it happens to be misnamed.
    let error = render(
        test_skeleton("refused/nested-manifest-folds-to-lowercase"),
        &Choices::new(),
    )
    .expect_err("a name that folds to Cargo.toml must be refused");

    assert_eq!(error.file(), Some("files/cargo.toml"));
    assert!(
        matches!(error.reason(), Reason::NestedManifest),
        "expected a nested-manifest refusal, got {error:?}"
    );
}
