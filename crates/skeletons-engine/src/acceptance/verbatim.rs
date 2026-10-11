//! Verbatim files: a skeleton's metadata can declare a file verbatim, and a
//! verbatim file is claimed byte for byte, exactly as written -- no fill, no
//! select, no new syntax in the text. The declaration is stated, never
//! inferred: `verbatim`, an array of paths relative to `files/`, under
//! `[package.metadata.skeletons]`.
//!
//! Every scenario reads a real skeleton crate under `test-skeletons/`.
//! Refusals about the declaration name `Cargo.toml` and no line, and the
//! tests hold each to its full wording, so a refusal for some other reason
//! cannot satisfy them.

use super::test_skeleton;
use crate::skeleton::{Choice, Choices, Reason, SkeletonIdentity, render};

/// Asserts that rendering the refused fixture `name` fails about `Cargo.toml`,
/// with no line, and with exactly `wording` as its reason -- and that the
/// whole message reads `skeleton <name>: Cargo.toml: <wording>`.
fn assert_refused_about_the_manifest(name: &str, wording: &str) {
    let error = render(test_skeleton(&format!("refused/{name}")), &Choices::new())
        .expect_err("a bad verbatim declaration must be refused");

    assert_eq!(
        error.skeleton(),
        &SkeletonIdentity::Named(name.to_owned()),
        "the refusal must name the skeleton"
    );
    assert_eq!(
        error.file(),
        Some("Cargo.toml"),
        "the declaration lives in the manifest, so the manifest is what is at fault"
    );
    assert_eq!(error.line(), None, "a manifest refusal names no line");
    assert_eq!(error.reason().to_string(), wording);
    assert_eq!(
        error.to_string(),
        format!("skeleton {name}: Cargo.toml: {wording}")
    );
}

fn cadence(value: &str) -> Choices {
    let mut choices = Choices::new();
    choices.insert("cadence", Choice::One(value.to_owned()));
    choices
}

#[test]
fn a_verbatim_file_is_claimed_byte_for_byte_beside_a_file_that_still_fills() {
    // The release workflow holds `${{ secrets.X }}` and `${{ github.ref }}`,
    // which the fill grammar would refuse in any other file. Declared
    // verbatim, it renders exactly as written, while `settings.yml` beside
    // it still fills `{{cadence}}` from the wearer's choice.
    let rendering = render(
        test_skeleton("renders/verbatim-workflow"),
        &cadence("daily"),
    )
    .expect("a verbatim file holding `${{` must render");

    assert_eq!(
        rendering.get(".github/workflows/release.yml"),
        Some(
            b"name: release\non:\n  push:\n    tags: [\"v*\"]\njobs:\n  publish:\n    \
              runs-on: ubuntu-latest\n    env:\n      TOKEN: ${{ secrets.X }}\n    steps:\n      \
              - env:\n          REF: ${{ github.ref }}\n        run: echo \"$REF\"\n"
                .as_slice()
        ),
        "the verbatim file must be claimed exactly as written"
    );
    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: daily\n".as_slice()),
        "a file not declared verbatim must still fill"
    );
    assert_eq!(rendering.iter().count(), 2, "exactly the two files render");
}

#[test]
fn a_verbatim_file_is_untouched_by_the_choices_of_the_options_used_elsewhere() {
    // The same skeleton with the option left at its default: the verbatim
    // file's bytes do not depend on the wearer's choices at all.
    let rendering = render(test_skeleton("renders/verbatim-workflow"), &Choices::new())
        .expect("the skeleton must render with its defaults");

    let claimed = rendering
        .get(".github/workflows/release.yml")
        .expect("the verbatim file must be claimed");
    assert!(
        claimed
            .windows(b"${{ secrets.X }}".len())
            .any(|window| window == b"${{ secrets.X }}"),
        "the expression must survive unchanged, got {}",
        String::from_utf8_lossy(claimed)
    );
    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: weekly\n".as_slice())
    );
}

#[test]
fn everything_the_grammar_reads_is_inert_inside_a_verbatim_file() {
    // The verbatim file holds a well-formed `{{cadence}}` (an option that is
    // used, and so declared, elsewhere), a well-formed directive line, the
    // same directive behind a no-break space, an unclosed `{{`, a leading
    // byte order mark, and no final newline. Every byte comes back as
    // written.
    let rendering = render(
        test_skeleton("renders/verbatim-grammar-inert"),
        &cadence("daily"),
    )
    .expect("a verbatim file must render whatever it holds");

    assert_eq!(
        rendering.get("inert.txt"),
        Some(
            b"\xEF\xBB\xBFwell-formed placeholder: {{cadence}}\n\
              # skeletons:partial anything\n\
              \xC2\xA0# skeletons:partial anything\n\
              unclosed: {{ not closed\n\
              no final newline"
                .as_slice()
        ),
        "no placeholder, directive, byte order mark or line ending may be touched"
    );
    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"schedule: daily\n".as_slice()),
        "the option still fills where it is used outside a verbatim file"
    );
}

#[test]
fn a_verbatim_file_that_needed_no_declaration_is_accepted_and_claimed_as_written() {
    // Declaring a file verbatim that holds nothing the grammar would read is
    // not an error: the declaration is stated, and it is honoured.
    let rendering = render(test_skeleton("renders/verbatim-unneeded"), &Choices::new())
        .expect("an unneeded verbatim declaration must not be refused");

    assert_eq!(
        rendering.get("plain.yml"),
        Some(b"name: plain\nvalue: 42\n".as_slice())
    );
}

#[test]
fn a_verbatim_file_need_not_be_utf_8() {
    // `blob.bin` holds bytes that are not valid UTF-8, a NUL among them, and
    // a `{{` that would open a placeholder in text. Declared verbatim, it is
    // claimed byte for byte rather than refused as not UTF-8.
    let rendering = render(test_skeleton("renders/verbatim-not-utf8"), &Choices::new())
        .expect("a binary verbatim file must render, not be refused as not UTF-8");

    assert_eq!(
        rendering.get("blob.bin"),
        Some(b"\x00\xFF\xFE\x80{{\x00\xC3(\n".as_slice())
    );
}

#[test]
fn a_verbatim_path_naming_no_file_under_files_is_refused_naming_it() {
    // `verbatim = ["missing.yml"]`, and `files/` holds only `present.yml`.
    assert_refused_about_the_manifest(
        "verbatim-names-nothing",
        "verbatim path `missing.yml` names no file; verbatim paths are relative to `files/`",
    );
}

#[test]
fn a_verbatim_declaration_on_a_partial_is_refused_spelled_relative_to_partials() {
    // `verbatim = ["cargo.yml"]`: no such file under `files/`, but a partial
    // of that name exists, so the refusal says so rather than "names no
    // file".
    assert_refused_about_the_manifest(
        "verbatim-names-partial",
        "verbatim path `cargo.yml` names a partial, and only a file under `files/` can be verbatim",
    );
}

#[test]
fn a_verbatim_declaration_on_a_partial_is_refused_spelled_by_its_tree_path() {
    // `verbatim = ["partials/cargo.yml"]`: the same partial, spelled the way
    // it sits in the skeleton's tree.
    assert_refused_about_the_manifest(
        "verbatim-names-partial-by-tree-path",
        "verbatim path `partials/cargo.yml` names a partial, and only a file under `files/` can \
         be verbatim",
    );
}

#[test]
fn a_verbatim_path_naming_a_directory_is_refused_and_says_to_list_the_files() {
    // `verbatim = [".github/workflows"]`, a directory holding `ci.yml`.
    assert_refused_about_the_manifest(
        "verbatim-names-directory",
        "verbatim path `.github/workflows` names a directory; list each file under it instead",
    );
}

#[test]
fn a_verbatim_path_listed_twice_is_refused_naming_it() {
    // `verbatim = ["a.yml", "a.yml"]`.
    assert_refused_about_the_manifest(
        "verbatim-listed-twice",
        "verbatim path `a.yml` is listed twice",
    );
}

#[test]
fn verbatim_that_is_not_an_array_is_refused_as_the_wrong_type() {
    // `verbatim = "a.yml"`: a string where an array belongs. The wording
    // reads "an array", not "a array".
    assert_refused_about_the_manifest(
        "verbatim-not-an-array",
        "expected `package.metadata.skeletons.verbatim` to be an array",
    );
}

#[test]
fn a_verbatim_entry_that_is_not_a_string_is_refused_as_the_wrong_type() {
    // `verbatim = [1]`: the array is right, its entry is not.
    assert_refused_about_the_manifest(
        "verbatim-entry-not-a-string",
        "expected `package.metadata.skeletons.verbatim` to be a string",
    );
}

#[test]
fn an_option_used_only_inside_a_verbatim_file_is_refused_as_unused() {
    // `{{cadence}}` appears only in `template.yml`, which is declared
    // verbatim, so it fills nothing: the option is declared and used by no
    // file that fills, and is refused as unused.
    let error = render(
        test_skeleton("refused/verbatim-option-used-only-there"),
        &Choices::new(),
    )
    .expect_err("an option only a verbatim file mentions must be refused as unused");

    assert!(
        matches!(error.reason(), Reason::OptionUnused { option } if option == "cadence"),
        "expected an unused-option refusal naming `cadence`, got {error:?}"
    );
}

#[test]
fn a_github_actions_expression_outside_a_verbatim_file_is_still_refused_and_says_how_to_fix_it() {
    // `${{ matrix.os }}` in a file not declared verbatim is still malformed.
    // The refusal's wording says a `{{` that is not a placeholder can stand
    // only in a file declared verbatim.
    let error = render(
        test_skeleton("refused/github-actions-expression"),
        &Choices::new(),
    )
    .expect_err("a GitHub Actions expression outside a verbatim file must be refused");

    assert_eq!(error.file(), Some("files/workflow.yml"));
    assert_eq!(error.line(), Some(6));
    assert!(
        matches!(error.reason(), Reason::PlaceholderMalformed),
        "expected a malformed-placeholder refusal, got {error:?}"
    );
    assert_eq!(
        error.reason().to_string(),
        "opens `{{` without a well-formed name and `}}` on the same line; a `{{` that is not a \
         placeholder can stand only in a file declared verbatim"
    );
}

#[test]
fn a_github_actions_expression_in_a_partial_is_refused_without_pointing_at_verbatim() {
    // A partial can never be declared verbatim, so its refusal must not send
    // the author there: following that hint would only earn the refusal for
    // a verbatim path that names a partial. It says where the text belongs
    // instead, a file under `files/` declared verbatim.
    let error = render(
        test_skeleton("refused/github-actions-expression-in-partial"),
        &Choices::new(),
    )
    .expect_err("a GitHub Actions expression in a partial must be refused");

    assert_eq!(error.file(), Some("partials/build.yml"));
    assert_eq!(error.line(), Some(3));
    let wording = error.reason().to_string();
    assert_eq!(
        wording,
        "opens `{{` without a well-formed name and `}}` on the same line; a partial cannot hold \
         a `{{` that is not a placeholder, so that text belongs in a file under `files/` \
         declared verbatim"
    );
    assert!(
        !wording.contains("can stand only in a file declared verbatim"),
        "a partial's refusal must not offer the fix a partial cannot take: {wording}"
    );
}
