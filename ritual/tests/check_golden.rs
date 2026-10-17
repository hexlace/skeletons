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
const EXPECTED_JSON_STDOUT: &str = r#"{
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
      "message": "[package.metadata.skeletons.ghost] in Cargo.toml names no dependency of Cargo.toml; add the dependency, or rename the table to the key the dependency is declared under"
    }
  ],
  "aborted": null
}
"#;

/// What `check --json` prints on stderr.
const EXPECTED_JSON_STDERR: &str = "ritual: 1 of 1 bone has drifted, and there is 1 refusal\n";
