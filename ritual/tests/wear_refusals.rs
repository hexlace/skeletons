//! Acceptance: `wear` refuses what it must, says what is wrong, and leaves
//! the command line crate's manifest and the workspace `Cargo.lock`
//! byte-identical to what they were.
//!
//! Every scenario reads both files before and after, as bytes, and compares
//! them, never trusting an exit code alone. Only one refusal comes after
//! `cargo add` has already changed both files (a crate that is not a
//! skeleton is only found out once Cargo has resolved it), which is what
//! makes the comparison mean something there; the others come before it, and
//! leave the files identical because nothing touched them.
//!
//! Byte-identical files cannot tell those two apart, so each scenario also
//! holds stderr to the refusal's whole line, word for word, filled in from
//! the fixture. The refusals that come before any change must not say the
//! project was put back; the one that undid a change must say so.

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::wear::{
    ManifestAndLockfile, assert_nothing_was_undone, assert_refused_and_untouched,
    assert_refused_with_line, commit_everything, fixture_for_wearing, git_step,
    the_single_refusal_line, wear_passthrough_plain,
};
use support::{
    Fixture, TemporaryDirectory, TestOutcome, checked_in_test_skeleton, wearing_table,
    write_package_manifest,
};

fn skeleton_path(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(checked_in_test_skeleton(name)
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned())
}

/// A workspace that already wears `passthrough-plain`, as a dev-dependency,
/// committed clean.
fn fixture_already_wearing_passthrough_plain() -> Result<Fixture, Box<dyn std::error::Error>> {
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        support::path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    fixture_for_wearing(&extra)
}

#[test]
fn a_skeleton_that_is_already_worn_is_refused_and_nothing_changes() -> TestOutcome {
    // `passthrough-plain` is already a dependency with its wearing table.
    // Cargo alone would accept the second `add` and quietly rewrite the line,
    // so `wear` has to be the one to say it is already worn.
    let fixture = fixture_already_wearing_passthrough_plain()?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "passthrough-plain is already worn, as `passthrough-plain` in Cargo.toml; a workspace \
         wears a skeleton once, so to change its options, edit \
         [package.metadata.skeletons.passthrough-plain] there",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_key_already_taken_by_another_dependency_is_refused_and_nothing_changes() -> TestOutcome {
    // `taken` is a dependency on a different crate. Wearing a skeleton under
    // that key would overwrite it, so `wear` must refuse, naming the key.
    let other = skeleton_path("enum-fill")?;
    let extra =
        format!("[dependencies]\ntaken = {{ package = \"enum-fill\", path = \"{other}\" }}\n");
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "taken",
        "--path",
        &path,
    ])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "`taken` is taken in Cargo.toml: its dependency on enum-fill is declared under `taken`; \
         give the `wear` task another key as its second argument",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_crate_that_is_not_a_skeleton_is_refused_after_cargo_ran_and_nothing_changes() -> TestOutcome {
    // `plain-crate` resolves fine but has no `[package.metadata.skeletons]`
    // table. That is only known once Cargo has added it (changing both the
    // manifest and the lockfile), so this is the refusal that has something
    // to undo: both files must come back byte for byte.
    let crate_directory = TemporaryDirectory::new("not-a-skeleton")?;
    write_package_manifest(crate_directory.path(), "", "plain-crate", "")?;
    let fixture = fixture_for_wearing("")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = crate_directory
        .path()
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned();

    let report = fixture.run(&["skeletons", "wear", "plain-crate", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "plain-crate 0.1.0 is not a skeleton: its manifest has no [package.metadata.skeletons] \
         table, so it cannot be worn; wear a crate that is one; ritual put the project back as \
         it found it",
    );
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must restore the manifest and Cargo.lock byte for byte"
    );
    Ok(())
}

#[test]
fn a_work_tree_with_uncommitted_changes_is_refused_and_nothing_changes() -> TestOutcome {
    // The command line crate's manifest has an uncommitted edit. `wear` is
    // about to write to that file, so it must refuse as `sync` does, listing
    // the file as a line of its own and then summing up, and must leave the uncommitted edit and the lockfile exactly
    // as they were.
    let fixture = fixture_for_wearing("")?;
    let mut edited = fixture.read("Cargo.toml")?;
    edited.extend_from_slice(b"\n# an edit nobody committed\n");
    fixture.write("Cargo.toml", &edited)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert!(
        report
            .stdout
            .lines()
            .any(|line| line == "Cargo.toml has uncommitted changes"),
        "stdout must list the dirty path as its own line; stdout was:\n{}",
        report.stdout
    );
    assert_refused_with_line(
        &report,
        "the working tree has 1 uncommitted change, so wear wrote nothing: it writes only into a \
         clean working tree, where git holds the manifest it changes and any Cargo.lock git \
         tracks; commit, stash or move it, then run the `wear` task again",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the uncommitted manifest and Cargo.lock byte-identical"
    );
    assert_eq!(
        fixture.read("Cargo.toml")?,
        edited,
        "the uncommitted edit must survive, as it was written"
    );
    Ok(())
}

#[test]
fn a_skeleton_already_depended_on_under_another_key_is_refused_and_nothing_changes() -> TestOutcome
{
    // `passthrough-plain` is already a dependency, under the key `other`, and
    // nothing wears it. Wearing it under its own name would give the manifest
    // a second dependency on one crate, which Cargo refuses only after
    // `cargo add` has written it, in words that offer no remedy. `wear` has to
    // see the dependency by its crate, and name the key it is under, since
    // that is the key the wearing table goes at.
    let path = skeleton_path("passthrough-plain")?;
    let extra = format!(
        "[dependencies]\nother = {{ package = \"passthrough-plain\", path = \"{path}\" }}\n"
    );
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "Cargo.toml already depends on passthrough-plain under the key `other`, without wearing \
         it; to wear it, add an empty [package.metadata.skeletons.other] table to Cargo.toml",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_dependency_whose_wearing_is_refused_is_sent_to_check_and_nothing_changes() -> TestOutcome {
    // `plain-crate` is a dependency with a wearing table at its key, but it is
    // no skeleton, so the reader refuses the wearing and nothing is worn. The
    // table is not missing, so telling the wearer to add one would be false:
    // the one thing that helps is the `check` task, which says why the
    // wearing is refused.
    let crate_directory = TemporaryDirectory::new("refused-wearing")?;
    write_package_manifest(crate_directory.path(), "", "plain-crate", "")?;
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        support::path_dependency_on("plain-crate", crate_directory.path()),
        wearing_table("plain-crate", ""),
    );
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = crate_directory
        .path()
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned();

    let report = fixture.run(&["skeletons", "wear", "plain-crate", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "Cargo.toml already depends on plain-crate under the key `plain-crate`, and has a \
         [package.metadata.skeletons.plain-crate] table for it, but that wearing is refused; \
         the `check` task says why, so run it and fix what it names in Cargo.toml",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_key_that_names_a_crate_the_compiler_provides_is_refused_and_nothing_changes() -> TestOutcome {
    // `std`, `test` and `proc-macro` are crates rustc supplies to every
    // build, so a dependency renamed to one of them would shadow, or be
    // shadowed by, the compiler's own. `proc-macro` is typed with its hyphen
    // and is the compiler's `proc_macro`: the two spellings are one name to
    // rustc, so the refusal must see through the hyphen. Each is refused
    // before `cargo add` runs, naming the key, and the files stay as they were.
    let fixture = fixture_for_wearing("")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    for key in ["std", "test", "proc-macro"] {
        let report = fixture.run(&[
            "skeletons",
            "wear",
            "passthrough-plain",
            key,
            "--path",
            &path,
        ])?;

        assert_ne!(report.exit_code, 0, "key {key}; stdout: {}", report.stdout);
        assert_refused_with_line(
            &report,
            &format!(
                "`{key}` cannot be worn as a dependency key: it names a crate the compiler \
                 provides; give the `wear` task another key as its second argument"
            ),
        );
        assert_nothing_was_undone(&report);
        assert_eq!(
            ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
            before,
            "a wear refused for the key `{key}` must leave the manifest and Cargo.lock \
             byte-identical"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Files git cannot hand back: `wear` writes the manifest and `Cargo.lock`
// and tells the wearer to commit them, so each must be a file git reads from
// the work tree and tracks.
// ---------------------------------------------------------------------

/// The refusal for a file git is told not to read, worded as `sync` words it
/// with `wear` in it. `marked` is how the line names the flags, `options` the
/// `git update-index` options that clear exactly those.
fn hidden_line(path: &str, marked: &str, options: &str) -> String {
    format!(
        "{path} is marked {marked} in git's index, so git does not read its bytes from the work \
         tree and would ignore what wear wrote there: run `git update-index {options} -- \
         {path}`, then run the `wear` task again"
    )
}

const SKIP_WORKTREE: (&str, &str) = ("skip-worktree", "--no-skip-worktree");
const ASSUME_UNCHANGED: (&str, &str) = ("assume-unchanged", "--no-assume-unchanged");
const BOTH_FLAGS: (&str, &str) = (
    "skip-worktree and assume-unchanged",
    "--no-skip-worktree --no-assume-unchanged",
);

/// Builds a clean workspace, optionally gives `hidden` a local edit, marks
/// it with `flags` (as `git update-index` options), and asserts `wear`
/// refuses with the line for `(marked, options)` and changes nothing.
fn assert_a_hidden_file_is_refused(
    hidden: &str,
    locally_edited: bool,
    flags: &[&str],
    (marked, options): (&str, &str),
) -> TestOutcome {
    let fixture = fixture_for_wearing("")?;
    if locally_edited {
        let mut edited = fixture.read(hidden)?;
        edited.extend_from_slice(b"\n# a local edit git is told not to look at\n");
        fixture.write(hidden, &edited)?;
    }
    // One `update-index` per flag: given both in a single call, git keeps
    // only `--assume-unchanged`, so the state the scenario names is never
    // built by accident.
    for flag in flags {
        git_step(&fixture, &["update-index", flag, "--", hidden])?;
    }
    let expected_tag = match (
        flags.contains(&"--skip-worktree"),
        flags.contains(&"--assume-unchanged"),
    ) {
        (true, true) => "s",
        (true, false) => "S",
        (false, true) => "h",
        (false, false) => "H",
    };
    let listed = git_step(&fixture, &["ls-files", "-v", "--", hidden])?;
    assert_eq!(
        listed.split(' ').next(),
        Some(expected_tag),
        "precondition: git must print the tag the scenario means; it printed: {listed}"
    );
    assert_eq!(
        git_step(&fixture, &["status", "--porcelain"])?,
        "",
        "precondition: git reports nothing for the hidden file, which is what makes it a trap"
    );
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_refused_and_untouched(
        &fixture,
        &report,
        &hidden_line(hidden, marked, options),
        &before,
        "Cargo.toml",
    )
}

#[test]
fn a_manifest_marked_skip_worktree_with_a_local_edit_is_refused_and_nothing_changes() -> TestOutcome
{
    // The manifest carries a local edit and is marked skip-worktree, so
    // `git status` is silent about it and git would never commit what `wear`
    // writes there. `wear` must refuse, leaving the hidden edit as it is.
    assert_a_hidden_file_is_refused("Cargo.toml", true, &["--skip-worktree"], SKIP_WORKTREE)
}

#[test]
fn a_manifest_marked_assume_unchanged_with_a_local_edit_is_refused_and_nothing_changes()
-> TestOutcome {
    // The same trap by the other flag: git assumes the file is as committed
    // and does not look.
    assert_a_hidden_file_is_refused(
        "Cargo.toml",
        true,
        &["--assume-unchanged"],
        ASSUME_UNCHANGED,
    )
}

#[test]
fn a_manifest_marked_with_both_flags_is_refused_and_nothing_changes() -> TestOutcome {
    // Both flags set at once, with a local edit: one line names both and the
    // command that clears both.
    assert_a_hidden_file_is_refused(
        "Cargo.toml",
        true,
        &["--skip-worktree", "--assume-unchanged"],
        BOTH_FLAGS,
    )
}

#[test]
fn a_manifest_marked_skip_worktree_without_an_edit_is_refused_all_the_same() -> TestOutcome {
    // The rule is the tag, not the bytes: with nothing edited, the manifest
    // still holds what git holds, and a write there would still be one git
    // never reads.
    assert_a_hidden_file_is_refused("Cargo.toml", false, &["--skip-worktree"], SKIP_WORKTREE)
}

#[test]
fn a_lockfile_marked_skip_worktree_is_refused_and_nothing_changes() -> TestOutcome {
    // `wear` rewrites `Cargo.lock` too, and tells the wearer to commit it;
    // a lockfile git is told not to read cannot be committed.
    assert_a_hidden_file_is_refused("Cargo.lock", false, &["--skip-worktree"], SKIP_WORKTREE)
}

#[test]
fn a_lockfile_marked_with_both_flags_is_refused_and_nothing_changes() -> TestOutcome {
    assert_a_hidden_file_is_refused(
        "Cargo.lock",
        false,
        &["--skip-worktree", "--assume-unchanged"],
        BOTH_FLAGS,
    )
}

#[test]
fn a_lockfile_marked_assume_unchanged_is_refused_and_nothing_changes() -> TestOutcome {
    assert_a_hidden_file_is_refused(
        "Cargo.lock",
        false,
        &["--assume-unchanged"],
        ASSUME_UNCHANGED,
    )
}

/// Makes `relative` untracked: removed from git's index and committed so,
/// with the file left on disk. When `ignored`, a committed `.gitignore` also
/// lists it, so `git status` is clean.
fn leave_untracked(fixture: &Fixture, relative: &str, ignored: bool) -> TestOutcome {
    if ignored {
        fixture.write(".gitignore", format!("{relative}\n").as_bytes())?;
        git_step(fixture, &["add", "--", ".gitignore"])?;
    }
    git_step(fixture, &["rm", "--cached", "--quiet", "--", relative])?;
    git_step(
        fixture,
        &["commit", "--quiet", "--message", "fixture: stop tracking"],
    )?;
    Ok(())
}

#[test]
fn a_manifest_that_git_ignores_and_does_not_track_is_refused_and_nothing_changes() -> TestOutcome {
    // `.gitignore` lists the manifest and git no longer tracks it, so the
    // work tree is clean and `git status` has nothing to say, yet nothing
    // `wear` writes there could be committed. It must refuse, in one line
    // that names the file and says nothing was written; the wording past
    // that is not pinned.
    let fixture = fixture_for_wearing("")?;
    leave_untracked(&fixture, "Cargo.toml", true)?;
    assert_eq!(
        git_step(&fixture, &["status", "--porcelain"])?,
        "",
        "precondition: the work tree must be clean"
    );
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    let line = the_single_refusal_line(&report);
    assert!(
        line.contains("Cargo.toml"),
        "the refusal must name the file; it was: {line}"
    );
    assert!(
        line.contains("wear wrote nothing"),
        "the refusal must say nothing was written; it was: {line}"
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_lockfile_that_git_ignores_and_does_not_track_is_worn_and_then_synced() -> TestOutcome {
    // The allowed case: a project that keeps no lockfile in git (a library's,
    // say) ignores `Cargo.lock`. `wear` has nothing to hand back there, so it
    // must succeed, and what it wrote must then commit, `sync` and `check`.
    let fixture = fixture_for_wearing("")?;
    leave_untracked(&fixture, "Cargo.lock", true)?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed with an ignored lockfile; stderr was: {}",
        report.stderr
    );
    commit_everything(&fixture, "fixture: wear")?;
    let sync_report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        sync_report.exit_code, 0,
        "sync must succeed on what wear wrote; stderr was: {}",
        sync_report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER,
        "sync must write the skeleton's file"
    );
    let check_report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        check_report.exit_code, 0,
        "check must report the written file matching; stdout: {}; stderr: {}",
        check_report.stdout, check_report.stderr
    );
    Ok(())
}

/// Asserts that `relative`, untracked and not ignored, is refused as one
/// uncommitted change, and nothing changes.
fn assert_an_untracked_file_is_an_uncommitted_change(relative: &str) -> TestOutcome {
    let fixture = fixture_for_wearing("")?;
    leave_untracked(&fixture, relative, false)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_refused_and_untouched(
        &fixture,
        &report,
        "the working tree has 1 uncommitted change, so wear wrote nothing: it writes only into a \
         clean working tree, where git holds the manifest it changes and any Cargo.lock git \
         tracks; commit, stash or move it, then run the `wear` task again",
        &before,
        "Cargo.toml",
    )?;
    assert!(
        report
            .stdout
            .lines()
            .any(|line| line == format!("{relative} is untracked")),
        "stdout must list {relative} as its own line; stdout was:\n{}",
        report.stdout
    );
    Ok(())
}

#[test]
fn a_manifest_that_git_does_not_track_is_refused_as_an_uncommitted_change() -> TestOutcome {
    // Pins behaviour that already holds, so every row of this class has a
    // test: untracked and not ignored, the manifest shows in `git status`,
    // and the existing dirty-tree refusal covers it.
    assert_an_untracked_file_is_an_uncommitted_change("Cargo.toml")
}

#[test]
fn a_lockfile_that_git_does_not_track_is_refused_as_an_uncommitted_change() -> TestOutcome {
    // Pins behaviour that already holds, as above, for `Cargo.lock`.
    assert_an_untracked_file_is_an_uncommitted_change("Cargo.lock")
}

// ---------------------------------------------------------------------
// Read-only files: `wear` has to write both, and a file it cannot write is
// a refusal before anything changes, not a failure part-way.
// ---------------------------------------------------------------------

#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn print_skip(reason: &str) {
    eprintln!("skipped: {reason}");
}

/// Makes `relative` read-only, runs `wear`, and asserts the refusal is one
/// line that names `relative` as the workspace shows it (never an absolute
/// path), ends with how to run `wear` again, and leaves both files as they
/// were.
///
/// A process that can write a 0o444 file anyway (a superuser) cannot show
/// this, so there the scenario says `skipped:` on stderr, where CI's job
/// summary collects it, and passes.
fn assert_a_read_only_file_is_refused(relative: &str) -> TestOutcome {
    let fixture = fixture_for_wearing("")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = fixture.root().join(relative);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))?;
    let restore = || std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));

    // Only a refusal on permission establishes the premise; any other
    // failure of the probe fails the test.
    match std::fs::OpenOptions::new().write(true).open(&path) {
        Ok(_) => {
            restore()?;
            print_skip("this process can write a 0o444 file");
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(error) => {
            restore()?;
            return Err(error.into());
        }
    }

    let report = wear_passthrough_plain(&fixture);
    restore()?;
    let report = report?;

    assert_ne!(
        report.exit_code, 0,
        "wear must refuse a read-only {relative}; stdout was: {}",
        report.stdout
    );
    let line = the_single_refusal_line(&report);
    assert!(
        line.contains(relative),
        "the refusal must name {relative}; it was: {line}"
    );
    let root = fixture.root().to_str().ok_or("path must be UTF-8")?;
    assert!(
        !report.stderr.contains(root),
        "the refusal must name the file relative to the workspace root, never an absolute path; \
         stderr was: {}",
        report.stderr
    );
    assert!(
        line.ends_with("then run the `wear` task again"),
        "the refusal must end by saying to run `wear` again; it was: {line}"
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_read_only_manifest_is_refused_before_anything_is_written() -> TestOutcome {
    assert_a_read_only_file_is_refused("Cargo.toml")
}

#[test]
fn a_read_only_lockfile_is_refused_before_anything_is_written() -> TestOutcome {
    assert_a_read_only_file_is_refused("Cargo.lock")
}

#[test]
fn a_wearing_table_at_the_other_spelling_of_the_key_is_refused_and_nothing_changes() -> TestOutcome
{
    // The manifest has `[package.metadata.skeletons.ab_cd]` and no dependency
    // under either spelling. To rustc `ab_cd` and `ab-cd` are one name, so
    // wearing `ab-cd` would leave two tables for one key, and `sync` would
    // refuse the pair. `wear` must see the table at the other spelling, name
    // it as it is spelled in the manifest, before `cargo add` has run, and
    // leave everything as it was.
    let fixture = fixture_for_wearing(&wearing_table("ab_cd", ""))?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&["skeletons", "wear", "ab-cd", "--path", &path])?;

    assert_refused_and_untouched(
        &fixture,
        &report,
        "Cargo.toml already has a [package.metadata.skeletons.ab_cd] table, which is `ab-cd` to \
         rustc; remove [package.metadata.skeletons.ab_cd], which names no dependency, then run \
         the `wear` task again",
        &before,
        "Cargo.toml",
    )
}
