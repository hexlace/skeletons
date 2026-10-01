//! Acceptance: when `check` or `sync` refuses, the message says what is
//! wrong and, where it names a remedy, the remedy works.
//!
//! A refusal that names a remedy which cannot work is worse than one that
//! names none: the user does what it says and meets the same refusal, or a
//! worse one. Each test here builds the situation, reads what the tool
//! said, and asserts the words that carry the meaning (never a whole
//! sentence) and the absence of the remedy that would fail.
//!
//! The words a test looks for are listed beside it, so a rewording that
//! keeps the meaning has somewhere to go: a test that lists several
//! alternatives accepts any of them.
//!
//! Platforms: macOS and Linux. Tests that need permissions to be enforced
//! say `skipped:` where the process can write anywhere (a superuser).

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::Fixture;
use support::slow_filter::{
    LOCAL_GIT_TIMEOUT_SECONDS, LOCAL_GIT_TIMEOUT_VARIABLE, arm_slow_filter,
};
use support::sync::{assert_refused_naming, fixture_wearing, settle_index};

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

/// Whether `text` contains any of `needles`, ignoring ASCII case.
fn mentions_any(text: &str, needles: &[&str]) -> bool {
    let lowered = text.to_lowercase();
    needles
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()))
}

const RENDER: &str = "rendered: bytes\n";

/// The summary's remedy for a file git could not vouch for. It is right for
/// a file whose bytes git simply does not hold; wrong for a file git holds
/// but cannot give back (a failing filter, an oversized blob) or hides.
const MOVE_IT_AWAY: &[&str] = &["move it away", "move them away"];

// ---------------------------------------------------------------------
// The two spellings of a folded name are told apart.
// ---------------------------------------------------------------------

#[test]
fn a_refusal_over_two_unicode_spellings_shows_how_they_differ() -> support::TestOutcome {
    // The skeleton claims `café.yml` spelled precomposed, and the directory
    // holds a file `café.yml` spelled decomposed (`e` and a combining
    // accent). The two look identical on screen, so a message reading
    // "café.yml is spelled café.yml on disk; rename café.yml to café.yml"
    // gives the user nothing to act on. The refusal must show the
    // difference in characters that display differently: escapes or code
    // points, or words that say one spelling is composed and the other
    // decomposed.
    //
    // Words accepted: `U+0301`, `\u0301`, `\xcc\x81`, `\314\201`,
    // `combining`, `decomposed`, `NFD`.
    //
    // Each name is followed by its code points, so the two no longer read as
    // identical text.
    let composed = "caf\u{e9}.yml";
    let decomposed = "cafe\u{301}.yml";
    let wearing = fixture_wearing(&[("unicode-pair", &[(composed, RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write(decomposed, b"a sibling that is not the claim\n")?;

    let report = fixture.run(&["skeletons", "check"])?;

    let combined = format!("{}{}", report.stdout, report.stderr);
    assert_ne!(
        report.exit_code, 0,
        "check must refuse a claim whose name is listed under another Unicode spelling; output \
         was: {combined:?}"
    );
    assert!(
        combined.contains(composed) || combined.contains(decomposed),
        "the refusal must name the claim; output was: {combined:?}"
    );
    assert!(
        mentions_any(
            &combined,
            &[
                "U+0301",
                "\\u0301",
                "\\xcc\\x81",
                "\\314\\201",
                "combining",
                "decomposed",
                "NFD",
            ],
        ),
        "the refusal must show how the two spellings differ, in text that reads differently; \
         output was: {combined:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// A remedy fits its cause, or there is none.
// ---------------------------------------------------------------------

/// Commits `committed` at `plain.yml` (a workspace that claims it, so it is
/// drifted) and returns the fixture, clean.
fn committed_plain(
    committed: &[u8],
) -> Result<support::sync::WearingFixture, Box<dyn std::error::Error>> {
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    wearing.fixture.write("plain.yml", committed)?;
    wearing.fixture.init_git_repository()?;
    Ok(wearing)
}

/// The remedy the refusal for a file that is not what git would check out
/// names.
const CHECKOUT_REMEDY: &str = "git checkout -- plain.yml";

#[test]
fn a_skip_worktree_file_that_differs_from_git_is_not_told_to_check_it_out() -> support::TestOutcome
{
    // `plain.yml` is skip-worktree and its disk bytes differ from the
    // committed ones. `sync` refuses (the file is not what git would give
    // back). The refusal must not say "remove it and run
    // `git checkout -- plain.yml`": for a skip-worktree path git answers
    // `pathspec did not match any file(s) known to git`, the user has by
    // then deleted the file, and the next run says the path is tracked but
    // absent.
    let wearing = committed_plain(b"committed: bytes\n")?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;
    fixture.write("plain.yml", b"edited: on disk, hidden by the flag\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !combined.contains(CHECKOUT_REMEDY),
        "the refusal must not name a `git checkout` that fails for a skip-worktree path; \
         combined output was: {combined:?}"
    );
    Ok(())
}

#[test]
fn a_file_under_a_nondeterministic_filter_is_not_told_to_check_it_out() -> support::TestOutcome {
    // A smudge filter that adds a different line every run (here the
    // process id). The file never equals what git would check out, because
    // there is no one thing git would check out;
    // `git checkout -- plain.yml` writes yet another variant and the next
    // run refuses again, for ever. The refusal must not send the user round
    // that loop.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    git(fixture, &["init", "--quiet"])?;
    git(fixture, &["config", "filter.varying.clean", "sed 1q"])?;
    git(
        fixture,
        &["config", "filter.varying.smudge", "sh -c 'cat; echo $$'"],
    )?;
    fixture.write(".gitattributes", b"plain.yml filter=varying\n")?;
    fixture.write("plain.yml", b"committed: first line\n")?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    git(fixture, &["checkout", "--", "plain.yml"])?;
    settle_index(fixture, &["plain.yml"])?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git calls the work tree clean"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !combined.contains(CHECKOUT_REMEDY),
        "the refusal must not name a remedy that reproduces the refusal; combined output was: \
         {combined:?}"
    );
    Ok(())
}

#[test]
fn a_file_hidden_from_the_work_tree_is_told_how_to_bring_it_back() -> support::TestOutcome {
    // `plain.yml` is tracked, marked skip-worktree and removed from disk, so
    // git ignores anything written there. The refusal must say what the user
    // can do about it: bring the path back into the work tree with
    // `git sparse-checkout add` or `git update-index --no-skip-worktree`.
    let wearing = committed_plain(b"committed: bytes\n")?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;
    std::fs::remove_file(fixture.root().join("plain.yml"))?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("git sparse-checkout add"),
        "the refusal must name `git sparse-checkout add`; combined output was: {combined:?}"
    );
    assert!(
        combined.contains("git update-index --no-skip-worktree"),
        "the refusal must name `git update-index --no-skip-worktree`; combined output was: \
         {combined:?}"
    );
    Ok(())
}

#[test]
fn a_file_hidden_from_the_work_tree_is_not_told_to_commit_it_or_move_it_away()
-> support::TestOutcome {
    // The same scenario, read for the summary line. The file is already
    // committed and is not there to move; "commit it, or move it away"
    // fits neither.
    let wearing = committed_plain(b"committed: bytes\n")?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;
    std::fs::remove_file(fixture.root().join("plain.yml"))?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !mentions_any(&combined, MOVE_IT_AWAY),
        "the summary must not tell the user to move away a file that is not there; combined \
         output was: {combined:?}"
    );
    Ok(())
}

#[test]
fn a_failing_required_filter_is_not_told_to_commit_or_move_the_file() -> support::TestOutcome {
    // `plain.yml` is committed and clean, and its `required` smudge filter
    // fails when git tries to check the file out. Git holds the file;
    // committing or moving it changes nothing about the filter. The summary
    // must not say to.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    git(fixture, &["init", "--quiet"])?;
    fixture.write(".gitattributes", b"plain.yml filter=broken\n")?;
    fixture.write("plain.yml", b"committed: bytes\n")?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    git(fixture, &["config", "filter.broken.clean", "cat"])?;
    git(fixture, &["config", "filter.broken.smudge", "false"])?;
    git(fixture, &["config", "filter.broken.required", "true"])?;
    settle_index(fixture, &["plain.yml"])?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !mentions_any(&combined, MOVE_IT_AWAY),
        "the summary must not tell the user to move away a file whose problem is a failing \
         filter; combined output was: {combined:?}"
    );
    Ok(())
}

#[test]
fn an_oversized_tracked_file_is_not_told_to_commit_it_or_move_it_away() -> support::TestOutcome {
    // `plain.yml` is a committed, clean file larger than the 16 MiB
    // `skeletons` will read. It is already committed. The summary must not
    // say to commit it.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    let oversized = vec![b'x'; 17 * 1024 * 1024];
    fixture.write("plain.yml", &oversized)?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml", "16 MiB"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !mentions_any(&combined, MOVE_IT_AWAY),
        "the summary must not tell the user to move away a file whose problem is its size; \
         combined output was: {combined:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// A claim whose staging name cannot exist is refused before anything is
// staged.
// ---------------------------------------------------------------------

/// A claim file name of `length` bytes. A filesystem takes at most 255 bytes
/// in a name, and `sync` stages `X` as `.X.skeletons-sync`: 16 bytes more, so 239
/// is the longest claim it can stage.
fn claim_of_length(length: usize) -> String {
    let suffix = ".yml";
    format!("{}{suffix}", "n".repeat(length - suffix.len()))
}

const LONGEST_STAGEABLE: usize = 239;

/// The words that say a name is too long for the filesystem.
const TOO_LONG: &[&str] = &["too long", "255", "longest"];

#[test]
fn check_refuses_a_claim_whose_staging_name_would_be_too_long() -> support::TestOutcome {
    // The skeleton claims a file whose own name is 250 bytes: legal on the
    // filesystem, but `sync` could never stage it (`.` + name +
    // `.skeletons-sync` is 266). `check` says `sync` will put a bone back only
    // if `sync` can; here it cannot, so `check` must refuse the claim and say
    // the name is too long.
    //
    // Words accepted: `too long`, `255`, `longest`.
    let claim = claim_of_length(250);
    let wearing = fixture_wearing(&[("long-name", &[(claim.as_str(), RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "check"])?;

    let combined = format!("{}{}", report.stdout, report.stderr);
    assert_ne!(
        report.exit_code, 0,
        "check must refuse; output was: {combined:?}"
    );
    assert!(
        combined.contains(&claim),
        "check must name the claim; output was: {combined:?}"
    );
    assert!(
        mentions_any(&combined, TOO_LONG),
        "check must say the name is too long; output was: {combined:?}"
    );
    Ok(())
}

#[test]
fn sync_refuses_a_claim_whose_staging_name_would_be_too_long_before_it_stages_anything()
-> support::TestOutcome {
    // The same claim, run through `sync`. It must refuse up front, as a
    // statement about the name, rather than fail in the middle of writing
    // with the operating system's own error
    // (`writing nnn….yml failed … File name too long (os error 63)`).
    let claim = claim_of_length(250);
    let wearing = fixture_wearing(&[("long-name", &[(claim.as_str(), RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &[&claim]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, TOO_LONG),
        "sync must say the name is too long; combined output was: {combined:?}"
    );
    assert!(
        !combined.contains("os error"),
        "sync must refuse the name up front, not report the operating system's own failure from \
         the middle of a write; combined output was: {combined:?}"
    );
    assert!(
        fixture.read(&claim).is_err(),
        "nothing may be written for a refused claim"
    );
    Ok(())
}

#[test]
fn a_claim_whose_staging_name_just_fits_is_still_written() -> support::TestOutcome {
    // Guard: a claim of 239 bytes stages as exactly 255. It is the longest
    // that can be written, and refusing it would refuse a file `sync` can
    // put back.
    let claim = claim_of_length(LONGEST_STAGEABLE);
    let wearing = fixture_wearing(&[("longest-name", &[(claim.as_str(), RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a claim whose staging name is exactly the limit must be written; stdout was: {}, \
         stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read(&claim)?, RENDER.as_bytes());
    Ok(())
}

// ---------------------------------------------------------------------
// Environmental failures name the path, the command where there is one,
// and something to do.
// ---------------------------------------------------------------------

/// Words that say what to do about an unwritable directory.
const WRITABLE_REMEDY: &[&str] = &[
    "writable",
    "chmod",
    "write permission",
    "permission to write",
];

#[test]
fn a_directory_sync_cannot_write_into_is_named_with_a_remedy() -> support::TestOutcome {
    // The skeleton claims `d/x.yml`; `d` exists (it holds a tracked file)
    // but is not writable by this user. `sync` cannot stage beside the
    // claim. The refusal must name the directory and say to make it
    // writable, not stop at "Permission denied (os error 13)".
    //
    // Words accepted: `writable`, `chmod`, `write permission`, `permission
    // to write`.
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

    // Only a refusal on permission establishes the premise; any other
    // failure of the probe write fails the test.
    match std::fs::write(directory.join("probe"), b"x") {
        Ok(()) => {
            restore()?;
            print_skip("this process can write into a 0o555 directory");
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(error) => return Err(error.into()),
    }

    let report = fixture.run(&["skeletons", "sync"]);
    restore()?;
    let report = report?;

    assert_refused_naming(&report, &["d/x.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, WRITABLE_REMEDY),
        "the refusal must say the directory has to be made writable; combined output was: \
         {combined:?}"
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

/// A git subcommand written the way a message names a command: in backticks,
/// as in `` `git ls-files` ``. A bare `` `git` `` names no command.
fn names_a_git_command(text: &str) -> bool {
    text.match_indices("`git ").any(|(start, _)| {
        text[start + "`git ".len()..]
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
    })
}

#[test]
fn a_content_filter_that_times_out_is_named_with_its_path_command_and_a_remedy()
-> support::TestOutcome {
    // A repository's content filter for `plain.yml` sleeps for longer than
    // `sync` allows a local git command. The command is killed. The refusal
    // must say which path was being examined, which git command was running,
    // and what to check, which a bare "running `git` failed, so sync wrote
    // nothing: timed out after 30s and was killed" does not.
    //
    // The remedy must name the filter and the `git check-attr` command that
    // finds it; how it words running `sync` again is not pinned here.
    //
    // The wait is the timeout itself, thirty seconds for a real `sync`. This
    // run sets the test-only override to `LOCAL_GIT_TIMEOUT_SECONDS`, so the
    // filter only has to outlast that.
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    let _slow_filter = arm_slow_filter(fixture, "plain.yml", b"committed: bytes\n")?;

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(LOCAL_GIT_TIMEOUT_VARIABLE, LOCAL_GIT_TIMEOUT_SECONDS)],
    )?;

    assert_refused_naming(&report, &["plain.yml", "timed out"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        names_a_git_command(&combined),
        "the refusal must name the git command that timed out, in backticks with its subcommand; \
         combined output was: {combined:?}"
    );
    // What to check is the filter itself, and the command that names it: a
    // remedy that only says to run `sync` again would send the reader round
    // the same timeout.
    for expected in [
        "a content filter this repository configures for plain.yml did not finish",
        "(`git check-attr filter -- plain.yml` names it); make it finish, then",
    ] {
        assert!(
            combined.contains(expected),
            "the refusal must name the filter to check and how to find it, {expected:?}; \
             combined output was: {combined:?}"
        );
    }
    // The wait it reports is the one this run was given, not the production
    // bound it would have had without the override.
    let waited = format!("timed out after {LOCAL_GIT_TIMEOUT_SECONDS} s");
    assert!(
        combined.contains(&waited),
        "the refusal must report the timeout it ran under, {waited:?}; combined output was: \
         {combined:?}"
    );
    Ok(())
}
