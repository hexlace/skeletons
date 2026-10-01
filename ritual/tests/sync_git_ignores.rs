//! Acceptance: `sync` never writes a file that git will ignore, and never
//! writes into a repository other than the one it was run in.
//!
//! A write only counts if a commit will see it. Four situations make git
//! look away from a path `sync` is about to write:
//!
//! - a file that is *present* but flagged skip-worktree or assume-unchanged,
//!   whose bytes equal what git holds, so nothing marks it as edited and
//!   `sync` "updates" a file whose change git never records;
//! - an index entry at a directory *above* an absent claim (a tracked file
//!   `a`, hidden, and a claim `a/b.yml`), which a question about entries at
//!   or under the claim never asks;
//! - a submodule, a gitlink or a nested repository *between* the workspace
//!   root and the claim, so the write lands in another repository's tree,
//!   which the workspace's own `git status` shows only as one untracked or
//!   modified directory;
//! - on a filesystem that treats two Unicode spellings as one name, an
//!   index entry hidden under the other spelling of the claim.
//!
//! Each refusal test asserts what the refusal said (it names the claim and
//! the thing git would look away from) and reads the disk back, never the
//! exit code alone. The guards at the end are the ordinary cases a refusal
//! drawn too wide would break.
//!
//! Platforms: every test here runs on macOS and Linux except the
//! decomposed-spelling one, which needs a filesystem that folds Unicode
//! normalisation (APFS) and says `skipped:` elsewhere.

mod support;

use support::sync::{NestedWorkspace, WearingFixture, assert_refused_naming, fixture_wearing};
use support::{Fixture, TemporaryDirectory};

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

const COMMITTED: &[u8] = b"committed: bytes\n";
const RENDER: &str = "rendered: bytes\n";

/// A workspace claiming `plain.yml`, committed with `COMMITTED` (which differs
/// from the render, so the file is drifted) and clean.
fn drifted_committed_plain() -> Result<WearingFixture, Box<dyn std::error::Error>> {
    let wearing = fixture_wearing(&[("plain", &[("plain.yml", RENDER)])])?;
    wearing.fixture.write("plain.yml", COMMITTED)?;
    wearing.fixture.init_git_repository()?;
    Ok(wearing)
}

/// Asserts the preconditions that make a flagged, present file the sharp
/// edge: git lists `flag` for `plain.yml`, reports the tree clean, and the
/// disk holds exactly what git holds.
fn assert_hidden_but_equal(fixture: &Fixture, flag: &str) -> support::TestOutcome {
    assert!(
        git(fixture, &["ls-files", "-v", "--", "plain.yml"])?.starts_with(&format!("{flag} ")),
        "precondition: git must list plain.yml with the {flag} flag"
    );
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git reports nothing for the flagged file"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        COMMITTED,
        "precondition: the disk holds exactly what git holds"
    );
    Ok(())
}

#[test]
fn a_present_skip_worktree_file_equal_to_git_is_refused_not_updated() -> support::TestOutcome {
    // `plain.yml` is committed with bytes that differ from the render and is
    // marked skip-worktree, its disk bytes equal to the committed ones. Git
    // reports nothing for it, so the new bytes would never be committed:
    // `git commit -a` records the old ones and `git diff HEAD` stays empty,
    // whatever `sync` reports. `sync` must refuse, naming the file and the
    // flag, and leave the file alone.
    let wearing = drifted_committed_plain()?;
    let fixture = &wearing.fixture;
    git(fixture, &["update-index", "--skip-worktree", "plain.yml"])?;
    assert_hidden_but_equal(fixture, "S")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, &["skip-worktree"]),
        "the refusal must name the flag that hides the file from git; combined output was: \
         {combined:?}"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        COMMITTED,
        "a refused sync must leave the file exactly as it was"
    );
    Ok(())
}

#[test]
fn a_present_assume_unchanged_file_equal_to_git_is_refused_not_updated() -> support::TestOutcome {
    // The same, with assume-unchanged in place of skip-worktree. Git stops
    // looking at the file, so a write to it is never seen.
    let wearing = drifted_committed_plain()?;
    let fixture = &wearing.fixture;
    git(
        fixture,
        &["update-index", "--assume-unchanged", "plain.yml"],
    )?;
    assert_hidden_but_equal(fixture, "h")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, &["assume-unchanged", "assume unchanged"]),
        "the refusal must name the flag that hides the file from git; combined output was: \
         {combined:?}"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        COMMITTED,
        "a refused sync must leave the file exactly as it was"
    );
    Ok(())
}

#[test]
fn a_tracked_file_hidden_above_an_absent_claim_is_refused_not_created() -> support::TestOutcome {
    // Git's index holds a file `a` (skip-worktree, so absent from disk and
    // invisible to `git status`), and the skeleton claims `a/b.yml`. Creating
    // `a/` and `a/b.yml` turns the tracked file into a directory in the work
    // tree: once the flag is cleared git reports `a` deleted and `a/b.yml`
    // untracked, and `git checkout -- a` destroys the bone. `sync` must
    // refuse, naming the claim, and create nothing.
    //
    // A proof that asks the index only about entries at or under the claim
    // finds none, calls the claim absent, and creates it.
    let wearing = fixture_wearing(&[("under-a-file", &[("a/b.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write("a", b"a tracked file\n")?;
    fixture.init_git_repository()?;
    git(fixture, &["update-index", "--skip-worktree", "a"])?;
    std::fs::remove_file(fixture.root().join("a"))?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git reports nothing for a removed skip-worktree file"
    );
    assert!(
        git(fixture, &["ls-files", "-v", "--", "a"])?.starts_with("S "),
        "precondition: git's index still tracks `a`, hidden by skip-worktree"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["a/b.yml"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, &["index"]),
        "the refusal must say the cause is in git's index; combined output was: {combined:?}"
    );
    assert!(
        !fixture.root().join("a").exists(),
        "sync must create neither the directory `a` nor the file in it"
    );
    assert_eq!(
        git(fixture, &["show", "HEAD:a"])?,
        "a tracked file",
        "HEAD must still hold the tracked file"
    );
    Ok(())
}

/// A plain repository holding one committed file, the source a submodule is
/// added from.
fn source_repository()
-> Result<(TemporaryDirectory, TemporaryDirectory), Box<dyn std::error::Error>> {
    let directory = TemporaryDirectory::new("sync-git-ignores-source")?;
    let home = TemporaryDirectory::new("sync-git-ignores-source-home")?;
    support::write(directory.path(), "inside.txt", b"inside\n")?;
    support::git::run(
        directory.path(),
        home.path(),
        &["init", "--quiet"],
        "git init",
    )?;
    support::git::run(directory.path(), home.path(), &["add", "--all"], "git add")?;
    support::git::run(
        directory.path(),
        home.path(),
        &["commit", "--quiet", "--message", "source repository"],
        "git commit",
    )?;
    Ok((directory, home))
}

/// Adds `source` as a submodule at `at`, commits it, and returns nothing
/// else: the workspace is left clean.
fn add_submodule(fixture: &Fixture, source: &TemporaryDirectory, at: &str) -> support::TestOutcome {
    let url = format!("file://{}", source.path().display());
    git(
        fixture,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            &url,
            at,
        ],
    )?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: add submodule"],
    )?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: the workspace is clean with the submodule in place"
    );
    Ok(())
}

/// The words that say a claim's path leads into another repository. A
/// refusal may put it as a submodule, a gitlink or a repository of its own;
/// any of them carries the meaning.
const ANOTHER_REPOSITORY: &[&str] = &["submodule", "gitlink", "repository"];

/// A clean workspace whose claim `sub/x.yml` lies inside the submodule
/// `sub`.
fn claim_inside_a_submodule() -> Result<WearingFixture, Box<dyn std::error::Error>> {
    let (source, _source_home) = source_repository()?;
    let wearing = fixture_wearing(&[("claims-sub", &[("sub/x.yml", RENDER)])])?;
    wearing.fixture.init_git_repository()?;
    add_submodule(&wearing.fixture, &source, "sub")?;
    Ok(wearing)
}

/// A clean workspace whose claim `nested/x.yml` lies inside a repository of
/// its own that the workspace's repository ignores.
fn claim_inside_an_ignored_nested_repository() -> Result<WearingFixture, Box<dyn std::error::Error>>
{
    let wearing = fixture_wearing(&[("claims-nested", &[("nested/x.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    fixture.write("nested/inside.txt", b"inside\n")?;
    support::git::run(
        &fixture.root().join("nested"),
        fixture.sandbox().home(),
        &["init", "--quiet"],
        "git init (nested)",
    )?;
    support::git::exclude_locally(fixture.root(), "nested/")?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: the workspace's own status is silent about the nested repository"
    );
    Ok(wearing)
}

/// Asserts `check` refused `claim` for lying inside another repository.
fn assert_check_refuses_another_repository(fixture: &Fixture, claim: &str) -> support::TestOutcome {
    let check = fixture.run(&["skeletons", "check"])?;
    let output = format!("{}{}", check.stdout, check.stderr);
    assert_ne!(
        check.exit_code, 0,
        "check must refuse; output was: {output:?}"
    );
    assert!(
        output.contains(claim),
        "check must name the claim; output was: {output:?}"
    );
    assert!(
        mentions_any(&output, ANOTHER_REPOSITORY),
        "check must say the claim lies inside another repository (its claim walk refuses the \
         path, not only sync); output was: {output:?}"
    );
    Ok(())
}

/// Asserts `sync` refused `claim` for lying inside another repository and
/// wrote nothing there.
fn assert_sync_refuses_another_repository(fixture: &Fixture, claim: &str) -> support::TestOutcome {
    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, &[claim]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, ANOTHER_REPOSITORY),
        "sync must say the claim lies inside another repository; combined output was: \
         {combined:?}"
    );
    assert!(
        fixture.read(claim).is_err(),
        "sync must not write into the other repository's work tree"
    );
    Ok(())
}

#[test]
fn check_refuses_an_absent_claim_inside_a_submodule() -> support::TestOutcome {
    // `sub` is a submodule and the skeleton claims `sub/x.yml`, which does not
    // exist. `check` walks every claim before it reads or reports it, and a
    // claim whose path leads into another repository is not one `skeletons`
    // may read or write. `check` must refuse, naming the claim and saying why,
    // and not report `drifted (missing)` and point at `sync`, which would
    // refuse it too.
    let wearing = claim_inside_a_submodule()?;
    assert_check_refuses_another_repository(&wearing.fixture, "sub/x.yml")
}

#[test]
fn sync_refuses_an_absent_claim_inside_a_submodule() -> support::TestOutcome {
    // The same claim. Writing it puts a file into the submodule's work tree,
    // where the workspace's repository shows only `sub` as modified and the
    // submodule's own status shows an untracked file. `sync` must refuse and
    // write nothing.
    let wearing = claim_inside_a_submodule()?;
    assert_sync_refuses_another_repository(&wearing.fixture, "sub/x.yml")
}

#[test]
fn check_refuses_an_absent_claim_inside_an_ignored_nested_repository() -> support::TestOutcome {
    // The same, with a repository that is not a submodule at all: `nested/`
    // is its own `git init`, excluded locally so the workspace's status stays
    // silent about it. Nothing in the workspace's index mentions `nested`, so
    // only the claim walk noticing a `.git` between the root and the claim
    // can refuse it.
    let wearing = claim_inside_an_ignored_nested_repository()?;
    assert_check_refuses_another_repository(&wearing.fixture, "nested/x.yml")
}

#[test]
fn sync_refuses_an_absent_claim_inside_an_ignored_nested_repository() -> support::TestOutcome {
    // As above, for `sync`: it must not put a file into a repository the
    // workspace's own repository ignores.
    let wearing = claim_inside_an_ignored_nested_repository()?;
    assert_sync_refuses_another_repository(&wearing.fixture, "nested/x.yml")
}

/// Commits `decomposed` (the decomposed Unicode spelling of a name) into
/// git's index without normalising it, hides the entry with skip-worktree,
/// and asserts the precondition that the index holds that spelling, hidden.
/// No work-tree entry is left under either spelling.
fn hide_decomposed_entry(fixture: &Fixture, decomposed: &str) -> support::TestOutcome {
    // The blob is written from a scratch file so no work-tree entry is left
    // behind under either spelling.
    fixture.write("blob.tmp", COMMITTED)?;
    let blob = git(fixture, &["hash-object", "-w", "blob.tmp"])?;
    std::fs::remove_file(fixture.root().join("blob.tmp"))?;
    git(
        fixture,
        &[
            "-c",
            "core.precomposeunicode=false",
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},{decomposed}"),
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
    let listing = git(
        fixture,
        &[
            "-c",
            "core.quotepath=false",
            "-c",
            "core.precomposeunicode=false",
            "ls-files",
            "-v",
        ],
    )?;
    assert!(
        listing.contains(&format!("S {decomposed}")),
        "precondition: the index holds the decomposed spelling, hidden; listing was: {listing:?}"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn an_index_entry_hidden_under_the_other_unicode_spelling_is_refused_not_created()
-> support::TestOutcome {
    // Git's index holds `café.yml` spelled decomposed (`e` and a combining
    // accent), hidden by skip-worktree, and the skeleton claims the same name
    // spelled precomposed. On APFS the two spellings are one file, so
    // creating the claim makes the hidden tracked entry read as modified and
    // the new file as untracked, and the next `sync` sees a dirty tree.
    // `sync` must refuse, naming the claim, and create nothing.
    //
    // Platform: macOS only. The premise (a filesystem that resolves one
    // spelling to the other) is checked at run time.
    let probe = TemporaryDirectory::new("sync-git-ignores-unicode-probe")?;
    std::fs::write(probe.path().join("caf\u{e9}.probe"), b"x")?;
    // `try_exists` rather than `exists`: a failed lookup is a fault in the
    // probe directory and fails the test, never a reading of "not resolved".
    if !probe.path().join("cafe\u{301}.probe").try_exists()? {
        eprintln!("skipped: this filesystem does not resolve one Unicode spelling to the other");
        return Ok(());
    }

    let composed = "caf\u{e9}.yml";
    let decomposed = "cafe\u{301}.yml";
    let wearing = fixture_wearing(&[("unicode", &[(composed, RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    hide_decomposed_entry(fixture, decomposed)?;
    assert!(
        std::fs::symlink_metadata(fixture.root().join(composed)).is_err(),
        "precondition: nothing is on disk under either spelling"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &[]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains(composed) || combined.contains(decomposed),
        "the refusal must name the claim; combined output was: {combined:?}"
    );
    assert!(
        mentions_any(
            &combined,
            &["which git's index tracks, are one name on this filesystem"]
        ),
        "the refusal must say the entry git's index tracks is one name with the claim here; \
         combined output was: {combined:?}"
    );
    assert!(
        mentions_any(&combined, &["--no-skip-worktree"]),
        "the refusal must offer the way to unhide the entry; combined output was: {combined:?}"
    );
    assert!(
        std::fs::symlink_metadata(fixture.root().join(composed)).is_err(),
        "sync must not create a file git would take for the hidden entry"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Guards: ordinary cases a refusal drawn too wide would break.
// ---------------------------------------------------------------------

#[test]
fn a_clean_committed_file_with_no_index_flags_is_still_updated() -> support::TestOutcome {
    // Guard: the same scenario as the two flag tests, without the flag.
    // Git sees the file, so an update is real and `sync` must make it.
    let wearing = drifted_committed_plain()?;
    let fixture = &wearing.fixture;
    assert!(
        git(fixture, &["ls-files", "-v", "--", "plain.yml"])?.starts_with("H "),
        "precondition: git lists plain.yml with no flag"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "an ordinary drifted file must be updated; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("plain.yml")?, RENDER.as_bytes());
    Ok(())
}

#[test]
fn a_workspace_in_a_repository_with_nothing_between_root_and_claim_still_syncs()
-> support::TestOutcome {
    // Guard: the repository's root is above the workspace, as in a monorepo,
    // and the claim is under a plain directory. No `.git` lies between the
    // workspace root and the claim, so nothing here is another repository.
    let nested = NestedWorkspace::wearing_passthrough_plain("crates/app")?;
    nested.fixture.init_git_repository()?;

    let report = nested.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a workspace below the repository root must still sync; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        !nested.read("plain.yml")?.is_empty(),
        "the claimed file must have been written"
    );
    Ok(())
}

#[test]
fn a_claim_beside_a_submodule_but_not_under_it_still_syncs() -> support::TestOutcome {
    // Guard: `sub` is a submodule; the claims are `other/y.yml` and
    // `sub-notes.yml`, which share a name prefix with it or sit next to it
    // but are not inside it. Both must be created.
    let (source, _source_home) = source_repository()?;
    let wearing = fixture_wearing(&[(
        "beside-submodule",
        &[("other/y.yml", RENDER), ("sub-notes.yml", RENDER)],
    )])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    add_submodule(fixture, &source, "sub")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "claims beside a submodule are ordinary; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("other/y.yml")?, RENDER.as_bytes());
    assert_eq!(fixture.read("sub-notes.yml")?, RENDER.as_bytes());
    Ok(())
}

#[test]
fn a_non_ascii_claim_with_no_index_variant_still_syncs() -> support::TestOutcome {
    // Guard: a claim with a non-ASCII name, nothing in the index under any
    // spelling of it. Refusing a Unicode difference must not become refusing
    // Unicode.
    let composed = "caf\u{e9}.yml";
    let wearing = fixture_wearing(&[("unicode-plain", &[(composed, RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a non-ASCII claim with no index variant must sync; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read(composed)?, RENDER.as_bytes());
    Ok(())
}
