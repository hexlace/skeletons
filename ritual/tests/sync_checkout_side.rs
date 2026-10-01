//! Acceptance: `sync` overwrites a tracked file only when git can give back
//! exactly what is on disk — that is, when the file on disk is what git
//! would *check out* for the index entry.
//!
//! Git being satisfied that the file is unmodified is a weaker statement.
//! A clean filter that is not the exact inverse of its smudge (an output
//! stripper, a redactor of local-only lines) maps a file holding content
//! git never stored onto the same object id as the committed one. `git
//! status` is silent, `git add` succeeded, and yet no object in the
//! repository holds those bytes: a rewrite loses them and `git checkout`
//! cannot bring them back. The proof is general: whatever differs from what
//! git would check out is refused, whatever the cause.
//!
//! Every test here reads the protected file's bytes back afterwards rather
//! than trusting the exit code, and checks that the refusal names the path.
//! Each one says in a `Platform:` comment which platform proves it. Nothing
//! here is platform-specific beyond a POSIX `sh`, `sed` and `tr`, so all of
//! them run on macOS and on Linux.
//!
//! Two of the tests run in a workspace *below* the repository root. Git
//! reads attributes by the path from the repository root, not from the
//! workspace, so a proof that asks git about the workspace-relative path
//! sees no attribute at all. One test pins that from each side: a filter
//! that applies to the file must be applied (the file is held), and a
//! smudge-side attribute that applies must be applied (the file is refused).
//!
//! The guards for "a clean filtered file is still held and updated" live in
//! `sync_worktree.rs`
//! (`a_clean_file_under_a_configured_clean_smudge_filter_is_not_refused`,
//! `a_clean_git_lfs_pointer_is_not_refused`); each starts from
//! `git checkout -- plain.yml` and asserts that `sync` replaced the file
//! with the render. For both, the file on disk is exactly what a checkout
//! writes.

mod support;

use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::sync::{NestedWorkspace, assert_refused_naming};
use support::{Fixture, TemporaryDirectory};

/// A fixture wearing `passthrough-plain` (which claims `plain.yml`), with
/// its manifest and lockfile written and a git repository initialised but
/// nothing committed yet, so a scenario can configure filters and
/// attributes before the first `git add`.
fn fixture_with_an_empty_repository() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = fixture_wearing_passthrough_plain()?;
    git(&fixture, &["init", "--quiet"])?;
    Ok(fixture)
}

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

fn commit_everything(fixture: &Fixture) -> support::TestOutcome {
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    Ok(())
}

/// Asserts the precondition every scenario below rests on: git itself
/// reports the work tree clean, so the refusal being tested can only come
/// from `sync`'s own proof and not from the whole-tree rule.
fn assert_git_reports_clean(fixture: &Fixture) -> support::TestOutcome {
    let status = support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?;
    assert_eq!(
        status, "",
        "precondition: git must call this work tree clean, or the scenario proves nothing"
    );
    Ok(())
}

#[test]
fn a_line_a_lossy_clean_filter_would_strip_is_never_overwritten() -> support::TestOutcome {
    // The claimed file carries a line (`local: ...`) that the repository's
    // clean filter strips on the way in. The line was added on disk and
    // `git add`ed; git holds `old`, says the tree is clean, and can never
    // give the line back. `sync` must refuse and leave the line where it is.
    //
    // Proving `clean(disk) == index` is not enough: it holds here (the
    // filter deletes the line), so `sync` would print `updated` and the
    // line would be gone. The proof compares the bytes on disk with what
    // git would check out for the index entry (`old`), which they do not
    // equal.
    //
    // Platform: macOS/APFS and Linux ext4 alike; the filter is `sed`.
    const HELD_BY_GIT: &[u8] = b"old\n";
    const ONLY_COPY: &[u8] = b"old\nlocal: only-copy-of-this-line\n";

    let fixture = fixture_with_an_empty_repository()?;
    git(
        &fixture,
        &["config", "filter.strip.clean", "sed '/^local:/d'"],
    )?;
    git(&fixture, &["config", "filter.strip.smudge", "cat"])?;
    fixture.write(".gitattributes", b"plain.yml filter=strip\n")?;
    fixture.write("plain.yml", HELD_BY_GIT)?;
    commit_everything(&fixture)?;

    fixture.write("plain.yml", ONLY_COPY)?;
    git(&fixture, &["add", "plain.yml"])?;
    assert_git_reports_clean(&fixture)?;
    assert_eq!(
        git(&fixture, &["show", ":plain.yml"])?,
        "old",
        "precondition: git's index holds `old`, never the extra line"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    assert!(
        report.stderr.contains(
            "sync cannot show that git holds what 1 file would replace, so it wrote nothing: \
             the line above says why"
        ),
        "the summary must speak of the file sync would replace, not of one it would create; \
         stderr was: {}",
        report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        ONLY_COPY,
        "a line git never stored must survive a refused sync"
    );
    Ok(())
}

/// The scripts and the counter a non-deterministic smudge filter runs from,
/// kept in a directory of their own, outside the work tree.
struct CountingFilter {
    directory: TemporaryDirectory,
}

impl CountingFilter {
    /// Writes a clean script that strips a trailing ` #<n>` from every line
    /// and a smudge script that appends ` #<n>` to every line, where `n` is
    /// one more than the last smudge that ever ran — so no two smudges of
    /// the same blob give the same bytes.
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TemporaryDirectory::new("sync-counting-filter")?;
        support::write(
            directory.path(),
            "clean.sh",
            b"#!/bin/sh\nsed 's/ #[0-9]*$//'\n",
        )?;
        let counter = directory.path().join("counter");
        let smudge = format!(
            "#!/bin/sh\n\
             n=$(cat '{counter}' 2>/dev/null || echo 0)\n\
             n=$((n + 1))\n\
             echo \"$n\" > '{counter}'\n\
             sed \"s/\\$/ #$n/\"\n",
            counter = counter.display()
        );
        support::write(directory.path(), "smudge.sh", smudge.as_bytes())?;
        Ok(Self { directory })
    }

    fn configure(&self, fixture: &Fixture) -> support::TestOutcome {
        let clean = format!("sh {}", self.directory.path().join("clean.sh").display());
        let smudge = format!("sh {}", self.directory.path().join("smudge.sh").display());
        git(fixture, &["config", "filter.counting.clean", &clean])?;
        git(fixture, &["config", "filter.counting.smudge", &smudge])?;
        Ok(())
    }
}

#[test]
fn a_file_whose_checkout_is_not_reproducible_is_never_overwritten() -> support::TestOutcome {
    // The repository's smudge filter embeds a counter in what it writes
    // (` #1`, then ` #2`, ...) and its clean filter strips it, so git reads
    // a checked-out file as clean. But no later checkout will ever produce
    // those bytes again, so the file on disk cannot be given back. `sync`
    // must refuse.
    //
    // Hashing the *clean* side is not enough: it is stable, so the file
    // would read as held and be overwritten. The proof asks git what it
    // would check out now and compares: the smudge has moved on to ` #2`,
    // which is not what is on disk.
    //
    // Platform: macOS and Linux alike; the filter is POSIX `sh` and `sed`.
    let filter = CountingFilter::new()?;
    let fixture = fixture_with_an_empty_repository()?;
    filter.configure(&fixture)?;
    fixture.write(".gitattributes", b"plain.yml filter=counting\n")?;
    fixture.write("plain.yml", b"name: committed\n")?;
    commit_everything(&fixture)?;

    // A real checkout runs the smudge once: the file on disk now holds
    // ` #1`, exactly as git wrote it.
    std::fs::remove_file(fixture.root().join("plain.yml"))?;
    git(&fixture, &["checkout", "--", "plain.yml"])?;
    let on_disk = fixture.read("plain.yml")?;
    assert_eq!(
        on_disk, b"name: committed #1\n",
        "precondition: the checkout ran the counting smudge exactly once"
    );
    assert_git_reports_clean(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    assert_eq!(
        fixture.read("plain.yml")?,
        on_disk,
        "bytes no later checkout can reproduce must survive a refused sync"
    );
    Ok(())
}

#[test]
fn a_checked_out_ident_file_is_held_and_updated() -> support::TestOutcome {
    // Guard, not a refusal: git's built-in `ident` attribute expands `$Id$`
    // to `$Id: <blob id> $` on checkout, and that expansion is a pure
    // function of the blob. A file that `git checkout` wrote is therefore
    // exactly what a second checkout would write, so it must still be held
    // and updated. (`ident` is deterministic; only a smudge whose output
    // varies between runs is refused.)
    //
    // `hash-object` cleans the expansion back to `$Id$`, which is the
    // committed blob. This guards the proof against refusing every filtered
    // file rather than only the lossy ones.
    //
    // Platform: macOS and Linux alike; `ident` is git's own.
    let fixture = fixture_with_an_empty_repository()?;
    fixture.write(".gitattributes", b"plain.yml ident\n")?;
    fixture.write("plain.yml", b"x $Id$ y\n")?;
    commit_everything(&fixture)?;
    std::fs::remove_file(fixture.root().join("plain.yml"))?;
    git(&fixture, &["checkout", "--", "plain.yml"])?;
    assert!(
        String::from_utf8(fixture.read("plain.yml")?)?.contains("$Id: "),
        "precondition: the checkout expanded `$Id$`"
    );
    assert_git_reports_clean(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a checked-out ident file is what git would check out, so it must be held; stdout was: \
         {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report.stdout.contains("updated plain.yml"),
        "sync must have actually written the file; stdout was: {}",
        report.stdout
    );
    assert_eq!(fixture.read("plain.yml")?, PASSTHROUGH_PLAIN_RENDER);
    Ok(())
}

#[test]
fn a_checked_out_utf16_file_is_held_and_updated() -> support::TestOutcome {
    // Guard, not a refusal: with `working-tree-encoding=UTF-16LE` git stores
    // the blob as UTF-8 and writes the work tree file as UTF-16LE, so the
    // bytes on disk are not the bytes in the object at all. `cat-file
    // --filters` re-encodes as a checkout does, so a file `git checkout`
    // wrote is exactly what a second checkout would write, and must still be
    // held and updated rather than refused for differing from its blob.
    //
    // This pins git's own conversion layer on a real Unix shape; the file
    // holds the render's bytes afterwards, UTF-8 as rendered.
    //
    // Platform: macOS and Linux alike; the encoding is git's own.
    let fixture = fixture_with_an_empty_repository()?;
    fixture.write(
        ".gitattributes",
        b"plain.yml working-tree-encoding=UTF-16LE\n",
    )?;
    let committed_text = "name: committed\nvalue: 1\n";
    let utf16le: Vec<u8> = committed_text
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    fixture.write("plain.yml", &utf16le)?;
    commit_everything(&fixture)?;
    std::fs::remove_file(fixture.root().join("plain.yml"))?;
    git(&fixture, &["checkout", "--", "plain.yml"])?;
    assert_eq!(
        fixture.read("plain.yml")?,
        utf16le,
        "precondition: the checkout wrote the file as UTF-16LE"
    );
    assert_git_reports_clean(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a checked-out UTF-16LE file is what git would check out, so it must be held; stdout \
         was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report.stdout.contains("updated plain.yml"),
        "sync must have actually written the file; stdout was: {}",
        report.stdout
    );
    assert_eq!(fixture.read("plain.yml")?, PASSTHROUGH_PLAIN_RENDER);
    Ok(())
}

/// The repository-relative directory the nested workspaces below live in.
const WORKSPACE_DIRECTORY: &str = "ws";

/// A repository whose root holds a workspace at [`WORKSPACE_DIRECTORY`],
/// wearing `passthrough-plain` (which claims `plain.yml`, so the claimed
/// file is `ws/plain.yml` to git). Nothing is committed yet.
fn nested_workspace_in_an_empty_repository() -> Result<NestedWorkspace, Box<dyn std::error::Error>>
{
    let nested = NestedWorkspace::wearing_passthrough_plain(WORKSPACE_DIRECTORY)?;
    git(&nested.fixture, &["init", "--quiet"])?;
    Ok(nested)
}

#[test]
fn a_clean_file_under_a_filter_in_a_workspace_below_the_repository_root_is_held_and_updated()
-> support::TestOutcome {
    // Guard, not a refusal: the workspace is `ws/` inside the repository,
    // and a root `.gitattributes` line names the file by its repository
    // path (`ws/plain.yml filter=upper`). A reversible filter (`tr` between
    // cases) applies, and the file was written by `git checkout`, so the
    // bytes on disk are exactly what a second checkout would write. It is
    // what git can give back, so `sync` must hold it and replace it.
    //
    // This is what stops a checkout-side proof from over-refusing when it
    // asks git about the path relative to the workspace (`plain.yml`)
    // instead of the path relative to the repository (`ws/plain.yml`): git
    // then applies no filter, the raw blob is upper-case, the disk is
    // lower-case, and every filtered file in a nested workspace is refused.
    //
    // Platform: macOS and Linux alike; the filter is `tr`.
    let nested = nested_workspace_in_an_empty_repository()?;
    let fixture = &nested.fixture;
    git(fixture, &["config", "filter.upper.clean", "tr 'a-z' 'A-Z'"])?;
    git(
        fixture,
        &["config", "filter.upper.smudge", "tr 'A-Z' 'a-z'"],
    )?;
    fixture.write(".gitattributes", b"ws/plain.yml filter=upper\n")?;
    nested.write("plain.yml", b"name: lowercase baseline\nvalue: 1\n")?;
    commit_everything(fixture)?;
    std::fs::remove_file(nested.workspace().join("plain.yml"))?;
    git(fixture, &["checkout", "--", "ws/plain.yml"])?;
    assert_eq!(
        nested.read("plain.yml")?,
        b"name: lowercase baseline\nvalue: 1\n",
        "precondition: the checkout smudged the file back to lower case"
    );
    assert_eq!(
        git(fixture, &["show", ":ws/plain.yml"])?,
        "NAME: LOWERCASE BASELINE\nVALUE: 1",
        "precondition: the filter applies to ws/plain.yml, so git stores it in upper case"
    );
    assert_git_reports_clean(fixture)?;

    let report = nested.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a file git checked out under a filter that applies by repository path is what git \
         would check out, so it must be held; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report.stdout.contains("updated plain.yml"),
        "sync must have actually written the file; stdout was: {}",
        report.stdout
    );
    assert_eq!(nested.read("plain.yml")?, PASSTHROUGH_PLAIN_RENDER);
    Ok(())
}

#[test]
fn a_file_a_repository_path_attribute_would_expand_differently_is_refused_in_a_nested_workspace()
-> support::TestOutcome {
    // The workspace is `ws/`, and a root `.gitattributes` line names the
    // file by its repository path (`ws/plain.yml ident`). The file was
    // committed holding an unexpanded `$Id$` and never checked out again, so
    // git would write `$Id: <blob id> $` where the disk holds `$Id$`. Those
    // are not the bytes git gives back, so `sync` must refuse and leave the
    // file alone.
    //
    // The attribute applies only to the path from the repository root. A
    // proof that asks git about the workspace-relative path (`plain.yml`)
    // sees no attribute, so git's checkout is the raw blob, which equals the
    // disk, and the file is falsely held and overwritten. The preconditions
    // below show both answers from git itself, so the test says which one
    // the proof must use.
    //
    // Proving what git would *record* is not enough: `$Id$` on disk records
    // as the committed blob, so the file would read as held and be
    // overwritten. The proof compares the disk with what git would check out
    // under the path from the repository root, and would fail to refuse if
    // that path lost its `ws/` prefix.
    //
    // Platform: macOS and Linux alike; `ident` is git's own.
    const COMMITTED_NEVER_CHECKED_OUT_AGAIN: &[u8] = b"x $Id$ y\n";

    let nested = nested_workspace_in_an_empty_repository()?;
    let fixture = &nested.fixture;
    fixture.write(".gitattributes", b"ws/plain.yml ident\n")?;
    nested.write("plain.yml", COMMITTED_NEVER_CHECKED_OUT_AGAIN)?;
    commit_everything(fixture)?;
    assert_git_reports_clean(fixture)?;

    let object = git(fixture, &["rev-parse", ":ws/plain.yml"])?;
    let cat_file = |path: &str| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let output = support::git::run_allow_failure(
            fixture.root(),
            fixture.sandbox().home(),
            &["cat-file", "--filters", &format!("--path={path}"), &object],
        )?;
        assert_eq!(
            output.status.code(),
            Some(0),
            "precondition: cat-file --filters must succeed for {path}"
        );
        Ok(output.stdout)
    };
    assert_ne!(
        cat_file("ws/plain.yml")?,
        COMMITTED_NEVER_CHECKED_OUT_AGAIN,
        "precondition: under the repository path the attribute applies, so git would expand \
         `$Id$` on checkout"
    );
    assert_eq!(
        cat_file("plain.yml")?,
        COMMITTED_NEVER_CHECKED_OUT_AGAIN,
        "precondition: under the workspace-relative path no attribute applies, so git would \
         write the raw blob, which is what is on disk"
    );

    let report = nested.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["plain.yml"]);
    assert_eq!(
        nested.read("plain.yml")?,
        COMMITTED_NEVER_CHECKED_OUT_AGAIN,
        "bytes git would not write back must survive a refused sync"
    );
    Ok(())
}
