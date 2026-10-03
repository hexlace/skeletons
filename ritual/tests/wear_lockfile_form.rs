//! Acceptance: `wear` neither refuses nor rolls back differently, and never
//! changes a byte of either file on the way, when the committed `Cargo.lock`
//! is current but not in the form Cargo itself would write.
//!
//! A lockfile can say exactly what Cargo would resolve and still differ from
//! what Cargo would write: the `[[package]]` entries in another order, no
//! blank lines between them, or a comment someone added. Cargo accepts such a
//! file under `--locked`, which compares meaning, and leaves it alone. Without
//! `--locked` it compares line by line and rewrites the file in its own form.
//! So a `wear` that lets Cargo resolve the workspace without `--locked`,
//! before it has checked the work tree or taken what it will restore, changes
//! the lockfile first and then reads its own change as the wearer's.
//!
//! Each form below goes through the same four stories as a clean lockfile
//! would: a dirty tree is refused as exactly the changes the wearer made, a
//! failed `cargo add` is rolled back rather than refused at the clean check,
//! a success commits as its own next step says and then `sync` and `check`
//! agree, and a lockfile git is told to hide is refused before anything is
//! rewritten. In every refusal and rollback the manifest and the lockfile are
//! byte-identical to before.
//!
//! The binary runs directly, never through `cargo run` or the `cargo ritual`
//! alias: Cargo would rewrite a lockfile in the wrong form before `wear`
//! started, and the test would measure Cargo. The only Cargo this file runs
//! itself is `cargo generate-lockfile` to build the fixture, and the
//! `cargo metadata` calls that show each form is what it claims to be.

mod support;

use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;

use support::git::status_porcelain;
use support::wear::{
    COMMAND_LINE_PACKAGE, ManifestAndLockfile, ONE_UNCOMMITTED_CHANGE_LINE, SKIP_WORKTREE,
    assert_a_hidden_file_is_refused, assert_nothing_was_undone, assert_refused_and_untouched,
    commit_everything, git_step, wear_passthrough_plain,
};
use support::{
    Fixture, Report, TemporaryDirectory, TestOutcome, cargo_command, checked_in_test_skeleton,
    isolate_from_the_enclosing_repository, write_package_manifest,
};

/// How the committed lockfile differs from the form Cargo would write while
/// meaning the same thing.
#[derive(Clone, Copy, Debug)]
enum LockfileForm {
    /// No blank line between the `[[package]]` entries.
    BlankLinesRemoved,
    /// The `[[package]]` entries in reverse order.
    PackagesReversed,
    /// One `#` comment line appended.
    CommentAdded,
}

impl LockfileForm {
    /// `generated`, the lockfile `cargo generate-lockfile` wrote, in this
    /// form.
    fn rewrite(self, generated: &str) -> Result<String, Box<dyn Error>> {
        let rewritten = match self {
            Self::BlankLinesRemoved => generated.lines().filter(|line| !line.is_empty()).fold(
                String::new(),
                |mut text, line| {
                    text.push_str(line);
                    text.push('\n');
                    text
                },
            ),
            Self::PackagesReversed => reverse_packages(generated)?,
            Self::CommentAdded => format!("{generated}# kept here by hand\n"),
        };
        assert_ne!(
            rewritten, generated,
            "{self:?} must change the lockfile; it was:\n{generated}"
        );
        Ok(rewritten)
    }
}

/// `generated` with its `[[package]]` entries in reverse order, each entry
/// and the blank line after it otherwise as written.
fn reverse_packages(generated: &str) -> Result<String, Box<dyn Error>> {
    let (header, entries) = generated
        .split_once("[[package]]")
        .ok_or("a generated lockfile of this fixture has packages")?;
    let mut entries: Vec<&str> = entries.split("[[package]]").map(str::trim).collect();
    assert!(
        entries.len() >= 2,
        "reversing needs two packages; the lockfile was:\n{generated}"
    );
    entries.reverse();
    let mut reversed = header.to_owned();
    for entry in entries {
        write!(reversed, "[[package]]\n{entry}\n\n")?;
    }
    let mut reversed = reversed.trim_end().to_owned();
    reversed.push('\n');
    Ok(reversed)
}

/// Runs `cargo metadata --format-version 1` in `directory`, with `extra`
/// arguments, in `fixture`'s sandbox, and returns its exit code and stderr.
fn cargo_metadata(
    fixture: &Fixture,
    directory: &Path,
    extra: &[&str],
) -> Result<(i32, String), Box<dyn Error>> {
    let mut command = cargo_command();
    command
        .current_dir(directory)
        .env("CARGO_HOME", fixture.sandbox().cargo_home())
        .env("HOME", fixture.sandbox().home())
        .env_remove("CARGO_TARGET_DIR")
        .args(["metadata", "--format-version", "1"])
        .args(extra);
    isolate_from_the_enclosing_repository(&mut command);
    let output = command.output()?;
    Ok((
        output
            .status
            .code()
            .ok_or("cargo metadata ended on a signal")?,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// The files of the fixture workspace, relative to its root.
const WORKSPACE_FILES: [&str; 5] = [
    "Cargo.toml",
    "Cargo.lock",
    "src/lib.rs",
    "helper/Cargo.toml",
    "helper/src/lib.rs",
];

/// Asserts what a form must be before `wear` runs: Cargo accepts the
/// lockfile under `--locked`, git sees a clean tree, and Cargo, on a
/// throwaway copy, would write something other than what is committed.
fn assert_the_form_is_what_it_claims(fixture: &Fixture, form: LockfileForm) -> TestOutcome {
    let (locked_exit, locked_stderr) = cargo_metadata(fixture, fixture.root(), &["--locked"])?;
    assert_eq!(
        locked_exit, 0,
        "precondition ({form:?}): cargo metadata --locked must accept the lockfile; stderr: \
         {locked_stderr}"
    );
    assert_eq!(
        status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition ({form:?}): the work tree must be clean"
    );

    let copy = TemporaryDirectory::new("lockfile-form-copy")?;
    for relative in WORKSPACE_FILES {
        support::write(copy.path(), relative, &fixture.read(relative)?)?;
    }
    let (unlocked_exit, unlocked_stderr) = cargo_metadata(fixture, copy.path(), &[])?;
    assert_eq!(
        unlocked_exit, 0,
        "precondition ({form:?}): cargo metadata must run on the copy; stderr: {unlocked_stderr}"
    );
    assert_ne!(
        support::read(copy.path(), "Cargo.lock")?,
        fixture.read("Cargo.lock")?,
        "precondition ({form:?}): cargo must want to write the lockfile differently, or the form \
         is not one"
    );
    Ok(())
}

/// A clean work tree whose committed lockfile is in `form`: the command
/// line's package depending on a second package, so the lockfile has two
/// `[[package]]` entries, committed as `cargo generate-lockfile` wrote it and
/// then committed again rewritten.
fn fixture_in_form(form: LockfileForm) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_package_manifest(
        fixture.root(),
        "",
        COMMAND_LINE_PACKAGE,
        "[dependencies]\nhelper = { path = \"helper\" }\n",
    )?;
    write_package_manifest(fixture.root(), "helper", "helper", "")?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    let generated = String::from_utf8(fixture.read("Cargo.lock")?)?;
    fixture.write("Cargo.lock", form.rewrite(&generated)?.as_bytes())?;
    commit_everything(&fixture, "fixture: the lockfile in another form")?;
    assert_the_form_is_what_it_claims(&fixture, form)?;
    Ok(fixture)
}

/// A dirty tree is refused as the wearer's own change alone: the manifest
/// has an uncommitted edit, the line counts one change, the lockfile is not
/// listed, and neither file moved.
fn assert_a_dirty_tree_is_refused_untouched(form: LockfileForm) -> TestOutcome {
    let fixture = fixture_in_form(form)?;
    let mut edited = fixture.read("Cargo.toml")?;
    edited.extend_from_slice(b"\n# an edit nobody committed\n");
    fixture.write("Cargo.toml", &edited)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_refused_and_untouched(
        &fixture,
        &report,
        ONE_UNCOMMITTED_CHANGE_LINE,
        &before,
        "Cargo.toml",
    )?;
    assert!(
        report
            .stdout
            .lines()
            .any(|line| line == "Cargo.toml has uncommitted changes"),
        "{form:?}: stdout must list the edited manifest; stdout was:\n{}",
        report.stdout
    );
    assert!(
        !report.stdout.contains("Cargo.lock"),
        "{form:?}: stdout must not list the lockfile, which the wearer did not change; stdout \
         was:\n{}",
        report.stdout
    );
    Ok(())
}

/// A `cargo add` that fails (a `--path` holding no crate) is rolled back, not
/// refused at the clean check, and leaves both files as they were.
fn assert_a_failed_cargo_add_is_rolled_back(form: LockfileForm) -> TestOutcome {
    let fixture = fixture_in_form(form)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let no_crate = TemporaryDirectory::new("holds-no-crate")?;
    let no_crate = no_crate
        .path()
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned();

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "--path",
        &no_crate,
    ])?;

    // The line is `ritual: cargo add failed: <what cargo said>; ritual put
    // the project back as it found it`. What cargo said names a path and is
    // cargo's wording, so only the two ends are pinned.
    assert!(
        report
            .stderr
            .lines()
            .any(|line| line.starts_with("ritual: cargo add failed: ")
                && line.ends_with("; ritual put the project back as it found it")),
        "{form:?}: stderr must hold one line saying cargo add failed and that ritual put the \
         project back; stderr was:\n{}",
        report.stderr
    );
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "{form:?}: a rolled-back wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

/// The files the second line of `report.stdout` tells the wearer to commit.
/// That line reads `commit <file> and <file>, then run ...`.
fn files_the_next_step_names(
    form: LockfileForm,
    report: &Report,
) -> Result<Vec<String>, Box<dyn Error>> {
    let next_step = report
        .stdout
        .lines()
        .nth(1)
        .ok_or_else(|| format!("{form:?}: stdout has no second line: {}", report.stdout))?;
    let named = next_step
        .strip_prefix("commit ")
        .and_then(|rest| rest.split_once(", then run"))
        .map(|(files, _)| files)
        .ok_or_else(|| format!("{form:?}: the next step is not a commit: {next_step}"))?;
    let files: Vec<String> = named.split(" and ").map(str::to_owned).collect();
    assert!(
        files.iter().any(|file| file == "Cargo.toml"),
        "{form:?}: the next step must name the manifest; it was: {next_step}"
    );
    Ok(files)
}

/// Commits exactly `files` and asserts that leaves the work tree clean.
fn commit_exactly_and_assert_clean(
    fixture: &Fixture,
    form: LockfileForm,
    files: &[String],
) -> TestOutcome {
    let mut add = vec!["add", "--"];
    add.extend(files.iter().map(String::as_str));
    git_step(fixture, &add)?;
    git_step(
        fixture,
        &[
            "commit",
            "--quiet",
            "--message",
            "fixture: what wear said to commit",
        ],
    )?;
    assert_eq!(
        status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "{form:?}: committing exactly what wear named must leave the tree clean"
    );
    Ok(())
}

/// Asserts `sync` succeeds and writes the skeleton's file, then that `check`
/// agrees the written file matches.
fn assert_sync_then_check_agree(fixture: &Fixture, form: LockfileForm) -> TestOutcome {
    let sync_report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        sync_report.exit_code, 0,
        "{form:?}: sync must succeed; stderr was: {}",
        sync_report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER,
        "{form:?}: sync must write the skeleton's file"
    );
    let check_report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        check_report.exit_code, 0,
        "{form:?}: check must report the written file matching; stdout: {}; stderr: {}",
        check_report.stdout, check_report.stderr
    );
    Ok(())
}

/// A success goes through: `wear` exits 0, the commit its second line names
/// leaves the tree clean, and `sync` and `check` agree.
fn assert_a_success_goes_through(form: LockfileForm) -> TestOutcome {
    let fixture = fixture_in_form(form)?;
    let skeleton = checked_in_test_skeleton("passthrough-plain");
    let skeleton = skeleton.to_str().ok_or("path must be UTF-8")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", skeleton])?;

    assert_eq!(
        report.exit_code, 0,
        "{form:?}: wear must succeed; stderr was: {}",
        report.stderr
    );
    let files = files_the_next_step_names(form, &report)?;
    commit_exactly_and_assert_clean(&fixture, form, &files)?;
    assert_sync_then_check_agree(&fixture, form)?;
    assert_nothing_was_undone(&report);
    Ok(())
}

#[test]
fn a_lockfile_without_blank_lines_does_not_make_a_dirty_tree_count_its_lockfile() -> TestOutcome {
    // Cargo would put the blank lines back, and `wear` must not let it: the
    // one uncommitted change is the manifest edit.
    assert_a_dirty_tree_is_refused_untouched(LockfileForm::BlankLinesRemoved)
}

#[test]
fn a_lockfile_with_reversed_packages_does_not_make_a_dirty_tree_count_its_lockfile() -> TestOutcome
{
    // Cargo would sort the packages, and `wear` must not let it.
    assert_a_dirty_tree_is_refused_untouched(LockfileForm::PackagesReversed)
}

#[test]
fn a_lockfile_with_a_comment_does_not_make_a_dirty_tree_count_its_lockfile() -> TestOutcome {
    // Cargo would drop the comment, and `wear` must not let it.
    assert_a_dirty_tree_is_refused_untouched(LockfileForm::CommentAdded)
}

#[test]
fn a_lockfile_without_blank_lines_still_rolls_back_a_failed_cargo_add() -> TestOutcome {
    // The failure has to be `cargo add`'s, with the rollback notice, not the
    // clean check refusing the lockfile `wear` itself rewrote.
    assert_a_failed_cargo_add_is_rolled_back(LockfileForm::BlankLinesRemoved)
}

#[test]
fn a_lockfile_with_reversed_packages_still_rolls_back_a_failed_cargo_add() -> TestOutcome {
    assert_a_failed_cargo_add_is_rolled_back(LockfileForm::PackagesReversed)
}

#[test]
fn a_lockfile_with_a_comment_still_rolls_back_a_failed_cargo_add() -> TestOutcome {
    assert_a_failed_cargo_add_is_rolled_back(LockfileForm::CommentAdded)
}

#[test]
fn a_lockfile_without_blank_lines_is_worn_committed_synced_and_checked() -> TestOutcome {
    assert_a_success_goes_through(LockfileForm::BlankLinesRemoved)
}

#[test]
fn a_lockfile_with_reversed_packages_is_worn_committed_synced_and_checked() -> TestOutcome {
    assert_a_success_goes_through(LockfileForm::PackagesReversed)
}

#[test]
fn a_lockfile_with_a_comment_is_worn_committed_synced_and_checked() -> TestOutcome {
    assert_a_success_goes_through(LockfileForm::CommentAdded)
}

#[test]
fn a_hidden_lockfile_with_a_comment_is_refused_before_anything_is_rewritten() -> TestOutcome {
    // Marked skip-worktree and carrying a comment git is told not to look at:
    // the hidden-file refusal must come before Cargo has had any chance to
    // rewrite the file, so the comment is still there afterwards.
    assert_a_hidden_file_is_refused("Cargo.lock", true, &["--skip-worktree"], SKIP_WORKTREE)
}
