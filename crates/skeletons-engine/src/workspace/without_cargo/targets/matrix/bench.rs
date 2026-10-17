//! The rows for `[[bench]]`, which has a fallback of its own in 2015.

use super::{E2021, NO_TARGET, Row, UNSET_EDITION, package, row};

/// Every bench row.
pub(super) fn rows() -> Vec<Row> {
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
            package(UNSET_EDITION, "[[bench]]\nname = \"bench\"\n"),
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
            package(UNSET_EDITION, "[[bench]]\nname = \"x\"\n"),
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
