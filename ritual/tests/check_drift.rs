//! Acceptance: whether a claimed file matches, or has drifted, against the
//! locked skeleton's own render.
//!
//! Every scenario but the last wears a checked-in test skeleton from
//! `crates/skeletons/test-skeletons/renders/`, taken by a `path =` dependency
//! from a fresh, single-crate Cargo workspace. All but the two build-script
//! scenarios at the end use one of these two:
//!
//! - `passthrough-plain` — one file, no options: `files/plain.yml` holds
//!   `name: unchanged\nvalue: 42\n` verbatim, since the skeleton has nothing to
//!   fill or select.
//! - `enum-fill` — one file, one `enum` option: `files/settings.yml` holds
//!   `schedule: {{cadence}}\n`, `cadence` is `daily`/`weekly`/`monthly`,
//!   defaulting to `weekly`.
//!
//! Each of those asserts on `check`'s exit status, its plain output, and —
//! where the distinction between a match and a drift, or between the two
//! drift reasons, is what the scenario is actually about — `--json`'s
//! per-file `drift` fact.
//!
//! The last scenario runs no `check`: it builds the build-script skeleton on
//! purpose, to show its marker file can appear at all.

mod support;

use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::{
    BUILD_SCRIPT_MARKER_VARIABLE, Fixture, Sandbox, TemporaryDirectory, checked_in_test_skeleton,
    isolate_from_the_enclosing_repository, path_dependency_on_test_skeleton, wearing_table,
    write_package_manifest,
};

/// Builds a fixture wearing `enum-fill` under the dependency key
/// `enum-fill`, recording `cadence = "daily"` (not the skeleton's own default,
/// `weekly`), and generates its lockfile.
fn fixture_wearing_enum_fill_with_daily_cadence() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}cadence = \"daily\"\n",
        path_dependency_on_test_skeleton("enum-fill", "enum-fill"),
        wearing_table("enum-fill", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

#[test]
fn byte_identical_claimed_file_reports_matches() -> support::TestOutcome {
    // Given a repository already holding exactly what the locked skeleton
    // renders, `check` reports that file as matching and exits zero.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 0,
        "a byte-identical claimed file must not fail check; stderr was: {}",
        report.stderr
    );
    assert!(
        report.stdout.contains("plain.yml"),
        "expected the claimed path to appear in the report; stdout was: {}",
        report.stdout
    );
    Ok(())
}

#[test]
fn one_changed_byte_anywhere_in_a_claimed_file_reports_drifted() -> support::TestOutcome {
    // The negative twin of the match case: one byte differs from the
    // render (`42` becomes `43`), so the file must report drifted and the
    // command must exit non-zero.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", b"name: unchanged\nvalue: 43\n")?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "one changed byte must fail check with a task refusal; stderr was: {}",
        report.stderr
    );
    assert!(
        report.stdout.contains("plain.yml"),
        "expected the drifted path to appear in the report; stdout was: {}",
        report.stdout
    );
    Ok(())
}

#[test]
fn a_reformatted_file_with_no_meaning_changed_still_reports_drifted() -> support::TestOutcome {
    // Comparison is byte-for-byte, never structural: reordering the two
    // lines leaves the same YAML meaning but different bytes, and must
    // still report drifted. This rules out a "smarter" comparison that only
    // flags semantic changes.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", b"value: 42\nname: unchanged\n")?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "a byte-reordered file must still fail check even though its meaning is unchanged; \
         stderr was: {}",
        report.stderr
    );
    Ok(())
}

#[test]
fn a_missing_claimed_file_reports_drifted_and_distinguishably_missing() -> support::TestOutcome {
    // A claimed file the repository never wrote at all must still report
    // drifted, and must be distinguishable from a changed file. `changed`
    // and `missing` are the two drift subtypes `check` reports; this
    // compares the plain output of the two cases, so it depends on their
    // wording differing and on no particular wording.
    let missing = fixture_wearing_passthrough_plain()?;
    // `plain.yml` is never written at all.
    let missing_report = missing.run(&["skeletons", "check"])?;
    assert_eq!(
        missing_report.exit_code, 1,
        "a missing claimed file must fail check with a task refusal; stderr was: {}",
        missing_report.stderr
    );
    assert!(
        missing_report.stdout.contains("plain.yml"),
        "expected the missing path to appear in the report; stdout was: {}",
        missing_report.stdout
    );

    let changed = fixture_wearing_passthrough_plain()?;
    changed.write("plain.yml", b"name: unchanged\nvalue: 43\n")?;
    let changed_report = changed.run(&["skeletons", "check"])?;

    assert_ne!(
        missing_report.stdout, changed_report.stdout,
        "a missing file and a changed file must be reported distinguishably, but the two \
         reports read identically"
    );
    Ok(())
}

#[test]
fn a_hand_written_file_byte_identical_to_the_render_matches_on_the_first_check()
-> support::TestOutcome {
    // A file the repository already holds, hand-authored before it ever
    // wore the skeleton, that happens to be byte-identical to the render,
    // must read as matching the very first time `check` runs — nothing
    // has to be generated or reformatted first. This is the same
    // assertion as the plain byte-identical case, but the story is
    // different: it is never the output of a prior `sync`.
    let fixture = fixture_wearing_passthrough_plain()?;
    // Written by hand, not copied from the skeleton's own `files/` directory,
    // and happening to match it byte for byte.
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(
        report.exit_code, 0,
        "a hand-written file that already matches the render must pass on the first check; \
         stderr was: {}",
        report.stderr
    );
    // Exit 0 alone is what a do-nothing stub also reports. The positive
    // fact this test is actually about — that the file was compared
    // and read as matching, on this very first run — only shows up in the
    // row `--json` reports for it.
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::bone_path(&bones[0])?, "plain.yml");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn recorded_option_values_decide_the_expected_render_not_the_skeletons_own_defaults()
-> support::TestOutcome {
    // `enum-fill`'s own default for `cadence` is `weekly`, but this
    // fixture records `cadence = "daily"`. A file holding the skeleton's
    // *default* render (`schedule: weekly`) must therefore report
    // drifted — the expected render is computed from the repository's own
    // recorded options, never from the skeleton's defaults — while a file
    // holding the render for the recorded value (`schedule: daily`) must
    // report matches.
    let against_the_skeletons_default = fixture_wearing_enum_fill_with_daily_cadence()?;
    against_the_skeletons_default.write("settings.yml", b"schedule: weekly\n")?;
    let default_report = against_the_skeletons_default.run(&["skeletons", "check"])?;
    assert_eq!(
        default_report.exit_code, 1,
        "a file rendered with the skeleton's own default, rather than the repository's recorded \
         option, must report drifted; stderr was: {}",
        default_report.stderr
    );

    let against_the_recorded_option = fixture_wearing_enum_fill_with_daily_cadence()?;
    against_the_recorded_option.write("settings.yml", b"schedule: daily\n")?;
    let recorded_report = against_the_recorded_option.run(&["skeletons", "check"])?;
    assert_eq!(
        recorded_report.exit_code, 0,
        "a file rendered with the repository's own recorded option must match; stderr was: {}",
        recorded_report.stderr
    );
    Ok(())
}

#[test]
fn a_worn_skeleton_carrying_a_build_script_is_rendered_and_compared_like_any_other()
-> support::TestOutcome {
    // `build-script-marker` ships a `build.rs` that writes a marker file to a
    // path this test names through an environment variable, if and only if it
    // is ever executed. Wearing it and running `check` against a byte-identical
    // claimed file must succeed exactly as for a skeleton with no build script,
    // and the marker file must never appear — proving `check` ran nothing the
    // skeleton shipped, as a fact this test process itself can observe, rather
    // than merely an exit code a do-nothing stub could produce by accident.
    // `sync_never_runs_a_worn_skeletons_build_script` in `sync.rs` proves the
    // same of `sync`, and
    // `the_build_script_marker_skeleton_writes_its_marker_when_it_is_built`
    // below shows the marker can appear at all, so its absence here means
    // something.
    //
    // If some future change made `check` shell out to `cargo build` on a
    // worn skeleton's own crate, the marker file would appear (or cargo would
    // report the build script's distinctive exit status), either of which
    // is a different, and equally informative, failure.
    let fixture = Fixture::new()?;
    let skeleton_path = checked_in_test_skeleton("build-script-marker");
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::path_dependency_on("build-script-marker", &skeleton_path),
        wearing_table("build-script-marker", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"build script must never run\n")?;

    let marker_directory = support::TemporaryDirectory::new("build-script-marker")?;
    let marker_path = marker_directory.path().join("build-script-ran.marker");

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            BUILD_SCRIPT_MARKER_VARIABLE,
            marker_path
                .to_str()
                .ok_or("marker path must be valid UTF-8")?,
        )],
    )?;

    assert_eq!(
        report.exit_code, 0,
        "a worn skeleton's own build script must never run, and its file must still match; \
         stderr was: {}",
        report.stderr
    );
    assert!(
        !marker_path.exists(),
        "the worn skeleton's build script must never run, but its marker file exists at {}",
        marker_path.display()
    );

    // Exit 0 and an absent marker are also what a do-nothing stub produces:
    // the fact that `check` genuinely read and compared the claimed file is
    // read back from `--json`.
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::bone_path(&bones[0])?, "thing.txt");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn the_build_script_marker_skeleton_writes_its_marker_when_it_is_built() -> support::TestOutcome {
    // Positive control for the two tests that assert the marker file never
    // appears (this file's and `sync.rs`'s): they would also pass if the
    // variable's name drifted between them and the skeleton's `build.rs`, or
    // if the script stopped writing anything. Here the skeleton is built on
    // purpose, with the variable set, and the marker must appear. The
    // build itself fails, since the script exits with a status of its own
    // after writing, so only the file is asserted. The crate is copied so
    // the build leaves no lockfile in the checked-in tree.
    let skeleton_path = checked_in_test_skeleton("build-script-marker");
    let crate_directory = TemporaryDirectory::new("build-script-marker-crate")?;
    for relative in ["Cargo.toml", "build.rs", "src/lib.rs"] {
        support::write(
            crate_directory.path(),
            relative,
            &support::read(&skeleton_path, relative)?,
        )?;
    }
    let target_directory = TemporaryDirectory::new("build-script-marker-target")?;
    let marker_directory = TemporaryDirectory::new("build-script-marker-written")?;
    let marker_path = marker_directory.path().join("build-script-ran.marker");
    let sandbox = Sandbox::new()?;

    let mut command = support::cargo_command();
    command
        .current_dir(crate_directory.path())
        .env("CARGO_HOME", sandbox.cargo_home())
        .env("HOME", sandbox.home())
        .env("CARGO_TARGET_DIR", target_directory.path())
        .env(BUILD_SCRIPT_MARKER_VARIABLE, &marker_path)
        .args(["build", "--offline"]);
    isolate_from_the_enclosing_repository(&mut command);
    let output = command.output()?;

    assert!(
        marker_path.exists(),
        "building the skeleton with {BUILD_SCRIPT_MARKER_VARIABLE} set must run its build \
         script and write the marker; cargo said: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&marker_path)?,
        b"the build script ran\n",
        "the marker must hold what the build script writes"
    );
    Ok(())
}
