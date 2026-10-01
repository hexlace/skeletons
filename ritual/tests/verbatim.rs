//! Acceptance: a repository wearing a skeleton with verbatim files, through the
//! real `check` and `sync` commands.
//!
//! - `verbatim-workflow` declares a `cadence` enum and one verbatim file,
//!   `.github/workflows/release.yml`, which holds `${{ secrets.X }}` and
//!   `${{ github.ref }}`. Beside it, `settings.yml` holds
//!   `schedule: {{cadence}}` and still fills.
//! - `verbatim-not-utf8` declares one verbatim file, `blob.bin`, whose bytes
//!   are not valid UTF-8 and hold a NUL.
//! - `passthrough-plain` is the ordinary skeleton the wearing-side scenarios
//!   wear.
//!
//! A verbatim file's render is its bytes, so `check` reads a byte-identical
//! copy as matching and any other as drifted, and `sync` writes the bytes back.

mod support;

use support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER;
use support::{
    Fixture, checked_in_test_skeleton, path_dependency_on, path_dependency_on_test_skeleton,
    wearing_table, write_package_manifest, write_workspace_root,
};

const WORKFLOW_PATH: &str = ".github/workflows/release.yml";

/// The bytes `verbatim-workflow`'s release workflow is claimed as.
const WORKFLOW: &[u8] = b"name: release\non:\n  push:\n    tags: [\"v*\"]\njobs:\n  publish:\n    \
                          runs-on: ubuntu-latest\n    env:\n      TOKEN: ${{ secrets.X }}\n    \
                          steps:\n      - env:\n          REF: ${{ github.ref }}\n        \
                          run: echo \"$REF\"\n";

/// What `settings.yml` renders as with the skeleton's default `cadence`.
const SETTINGS: &[u8] = b"schedule: weekly\n";

/// The bytes `verbatim-not-utf8`'s `blob.bin` is claimed as.
const BLOB: &[u8] = b"\x00\xFF\xFE\x80{{\x00\xC3(\n";

/// Wears `skeleton` in `fixture`'s only package, records no options, and
/// generates the lockfile.
fn wear(fixture: &Fixture, skeleton: &str) -> Result<(), Box<dyn std::error::Error>> {
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton(skeleton, skeleton),
        wearing_table(skeleton, ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()
}

/// Runs `check --json` and returns `(drift state, drift reason)` of the row for
/// `path`, alongside the exit code.
fn drift_of(
    fixture: &Fixture,
    path: &str,
) -> Result<(i32, String, Option<String>), Box<dyn std::error::Error>> {
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    let row = bones
        .iter()
        .find(|row| support::json::bone_path(row).is_ok_and(|found| found == path))
        .ok_or_else(|| {
            format!(
                "no row for {path}; stdout was: {}; stderr was: {}",
                report.stdout, report.stderr
            )
        })?;
    Ok((
        report.exit_code,
        support::json::bone_drift_state(row)?.to_owned(),
        support::json::bone_drift_reason(row)?.map(str::to_owned),
    ))
}

/// Commits everything present, the clean baseline `sync` writes over.
fn commit_baseline(fixture: &Fixture) -> Result<(), Box<dyn std::error::Error>> {
    fixture.init_git_repository()
}

#[test]
fn a_byte_identical_verbatim_workflow_reads_as_matching() -> support::TestOutcome {
    // The wearer holds exactly the bytes the skeleton ships, `${{ … }}`
    // expressions included, and the templated file beside it. `check` reads
    // the verbatim file's own row as matching.
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-workflow")?;
    fixture.write(WORKFLOW_PATH, WORKFLOW)?;
    fixture.write("settings.yml", SETTINGS)?;

    let (exit_code, state, _) = drift_of(&fixture, WORKFLOW_PATH)?;

    assert_eq!(exit_code, 0, "a byte-identical verbatim file must match");
    assert_eq!(state, "matches");
    Ok(())
}

#[test]
fn an_edited_verbatim_workflow_reads_as_changed_and_the_templated_file_still_matches()
-> support::TestOutcome {
    // One byte of the workflow differs (`v*` becomes `w*`). That row reads
    // drifted, as changed; the templated file beside it, untouched, still
    // matches, so the drift is attributed to the file that holds it.
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-workflow")?;
    let edited = String::from_utf8(WORKFLOW.to_vec())?.replace("v*", "w*");
    fixture.write(WORKFLOW_PATH, edited.as_bytes())?;
    fixture.write("settings.yml", SETTINGS)?;

    let (exit_code, state, reason) = drift_of(&fixture, WORKFLOW_PATH)?;
    let (_, settings_state, _) = drift_of(&fixture, "settings.yml")?;

    assert_eq!(exit_code, 1, "an edited verbatim file must fail check");
    assert_eq!(state, "drifted");
    assert_eq!(reason.as_deref(), Some("changed"));
    assert_eq!(settings_state, "matches");
    Ok(())
}

#[test]
fn sync_creates_a_missing_verbatim_workflow_with_exact_bytes() -> support::TestOutcome {
    // Neither file exists. `sync` writes the workflow exactly as shipped, and
    // `check` then reads it as matching.
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-workflow")?;
    commit_baseline(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    assert_eq!(fixture.read(WORKFLOW_PATH)?, WORKFLOW);
    assert_eq!(fixture.read("settings.yml")?, SETTINGS);
    let (exit_code, state, _) = drift_of(&fixture, WORKFLOW_PATH)?;
    assert_eq!(exit_code, 0);
    assert_eq!(state, "matches");
    Ok(())
}

#[test]
fn sync_restores_an_edited_verbatim_workflow_to_its_exact_bytes() -> support::TestOutcome {
    // The workflow is committed edited. `sync` writes the shipped bytes back,
    // read from disk afterwards rather than inferred from the exit code.
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-workflow")?;
    let edited = String::from_utf8(WORKFLOW.to_vec())?.replace("secrets.X", "secrets.Y");
    fixture.write(WORKFLOW_PATH, edited.as_bytes())?;
    fixture.write("settings.yml", SETTINGS)?;
    commit_baseline(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    assert_eq!(fixture.read(WORKFLOW_PATH)?, WORKFLOW);
    Ok(())
}

#[test]
fn a_binary_verbatim_file_matches_drifts_and_is_synced_back_to_exact_bytes() -> support::TestOutcome
{
    // `blob.bin` is not valid UTF-8. Held exactly, it matches; with one byte
    // changed it is drifted as changed; `sync` restores every byte.
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-not-utf8")?;
    fixture.write("blob.bin", BLOB)?;

    let (exit_code, state, _) = drift_of(&fixture, "blob.bin")?;
    assert_eq!(exit_code, 0, "an identical binary verbatim file must match");
    assert_eq!(state, "matches");

    let mut changed = BLOB.to_vec();
    changed[1] = 0xFD;
    fixture.write("blob.bin", &changed)?;
    let (exit_code, state, reason) = drift_of(&fixture, "blob.bin")?;
    assert_eq!(
        exit_code, 1,
        "a changed binary verbatim file must fail check"
    );
    assert_eq!(state, "drifted");
    assert_eq!(reason.as_deref(), Some("changed"));

    commit_baseline(&fixture)?;
    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    assert_eq!(fixture.read("blob.bin")?, BLOB);
    Ok(())
}

#[test]
fn sync_creates_a_missing_binary_verbatim_file_with_exact_bytes() -> support::TestOutcome {
    let fixture = Fixture::new()?;
    wear(&fixture, "verbatim-not-utf8")?;
    commit_baseline(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    assert_eq!(fixture.read("blob.bin")?, BLOB);
    Ok(())
}

#[test]
fn a_workspace_member_that_is_itself_a_verbatim_skeleton_draws_no_refusal() -> support::TestOutcome
{
    // Two members: `wearer` wears `passthrough-plain`; `skeleton` is a crate
    // whose own `[package.metadata.skeletons]` declares `verbatim`. That key
    // is the member's skeleton schema, not a worn dependency, so `check`
    // reads the wearer's file as matching and refuses nothing.
    let fixture = Fixture::new()?;
    write_workspace_root(fixture.root(), &["wearer", "skeleton"])?;
    let wearer = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "wearer", "wearer", &wearer)?;
    write_package_manifest(
        fixture.root(),
        "skeleton",
        "skeleton",
        "[package.metadata.skeletons]\nverbatim = [\".github/workflows/ci.yml\"]\n",
    )?;
    fixture.generate_lockfile()?;
    // Claimed paths are relative to the workspace root, not the member.
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    let document = support::json::parse(&report.stdout)?;
    assert_eq!(
        support::json::refusals(&document)?.len(),
        0,
        "a member's own `verbatim` must not be read as a worn dependency; stdout was: {}",
        report.stdout
    );
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn a_dependency_keyed_verbatim_is_refused_as_a_reserved_key_naming_it() -> support::TestOutcome {
    // `verbatim` is reserved exactly as `options` is: a member that declares a
    // dependency under that key cannot wear it, because
    // `[package.metadata.skeletons.verbatim]` is where a skeleton declares
    // its own verbatim files. The refusal is a `reserved-key`, the
    // dependency field holds the key, and the message names it.
    let fixture = Fixture::new()?;
    let skeleton = checked_in_test_skeleton("passthrough-plain");
    let dependency = path_dependency_on("verbatim", &skeleton)
        .replace(" }\n", ", package = \"passthrough-plain\" }\n");
    let extra = format!("[dependencies]\n{dependency}\n[package.metadata.skeletons.verbatim]\n");
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    let document = support::json::parse(&report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(refusals.len(), 1, "stdout was: {}", report.stdout);
    assert_eq!(support::json::refusal_kind(&refusals[0])?, "reserved-key");
    assert_eq!(
        support::json::refusal_dependency(&refusals[0])?,
        Some("verbatim")
    );
    assert!(
        support::json::refusal_message(&refusals[0])?.contains("`verbatim`"),
        "the message must name the reserved key; got {}",
        support::json::refusal_message(&refusals[0])?
    );
    assert_eq!(report.exit_code, 1, "stderr was: {}", report.stderr);
    Ok(())
}
