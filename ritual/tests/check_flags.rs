//! Acceptance: `--drifted`, `--behind`, `--fail-behind`, `--json`, and the
//! default exit-code rule they all sit on top of.

mod support;

use support::git::SkeletonRepository;
use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::{
    Fixture, checked_in_refused_skeleton, path_dependency_on, path_dependency_on_test_skeleton,
    wearing_table, write_package_manifest,
};

/// Builds a fixture wearing two skeletons at once: `passthrough-plain`
/// under its own name, and `enum-fill` under its own name with `cadence`
/// recorded as `daily`. Neither dependency is pinned through git — both
/// are `path =`, so both read `pinned`, never `behind`.
fn fixture_wearing_two_path_skeletons() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n{}{}cadence = \"daily\"\n",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        path_dependency_on_test_skeleton("enum-fill", "enum-fill"),
        wearing_table("passthrough-plain", ""),
        wearing_table("enum-fill", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

#[test]
fn behind_alone_never_fails_the_default_exit_status() -> support::TestOutcome {
    // A skeleton that is behind, with its claimed file otherwise matching,
    // must not make the default (no-flags) `check` exit non-zero: only
    // drift does that, on its own.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("behind-only", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nbehind-only = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("behind-only", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("behind-only", "thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    // Matches the locked (`1.0.0`) render exactly.
    fixture.write("thing.txt", b"v1\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "a behind-but-undrifted file must not fail the default check; stderr was: {}",
        report.stderr
    );
    // The exit code alone cannot distinguish "behind, but that never fails
    // the default" from "the behind fact was never computed at all" (which
    // a do-nothing stub also reports as exit 0). Reading the fact itself
    // back out of `--json` is what proves it is really `behind`, not
    // `pinned` or `current`, underneath that passing exit code.
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    Ok(())
}

#[test]
fn drifted_flag_narrows_rows_shown_without_changing_exit_status() -> support::TestOutcome {
    // Wearing `passthrough-plain` (matching) and `enum-fill` (drifted),
    // `--drifted` must show only `enum-fill`'s row, but the exit code must
    // be the same non-zero status the default (unfiltered) run already
    // gives — narrowing what is shown never changes whether the command
    // succeeds.
    let fixture = fixture_wearing_two_path_skeletons()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.write("settings.yml", b"schedule: NOT-DAILY\n")?; // drifted

    let default_report = fixture.run(&["skeletons", "check"])?;
    let drifted_report = fixture.run(&["skeletons", "check", "--drifted"])?;

    assert_eq!(
        default_report.exit_code, 1,
        "the unfiltered run must fail on the drifted file; stderr was: {}",
        default_report.stderr
    );
    assert_eq!(
        drifted_report.exit_code, default_report.exit_code,
        "--drifted must not change the exit status from the unfiltered run"
    );
    assert!(
        drifted_report.stdout.contains("settings.yml"),
        "--drifted must still show the drifted row; stdout was: {}",
        drifted_report.stdout
    );
    assert!(
        !drifted_report.stdout.contains("plain.yml"),
        "--drifted must narrow away the matching row; stdout was: {}",
        drifted_report.stdout
    );
    Ok(())
}

#[test]
fn behind_flag_narrows_rows_shown_without_changing_exit_status() -> support::TestOutcome {
    // Wearing a `path =` skeleton (always `pinned`, never `behind`) and a
    // `tag =` skeleton that is behind, both with matching files, `--behind`
    // must show only the behind row, and must not change the exit status
    // from the unfiltered run (which is zero: neither file is drifted).
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("behind-flag", "behind-thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\nbehind-flag = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        repository.file_url(),
        wearing_table("passthrough-plain", ""),
        wearing_table("behind-flag", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("behind-flag", "behind-thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.write("behind-thing.txt", b"v1\n")?;

    let default_report = fixture.run(&["skeletons", "check"])?;
    let behind_report = fixture.run(&["skeletons", "check", "--behind"])?;

    assert_eq!(
        default_report.exit_code, 0,
        "neither file is drifted, so the unfiltered run must pass; stderr was: {}",
        default_report.stderr
    );
    assert_eq!(
        behind_report.exit_code, default_report.exit_code,
        "--behind must not change the exit status from the unfiltered run"
    );
    assert!(
        behind_report.stdout.contains("behind-thing.txt"),
        "--behind must show the behind row; stdout was: {}",
        behind_report.stdout
    );
    assert!(
        !behind_report.stdout.contains("plain.yml"),
        "--behind must narrow away the pinned row; stdout was: {}",
        behind_report.stdout
    );
    Ok(())
}

#[test]
fn drifted_and_behind_together_show_the_union_of_either_flag_alone() -> support::TestOutcome {
    // Three files: `plain.yml` (matches, pinned — neither flag would show
    // it), `settings.yml` (drifted, pinned — `--drifted` shows it), and
    // `behind-thing.txt` (matches, behind — `--behind` shows it).
    // `--drifted --behind` together must show the second and third, but
    // not the first.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("behind-union", "behind-thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}\nbehind-union = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n\
         {}{}{}cadence = \"daily\"\n",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        path_dependency_on_test_skeleton("enum-fill", "enum-fill"),
        repository.file_url(),
        wearing_table("passthrough-plain", ""),
        wearing_table("behind-union", ""),
        wearing_table("enum-fill", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("behind-union", "behind-thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.write("settings.yml", b"schedule: NOT-DAILY\n")?; // drifted
    fixture.write("behind-thing.txt", b"v1\n")?; // matches, but behind

    let report = fixture.run(&["skeletons", "check", "--drifted", "--behind"])?;

    assert_eq!(
        report.exit_code, 1,
        "the drifted file still fails the command even with both filters; stderr was: {}",
        report.stderr
    );
    assert!(
        report.stdout.contains("settings.yml"),
        "the drifted row must appear; stdout was: {}",
        report.stdout
    );
    assert!(
        report.stdout.contains("behind-thing.txt"),
        "the behind row must appear; stdout was: {}",
        report.stdout
    );
    assert!(
        !report.stdout.contains("plain.yml"),
        "the matching, pinned row must not appear; stdout was: {}",
        report.stdout
    );
    Ok(())
}

#[test]
fn fail_behind_exits_nonzero_on_a_behind_row_that_the_default_would_pass() -> support::TestOutcome {
    // The same behind-only fixture as `behind_alone_never_fails_the_default_exit_status`,
    // but now with `--fail-behind`: the default run must still pass, and
    // adding the flag must turn that same, otherwise-unchanged situation
    // into a failure.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("fail-behind", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nfail-behind = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("fail-behind", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("fail-behind", "thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    fixture.write("thing.txt", b"v1\n")?;

    let default_report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        default_report.exit_code, 0,
        "the default run must pass: behind alone never fails it; stderr was: {}",
        default_report.stderr
    );

    let fail_behind_report = fixture.run(&["skeletons", "check", "--fail-behind"])?;
    assert_eq!(
        fail_behind_report.exit_code, 1,
        "--fail-behind must fail the same, otherwise-passing situation; stderr was: {}",
        fail_behind_report.stderr
    );
    Ok(())
}

#[test]
fn json_reports_the_shape_marker_and_both_facts_for_a_matching_pinned_file() -> support::TestOutcome
{
    // `--json` must carry an explicit shape marker, and report, for the
    // one bone here, both of its independent facts: `matches` (it
    // is byte-identical) and `pinned` (it is a `path =` dependency, which
    // never reports behind).
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    assert_eq!(
        support::json::format_version(&document)?,
        1,
        "the shape marker must be format_version 1; document was: {document}"
    );

    let bones = support::json::bones(&document)?;
    assert_eq!(
        bones.len(),
        1,
        "expected exactly one bone row; bones were: {bones:?}"
    );
    let row = &bones[0];
    assert_eq!(support::json::bone_path(row)?, "plain.yml");
    assert_eq!(support::json::bone_drift_state(row)?, "matches");
    assert_eq!(support::json::behind_state(row)?, "pinned");
    Ok(())
}

#[test]
fn json_reports_refusals_alongside_the_files_it_could_still_determine() -> support::TestOutcome {
    // A fleet reader must be able to tell a repository that refused a
    // skeleton from one that merely drifted, from `--json` alone: the
    // refused skeleton appears in a `refusals` array, and the healthy
    // skeleton's own file still appears in `bones`.
    let fixture = Fixture::new()?;
    let broken_skeleton = checked_in_refused_skeleton("unknown-key");
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

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(
        refusals.len(),
        1,
        "expected exactly one refusal; refusals were: {refusals:?}"
    );
    assert_eq!(
        support::json::refusal_kind(&refusals[0])?,
        "skeleton-invalid",
        "unknown-key's own defect is in the skeleton's schema, not the wearer's choices; \
         refusal was: {}",
        refusals[0]
    );
    assert_eq!(
        support::json::refusal_skeleton(&refusals[0])?,
        Some("unknown-key")
    );

    let bones = support::json::bones(&document)?;
    assert!(
        bones
            .iter()
            .any(|row| support::json::bone_path(row).ok() == Some("plain.yml")),
        "the healthy skeleton's own file must still be reported; bones were: {bones:?}"
    );
    Ok(())
}

#[test]
fn behind_flag_shows_an_undetermined_row_not_only_rows_that_are_behind() -> support::TestOutcome {
    // Wearing a `path =` skeleton (pinned, matching — neither flag would
    // show it) and a skeleton taken from a registry other than the default one
    // (undetermined, matching), `--behind` must show the undetermined row
    // too — undetermined sits alongside real behind rows in this filter,
    // never only genuine behind rows — and must not change the exit status
    // from the unfiltered run (neither file is drifted).
    let fixture = Fixture::new()?;
    support::write_vendored_other_registry_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "undetermined-thing.txt",
        "from another registry\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n{}{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        support::other_registry_dependency("semver", "1.0.7"),
        wearing_table("passthrough-plain", ""),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.write("undetermined-thing.txt", b"from another registry\n")?;

    let default_report = fixture.run(&["skeletons", "check"])?;
    let behind_report = fixture.run(&["skeletons", "check", "--behind"])?;

    assert_eq!(
        default_report.exit_code, 0,
        "neither file is drifted, so the unfiltered run must pass; stderr was: {}",
        default_report.stderr
    );
    assert_eq!(
        behind_report.exit_code, default_report.exit_code,
        "--behind must not change the exit status from the unfiltered run"
    );
    assert!(
        behind_report.stdout.contains("undetermined-thing.txt"),
        "--behind must show the undetermined row, not only rows that are behind; stdout was: {}",
        behind_report.stdout
    );
    assert!(
        !behind_report.stdout.contains("plain.yml"),
        "--behind must narrow away the pinned, matching row; stdout was: {}",
        behind_report.stdout
    );
    Ok(())
}

#[test]
fn fail_behind_exits_nonzero_on_an_undetermined_row_that_the_default_would_pass()
-> support::TestOutcome {
    // The same registry-other fixture as the `--behind` test above, wearing
    // only the undetermined skeleton: the default run must pass (nothing
    // drifted, and undetermined alone never fails the default), and
    // `--fail-behind` must turn that same, otherwise-unchanged situation
    // into a failure on the strength of the undetermined row alone —
    // `--fail-behind` fails on undetermined exactly as it does on a genuine
    // behind row.
    let fixture = Fixture::new()?;
    support::write_vendored_other_registry_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "thing.txt",
        "from another registry\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::other_registry_dependency("semver", "1.0.7"),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"from another registry\n")?;

    let default_report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        default_report.exit_code, 0,
        "the default run must pass: an undetermined row alone never fails it; stderr was: {}",
        default_report.stderr
    );

    let fail_behind_report = fixture.run(&["skeletons", "check", "--fail-behind"])?;
    assert_eq!(
        fail_behind_report.exit_code, 1,
        "--fail-behind must fail on an undetermined row alone, the same otherwise-passing \
         situation; stderr was: {}",
        fail_behind_report.stderr
    );
    Ok(())
}
