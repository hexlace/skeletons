//! Acceptance: the two independent facts `check` reports per bone,
//! how it reports a refusal, and that it stores nothing of its own between
//! runs.

mod support;

use support::git::SkeletonRepository;
use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::{
    Fixture, path_dependency_on, path_dependency_on_test_skeleton, wearing_table,
    write_package_manifest,
};

#[test]
fn drift_and_behind_are_reported_independently_for_the_same_file() -> support::TestOutcome {
    // A file that is both changed from its render *and* whose skeleton has
    // a newer tag than the one locked must report both facts at once,
    // neither hiding the other. Locks against tag `1.0.0`, then a newer
    // tag `1.1.0` is created afterward — `check` never re-locks, so the
    // only way it can see `1.1.0` is by asking the remote, which is a real,
    // local `file://` repository here.
    //
    // A regression that reported only the drift fact and not the behind
    // fact (or vice versa) would fail at the `--json` row assertions below,
    // which is the failure mode this test exists to catch.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("drifted-and-behind", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n\
         drifted-and-behind = {{ git = \"{}\", tag = \"1.0.0\" }}\n\
         \n\
         {}",
        repository.file_url(),
        wearing_table("drifted-and-behind", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    // A newer tag appears only after the lock, so `check` can only learn
    // about it by asking the remote.
    repository.commit_skeleton("drifted-and-behind", "thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    // Neither `v1\n` (the locked render) nor `v2\n`: a plain change.
    fixture.write("thing.txt", b"neither\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 1,
        "a drifted file must fail check regardless of behind status; stderr was: {}",
        report.stderr
    );

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    let row = bones
        .iter()
        .find(|row| support::json::bone_path(row).ok() == Some("thing.txt"))
        .ok_or_else(|| format!("expected a row for thing.txt; bones were: {bones:?}"))?;
    assert_eq!(
        support::json::bone_drift_state(row)?,
        "drifted",
        "row was: {row}"
    );
    assert_eq!(
        support::json::bone_drift_reason(row)?,
        Some("changed"),
        "row was: {row}"
    );
    assert_eq!(
        support::json::behind_state(row)?,
        "behind",
        "row was: {row}"
    );
    Ok(())
}

#[test]
fn a_refused_skeletons_own_render_fails_the_command_while_still_reporting_other_worn_skeletons()
-> support::TestOutcome {
    // `refused/unknown-key` (checked in, unmodified) carries a manifest
    // schema defect — an unrecognised key — that refuses regardless of
    // anything a wearer chooses. Wearing it alongside a healthy skeleton,
    // `passthrough-plain`, must fail the command overall (naming the
    // broken skeleton) while still reporting `passthrough-plain`'s own file
    // as matching.
    let fixture = Fixture::new()?;
    let broken_skeleton = support::checked_in_refused_skeleton("unknown-key");
    let extra = format!(
        "[dependencies]\n{}{}\n{}{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        path_dependency_on("unknown-key", &broken_skeleton),
        wearing_table("passthrough-plain", ""),
        wearing_table("unknown-key", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "a broken worn skeleton must fail the overall command; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("unknown-key"),
        "the refusal must name the broken skeleton; combined output was: {combined}"
    );
    assert!(
        report.stdout.contains("plain.yml"),
        "the healthy skeleton's own file must still be reported; stdout was: {}",
        report.stdout
    );
    Ok(())
}

#[test]
fn a_recorded_option_value_outside_the_skeletons_declared_set_is_refused_naming_the_skeleton()
-> support::TestOutcome {
    // `enum-fill` declares `cadence` as one of `daily`/`weekly`/`monthly`.
    // Recording `cadence = "yearly"` in the wearing table is a
    // repository-recorded option value the skeleton does not declare, and
    // must refuse, naming the skeleton.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}cadence = \"yearly\"\n",
        path_dependency_on_test_skeleton("enum-fill", "enum-fill"),
        wearing_table("enum-fill", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "an undeclared recorded option value must refuse; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("enum-fill"),
        "the refusal must name the skeleton; combined output was: {combined}"
    );
    Ok(())
}

#[test]
fn an_unreadable_lockfile_aborts_the_whole_command_rather_than_reading_as_wearing_nothing()
-> support::TestOutcome {
    // A `Cargo.lock` that is not valid at all — `skeletons` cannot read it — must
    // abort the whole command with a task refusal (exit 1, a
    // `ritual: <message>` line on stderr), never silently succeed as
    // though the repository wore nothing.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    // Corrupt the lockfile after it was generated validly.
    fixture.write("Cargo.lock", b"this is not { valid = toml at all\n")?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "an unreadable lockfile must abort with a task refusal, not exit 0 or 2; stdout was: {}, \
         stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report
            .stderr
            .lines()
            .any(|line| line.starts_with("ritual: ")),
        "a task refusal must write a `ritual: <message>` line to stderr; stderr was: {}",
        report.stderr
    );
    assert!(
        !report.stdout.to_lowercase().contains("wears no skeletons"),
        "an unreadable lockfile must never be reported as though nothing were worn; stdout was: {}",
        report.stdout
    );
    Ok(())
}

#[test]
fn check_writes_nothing_new_to_the_repository_beyond_the_claimed_files_themselves()
-> support::TestOutcome {
    // Beyond a claimed file itself (which this test never lets `check`
    // touch — `check` only reads), running `check` must write no manifest,
    // cache, or stored hash anywhere in the workspace. Every regular file
    // under the workspace root, with its bytes (aside from `target/`,
    // cargo's own, expected, incidental build directory), must read
    // identically before and after.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let before = support::snapshot_workspace_files(fixture.root(), &["target"])?;
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "the fixture must pass check for this test to say anything about what it wrote; \
         stderr was: {}",
        report.stderr
    );
    let after = support::snapshot_workspace_files(fixture.root(), &["target"])?;

    assert_eq!(
        before, after,
        "check must write nothing new to the workspace beyond the claimed files it reads"
    );
    // "Wrote nothing new" is true, vacuously, of a `check` that never ran at
    // all — a do-nothing stub leaves `before == after` just as much as a
    // real, working `check` does. What distinguishes them is whether `check`
    // actually looked at `plain.yml` and reported it: read that positive
    // fact back from `--json`, which a stub's empty stdout cannot produce.
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(
        bones.len(),
        1,
        "expected the one bone to be reported; bones were: {bones:?}"
    );
    assert_eq!(support::json::bone_path(&bones[0])?, "plain.yml");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

/// A fixture wearing `nested-dotfiles` (which claims
/// `.github/dependabot.yml`, `a/b/deep.yml`, and `root.yml`) with two
/// independent unsafe-path defects at once: `.github` is a regular file, so
/// `.github/dependabot.yml` cannot exist beneath it (`not-a-directory-above`),
/// and `root.yml` is a directory, so it cannot be read as the claimed
/// regular file (`not-a-file`).
fn fixture_with_two_unsafe_claimed_paths() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("nested-dotfiles", "nested-dotfiles"),
        wearing_table("nested-dotfiles", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    // `.github` is a regular file, not a directory.
    fixture.write(".github", b"not a directory\n")?;
    // `root.yml` is a directory, not a regular file.
    fixture.write("root.yml/dummy.txt", b"root.yml must be a directory\n")?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

#[test]
fn a_skeleton_with_two_unsafe_claimed_paths_reports_two_refusals() -> support::TestOutcome {
    // Both unsafe-path defects must be reported as their own refusal, named
    // by their own cause, and `summary.refusals` must count both — never
    // stop at the first bad path found, in either the JSON or the human
    // output.
    let fixture = fixture_with_two_unsafe_claimed_paths()?;

    let json_report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        json_report.exit_code, 1,
        "two unsafe claimed paths must fail check; stderr was: {}",
        json_report.stderr
    );

    let document = support::json::parse(&json_report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    let unsafe_path_refusals: Vec<&serde_json::Value> = refusals
        .iter()
        .filter(|refusal| support::json::refusal_kind(refusal).ok() == Some("unsafe-path"))
        .collect();
    assert_eq!(
        unsafe_path_refusals.len(),
        2,
        "expected two unsafe-path refusals, one per bad claimed path; refusals were: {refusals:?}"
    );
    let causes: Vec<Option<&str>> = unsafe_path_refusals
        .iter()
        .map(|refusal| support::json::refusal_cause(refusal).ok().flatten())
        .collect();
    assert!(
        causes.contains(&Some("not-a-directory-above")),
        "expected a not-a-directory-above refusal for .github; refusals were: {refusals:?}"
    );
    assert!(
        causes.contains(&Some("not-a-file")),
        "expected a not-a-file refusal for root.yml; refusals were: {refusals:?}"
    );
    assert_eq!(
        support::json::summary_count(&document, "refusals")?,
        2,
        "summary.refusals must count both unsafe-path refusals"
    );

    let human_report = fixture.run(&["skeletons", "check"])?;
    let refused_line_count = human_report
        .stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("refused:"))
        .count();
    assert_eq!(
        refused_line_count, 2,
        "expected two `refused:` lines in the human output, not just the first path found; \
         stdout was: {}",
        human_report.stdout
    );
    Ok(())
}

#[test]
fn a_skeleton_with_two_unsafe_paths_sync_refuses_and_writes_nothing() -> support::TestOutcome {
    // `sync`, against the same tree, must refuse and touch neither bad
    // path's bytes, proved by reading them back — and must print both
    // refusals, not just the first path found.
    let fixture = fixture_with_two_unsafe_claimed_paths()?;

    let dot_github_before = fixture.read(".github")?;
    let root_yml_before = fixture.read("root.yml/dummy.txt")?;

    let sync_report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        sync_report.exit_code, 1,
        "sync must refuse when its worn skeleton has unsafe claimed paths; stderr was: {}",
        sync_report.stderr
    );
    let sync_refused_line_count = sync_report
        .stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("refused:"))
        .count();
    assert_eq!(
        sync_refused_line_count, 2,
        "sync must print both refusals, not just the first path found; stdout was: {}",
        sync_report.stdout
    );
    assert_eq!(
        fixture.read(".github")?,
        dot_github_before,
        "sync must leave the not-a-directory-above path exactly as it was"
    );
    assert_eq!(
        fixture.read("root.yml/dummy.txt")?,
        root_yml_before,
        "sync must leave the not-a-file path exactly as it was"
    );
    Ok(())
}
