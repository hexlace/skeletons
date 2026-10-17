//! Each shape of target table, edition and file layout whose verdict was measured on Cargo
//! 1.85 and 1.95, with the verdict as a fixed expected value.
//!
//! A row is a manifest, the files beside it, and either the library name the package ends up
//! with (`Ok(None)` for a package with no library) or the sentence Cargo refuses it with. The
//! rows are data so that a shape missed is one row to add; the differential tests in
//! `differential.rs` run every one of them through Cargo itself.

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
const NONE: &str = "";

/// What Cargo said when it could not find the binary `x`.
const BIN_X_NOT_FOUND: &str = "can't find `x` bin at `src/bin/x.rs` or `src/bin/x/main.rs`. \
                               Please specify bin.path if you want to use a non-default path.";

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
    let mut rows = library_rows();
    rows.extend(binary_rows());
    rows.extend(binary_fallback_rows());
    rows.extend(binary_name_rows());
    rows.extend(other_rows());
    rows.extend(test_and_example_rows());
    rows.extend(bench_rows());
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

/// The rows for the library: its name before its path, and where its path comes from.
fn library_rows() -> Vec<Row> {
    let hyphen = "library target names cannot contain hyphens: bad-name";
    let empty = "library target names cannot be empty";
    let not_found = "can't find library `p`, rename file to `src/lib.rs` or specify lib.path";
    vec![
        row(
            "a hyphenated lib name with a lib file",
            package(E2021, "[lib]\nname = \"bad-name\"\n"),
            &["src/lib.rs"],
            Err(hyphen),
        ),
        row(
            "a hyphenated lib name with no file: the name comes first",
            package(E2021, "[lib]\nname = \"bad-name\"\n"),
            &[],
            Err(hyphen),
        ),
        row(
            "an empty lib name",
            package(E2021, "[lib]\nname = \"\"\n"),
            &["src/lib.rs"],
            Err(empty),
        ),
        row(
            "a blank lib name",
            package(E2021, "[lib]\nname = \"  \"\n"),
            &["src/lib.rs"],
            Err(empty),
        ),
        row(
            "a lib name that starts with a digit",
            package(E2021, "[lib]\nname = \"1abc\"\n"),
            &["src/lib.rs"],
            Ok(Some("1abc")),
        ),
        row(
            "a lib name that is a keyword",
            package(E2021, "[lib]\nname = \"fn\"\n"),
            &["src/lib.rs"],
            Ok(Some("fn")),
        ),
        row(
            "a lib name with a dot",
            package(E2021, "[lib]\nname = \"a.b\"\n"),
            &["src/lib.rs"],
            Ok(Some("a.b")),
        ),
        row(
            "a lib path to a file that is not there",
            package(E2021, "[lib]\npath = \"missing.rs\"\n"),
            &[],
            Ok(Some("p")),
        ),
        row(
            "a lib path to a file that is not there, in 2015",
            package(NONE, "[lib]\npath = \"missing.rs\"\n"),
            &[],
            Ok(Some("p")),
        ),
        row(
            "a lib table with no path takes src/<name>.rs in 2015",
            package(NONE, "[lib]\n"),
            &["src/p.rs"],
            Ok(Some("p")),
        ),
        row(
            "a named lib table with no path takes src/<name>.rs in 2015",
            package(E2015, "[lib]\nname = \"zz\"\n"),
            &["src/zz.rs"],
            Ok(Some("zz")),
        ),
        row(
            "a lib table with no path finds no src/<name>.rs in 2021",
            package(E2021, "[lib]\n"),
            &["src/p.rs"],
            Err(not_found),
        ),
        row(
            "a lib table with no file at all in 2015",
            package(NONE, "[lib]\nname = \"zz\"\n"),
            &["src/main.rs"],
            Err("can't find library `zz`, rename file to `src/lib.rs` or specify lib.path"),
        ),
    ]
}

/// The rows for `[[bin]]`.
fn binary_rows() -> Vec<Row> {
    let table = "[[bin]]\nname = \"x\"\n";
    vec![
        row(
            "a bin with a path and a lib",
            package(E2021, "[[bin]]\nname = \"x\"\npath = \"b.rs\"\n"),
            &["src/lib.rs"],
            Ok(Some("p")),
        ),
        row(
            "a bin with a path that is the package's only target",
            package(E2021, "[[bin]]\nname = \"x\"\npath = \"b.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "a bin with no path and no file, with a lib",
            package(E2021, table),
            &["src/lib.rs"],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin with no path and no file, with no lib",
            package(E2021, table),
            &[],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin with no path and no file, in 2015",
            package(NONE, table),
            &[],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin in src/bin in 2021",
            package(E2021, table),
            &["src/bin/x.rs"],
            Ok(None),
        ),
        row(
            "a bin in a src/bin directory in 2021",
            package(E2021, table),
            &["src/bin/x/main.rs"],
            Ok(None),
        ),
        row(
            "a bin named for another file than src/main.rs in 2021",
            package(E2021, table),
            &["src/main.rs"],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin named for the package takes src/main.rs in 2021",
            package(E2021, "[[bin]]\nname = \"p\"\n"),
            &["src/main.rs"],
            Ok(None),
        ),
    ]
}

/// The rows for the places a binary falls back to in 2015, and for two files with one name.
fn binary_fallback_rows() -> Vec<Row> {
    let table = "[[bin]]\nname = \"x\"\n";
    vec![
        row(
            "a bin falls back to src/main.rs in 2015",
            package(NONE, table),
            &["src/main.rs"],
            Ok(None),
        ),
        row(
            "a bin falls back to src/<name>.rs in 2015 with no lib",
            package(NONE, table),
            &["src/x.rs"],
            Ok(None),
        ),
        row(
            "a bin does not fall back to src/<name>.rs in 2015 beside a lib",
            package(NONE, table),
            &["src/lib.rs", "src/x.rs"],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin falls back to src/main.rs in 2015 beside a lib",
            package(NONE, table),
            &["src/lib.rs", "src/x.rs", "src/main.rs"],
            Ok(Some("p")),
        ),
        row(
            "a bin falls back to src/bin/main.rs in 2015",
            package(NONE, table),
            &["src/bin/main.rs"],
            Ok(None),
        ),
        row(
            "two files for a bin fall back to src/main.rs in 2015",
            package(NONE, table),
            &["src/bin/x.rs", "src/bin/x/main.rs", "src/main.rs"],
            Ok(None),
        ),
        row(
            "two files for a bin in 2015 with no fallback",
            package(NONE, table),
            &["src/bin/x.rs", "src/bin/x/main.rs"],
            Err("cannot infer path for `x` bin"),
        ),
        row(
            "two files for a bin in 2021",
            package(E2021, table),
            &["src/bin/x.rs", "src/bin/x/main.rs"],
            Err("cannot infer path for `x` bin"),
        ),
    ]
}

/// The rows for the names a binary can have, discovered or written.
fn binary_name_rows() -> Vec<Row> {
    vec![
        row(
            "a bin with no name and a path",
            package(E2021, "[[bin]]\npath = \"b.rs\"\n"),
            &[],
            Err("binary target bin.name is required"),
        ),
        row(
            "a bin with no name and no path",
            package(E2021, "[[bin]]\n"),
            &["src/main.rs"],
            Err("binary target bin.name is required"),
        ),
        row(
            "a bin with an empty name",
            package(E2021, "[[bin]]\nname = \"\"\npath = \"b.rs\"\n"),
            &[],
            Err("binary target names cannot be empty"),
        ),
        row(
            "a bin named deps",
            package(E2021, "[[bin]]\nname = \"deps\"\npath = \"b.rs\"\n"),
            &[],
            Err(
                "the binary target name `deps` is forbidden, it conflicts with cargo's build \
                 directory names",
            ),
        ),
        row(
            "a bin named build",
            package(E2021, "[[bin]]\nname = \"build\"\npath = \"b.rs\"\n"),
            &[],
            Err(
                "the binary target name `build` is forbidden, it conflicts with cargo's build \
                 directory names",
            ),
        ),
        row(
            "two bins with one name",
            package(
                E2021,
                "[[bin]]\nname = \"x\"\npath = \"a.rs\"\n[[bin]]\nname = \"x\"\npath = \"b.rs\"\n",
            ),
            &[],
            Err("found duplicate binary name x, but all binary targets must have a unique name"),
        ),
        row(
            "a hyphen in a bin name",
            package(E2021, "[[bin]]\nname = \"a-b\"\npath = \"b.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "a discovered bin and src/main.rs with one name",
            package(E2021, NONE),
            &["src/main.rs", "src/bin/p.rs"],
            Err("found duplicate binary name p, but all binary targets must have a unique name"),
        ),
        row(
            "two discovered bins with one name",
            package(E2021, NONE),
            &["src/bin/x.rs", "src/bin/x/main.rs"],
            Err("found duplicate binary name x, but all binary targets must have a unique name"),
        ),
        row(
            "a discovered bin named deps",
            package(E2021, NONE),
            &["src/bin/deps.rs"],
            Err(
                "the binary target name `deps` is forbidden, it conflicts with cargo's build \
                 directory names",
            ),
        ),
        row(
            "a hidden file in src/bin is no target",
            package(E2021, NONE),
            &["src/bin/.h/main.rs"],
            Err(NO_TARGET),
        ),
    ]
}

/// The rows for `[[test]]`, `[[example]]` and `[[bench]]`, which are dropped when their file is
/// not there rather than refused.
fn other_rows() -> Vec<Row> {
    vec![
        row(
            "a test with a path and no other target",
            package(E2021, "[[test]]\nname = \"x\"\npath = \"t.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "a test with no path and no file beside a lib",
            package(E2021, "[[test]]\nname = \"x\"\n"),
            &["src/lib.rs"],
            Ok(Some("p")),
        ),
        row(
            "a test with no path and no file, and nothing else",
            package(E2021, "[[test]]\nname = \"x\"\n"),
            &[],
            Err(NO_TARGET),
        ),
        row(
            "a test with no path found in tests",
            package(E2021, "[[test]]\nname = \"x\"\n"),
            &["tests/x.rs"],
            Ok(None),
        ),
        row(
            "a test in 2015 beside another test file discovers nothing else",
            package(NONE, "[[test]]\nname = \"x\"\n"),
            &["tests/y.rs"],
            Err(NO_TARGET),
        ),
        row(
            "a test in 2021 beside another test file discovers it",
            package(E2021, "[[test]]\nname = \"x\"\n"),
            &["tests/y.rs"],
            Ok(None),
        ),
        row(
            "a test in 2015 beside another test file, with autotests on",
            package(NONE, "autotests = true\n[[test]]\nname = \"x\"\n"),
            &["tests/y.rs"],
            Ok(None),
        ),
        row(
            "an empty tests array in 2015 turns discovery off",
            format!("test = []\n{}", package(NONE, NONE)),
            &["tests/y.rs"],
            Err(NO_TARGET),
        ),
        row(
            "an empty tests array in 2021 leaves discovery on",
            format!("test = []\n{}", package(E2021, NONE)),
            &["tests/y.rs"],
            Ok(None),
        ),
        row(
            "an empty bin array in 2015 turns discovery off",
            format!("bin = []\n{}", package(NONE, NONE)),
            &["src/bin/y.rs"],
            Err(NO_TARGET),
        ),
        row(
            "a test with two files is dropped",
            package(E2021, "[[test]]\nname = \"x\"\n"),
            &["src/lib.rs", "tests/x.rs", "tests/x/main.rs"],
            Ok(Some("p")),
        ),
    ]
}

/// The rows for the names of tests and examples.
fn test_and_example_rows() -> Vec<Row> {
    vec![
        row(
            "a test with no name",
            package(E2021, "[[test]]\npath = \"t.rs\"\n"),
            &["src/lib.rs"],
            Err("test target test.name is required"),
        ),
        row(
            "a test with an empty name",
            package(E2021, "[[test]]\nname = \"\"\npath = \"t.rs\"\n"),
            &["src/lib.rs"],
            Err("test target names cannot be empty"),
        ),
        row(
            "two tests with one name",
            package(
                E2021,
                "[[test]]\nname = \"x\"\npath = \"a.rs\"\n\
                 [[test]]\nname = \"x\"\npath = \"b.rs\"\n",
            ),
            &["src/lib.rs"],
            Err("found duplicate test name x, but all test targets must have a unique name"),
        ),
        row(
            "two discovered tests with one name",
            package(E2021, NONE),
            &["src/lib.rs", "tests/x.rs", "tests/x/main.rs"],
            Err("found duplicate test name x, but all test targets must have a unique name"),
        ),
        row(
            "a hidden file in tests is no target",
            package(E2021, NONE),
            &["tests/.h.rs"],
            Err(NO_TARGET),
        ),
        row(
            "an example with a path and no other target",
            package(E2021, "[[example]]\nname = \"x\"\npath = \"e.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "an example with no path and no file, and nothing else",
            package(NONE, "[[example]]\nname = \"x\"\n"),
            &[],
            Err(NO_TARGET),
        ),
        row(
            "an example with no path and no file beside a lib",
            package(E2021, "[[example]]\nname = \"x\"\n"),
            &["src/lib.rs"],
            Ok(Some("p")),
        ),
        row(
            "an example with an empty name",
            package(E2021, "[[example]]\nname = \"\"\npath = \"e.rs\"\n"),
            &["src/lib.rs"],
            Err("example target names cannot be empty"),
        ),
        row(
            "an example with no name",
            package(E2021, "[[example]]\npath = \"e.rs\"\n"),
            &["src/lib.rs"],
            Err("example target example.name is required"),
        ),
        row(
            "two examples with one name",
            package(
                E2021,
                "[[example]]\nname = \"x\"\npath = \"a.rs\"\n\
                 [[example]]\nname = \"x\"\npath = \"b.rs\"\n",
            ),
            &["src/lib.rs"],
            Err("found duplicate example name x, but all example targets must have a unique name"),
        ),
        row(
            "two dropped examples with one name are not duplicates",
            package(
                E2021,
                "[[example]]\nname = \"x\"\n[[example]]\nname = \"x\"\n",
            ),
            &["src/lib.rs"],
            Ok(Some("p")),
        ),
    ]
}

/// The rows for `[[bench]]`, which has a fallback of its own in 2015.
fn bench_rows() -> Vec<Row> {
    vec![
        row(
            "a bench with a path and no other target",
            package(E2021, "[[bench]]\nname = \"x\"\npath = \"b.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "a bench with no path and no file, and nothing else",
            package(E2021, "[[bench]]\nname = \"x\"\n"),
            &[],
            Err(NO_TARGET),
        ),
        row(
            "a bench with no path and no file beside a lib",
            package(E2021, "[[bench]]\nname = \"x\"\n"),
            &["src/lib.rs"],
            Ok(Some("p")),
        ),
        row(
            "a bench named bench takes src/bench.rs in 2015",
            package(NONE, "[[bench]]\nname = \"bench\"\n"),
            &["src/bench.rs"],
            Ok(None),
        ),
        row(
            "a bench named bench does not take src/bench.rs in 2021",
            package(E2021, "[[bench]]\nname = \"bench\"\n"),
            &["src/bench.rs"],
            Err(NO_TARGET),
        ),
        row(
            "a bench named x does not take src/bench.rs in 2015",
            package(NONE, "[[bench]]\nname = \"x\"\n"),
            &["src/bench.rs"],
            Err(NO_TARGET),
        ),
        row(
            "a bench with no name",
            package(E2021, "[[bench]]\npath = \"b.rs\"\n"),
            &["src/lib.rs"],
            Err("benchmark target bench.name is required"),
        ),
        row(
            "a bench with an empty name",
            package(E2021, "[[bench]]\nname = \" \"\npath = \"b.rs\"\n"),
            &["src/lib.rs"],
            Err("benchmark target names cannot be empty"),
        ),
        row(
            "two benches with one name",
            package(
                E2021,
                "[[bench]]\nname = \"x\"\npath = \"a.rs\"\n\
                 [[bench]]\nname = \"x\"\npath = \"b.rs\"\n",
            ),
            &["src/lib.rs"],
            Err("found duplicate bench name x, but all bench targets must have a unique name"),
        ),
    ]
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
    assert_rows(library_rows());
}

#[test]
fn the_binary_rules_match_cargo() {
    // Each row is a `[[bin]]` or discovered binary shape measured on Cargo: a table with no path
    // needs a file, 2015 has its own fallbacks, and a name or a duplicate Cargo refuses is refused.
    let mut rows = binary_rows();
    rows.extend(binary_fallback_rows());
    rows.extend(binary_name_rows());
    assert_rows(rows);
}

#[test]
fn the_test_example_and_bench_rules_match_cargo() {
    // Each row is a `[[test]]`, `[[example]]` or `[[bench]]` shape measured on Cargo: with no file
    // it is dropped rather than refused, so it counts as a target only when its path is written
    // or its file is found.
    let mut rows = other_rows();
    rows.extend(test_and_example_rows());
    rows.extend(bench_rows());
    assert_rows(rows);
}
