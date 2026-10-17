//! Acceptance: the exact bytes `check` and `check --json` print, and the
//! status they exit with, for a workspace that wears a skeleton, has a drifted
//! file and has a refused wearing.
//!
//! Every other scenario in this directory asserts on parts of the output. This
//! one pins all of it, stdout, stderr and exit status, so that a change which
//! alters anything the two commands print, for any reason, fails here. The
//! expected text is a fixed value written below; the one place it varies is the
//! checked-in skeleton's location, which the commands print and the tests fill
//! in.

mod support;

use support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER;
use support::{
    Fixture, TestOutcome, path_dependency_on_test_skeleton, wearing_table, write_package_manifest,
};

/// The workspace under test: `passthrough-plain` worn under its own name, a
/// claimed file `plain.yml` holding other bytes than the skeleton renders, and
/// a wearing table `ghost` that no dependency is declared under.
fn drifted_and_refused() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
        wearing_table("ghost", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    assert_ne!(
        PASSTHROUGH_PLAIN_RENDER, b"name: edited\n",
        "the fixture's edit must differ from the render"
    );
    fixture.write("plain.yml", b"name: edited\n")?;
    Ok(fixture)
}

#[test]
fn check_prints_these_exact_bytes_for_a_drifted_and_refused_workspace() -> TestOutcome {
    // Given a workspace with one worn skeleton whose file has changed and one
    // wearing that names no dependency, `check` exits 1 and prints exactly the
    // text below on stdout and nothing on stderr. The text is a fixed value, so
    // a reworded line, a new line or a reordered one fails.
    let fixture = drifted_and_refused()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);
    assert_eq!(
        report.stdout,
        EXPECTED_HUMAN_STDOUT.replace("<skeleton>", &skeleton_location()?)
    );
    assert_eq!(report.stderr, EXPECTED_HUMAN_STDERR);
    Ok(())
}

#[test]
fn check_json_prints_these_exact_bytes_for_a_drifted_and_refused_workspace() -> TestOutcome {
    // The same workspace through `--json`: the whole document, byte for byte,
    // the exit status and an empty stderr.
    let fixture = drifted_and_refused()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);
    assert_eq!(
        report.stdout,
        EXPECTED_JSON_STDOUT.replace("<skeleton>", &skeleton_location()?)
    );
    assert_eq!(report.stderr, EXPECTED_JSON_STDERR);
    Ok(())
}

/// Where the checked-in skeleton lives as the two commands print it, which is
/// the only text in their output that names a place outside the workspace.
fn skeleton_location() -> Result<String, Box<dyn std::error::Error>> {
    let path = std::fs::canonicalize(support::checked_in_test_skeleton("passthrough-plain"))?;
    Ok(path
        .to_str()
        .ok_or("the skeleton's path is not text")?
        .to_owned())
}

/// What `check` prints on stdout, with `<skeleton>` standing for the
/// skeleton's location.
const EXPECTED_HUMAN_STDOUT: &str = "passthrough-plain in Cargo.toml: passthrough-plain 0.0.0 \
from path <skeleton>, pinned\n  drifted (changed)  plain.yml\n\nrefused: \
[package.metadata.skeletons.ghost] in Cargo.toml names no dependency of Cargo.toml; add the \
dependency, or rename the table to the key the dependency is declared under\n\n1 bone: 1 \
drifted. 1 worn skeleton: 1 pinned. 1 refusal.\n";

/// What `check` prints on stderr.
const EXPECTED_HUMAN_STDERR: &str = "ritual: 1 of 1 bone has drifted, and there is 1 refusal\n";

/// What `check --json` prints on stdout, with `<skeleton>` standing for the
/// skeleton's location.
const EXPECTED_JSON_STDOUT: &str = concat!(
    r#"{
  "format_version": 1,
  "filters": [],
  "summary": {
    "bones": 1,
    "matches": 0,
    "drifted": 1,
    "skeletons": 1,
    "current": 0,
    "behind": 0,
    "pinned": 1,
    "undetermined": 0,
    "refusals": 1,
    "failed": true
  },
  "skeletons": [
    {
      "manifest": "Cargo.toml",
      "dependency": "passthrough-plain",
      "skeleton": "passthrough-plain",
      "version": "0.0.0",
      "pin": {
        "kind": "path",
        "path": "<skeleton>",
        "detail": "path <skeleton>"
      },
      "behind": {
        "state": "pinned"
      },
      "refused": false
    }
  ],
  "bones": [
    {
      "path": "plain.yml",
      "manifest": "Cargo.toml",
      "dependency": "passthrough-plain",
      "skeleton": "passthrough-plain",
      "version": "0.0.0",
      "pin": {
        "kind": "path",
        "path": "<skeleton>",
        "detail": "path <skeleton>"
      },
      "drift": {
        "state": "drifted",
        "reason": "changed"
      },
      "behind": {
        "state": "pinned"
      }
    }
  ],
  "refusals": [
    {
      "kind": "names-no-dependency",
      "manifest": "Cargo.toml",
      "dependency": "ghost",
      "message": "[package.metadata.skeletons.ghost] in Cargo.toml names no dependency of "#,
    r#"Cargo.toml; add the dependency, or rename the table to the key the dependency is "#,
    r#"declared under"
    }
  ],
  "aborted": null
}
"#
);

/// What `check --json` prints on stderr.
const EXPECTED_JSON_STDERR: &str = "ritual: 1 of 1 bone has drifted, and there is 1 refusal\n";

/// The directories a wider workspace depends on, which must outlive the
/// fixture that points at them.
struct WiderWorkspace {
    fixture: Fixture,
    _dependencies: Vec<support::TemporaryDirectory>,
}

/// A dependency with a library and no `[package.metadata.skeletons]`, which a
/// wearing cannot name as a skeleton.
fn library_without_skeleton_table()
-> Result<support::TemporaryDirectory, Box<dyn std::error::Error>> {
    let directory = support::TemporaryDirectory::new("golden-plain-library")?;
    write_package_manifest(directory.path(), "", "plain-library", "")?;
    Ok(directory)
}

/// A dependency with a binary and no library, which cargo writes no edge for.
fn binary_only() -> Result<support::TemporaryDirectory, Box<dyn std::error::Error>> {
    let directory = support::TemporaryDirectory::new("golden-binary-only")?;
    support::write(
        directory.path(),
        "Cargo.toml",
        b"[package]\nname = \"binary-only\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
          publish = false\n",
    )?;
    support::write(directory.path(), "src/main.rs", b"fn main() {}\n")?;
    Ok(directory)
}

/// The workspace under test for the wider pair: a worn skeleton whose file
/// matches, a wearing of a binary-only package, a wearing of a library with no
/// skeleton table, and a `skeletons` table holding a string where a wearing
/// table belongs.
fn matching_and_three_refusals() -> Result<WiderWorkspace, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let binary = binary_only()?;
    let library = library_without_skeleton_table()?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n{}\n[package.metadata.skeletons]\noddity = \"text\"\n\n{}\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        support::path_dependency_on("binary-only", binary.path()),
        support::path_dependency_on("plain-library", library.path()),
        wearing_table("passthrough-plain", ""),
        wearing_table("binary-only", ""),
        wearing_table("plain-library", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    Ok(WiderWorkspace {
        fixture,
        _dependencies: vec![binary, library],
    })
}

#[test]
fn check_prints_these_exact_bytes_across_more_refusal_kinds() -> TestOutcome {
    // A worn skeleton whose file matches, plus an `unresolved`, a
    // `not-a-skeleton` and a `not-a-table` refusal: `check` exits 1 and prints
    // exactly the text below, so a reworded message for any of the kinds fails.
    let workspace = matching_and_three_refusals()?;

    let report = workspace.fixture.run(&["skeletons", "check"])?;

    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);
    assert_eq!(
        report.stdout,
        WIDER_HUMAN_STDOUT.replace("<skeleton>", &skeleton_location()?)
    );
    assert_eq!(report.stderr, WIDER_HUMAN_STDERR);
    Ok(())
}

#[test]
fn check_json_prints_these_exact_bytes_across_more_refusal_kinds() -> TestOutcome {
    // The same workspace through `--json`: every refusal's kind word and
    // message, byte for byte.
    let workspace = matching_and_three_refusals()?;

    let report = workspace.fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);
    assert_eq!(
        report.stdout,
        WIDER_JSON_STDOUT.replace("<skeleton>", &skeleton_location()?)
    );
    assert_eq!(report.stderr, WIDER_JSON_STDERR);
    Ok(())
}

/// What `check` prints on stdout for the wider workspace, with `<skeleton>`
/// standing for the skeleton's location.
const WIDER_HUMAN_STDOUT: &str = "passthrough-plain in Cargo.toml: passthrough-plain 0.0.0 \
from path <skeleton>, pinned\n  matches            plain.yml\n\nrefused: plain-library in \
Cargo.toml is plain-library 0.1.0, which is not a skeleton: its manifest has no \
[package.metadata.skeletons] table; drop [package.metadata.skeletons.plain-library], or depend \
on a skeleton\nrefused: [package.metadata.skeletons.oddity] in Cargo.toml is not a table; it \
reads [package.metadata.skeletons.oddity] on a line of its own, with the skeleton's options, if \
any, under it\nrefused: binary-only in Cargo.toml is declared, but cargo resolved no package for \
it, so there is no locked skeleton to compare against\n\n1 bone: 1 matches. 1 worn skeleton: 1 \
pinned. 3 refusals.\n";

/// What `check` prints on stderr for the wider workspace.
const WIDER_HUMAN_STDERR: &str = "ritual: there are 3 refusals\n";

/// What `check --json` prints on stdout for the wider workspace, with
/// `<skeleton>` standing for the skeleton's location.
const WIDER_JSON_STDOUT: &str = concat!(
    r#"{
  "format_version": 1,
  "filters": [],
  "summary": {
    "bones": 1,
    "matches": 1,
    "drifted": 0,
    "skeletons": 1,
    "current": 0,
    "behind": 0,
    "pinned": 1,
    "undetermined": 0,
    "refusals": 3,
    "failed": true
  },
  "skeletons": [
    {
      "manifest": "Cargo.toml",
      "dependency": "passthrough-plain",
      "skeleton": "passthrough-plain",
      "version": "0.0.0",
      "pin": {
        "kind": "path",
        "path": "<skeleton>",
        "detail": "path <skeleton>"
      },
      "behind": {
        "state": "pinned"
      },
      "refused": false
    }
  ],
  "bones": [
    {
      "path": "plain.yml",
      "manifest": "Cargo.toml",
      "dependency": "passthrough-plain",
      "skeleton": "passthrough-plain",
      "version": "0.0.0",
      "pin": {
        "kind": "path",
        "path": "<skeleton>",
        "detail": "path <skeleton>"
      },
      "drift": {
        "state": "matches"
      },
      "behind": {
        "state": "pinned"
      }
    }
  ],
  "refusals": [
    {
      "kind": "not-a-skeleton",
      "manifest": "Cargo.toml",
      "dependency": "plain-library",
      "package": {
        "name": "plain-library",
        "version": "0.1.0"
      },
      "message": "plain-library in Cargo.toml is plain-library 0.1.0, "#,
    r#"which is not a skeleton: its manifest has no [package.metadata.skeletons] table; "#,
    r#"drop [package.metadata.skeletons.plain-library], or depend on a skeleton"
    },
    {
      "kind": "not-a-table",
      "manifest": "Cargo.toml",
      "dependency": "oddity",
      "message": "[package.metadata.skeletons.oddity] in Cargo.toml is not a table; "#,
    r#"it reads [package.metadata.skeletons.oddity] on a line of its own, "#,
    r#"with the skeleton's options, if any, under it"
    },
    {
      "kind": "unresolved",
      "manifest": "Cargo.toml",
      "dependency": "binary-only",
      "message": "binary-only in Cargo.toml is declared, but cargo resolved no package "#,
    r#"for it, so there is no locked skeleton to compare against"
    }
  ],
  "aborted": null
}
"#
);

/// What `check --json` prints on stderr for the wider workspace.
const WIDER_JSON_STDERR: &str = "ritual: there are 3 refusals\n";
