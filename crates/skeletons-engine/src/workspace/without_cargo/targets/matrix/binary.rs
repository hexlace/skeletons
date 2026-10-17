//! The rows for `[[bin]]` and for discovered binaries.

use super::{E2021, NO_TABLES, NO_TARGET, Row, UNSET_EDITION, package, row};

/// What Cargo said when it could not find the binary `x`.
const BIN_X_NOT_FOUND: &str = "can't find `x` bin at `src/bin/x.rs` or `src/bin/x/main.rs`. \
                               Please specify bin.path if you want to use a non-default path.";

/// Every binary row.
pub(super) fn rows() -> Vec<Row> {
    let mut rows = table_rows();
    rows.extend(fallback_rows());
    rows.extend(written_name_rows());
    rows.extend(discovered_name_rows());
    rows
}

/// The rows for `[[bin]]`.
fn table_rows() -> Vec<Row> {
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
            package(UNSET_EDITION, table),
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
fn fallback_rows() -> Vec<Row> {
    let table = "[[bin]]\nname = \"x\"\n";
    vec![
        row(
            "a bin falls back to src/main.rs in 2015",
            package(UNSET_EDITION, table),
            &["src/main.rs"],
            Ok(None),
        ),
        row(
            "a bin falls back to src/<name>.rs in 2015 with no lib",
            package(UNSET_EDITION, table),
            &["src/x.rs"],
            Ok(None),
        ),
        row(
            "a bin does not fall back to src/<name>.rs in 2015 beside a lib",
            package(UNSET_EDITION, table),
            &["src/lib.rs", "src/x.rs"],
            Err(BIN_X_NOT_FOUND),
        ),
        row(
            "a bin falls back to src/main.rs in 2015 beside a lib",
            package(UNSET_EDITION, table),
            &["src/lib.rs", "src/x.rs", "src/main.rs"],
            Ok(Some("p")),
        ),
        row(
            "a bin falls back to src/bin/main.rs in 2015",
            package(UNSET_EDITION, table),
            &["src/bin/main.rs"],
            Ok(None),
        ),
        row(
            "two files for a bin fall back to src/main.rs in 2015",
            package(UNSET_EDITION, table),
            &["src/bin/x.rs", "src/bin/x/main.rs", "src/main.rs"],
            Ok(None),
        ),
        row(
            "two files for a bin in 2015 with no fallback",
            package(UNSET_EDITION, table),
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

/// The rows for the names a `[[bin]]` table can write.
fn written_name_rows() -> Vec<Row> {
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
    ]
}

/// The rows for the names of binaries Cargo discovers in `src`.
fn discovered_name_rows() -> Vec<Row> {
    vec![
        row(
            "a discovered bin and src/main.rs with one name",
            package(E2021, NO_TABLES),
            &["src/main.rs", "src/bin/p.rs"],
            Err("found duplicate binary name p, but all binary targets must have a unique name"),
        ),
        row(
            "two discovered bins with one name",
            package(E2021, NO_TABLES),
            &["src/bin/x.rs", "src/bin/x/main.rs"],
            Err("found duplicate binary name x, but all binary targets must have a unique name"),
        ),
        row(
            "a discovered bin named deps",
            package(E2021, NO_TABLES),
            &["src/bin/deps.rs"],
            Err(
                "the binary target name `deps` is forbidden, it conflicts with cargo's build \
                 directory names",
            ),
        ),
        row(
            "a hidden file in src/bin is no target",
            package(E2021, NO_TABLES),
            &["src/bin/.h/main.rs"],
            Err(NO_TARGET),
        ),
    ]
}
