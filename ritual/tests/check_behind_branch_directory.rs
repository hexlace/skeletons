//! Acceptance: `behind` for a skeleton pinned by `branch =` (or an
//! unqualified `git =`, which follows the same rule as `branch =` against
//! the remote's default branch) inside a git repository that holds more
//! than one skeleton crate, each in its own directory.
//!
//! A branch-pinned skeleton is behind only when a commit past the locked
//! one touches *that skeleton's own directory* — its package directory,
//! relative to the repository root — because that is what `cargo update`
//! would change for it. The remote's own head is read cheaply first
//! (`git ls-remote`); only when it differs from what is locked does
//! `check` read the locked directory's own tree from Cargo's checkout and
//! fetch the head alone into a temporary repository of its own, comparing
//! tree objects rather than commit history.

mod support;

use support::git::SkeletonRepository;
use support::{Fixture, wearing_table, write_package_manifest};

/// A fixture wearing `crate-a` from `repository`, pinned by `pin_toml` (the
/// `tag = "…"` / `branch = "…"` / nothing at all fragment inside the
/// dependency's own `{ … }` table), with its manifest and lockfile written
/// and `thing-a.txt` written matching `"v1\n"` — the content
/// `repository`'s own initial `crate-a` commit carries, so `check` never
/// reports a claimed-file refusal or drift muddying a `behind`-only
/// assertion.
fn fixture_wearing_crate_a(
    repository: &SkeletonRepository,
    pin_toml: &str,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ncrate-a = {{ git = \"{}\"{pin_toml} }}\n\n{}",
        repository.file_url(),
        wearing_table("crate-a", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing-a.txt", b"v1\n")?;
    Ok(fixture)
}

/// The one bone `check --json` reports for a fixture wearing only
/// `crate-a`, asserting there is exactly one.
fn only_bone(
    document: &serde_json::Value,
) -> Result<&serde_json::Value, Box<dyn std::error::Error>> {
    let bones = support::json::bones(document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    Ok(&bones[0])
}

#[test]
fn a_branch_pin_stays_current_when_a_later_commit_touches_only_a_sibling_directory()
-> support::TestOutcome {
    // `main` moves forward after the lock, but the new commit's only change
    // is inside `crate-b`'s own directory. `cargo update` would bring
    // `crate-a` nothing new, so the pin must read current, not behind —
    // the directory-scoped rule. Comparing remote-head shas directly would
    // read this as behind regardless of which directory changed.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;

    repository.commit_skeleton_at(
        "crate-b",
        "crate-b",
        "thing-b.txt",
        "v2\n",
        "touch only crate-b",
    )?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(
        support::json::behind_state(bone)?,
        "current",
        "a commit touching only a sibling crate's own directory must not make this branch pin \
         read behind"
    );
    Ok(())
}

/// Every entry directly under the real operating system temporary directory
/// whose name is one *this one process* could have created —
/// `behind::temporary_directory`'s own naming convention,
/// `skeletons-behind-<pid>-<slot>-<attempt>`, filtered to `pid`. Scoped to one
/// pid rather than the bare `skeletons-behind-` prefix so this reads correctly
/// under `cargo test`'s own default parallelism: a sibling test in this same
/// file can be fetching its own snapshot, into its own same-prefixed
/// directory, at the exact moment this one is asked, and that must not read
/// as a leftover of *this* run.
fn skeletons_behind_temporary_entries_for(
    pid: u32,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let prefix = format!("skeletons-behind-{pid}-");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(std::env::temp_dir())? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) {
            found.push(name);
        }
    }
    Ok(found)
}

#[test]
fn a_branch_pin_reads_behind_when_a_commit_past_the_lock_touches_this_crates_own_directory()
-> support::TestOutcome {
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "touch crate-a's own directory",
    )?;

    // The head has moved past the lock, so `check` exercises the full
    // mechanism: reading Cargo's own checkout and fetching into a temporary
    // repository. Snapshot `CARGO_HOME` before and after — `check` reads
    // Cargo's checkout under it but must never write there — and confirm no
    // `skeletons-behind-<this run's own pid>-*` directory survives the run,
    // wherever the operating system's own temporary directory is.
    let cargo_home_before = support::snapshot_workspace_files(fixture.sandbox().cargo_home(), &[])?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(support::json::behind_state(bone)?, "behind");

    let cargo_home_after = support::snapshot_workspace_files(fixture.sandbox().cargo_home(), &[])?;
    assert_eq!(
        cargo_home_before, cargo_home_after,
        "check reads Cargo's own checkout under CARGO_HOME but must never write to it"
    );
    let leftover = skeletons_behind_temporary_entries_for(report.pid)?;
    assert!(
        leftover.is_empty(),
        "no skeletons-behind-{}-* temporary directory must survive the check run that created it; \
         found: {leftover:?}",
        report.pid
    );
    Ok(())
}

#[test]
fn a_branch_pin_after_a_touch_to_this_crates_directory_that_a_later_commit_reverts()
-> support::TestOutcome {
    // A commit whose change a later commit reverts leaves nothing newer,
    // and the answer to give is the one that is correct about what `cargo
    // update` would bring. `behind` compares the skeleton's own directory
    // *tree object* at the remote head against the locked tree, never
    // commit-by-commit history, so a touch-then-revert leaves the two tree
    // ids identical — `cargo update` would bring `crate-a` nothing new, and
    // this must read `current`, exactly as
    // `a_branch_pin_stays_current_when_a_later_commit_touches_only_a_sibling_directory`
    // does for a sibling's own untouched directory.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "touch crate-a's own directory",
    )?;
    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v1\n",
        "revert crate-a's own directory back to what was locked",
    )?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(
        support::json::behind_state(bone)?,
        "current",
        "a touch to this crate's own directory that a later commit reverts must read current: \
         cargo update would bring crate-a nothing new"
    );
    Ok(())
}

#[test]
fn an_unqualified_git_dependency_stays_current_when_only_the_other_crates_directory_changes()
-> support::TestOutcome {
    // The same directory-scoped rule, for a pin naming no `tag`/`branch`/
    // `rev` at all — judged against the remote's own default branch.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, "")?;

    repository.commit_skeleton_at(
        "crate-b",
        "crate-b",
        "thing-b.txt",
        "v2\n",
        "touch only crate-b",
    )?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(
        support::json::behind_state(bone)?,
        "current",
        "an unqualified git dependency must follow the same directory-scoped rule as an \
         explicit branch pin"
    );
    Ok(())
}

#[test]
fn a_branch_disappearing_after_lock_reads_undetermined_in_a_many_crate_repository()
-> support::TestOutcome {
    // The same "undetermined, never current" rule `check_behind.rs`
    // already proves for a single-crate repository, proved again here so a
    // repository holding more than one skeleton does not change it: once
    // the remote is unreachable, the directory-scoping machinery must not
    // be able to answer "current" just because it saw nothing to compare.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;

    repository.remove()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "an unreachable remote must not change the default exit status when nothing has \
         drifted; stderr was: {}",
        report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(support::json::behind_state(bone)?, "undetermined");
    assert_eq!(support::json::behind_reason(bone)?, Some("unreachable"));
    Ok(())
}

#[test]
fn a_branch_pin_whose_head_equals_locked_reads_current_without_ever_fetching_a_snapshot()
-> support::TestOutcome {
    // The cheap path: when `ls-remote`'s own head already equals
    // what is locked, `check` never reads Cargo's checkout and never
    // fetches anything — there is nothing to compare. The remote-query log
    // is the positive proof: it must show the cheap branch-head query ran,
    // and must show no `snapshot` line at all, which only a fetch would
    // have logged.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;

    let log_directory = support::TemporaryDirectory::new("branch-directory-cheap-path-log")?;
    let log_path = log_directory.path().join("remote.log");
    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_REMOTE_LOG",
            log_path.to_str().ok_or("log path must be UTF-8")?,
        )],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(support::json::behind_state(bone)?, "current");

    let log = support::read_remote_log(&log_path)?;
    assert!(
        log.contains("branch main"),
        "the cheap ls-remote query must still have run and been logged; log was: {log:?}"
    );
    assert!(
        !log.contains("snapshot"),
        "a head that already equals what is locked must never trigger a directory snapshot \
         fetch; log was: {log:?}"
    );
    Ok(())
}

#[test]
fn sync_still_makes_no_query_in_a_many_crate_branch_pinned_repository() -> support::TestOutcome {
    // `behind`'s own directory-scoped rule is entirely a `check`-side
    // question; `sync` never asks a remote at all, for any pin kind,
    // whether or not that remote's own repository holds more than one
    // skeleton crate. Proved directly through the remote-query log, the
    // same instrument `sync.rs`'s own single-crate positive control uses.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;

    let fixture = fixture_wearing_crate_a(&repository, ", branch = \"main\"")?;
    fixture.init_git_repository()?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "touch crate-a's own directory",
    )?;

    let log_directory = support::TemporaryDirectory::new("branch-directory-sync-log")?;
    let log_path = log_directory.path().join("remote.log");
    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            "SKELETONS_TEST_ONLY_REMOTE_LOG",
            log_path.to_str().ok_or("log path must be UTF-8")?,
        )],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let log = support::read_remote_log(&log_path)?;
    assert_eq!(
        log, "",
        "sync must append nothing to the remote-query log, even against a many-crate, \
         branch-pinned repository whose head has moved; log was: {log:?}"
    );
    Ok(())
}
