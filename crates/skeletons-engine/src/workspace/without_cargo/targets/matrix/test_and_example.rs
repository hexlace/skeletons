//! The rows for the names of tests and examples.

use super::{E2021, NO_TABLES, NO_TARGET, Row, UNSET_EDITION, package, row};

/// Every row for the names of tests and examples.
pub(super) fn rows() -> Vec<Row> {
    let mut rows = test_name_rows();
    rows.extend(example_name_rows());
    rows
}

/// The rows for the names of tests, written or discovered.
fn test_name_rows() -> Vec<Row> {
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
            package(E2021, NO_TABLES),
            &["src/lib.rs", "tests/x.rs", "tests/x/main.rs"],
            Err("found duplicate test name x, but all test targets must have a unique name"),
        ),
        row(
            "a hidden file in tests is no target",
            package(E2021, NO_TABLES),
            &["tests/.h.rs"],
            Err(NO_TARGET),
        ),
    ]
}

/// The rows for examples: the ones that are the only target, and the names they can have.
fn example_name_rows() -> Vec<Row> {
    vec![
        row(
            "an example with a path and no other target",
            package(E2021, "[[example]]\nname = \"x\"\npath = \"e.rs\"\n"),
            &[],
            Ok(None),
        ),
        row(
            "an example with no path and no file, and nothing else",
            package(UNSET_EDITION, "[[example]]\nname = \"x\"\n"),
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
