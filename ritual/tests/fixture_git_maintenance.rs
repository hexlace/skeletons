//! Acceptance: no fixture `git` command leaves background maintenance
//! running against a fixture's tree.
//!
//! `git commit` (and `merge`, `fetch` and others) spawn `git maintenance run
//! --auto --detach`, with or without `--quiet`. The detached child outlives
//! the command that started it, and holds or removes a lock under `.git/`
//! after that command has returned, so a fixture repository can still be
//! changing underneath a test that believes it is quiescent, or be removed
//! while a child is still writing into it.
//!
//! The effect is not observed directly. A process listing differs by
//! platform, and the detached child may already have exited by the time it
//! is looked for, so a check that finds nothing proves nothing. What every
//! fixture command actually controls is its own effective configuration, so
//! this test reads `maintenance.auto` and `gc.auto` back through the same
//! builder every fixture command is built with, [`support::git::run`]: if
//! that builder turns both off, no command it builds can start maintenance.

mod support;

use support::TemporaryDirectory;

#[test]
fn every_fixture_git_command_has_background_maintenance_turned_off() -> support::TestOutcome {
    // Reads each setting back with `git config --get`, run through the
    // fixture builder, which reports the value git would act on for that
    // command, including anything passed with `-c`.
    let repository = TemporaryDirectory::new("fixture-git-maintenance")?;
    let home = TemporaryDirectory::new("fixture-git-maintenance-home")?;
    support::git::run(
        repository.path(),
        home.path(),
        &["init", "--quiet"],
        "git init",
    )?;

    let maintenance_auto = support::git::run(
        repository.path(),
        home.path(),
        &["config", "--get", "maintenance.auto"],
        "git config --get maintenance.auto",
    )?;
    let gc_auto = support::git::run(
        repository.path(),
        home.path(),
        &["config", "--get", "gc.auto"],
        "git config --get gc.auto",
    )?;

    assert_eq!(
        maintenance_auto, "false",
        "maintenance.auto must be off for fixture git commands"
    );
    assert_eq!(gc_auto, "0", "gc.auto must be off for fixture git commands");
    Ok(())
}
