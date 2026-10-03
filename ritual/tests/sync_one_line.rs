//! Acceptance: every message `sync` prints is one line, whatever the names
//! in it hold.
//!
//! A claimed file, or a directory it sits in, may carry a newline in its name
//! (the wearer's own repository, and legitimate on the filesystems this runs
//! on). Each scenario here reaches one of `sync`'s messages with such a name
//! by running `sync` against a real git repository, then asserts two things
//! of everything it printed: no line breaks at the newline, and the newline
//! shows as the two characters `\n`. Each also asserts a phrase that only
//! that message carries, so a scenario cannot pass on some other message.
//!
//! A test whose name must exist on disk first checks that the filesystem
//! keeps the name as written, and prints `skipped:` when it does not.
//!
//! Platforms: macOS and Linux.

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::one_line::{
    NAME, assert_name_stays_on_one_line, filesystem_keeps_the_name, skip, toml_string,
};
use support::sync::{WearingFixture, assert_refused_naming, fixture_wearing};
use support::{Fixture, Report, TestOutcome};

const RENDER: &str = "rendered: bytes\n";
const COMMITTED: &[u8] = b"committed: bytes\n";

/// The claimed name every scenario uses unless it needs another shape.
fn claimed() -> String {
    format!("{NAME}.yml")
}

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

/// Everything `sync` printed.
fn printed(report: &Report) -> String {
    format!("{}{}", report.stdout, report.stderr)
}

/// A workspace claiming `claims` under one skeleton, not yet a repository.
fn wearing_claims(claims: &[(&str, &str)]) -> Result<WearingFixture, Box<dyn std::error::Error>> {
    fixture_wearing(&[("claims-newline", claims)])
}

/// Asserts `report` is a refusal that carries `marker` (words only that
/// message has) and keeps every name on one line.
fn assert_refusal_on_one_line(report: &Report, marker: &str, what: &str) {
    assert_refused_naming(report, &[marker]);
    assert_name_stays_on_one_line(&printed(report), what);
}

/// Skips the calling test, returning `true`, when the filesystem cannot keep
/// `names` exactly as written.
fn skipped_for(names: &[&str]) -> bool {
    if names.iter().all(|name| filesystem_keeps_the_name(name)) {
        return false;
    }
    skip("the filesystem cannot hold a file name with a newline in it");
    true
}

#[test]
fn a_file_sync_creates_under_a_name_holding_a_newline_is_reported_on_one_line() -> TestOutcome {
    // Clean repository, claim missing: `sync` writes it and reports
    // `created <path> (...)`. The report line names the path.
    let name = claimed();
    if skipped_for(&[&name]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "sync must write the file; stdout was {:?}, stderr was {:?}",
        report.stdout, report.stderr
    );
    assert!(
        printed(&report).contains("created "),
        "sync must report the file it created; output was {:?}",
        printed(&report)
    );
    assert_name_stays_on_one_line(&printed(&report), "the created line");
    assert_eq!(
        fixture.read(&name)?,
        RENDER.as_bytes(),
        "the file is written under its exact name"
    );
    Ok(())
}

#[test]
fn a_refusal_over_a_symbolic_link_holding_a_newline_is_printed_on_one_line() -> TestOutcome {
    // The claim is a committed symbolic link. `sync` prints the same refusal
    // `check` reports, after `refused: `.
    let name = claimed();
    if skipped_for(&[&name]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.write("target.yml", RENDER.as_bytes())?;
    std::os::unix::fs::symlink("target.yml", fixture.root().join(&name))?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(&report, "is a symbolic link", "refused: symbolic-link");
    Ok(())
}

#[test]
fn a_refusal_over_a_wearing_key_holding_a_newline_is_printed_on_one_line() -> TestOutcome {
    // A wearing table keyed `"first\nsecond"` names no dependency. `sync`
    // prints the refusal after `refused: `.
    let fixture = Fixture::new()?;
    support::write_package_manifest(
        fixture.root(),
        "",
        "wearer",
        &format!("[package.metadata.skeletons.{}]\n", toml_string(NAME)),
    )?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(
        &report,
        "names no dependency",
        "refused: names-no-dependency",
    );
    Ok(())
}

#[test]
fn an_untracked_file_holding_a_newline_is_listed_on_one_line() -> TestOutcome {
    // `sync` writes only into a clean tree. A stray untracked file named
    // `first\nsecond.txt` blocks it, and the dirty-tree line names the file.
    let stray = format!("{NAME}.txt");
    if skipped_for(&[&stray]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[("plain.yml", RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    fixture.write(&stray, b"not committed\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(&report, "is untracked", "the dirty-tree line");
    Ok(())
}

#[test]
fn a_claim_git_ignores_under_a_name_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // `.gitignore` says `first*`, which covers the claimed
    // `first\nsecond.yml`. `sync` refuses to create a file git would never
    // show, naming the path in the refusal and in the remedy it gives.
    let name = claimed();
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    support::git::ignore(fixture.root(), "first*")?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(&report, "is ignored by git", "the ignored-claim line");
    Ok(())
}

#[test]
fn a_skip_worktree_file_under_a_name_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // The claimed file is committed with bytes that differ from the render
    // and flagged skip-worktree, so git would never see what `sync` wrote.
    // The line names the path in the refusal and in the `git update-index`
    // remedy it gives.
    let name = claimed();
    if skipped_for(&[&name]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.write(&name, COMMITTED)?;
    fixture.init_git_repository()?;
    git(fixture, &["update-index", "--skip-worktree", "--", &name])?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(&report, "skip-worktree", "the hidden-from-work-tree line");
    Ok(())
}

#[test]
fn a_tracked_file_absent_from_the_work_tree_under_a_name_holding_a_newline_is_refused_on_one_line()
-> TestOutcome {
    // The claimed file is tracked, flagged skip-worktree and removed from
    // disk, so git reports nothing and would ignore what `sync` created. The
    // line names the path in the refusal and in the remedy.
    let name = claimed();
    if skipped_for(&[&name]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.write(&name, COMMITTED)?;
    fixture.init_git_repository()?;
    git(fixture, &["update-index", "--skip-worktree", "--", &name])?;
    std::fs::remove_file(fixture.root().join(&name))?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(
        &report,
        "but absent from the work tree",
        "the tracked-but-absent line",
    );
    Ok(())
}

#[test]
fn a_staging_name_that_is_another_claim_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // `sync` stages `first\nsecond` at `.first\nsecond.skeletons-sync`, and
    // the skeleton also claims exactly that name. The refusal names both.
    let claim = NAME.to_owned();
    let staging = format!(".{NAME}.skeletons-sync");
    let wearing = wearing_claims(&[(&claim, RENDER), (&staging, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(
        &report,
        "which is itself a claimed path",
        "the staging-claimed line",
    );
    Ok(())
}

#[test]
fn a_file_at_the_staging_name_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // A leftover file already sits at `.first\nsecond.yml.skeletons-sync`,
    // excluded locally so the tree is clean. `sync` never overwrites it and
    // says so, naming the file and the claim.
    let name = claimed();
    let staging = format!(".{name}.skeletons-sync");
    if skipped_for(&[&staging]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&name, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    support::git::exclude_locally(fixture.root(), ".first*")?;
    fixture.write(&staging, b"a leftover\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refusal_on_one_line(
        &report,
        "it never overwrites or removes anything it did not create",
        "the staging-collision line",
    );
    Ok(())
}

#[test]
fn a_directory_holding_a_newline_that_sync_cannot_write_into_is_named_on_one_line() -> TestOutcome {
    // The claim `first\nsecond/x.yml` sits in a directory that exists and is
    // not writable. `sync` names the directory, and the `chmod` remedy that
    // repeats it.
    let directory = NAME;
    let file = format!("{directory}/x.yml");
    let tracked = format!("{directory}/tracked.txt");
    if skipped_for(&[directory]) {
        return Ok(());
    }
    let wearing = wearing_claims(&[(&file, RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.write(&tracked, b"tracked\n")?;
    fixture.init_git_repository()?;
    let directory_path = fixture.root().join(directory);
    std::fs::set_permissions(&directory_path, std::fs::Permissions::from_mode(0o555))?;
    let restore =
        || std::fs::set_permissions(&directory_path, std::fs::Permissions::from_mode(0o755));
    match std::fs::write(directory_path.join("probe"), b"x") {
        Ok(()) => {
            restore()?;
            skip("this process can write into a 0o555 directory");
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(error) => return Err(error.into()),
    }

    let report = fixture.run(&["skeletons", "sync"]);
    restore()?;
    let report = report?;

    assert_refusal_on_one_line(
        &report,
        "sync cannot create anything in",
        "the unwritable-directory line",
    );
    Ok(())
}
