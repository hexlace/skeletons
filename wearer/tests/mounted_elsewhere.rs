//! Acceptance: a project that mounts the bundle under a key other than
//! `skeletons` is told to run commands that exist.
//!
//! The command namespace comes from the key a project mounts the bundle under,
//! so a wearer whose command line reads `wearer tools sync` never runs
//! anything called `skeletons sync`. These tests assert that no remedy the
//! tool prints names the default key's command line, and that each remedy is
//! still there.
//!
//! Each test builds one situation that makes `check` or `sync` print a remedy,
//! runs the `wearer` binary (this package's command line, which mounts the
//! bundle as `tools`), and asserts two things about what it printed:
//!
//! - the remedy is present, keyed on words that belong to the remedy and not
//!   to the command line (`again`, `chmod u+w`, `git rm --cached`), so a case
//!   that stopped provoking its message cannot pass by printing nothing;
//! - no command line for the default key appears: not `cargo ritual
//!   skeletons`, and not `skeletons sync` or `skeletons check` under any
//!   binary name.
//!
//! Remedies no fixture here can provoke, because they follow a failure the
//! operating system would have to inject, are listed at the foot of the file.
//!
//! Platforms: macOS and Linux. Tests that need permissions to be enforced, or
//! a filesystem that folds Unicode spellings, say `skipped:` where the premise
//! does not hold.

// The support module is shared with `skeletons-ritual`'s acceptance tests
// rather than copied, so the two suites build fixtures the same way.
#[path = "../../ritual/tests/support/mod.rs"]
mod support;

use std::error::Error;
use std::os::unix::fs::PermissionsExt as _;

use support::shell_quote::shell_quoted;
use support::slow_filter::{
    LOCAL_GIT_TIMEOUT_SECONDS, LOCAL_GIT_TIMEOUT_VARIABLE, arm_slow_filter,
};
use support::sync::{WearingFixture, assert_refused_naming, fixture_wearing, settle_index};
use support::{Fixture, Report, Sandbox, TemporaryDirectory, TestOutcome};

/// The key this command line mounts the bundle under.
const MOUNT_KEY: &str = "tools";

/// The command lines a remedy would print if it named the default key, however
/// the binary is named: the full form under `cargo ritual`, and the subcommand
/// paths that only exist when the bundle is mounted as `skeletons`. Each is
/// asserted absent from every remedy.
const DEFAULT_KEY_COMMANDS: [&str; 3] = [
    "cargo ritual skeletons",
    "skeletons sync",
    "skeletons check",
];

/// Remedy phrasings that give a subcommand with no task word: `/bin/sync` is
/// a real command on macOS and Linux that prints nothing and exits 0, so
/// someone following ``run `sync` again`` from a log sees success while their files
/// are still drifted. Each is asserted absent from every remedy.
const BARE_SUBCOMMAND_REMEDIES: [&str; 4] = [
    "run `sync`",
    "run `check`",
    "; `sync` puts",
    "(`check` works",
];

const RENDER: &str = "rendered: bytes\n";
const COMMITTED: &[u8] = b"committed: bytes\n";

fn sync(fixture: &Fixture) -> Result<Report, Box<dyn Error>> {
    fixture.run(&[MOUNT_KEY, "sync"])
}

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

/// Asserts `report` is a refusal (not a success, not a panic) that carries
/// every one of `remedy_words` and names no command line for the default key.
///
/// `remedy_words` is what makes the case non-vacuous: a run that stopped
/// reaching its message prints none of them, and fails here rather than
/// passing on the absence of a command it never printed.
fn assert_remedy_names_the_mounted_command(report: &Report, remedy_words: &[&str]) {
    assert_ne!(
        report.exit_code, 0,
        "the run must refuse; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_ne!(
        report.exit_code, 101,
        "the run must refuse, not panic; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    for word in remedy_words {
        assert!(
            combined.contains(word),
            "the message must carry the remedy word {word:?}; combined output was: {combined:?}"
        );
    }
    for bare in BARE_SUBCOMMAND_REMEDIES {
        assert!(
            !combined.contains(bare),
            "a bare subcommand after {bare:?} reads as the shell's own `sync`, which flushes \
             buffers and exits 0, so the remedy must name the subcommand as a task; combined \
             output was: {combined:?}"
        );
    }
    for command in DEFAULT_KEY_COMMANDS {
        assert!(
            !combined.contains(command),
            "the bundle is mounted as `{MOUNT_KEY}`, so no remedy may name {command:?}; \
             combined output was: {combined:?}"
        );
    }
}

/// Runs `sync` and asserts the refusal names `claim`, carries `remedy_words`
/// and names no default-key command line.
fn assert_sync_refusal(fixture: &Fixture, claim: &str, remedy_words: &[&str]) -> TestOutcome {
    let report = sync(fixture)?;
    assert_refused_naming(&report, &[claim]);
    assert_remedy_names_the_mounted_command(&report, remedy_words);
    Ok(())
}

// ---------------------------------------------------------------------
// The control: the second command line is what these tests say it is.
// ---------------------------------------------------------------------

#[test]
fn the_bundle_answers_under_the_mount_key_and_not_under_its_own_name() -> TestOutcome {
    // The fixture binary mounts the bundle as `tools`. If it also
    // answered as `skeletons`, every other test in this file would be
    // testing the default key. `--help` needs no workspace.
    let sandbox = Sandbox::new()?;
    let directory = std::env::current_dir()?;

    let mounted = support::run_ritual(&directory, &sandbox, &[MOUNT_KEY, "sync", "--help"])?;
    let default_key = support::run_ritual(&directory, &sandbox, &["skeletons", "sync", "--help"])?;

    assert_eq!(
        mounted.exit_code, 0,
        "`{MOUNT_KEY} sync --help` must answer; stderr was: {}",
        mounted.stderr
    );
    assert_ne!(
        default_key.exit_code, 0,
        "`skeletons sync` must not exist on a command line that mounts the bundle as \
         `{MOUNT_KEY}`; stdout was: {}",
        default_key.stdout
    );
    Ok(())
}

// ---------------------------------------------------------------------
// `check`.
// ---------------------------------------------------------------------

#[test]
fn check_names_the_command_that_puts_a_drifted_file_back() -> TestOutcome {
    // `plain.yml` differs from its render. `check` says how many bones have
    // drifted and which command puts them back. The remedy is `sync`, in
    // whatever form: the words are `drifted` and `sync`.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write("plain.yml", COMMITTED)?;

    let report = fixture.run(&[MOUNT_KEY, "check"])?;

    assert_remedy_names_the_mounted_command(&report, &["drifted", "the `sync` task puts it back"]);
    Ok(())
}

#[test]
fn check_with_a_stale_lockfile_names_the_command_to_run_again() -> TestOutcome {
    // `Cargo.lock` is gone. `check` reads it and never writes it, so it says
    // to update the lockfile and then to run the `check` task again.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    std::fs::remove_file(fixture.root().join("Cargo.lock"))?;

    let report = fixture.run(&[MOUNT_KEY, "check"])?;

    assert_remedy_names_the_mounted_command(
        &report,
        &[
            "Cargo.lock",
            "cargo update --workspace",
            "then run the `check` task again",
        ],
    );
    Ok(())
}

// ---------------------------------------------------------------------
// `sync`: refusing before it writes.
// ---------------------------------------------------------------------

#[test]
fn sync_with_a_stale_lockfile_names_the_command_to_run_again() -> TestOutcome {
    // The same missing lockfile, met by `sync`.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    std::fs::remove_file(fixture.root().join("Cargo.lock"))?;

    let report = sync(fixture)?;

    assert_remedy_names_the_mounted_command(
        &report,
        &[
            "Cargo.lock",
            "cargo update --workspace",
            "then run the `sync` task again",
        ],
    );
    Ok(())
}

#[test]
fn sync_in_a_redirected_git_environment_names_the_commands_for_outside_a_hook() -> TestOutcome {
    // `GIT_DIR` is set, as it is inside a git hook, and something would be
    // written. `sync` refuses, naming the variable, and says to run the
    // `sync` task outside a hook and that the `check` task works inside one.
    // Two commands in one sentence, both to be named for this command line.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    let elsewhere = TemporaryDirectory::new("mounted-elsewhere-git-dir")?;
    let elsewhere_path = elsewhere
        .path()
        .to_str()
        .ok_or("GIT_DIR path must be UTF-8")?;

    let report = fixture.run_with_env(&[MOUNT_KEY, "sync"], &[("GIT_DIR", elsewhere_path)])?;

    assert_refused_naming(&report, &["GIT_DIR"]);
    assert_remedy_names_the_mounted_command(
        &report,
        &[
            "run the `sync` task outside a git hook",
            "(the `check` task works inside one)",
        ],
    );
    Ok(())
}

#[test]
fn sync_over_a_dirty_tree_names_the_command_to_run_again() -> TestOutcome {
    // An unrelated untracked file makes the working tree dirty. `sync` writes
    // only into a clean one, and says to commit, stash or move it and run the
    // `sync` task again.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    fixture.write("unrelated.txt", b"never claimed by any skeleton\n")?;

    let report = sync(fixture)?;

    assert_refused_naming(&report, &["unrelated.txt"]);
    assert_remedy_names_the_mounted_command(
        &report,
        &[
            "uncommitted",
            "commit, stash or move",
            "run the `sync` task again",
        ],
    );
    Ok(())
}

// ---------------------------------------------------------------------
// `sync`: a path git cannot vouch for.
// ---------------------------------------------------------------------

/// A workspace claiming `plain.yml`, committed with bytes that differ from the
/// render (so it is drifted), and clean.
fn drifted_committed_plain() -> Result<WearingFixture, Box<dyn Error>> {
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    wearing.fixture.write("plain.yml", COMMITTED)?;
    wearing.fixture.init_git_repository()?;
    Ok(wearing)
}

#[test]
fn a_file_git_cannot_give_back_names_the_command_to_run_again() -> TestOutcome {
    // The claimed file carries a line that the repository's clean filter
    // strips on the way in. The line was added on disk and staged; git holds
    // the old bytes, calls the tree clean, and can never give the line back.
    // The remedy is to keep a copy, restore the file with `git checkout`, and
    // run the `sync` task again.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    git(fixture, &["init", "--quiet"])?;
    git(
        fixture,
        &["config", "filter.strip.clean", "sed '/^local:/d'"],
    )?;
    git(fixture, &["config", "filter.strip.smudge", "cat"])?;
    fixture.write(".gitattributes", b"plain.yml filter=strip\n")?;
    fixture.write("plain.yml", b"old\n")?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    fixture.write("plain.yml", b"old\nlocal: only-copy-of-this-line\n")?;
    git(fixture, &["add", "plain.yml"])?;

    assert_sync_refusal(
        fixture,
        "plain.yml",
        &["git checkout -- plain.yml", "run the `sync` task again"],
    )
}

#[test]
fn a_tracked_file_hidden_from_the_work_tree_names_the_command_to_run_again() -> TestOutcome {
    // `plain.yml` is tracked, skip-worktree and removed from disk. The
    // remedy is to bring it back into the work tree and run the `sync`
    // task again.
    let wearing = drifted_committed_plain()?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;
    std::fs::remove_file(fixture.root().join("plain.yml"))?;

    assert_sync_refusal(
        fixture,
        "plain.yml",
        &[
            "git sparse-checkout add",
            "absent from the work tree",
            "run the `sync` task again",
        ],
    )
}

#[test]
fn a_present_file_git_does_not_read_names_the_command_to_run_again() -> TestOutcome {
    // `plain.yml` is committed, skip-worktree and present with the committed
    // bytes. Git reports nothing for it, so an update would never be
    // committed. The remedy is `git update-index --no-skip-worktree`, then
    // `sync` again.
    let wearing = drifted_committed_plain()?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;

    assert_sync_refusal(
        fixture,
        "plain.yml",
        &[
            "git update-index --no-skip-worktree",
            "run the `sync` task again",
        ],
    )
}

#[test]
fn a_tracked_file_where_a_directory_is_needed_names_the_command_to_run_again() -> TestOutcome {
    // Git tracks a file `a` (hidden, so absent from disk) where the
    // skeleton needs a directory for `a/b.yml`. The remedy is to remove the
    // entry from the index, commit, and run the `sync` task again.
    let wearing = fixture_wearing(&[("under-a-file", &[("a/b.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write("a", b"a tracked file\n")?;
    fixture.init_git_repository()?;
    git(fixture, &["update-index", "--skip-worktree", "a"])?;
    std::fs::remove_file(fixture.root().join("a"))?;

    assert_sync_refusal(
        fixture,
        "a/b.yml",
        &["git rm --cached -- a", "run the `sync` task again"],
    )
}

#[test]
fn a_claim_git_ignores_names_the_command_to_run_again() -> TestOutcome {
    // `cfg/.gitignore` ignores `*.local.yml` and the skeleton claims
    // `cfg/editor.local.yml`. Either remove the rule, or create the file and
    // `git add -f` it; then run the `sync` task again.
    let wearing = fixture_wearing(&[("claims-cfg", &[("cfg/editor.local.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write(
        "cfg/.gitignore",
        b"# machine-local settings stay out of git\n*.local.yml\n",
    )?;
    fixture.init_git_repository()?;

    assert_sync_refusal(
        fixture,
        "cfg/editor.local.yml",
        &["git add -f", "*.local.yml", "run the `sync` task again"],
    )
}

#[test]
fn a_file_at_the_staging_name_names_the_command_to_run_again() -> TestOutcome {
    // A regular file already sits at `.plain.yml.skeletons-sync`, the name
    // `sync` stages `plain.yml` at. It never overwrites what it did not
    // create; the remedy is to move that file away and run the `sync` task
    // again.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    support::git::exclude_locally(fixture.root(), ".plain.yml.skeletons-sync")?;
    fixture.write(
        ".plain.yml.skeletons-sync",
        b"a leftover that must survive\n",
    )?;

    assert_sync_refusal(
        fixture,
        "plain.yml",
        &[
            "never overwrites or removes anything it did not create",
            "run the `sync` task again",
        ],
    )
}

#[test]
fn a_directory_sync_cannot_write_into_names_the_command_to_run_again() -> TestOutcome {
    // The skeleton claims `d/x.yml`; `d` exists but this user cannot write
    // into it. The remedy is `chmod u+w d`, then `sync` again.
    //
    // Platform: needs permissions to be enforced; a superuser can write into
    // any directory, so there `skipped:` is printed.
    let wearing = fixture_wearing(&[("in-directory", &[("d/x.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write("d/tracked.txt", b"tracked\n")?;
    fixture.init_git_repository()?;
    let directory = fixture.root().join("d");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))?;
    let restore = || std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755));

    match std::fs::write(directory.join("probe"), b"x") {
        Ok(()) => {
            restore()?;
            print_skip("this process can write into a 0o555 directory");
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(error) => return Err(error.into()),
    }

    let report = sync(fixture);
    restore()?;
    let report = report?;

    assert_refused_naming(&report, &["d/x.yml"]);
    assert_remedy_names_the_mounted_command(&report, &["chmod u+w d", "run the `sync` task again"]);
    Ok(())
}

#[test]
fn a_file_changed_while_sync_proves_another_names_the_command_to_run_again() -> TestOutcome {
    // The skeleton claims `a.yml` and `b.yml`, both committed and drifted.
    // A content filter on `b.yml` writes into `a.yml` while `sync` proves
    // `b.yml`, standing in for an editor save. `sync` refuses to replace
    // what it no longer holds proof of, and says to run the `sync` task
    // again.
    let wearing = fixture_wearing(&[(
        "two-files",
        &[("a.yml", "render-a\n"), ("b.yml", "render-b\n")],
    )])?;
    let fixture = &wearing.fixture;
    let scripts = TemporaryDirectory::new("mounted-elsewhere-interfering-filter")?;
    let armed = scripts.path().join("armed");
    let script = format!(
        "#!/bin/sh\nif [ -e {armed} ]; then printf precious > {target}; fi\ncat\n",
        armed = shell_quoted(&armed),
        target = shell_quoted(&fixture.root().join("a.yml")),
    );
    support::write(scripts.path(), "filter.sh", script.as_bytes())?;
    let command = format!("sh {}", shell_quoted(&scripts.path().join("filter.sh")));
    git(fixture, &["init", "--quiet"])?;
    git(fixture, &["config", "filter.interfere.clean", &command])?;
    git(fixture, &["config", "filter.interfere.smudge", &command])?;
    fixture.write(".gitattributes", b"b.yml filter=interfere\n")?;
    fixture.write("a.yml", b"old-a\n")?;
    fixture.write("b.yml", b"old-b\n")?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    settle_index(fixture, &["a.yml", "b.yml"])?;
    std::fs::write(&armed, b"")?;

    assert_sync_refusal(fixture, "a.yml", &["run the `sync` task again"])
}

// ---------------------------------------------------------------------
// `sync`: git taking too long.
// ---------------------------------------------------------------------

#[test]
fn a_content_filter_that_times_out_names_the_command_to_run_again() -> TestOutcome {
    // A content filter for `plain.yml` sleeps for longer than `sync` allows a
    // local git command, which is killed. The remedy is to make the filter
    // finish and run the `sync` task again.
    //
    // The wait is the timeout itself, thirty seconds for a real `sync`. This
    // run sets the test-only override to `LOCAL_GIT_TIMEOUT_SECONDS`, so the
    // filter only has to outlast that.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    let _slow_filter = arm_slow_filter(fixture, "plain.yml", b"committed: bytes\n")?;

    let report = fixture.run_with_env(
        &[MOUNT_KEY, "sync"],
        &[(LOCAL_GIT_TIMEOUT_VARIABLE, LOCAL_GIT_TIMEOUT_SECONDS)],
    )?;

    assert_refused_naming(&report, &["plain.yml"]);
    assert_remedy_names_the_mounted_command(&report, &["timed out", "run the `sync` task again"]);
    Ok(())
}

// ---------------------------------------------------------------------
// `sync`: a name git hides under another Unicode spelling.
// ---------------------------------------------------------------------

#[test]
fn an_entry_hidden_under_another_spelling_names_the_command_that_shows_the_spelling() -> TestOutcome
{
    // Git's index holds `café.yml` spelled decomposed, hidden by
    // skip-worktree, and the skeleton claims it spelled precomposed. On a
    // filesystem that takes the two for one name, creating the claim would
    // shadow the hidden entry, so `sync` refuses and says to bring the entry
    // back and run the `check` task to see how the name is spelled on disk.
    //
    // Platform: macOS only. The premise is checked at run time.
    let probe = TemporaryDirectory::new("mounted-elsewhere-unicode-probe")?;
    std::fs::write(probe.path().join("caf\u{e9}.probe"), b"x")?;
    if !probe.path().join("cafe\u{301}.probe").try_exists()? {
        print_skip("this filesystem does not resolve one Unicode spelling to the other");
        return Ok(());
    }
    let composed = "caf\u{e9}.yml";
    let decomposed = "cafe\u{301}.yml";
    let wearing = fixture_wearing(&[("unicode", &[(composed, RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    fixture.write("blob.tmp", COMMITTED)?;
    let blob = git(fixture, &["hash-object", "-w", "blob.tmp"])?;
    std::fs::remove_file(fixture.root().join("blob.tmp"))?;
    let cacheinfo = format!("100644,{blob},{decomposed}");
    git(
        fixture,
        &[
            "-c",
            "core.precomposeunicode=false",
            "update-index",
            "--add",
            "--cacheinfo",
            &cacheinfo,
        ],
    )?;
    git(
        fixture,
        &[
            "commit",
            "--quiet",
            "--message",
            "fixture: decomposed entry",
        ],
    )?;
    git(
        fixture,
        &[
            "-c",
            "core.precomposeunicode=false",
            "update-index",
            "--skip-worktree",
            decomposed,
        ],
    )?;

    let report = sync(fixture)?;

    assert_remedy_names_the_mounted_command(
        &report,
        &["run the `check` task to see how it is spelled on disk"],
    );
    Ok(())
}

#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn print_skip(reason: &str) {
    eprintln!("skipped: {reason}");
}

// ---------------------------------------------------------------------
// Not reached.
//
// These remedies follow a failure only the operating system could inject
// mid-write, and no seam or fixture here provokes one:
//
// - `sync` finished every write and then found one changed on read-back
//   ("changed after sync wrote it"), whose remedy names `check`;
// - a rename, or a link creation, failing part-way through the writes;
// - the staging directory appearing between the check and the create;
// - the git-index and git-ignore timeouts, which need git itself to stall
//   on one specific question rather than on a content filter.
// ---------------------------------------------------------------------
