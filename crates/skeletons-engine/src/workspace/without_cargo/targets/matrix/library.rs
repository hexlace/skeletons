//! The rows for the library: its name before its path, and where its path comes from.

use super::{E2015, E2021, Row, UNSET_EDITION, package, row};

/// Every library row.
pub(super) fn rows() -> Vec<Row> {
    let mut rows = name_rows();
    rows.extend(path_rows());
    rows
}

/// The rows for the name of the library, which Cargo checks before it looks for a file.
fn name_rows() -> Vec<Row> {
    let hyphen = "library target names cannot contain hyphens: bad-name";
    let empty = "library target names cannot be empty";
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
    ]
}

/// The rows for where the library's file comes from: a written path, or a name in 2015.
fn path_rows() -> Vec<Row> {
    let not_found = "can't find library `p`, rename file to `src/lib.rs` or specify lib.path";
    vec![
        row(
            "a lib path to a file that is not there",
            package(E2021, "[lib]\npath = \"missing.rs\"\n"),
            &[],
            Ok(Some("p")),
        ),
        row(
            "a lib path to a file that is not there, in 2015",
            package(UNSET_EDITION, "[lib]\npath = \"missing.rs\"\n"),
            &[],
            Ok(Some("p")),
        ),
        row(
            "a lib table with no path takes src/<name>.rs in 2015",
            package(UNSET_EDITION, "[lib]\n"),
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
            package(UNSET_EDITION, "[lib]\nname = \"zz\"\n"),
            &["src/main.rs"],
            Err("can't find library `zz`, rename file to `src/lib.rs` or specify lib.path"),
        ),
    ]
}
