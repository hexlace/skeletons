//! Acceptance: whether a worn skeleton is behind, for the cases that need a
//! registry this suite stands up itself — a registry-pinned skeleton's own
//! `behind` answer, a skeleton from a registry other than the default one, and
//! a crates.io network that cannot be reached at all — through the
//! test-only entry points that stand in for it: real, captured crates.io
//! sparse-index files, read back through `SKELETONS_TEST_ONLY_CRATES_IO_INDEX`,
//! and the test-only remote-query log, `SKELETONS_TEST_ONLY_REMOTE_LOG`.
//!
//! Every dependency here is a directory-source ("vendored") crate under a
//! fixture's own isolated `CARGO_HOME`, resolving entirely offline yet
//! reporting its source exactly as a real crates.io (or second-registry)
//! dependency would — see `support::write_vendored_crates_io_skeleton` and
//! `support::write_vendored_other_registry_skeleton`. Whether a skeleton is
//! *behind* is decided by asking a captured, real answer from crates.io,
//! never a hand-written one: see `crates/skeletons/src/behind/captures/readme.md`
//! for how and when those answers were captured, and what each one proves.
//!
//! A git remote that cannot be reached at all is covered separately, in
//! `check_behind.rs`, alongside the other pin kinds this suite stands up its
//! own git history for.

mod support;

use support::{Fixture, wearing_table, write_package_manifest};

#[test]
fn a_registry_pinned_skeleton_reports_behind_when_a_newer_non_yanked_release_exists()
-> support::TestOutcome {
    // Locked at `semver` 1.0.7. The captured, real crates.io index for
    // `semver` lists a newer, non-yanked, non-prerelease release, 1.0.28 —
    // see `crates/skeletons/src/behind/captures/readme.md`. `check` must ask
    // crates.io (through the captured-index seam) and report behind, naming
    // 1.0.28, and the query itself must show up in the remote-query log —
    // the positive control that a `behind` answer here is a read one, not
    // an assumption.
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "thing.txt",
        "locked at 1.0.7\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::registry_dependency("semver", "1.0.7"),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"locked at 1.0.7\n")?;

    let log_directory = support::TemporaryDirectory::new("registry-behind-log")?;
    let log_path = log_directory.path().join("remote.log");

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[
            (
                "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
                support::captured_crates_io_index()
                    .to_str()
                    .ok_or("index path must be UTF-8")?,
            ),
            (
                "SKELETONS_TEST_ONLY_REMOTE_LOG",
                log_path.to_str().ok_or("log path must be UTF-8")?,
            ),
        ],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    assert_eq!(
        support::json::behind_newer_version(&bones[0])?,
        "1.0.28",
        "the newer version must be the real newest release the captured index lists"
    );

    let log = support::read_remote_log(&log_path)?;
    assert!(
        log.contains("crates-io semver"),
        "the behind answer must come from a logged crates-io query for `semver`; log was: {log:?}"
    );
    Ok(())
}

#[test]
fn a_registry_pinned_skeleton_reports_current_when_locked_at_the_newest_release()
-> support::TestOutcome {
    // The negative twin: locked at `semver` 1.0.28, the newest release the
    // capture lists, so nothing is newer and the row must read current.
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.28",
        "thing.txt",
        "locked at 1.0.28\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::registry_dependency("semver", "1.0.28"),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"locked at 1.0.28\n")?;

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
            support::captured_crates_io_index()
                .to_str()
                .ok_or("index path must be UTF-8")?,
        )],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "current");
    Ok(())
}

#[test]
fn a_registry_pinned_skeleton_with_only_prerelease_versions_newer_reports_current()
-> support::TestOutcome {
    // `rustls`'s capture carries prerelease versions (`0.24.0-dev.0`,
    // `0.24.0-dev.1`) above the newest real release, 0.23.45. Locked at
    // 0.23.45, the row must read current: a prerelease newer than the
    // locked version does not count as "newer" for this purpose.
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "rustls",
        "0.23.45",
        "thing.txt",
        "locked at 0.23.45\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::registry_dependency("rustls", "0.23.45"),
        wearing_table("rustls", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"locked at 0.23.45\n")?;

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
            support::captured_crates_io_index()
                .to_str()
                .ok_or("index path must be UTF-8")?,
        )],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(
        support::json::behind_state(&bones[0])?,
        "current",
        "a prerelease newer than the locked version must never count as newer"
    );
    Ok(())
}

#[test]
fn a_registry_pinned_skeleton_with_build_metadata_containing_a_hyphen_reports_behind()
-> support::TestOutcome {
    // `toml`'s real versions carry build metadata containing a `-`
    // (`1.1.5+spec-1.1.0`, `1.1.6+spec-1.1.0`). Locked at 1.1.5+spec-1.1.0,
    // the row must still read behind, naming 1.1.6+spec-1.1.0 — proving the
    // reader does not mistake build metadata's own `-` for a prerelease
    // marker and wrongly discard the newer version.
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "toml",
        "1.1.5+spec-1.1.0",
        "thing.txt",
        "locked at 1.1.5\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::registry_dependency("toml", "1.1.5"),
        wearing_table("toml", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"locked at 1.1.5\n")?;

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
            support::captured_crates_io_index()
                .to_str()
                .ok_or("index path must be UTF-8")?,
        )],
    )?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    assert_eq!(
        support::json::behind_newer_version(&bones[0])?,
        "1.1.6+spec-1.1.0",
        "build metadata containing a `-` must not be mistaken for a prerelease marker"
    );
    Ok(())
}

#[test]
fn a_skeleton_from_a_registry_other_than_the_default_reads_undetermined_with_no_query_made()
-> support::TestOutcome {
    // A skeleton taken with `{ version = "…", registry = "other" }`, resolving
    // to the source `sparse+https://example.invalid/index/`, must read
    // undetermined, reason `other-registry` — and, since `skeletons` asks only
    // crates.io, the remote-query log must carry no crates-io entry for it
    // at all, proving no request was even attempted rather than merely that
    // none succeeded.
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

    let log_directory = support::TemporaryDirectory::new("other-registry-log")?;
    let log_path = log_directory.path().join("remote.log");

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_REMOTE_LOG",
            log_path.to_str().ok_or("log path must be UTF-8")?,
        )],
    )?;
    assert_eq!(
        report.exit_code, 0,
        "an undetermined-behind row must not fail the default check; stderr was: {}",
        report.stderr
    );

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("other-registry")
    );

    let log = support::read_remote_log(&log_path)?;
    assert!(
        !log.contains("semver"),
        "a skeleton from a non-default registry must never be queried at all; log was: {log:?}"
    );
    Ok(())
}

#[test]
fn network_unreachable_reads_undetermined_and_does_not_change_the_default_exit_status()
-> support::TestOutcome {
    // `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` set to a directory that holds no
    // capture for this crate at all: exactly the "unreachable" answer the
    // seam gives when the variable is set but the file for that crate is
    // missing. The row must read undetermined, reason `unreachable`, and —
    // since the claimed file otherwise matches — the default (no
    // `--fail-behind`) exit status must still be 0: undetermined never
    // affects it on its own.
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "thing.txt",
        "locked at 1.0.7\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::registry_dependency("semver", "1.0.7"),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"locked at 1.0.7\n")?;

    // An empty directory: the seam is "given" an index root, but it holds no
    // capture for `semver`, which is exactly the documented "unreachable"
    // shape (the variable is set, but the file for this crate is absent).
    let empty_index = support::TemporaryDirectory::new("empty-crates-io-index")?;

    let report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[(
            "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
            empty_index
                .path()
                .to_str()
                .ok_or("index path must be UTF-8")?,
        )],
    )?;
    assert_eq!(
        report.exit_code, 0,
        "an unreachable-behind row must not change the default exit status when the file still \
         matches; stderr was: {}",
        report.stderr
    );

    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("unreachable")
    );
    Ok(())
}
