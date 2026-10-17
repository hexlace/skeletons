//! The rows for `[[test]]`, `[[example]]` and `[[bench]]`, which are dropped when their file is
//! not there rather than refused.

use super::{E2021, NO_TABLES, NO_TARGET, Row, UNSET_EDITION, package, row};

/// Every row for a target that is dropped when its file is not there.
pub(super) fn rows() -> Vec<Row> {
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
            package(UNSET_EDITION, "[[test]]\nname = \"x\"\n"),
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
            package(UNSET_EDITION, "autotests = true\n[[test]]\nname = \"x\"\n"),
            &["tests/y.rs"],
            Ok(None),
        ),
        row(
            "an empty tests array in 2015 turns discovery off",
            format!("test = []\n{}", package(UNSET_EDITION, NO_TABLES)),
            &["tests/y.rs"],
            Err(NO_TARGET),
        ),
        row(
            "an empty tests array in 2021 leaves discovery on",
            format!("test = []\n{}", package(E2021, NO_TABLES)),
            &["tests/y.rs"],
            Ok(None),
        ),
        row(
            "an empty bin array in 2015 turns discovery off",
            format!("bin = []\n{}", package(UNSET_EDITION, NO_TABLES)),
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
