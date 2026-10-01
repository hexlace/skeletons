//! Acceptance: a repository wearing a skeleton with an optional `text` option,
//! through the real `check` and `sync` commands.
//!
//! - `text-optional` declares `assignee`, a `text` with no default. Its only
//!   file, `dependabot.yml`, holds two lines that carry `{{assignee}}` and two
//!   that do not; an unset `assignee` drops the two that carry it.
//! - `text-value-not-rescanned` declares a `cadence` enum, a `set` of
//!   ecosystems and an optional `note`; its `notes.yml` opens with a line that
//!   is only `{{note}}`, so a value typed by the wearer sits where a
//!   placeholder or a directive could be read.

mod support;

use support::{Fixture, path_dependency_on_test_skeleton, wearing_table, write_package_manifest};

/// The bytes `text-optional`'s `dependabot.yml` renders as with `assignee` unset.
const RENDER_UNSET: &[u8] = b"version: 2\nopen-pull-requests-limit: 5\n";

/// The bytes it renders as with `assignee = "octocat"`.
const RENDER_SET: &[u8] = b"version: 2\nassignees: [\"octocat\"]\nopen-pull-requests-limit: 5\n\
                            reviewers: [\"octocat\"]\n";

/// Writes a workspace manifest wearing `skeleton` with `options_toml` recorded.
fn wear(
    fixture: &Fixture,
    skeleton: &str,
    options_toml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton(skeleton, skeleton),
        wearing_table(skeleton, options_toml),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)
}

#[test]
fn a_file_with_the_optional_line_dropped_reads_as_matching_until_the_option_is_set()
-> support::TestOutcome {
    // The wearer records nothing for `assignee` and holds the file without the
    // lines that carry it: `check` reads it as matching. Recording a value
    // afterwards makes the same file drifted, because the render now holds
    // those lines, and `sync` writes them; `check` then reads it as matching
    // again. The file's bytes are read back after `sync`, never inferred from
    // its exit code.
    let fixture = Fixture::new()?;
    wear(&fixture, "text-optional", "")?;
    fixture.generate_lockfile()?;
    fixture.write("dependabot.yml", RENDER_UNSET)?;
    fixture.init_git_repository()?;

    let matching = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        matching.exit_code, 0,
        "a file holding the render with the optional lines dropped must match; stdout was: {}; \
         stderr was: {}",
        matching.stdout, matching.stderr
    );

    wear(&fixture, "text-optional", "assignee = \"octocat\"\n")?;
    // Recording the value is a change of its own, committed as a wearer would
    // commit it: `sync` writes only over a clean work tree.
    for arguments in [
        ["add", "--all"].as_slice(),
        ["commit", "--quiet", "--message", "fixture: record assignee"].as_slice(),
    ] {
        support::git::run(
            fixture.root(),
            fixture.sandbox().home(),
            arguments,
            "git (fixture step)",
        )?;
    }

    let drifted = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        drifted.exit_code, 1,
        "once the option is set, the file without its lines must have drifted; stdout was: {}; \
         stderr was: {}",
        drifted.stdout, drifted.stderr
    );
    assert!(
        drifted.stdout.contains("dependabot.yml") && drifted.stdout.contains("changed"),
        "the report must name the path as changed; stdout was: {}",
        drifted.stdout
    );

    let synced = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        synced.exit_code, 0,
        "sync must write the filled lines; stdout was: {}; stderr was: {}",
        synced.stdout, synced.stderr
    );
    assert_eq!(
        fixture.read("dependabot.yml")?,
        RENDER_SET,
        "sync must write exactly the render with the option set"
    );

    let settled = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        settled.exit_code, 0,
        "check must match right after sync; stdout was: {}; stderr was: {}",
        settled.stdout, settled.stderr
    );
    Ok(())
}

#[test]
fn a_file_still_holding_the_optional_line_reads_as_drifted_while_the_option_is_unset()
-> support::TestOutcome {
    // The other direction: with nothing recorded the render drops the lines, so
    // a file that still holds them is not the render. This keeps the matching
    // case above honest: matching is not simply "anything passes".
    let fixture = Fixture::new()?;
    wear(&fixture, "text-optional", "")?;
    fixture.generate_lockfile()?;
    fixture.write("dependabot.yml", RENDER_SET)?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "lines the render drops must count as drift; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report.stdout.contains("dependabot.yml") && report.stdout.contains("changed"),
        "the report must name the path as changed; stdout was: {}",
        report.stdout
    );
    Ok(())
}

/// Wears `text-value-not-rescanned` with `note` recorded as `note_toml` (a TOML
/// string literal), runs `sync` and then `check`, and asserts that `notes.yml`
/// holds `expected` and that `check` reads it as matching.
fn assert_note_written_verbatim_and_clean(
    note_toml: &str,
    expected: &[u8],
) -> support::TestOutcome {
    let fixture = Fixture::new()?;
    wear(
        &fixture,
        "text-value-not-rescanned",
        &format!("note = {note_toml}\n"),
    )?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    let synced = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        synced.exit_code, 0,
        "sync must accept a note that only looks like grammar; stdout was: {}; stderr was: {}",
        synced.stdout, synced.stderr
    );
    assert_eq!(
        fixture.read("notes.yml")?,
        expected,
        "the wearer's text must be written exactly as typed"
    );

    let checked = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        checked.exit_code, 0,
        "what sync wrote must check clean; stdout was: {}; stderr was: {}",
        checked.stdout, checked.stderr
    );
    Ok(())
}

#[test]
fn a_value_holding_a_placeholder_is_written_verbatim_and_then_checks_clean() -> support::TestOutcome
{
    assert_note_written_verbatim_and_clean(
        "\"{{cadence}}\"",
        b"{{cadence}}\ncadence: weekly\njobs:\ncargo-job: build\n",
    )
}

#[test]
fn a_value_holding_a_directive_is_written_verbatim_and_then_checks_clean() -> support::TestOutcome {
    assert_note_written_verbatim_and_clean(
        "\"# skeletons:partial ecosystems\"",
        b"# skeletons:partial ecosystems\ncadence: weekly\njobs:\ncargo-job: build\n",
    )
}
