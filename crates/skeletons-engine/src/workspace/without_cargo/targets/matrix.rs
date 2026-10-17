//! Each shape of target table, edition and file layout whose verdict was measured on Cargo
//! 1.85 and 1.95, with the verdict as a fixed expected value.
//!
//! A row is a manifest, the files beside it, and either the library name the package ends up
//! with (`Ok(None)` for a package with no library) or the sentence Cargo refuses it with. The
//! rows are data so that a shape missed is one row to add; the differential tests in
//! `differential.rs` run every one of them through Cargo itself. There is one file for each kind
//! of target, and each gives its rows from `rows`.

mod bench;
mod binary;
mod library;
mod other;
mod test_and_example;

use super::tests::read;

/// The sentence for a package none of whose targets exist.
const NO_TARGET: &str = "the package has no target: it needs a [lib], a [[bin]], or a source file \
                         Cargo discovers as a target";

/// A manifest for the package `p`. `edition` is the line after `version`, or empty for the 2015
/// default; `tables` follows the `[package]` keys.
fn package(edition: &str, tables: &str) -> String {
    format!("[package]\nname = \"p\"\nversion = \"0.1.0\"\n{edition}\n{tables}")
}

const E2021: &str = "edition = \"2021\"";
const E2015: &str = "edition = \"2015\"";

/// The edition line of a manifest that writes none, which Cargo reads as 2015.
const UNSET_EDITION: &str = "";

/// The tables of a manifest that writes none after its `[package]` keys.
const NO_TABLES: &str = "";

/// One measured shape: its name, manifest, files, and the library or refusal Cargo gave.
pub(in super::super) struct Row {
    pub(in super::super) name: &'static str,
    pub(in super::super) manifest: String,
    pub(in super::super) files: &'static [&'static str],
    pub(in super::super) expected: Result<Option<&'static str>, &'static str>,
}

/// The sentence Cargo itself prints for the refusal this reader words as `sentence`: the same
/// one, except for a package with no target, which Cargo words its own way.
pub(in super::super) fn cargo_sentence(sentence: &'static str) -> &'static str {
    if sentence == NO_TARGET {
        "no targets specified in the manifest"
    } else {
        sentence
    }
}

/// Every measured shape.
pub(in super::super) fn rows() -> Vec<Row> {
    let mut rows = library::rows();
    rows.extend(binary::rows());
    rows.extend(other::rows());
    rows.extend(test_and_example::rows());
    rows.extend(bench::rows());
    rows
}

fn row(
    name: &'static str,
    manifest: String,
    files: &'static [&'static str],
    expected: Result<Option<&'static str>, &'static str>,
) -> Row {
    Row {
        name,
        manifest,
        files,
        expected,
    }
}

/// Reads every row and requires the verdict Cargo gave, naming each row that differs.
fn assert_rows(rows: Vec<Row>) {
    let mismatches: Vec<String> = rows
        .into_iter()
        .filter_map(|row| {
            let expected = row
                .expected
                .map(|library| library.map(str::to_owned))
                .map_err(str::to_owned);
            let actual = read(&row.manifest, row.files);
            (actual != expected).then(|| {
                format!(
                    "{}: expected {expected:?}, got {actual:?}\n{}\nfiles: {:?}",
                    row.name, row.manifest, row.files
                )
            })
        })
        .collect();

    assert!(
        mismatches.is_empty(),
        "{} shapes differ from Cargo:\n{}",
        mismatches.len(),
        mismatches.join("\n---\n")
    );
}

#[test]
fn the_library_name_and_path_rules_match_cargo() {
    // Each row is a library shape measured on Cargo 1.85 and 1.95: its name is checked before its
    // path, a written path is never looked for, and 2015 alone falls back to `src/<name>.rs`.
    assert_rows(library::rows());
}

#[test]
fn the_binary_rules_match_cargo() {
    // Each row is a `[[bin]]` or discovered binary shape measured on Cargo: a table with no path
    // needs a file, 2015 has its own fallbacks, and a name or a duplicate Cargo refuses is refused.
    assert_rows(binary::rows());
}

#[test]
fn the_test_example_and_bench_rules_match_cargo() {
    // Each row is a `[[test]]`, `[[example]]` or `[[bench]]` shape measured on Cargo: with no file
    // it is dropped rather than refused, so it counts as a target only when its path is written
    // or its file is found.
    let mut rows = other::rows();
    rows.extend(test_and_example::rows());
    rows.extend(bench::rows());
    assert_rows(rows);
}
