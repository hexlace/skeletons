//! A feature change on a package outside the workspace, made after the lock,
//! which `check` aborts on and `check_without_cargo` reports through.
//!
//! The oracle is `check` on the same workspace once Cargo brings the lock up
//! to date, held equal with `assert_eq!` and, so that two equal wrong reports
//! cannot pass, pinned to values written out here. The package outside the
//! workspace is `y`, a path package with an optional path dependency `z`; no
//! registry is needed to build it. `cargo generate-lockfile` brings the lock up
//! to date here: with path packages only there is no registry choice, so it
//! gives the lock Cargo would.

mod support;

use skeletons_library::{FileState, PackageLocations, check, check_without_cargo};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, TestOutcome, checked_files, path_dependency,
    wearing_table, worn_once,
};

/// What `y` writes to depend on `z` without any feature enabling it.
const Y_OPTIONAL_Z: &str = "[dependencies]\nz = { path = \"../z\", optional = true }\n";

/// What `y` writes to depend on `z` and to enable it by default.
const Y_DEFAULT_ENABLES_Z: &str = "[dependencies]\nz = { path = \"../z\", optional = true }\n\
                                   [features]\ndefault = [\"z\"]\n";

/// Locks a workspace whose member wears `house-rules` and depends on `y`, as
/// `y` is written by `manifest_tail_when_locked`; then rewrites `y` as
/// `manifest_tail_after`, without locking again. Requires that `check` aborts
/// on the stale lock, takes `check_without_cargo`'s report on it, brings the
/// lock up to date, and requires `check`'s report to equal it and to hold the values
/// written here.
fn assert_stale_report_is_the_relocked_one(
    manifest_tail_when_locked: &str,
    manifest_tail_after: &str,
) -> TestOutcome {
    let fixture = Fixture::new()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    let y_directory = fixture.write_crate("y", "0.1.0", manifest_tail_when_locked)?;
    fixture.write_crate("z", "0.1.0", "")?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            fixture.rules_dependency(),
            path_dependency("y", "y", &y_directory)
        ),
        &wearing_table("rules"),
    )?;
    fixture.generate_lockfile()?;
    fixture.write_crate("y", "0.1.0", manifest_tail_after)?;

    let Err(error) = check(fixture.project()) else {
        return Err("check read a workspace whose lock a feature change made stale".into());
    };
    assert!(error.is_lockfile_not_current(), "{error:?}");
    let stale = check_without_cargo(fixture.project(), &PackageLocations::new())?;

    fixture.generate_lockfile()?;
    let relocked = check(fixture.project())?;

    assert_eq!(stale, relocked);
    let skeleton = worn_once(&stale, "house-rules")?;
    assert_eq!(skeleton.version().to_string(), "4.5.6");
    assert_eq!(skeleton.dependency_key(), "rules");
    let states: Vec<FileState> = checked_files(skeleton)?
        .iter()
        .map(skeletons_library::ClaimedFile::state)
        .collect();
    assert_eq!(states, [FileState::Matches, FileState::Missing]);
    assert!(stale.refusals().is_empty(), "{:?}", stale.refusals());
    Ok(())
}

#[test]
fn an_outsiders_dependency_enabled_after_the_lock_is_reported_as_after_a_relock() -> TestOutcome {
    // Given a lock made while `y`'s optional `z` was enabled by nothing, then
    // `y` edited so its default feature enables `z`, `check` aborts on the
    // lock, which does not list `z` under `y`. `check_without_cargo` reports
    // through it, and the report equals the one `check` gives after the lock
    // is brought up to date: the feature change touches no member, no wearing and no
    // package a member's dependency is locked to.
    assert_stale_report_is_the_relocked_one(Y_OPTIONAL_Z, Y_DEFAULT_ENABLES_Z)
}

#[test]
fn an_outsiders_dependency_dropped_after_the_lock_is_reported_as_after_a_relock() -> TestOutcome {
    // The mirror: the lock was made while `y`'s default feature enabled `z`,
    // so it lists `z` under `y`; the edit drops that feature, and `check`
    // aborts on the lock for listing a package nothing wants. The reports
    // are held equal as above.
    assert_stale_report_is_the_relocked_one(Y_DEFAULT_ENABLES_Z, Y_OPTIONAL_Z)
}

#[test]
fn the_stale_pair_is_not_vacuous_without_the_edit() -> TestOutcome {
    // Control for the two above: with `y` left as it was when locked, `check`
    // succeeds, so the abort they require comes from the edit and not from the
    // fixture.
    let fixture = Fixture::new()?;
    let y_directory = fixture.write_crate("y", "0.1.0", Y_OPTIONAL_Z)?;
    fixture.write_crate("z", "0.1.0", "")?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            fixture.rules_dependency(),
            path_dependency("y", "y", &y_directory)
        ),
        &wearing_table("rules"),
    )?;
    fixture.generate_lockfile()?;

    let report = check(fixture.project())?;

    assert_eq!(
        worn_once(&report, "house-rules")?.version().to_string(),
        "4.5.6"
    );
    Ok(())
}
