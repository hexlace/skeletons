//! Acceptance: every path `sync` opens for writing — the staging file
//! beside a claimed path, not only the claimed path itself — gets the same
//! guarantees: a symbolic link sitting there is never followed, and a
//! regular file already sitting there is never truncated.
//!
//! A staging file opened with plain `File::create` would follow a symlink
//! and truncate an existing regular file, so neither of `sync`'s promises
//! ("never writes outside the workspace root", "never through a symbolic
//! link") would hold for the staging path, only for the claimed path
//! itself. `sync` creates its staging file exclusively
//! (`sync/write/staging.rs`) instead, and these tests prove both promises
//! against the real binary.
//!
//! The staging name is deterministic (`.<name>.skeletons-sync`), so both tests
//! plant directly at that name, with no process id to discover first.
//!
//! Cleanup between a staging file's own creation and the write that follows
//! it (a failure after the staging file exists but before it is filled must
//! not leave it behind) is not reachable at this, process level: a
//! process-level test can plant something *before* `sync` starts, but
//! cannot inject a failure *between* two syscalls inside `sync`'s own
//! running process. That case is covered by unit tests inside
//! `sync/write/staging.rs` and `sync/write.rs` themselves.
//!
//! `sync` also refuses any untracked file anywhere in the work tree before
//! it reaches the staging code these tests exercise. Each planted entry is
//! therefore excluded locally (`.git/info/exclude`, never committed) rather
//! than left untracked, so these tests keep exercising the staging-name
//! collision refusal specifically.

mod support;

use support::TemporaryDirectory;
use support::passthrough_plain::fixture_wearing_passthrough_plain_with_clean_baseline;

/// The text unique to the staging-file form of `WriteFailure::Collision`'s
/// own message (`sync/message.rs`) — absent from its directory form, from the
/// whole-worktree dirty message and from every other refusal `sync` reports.
/// Asserted on so these tests fail loudly, for the right reason, if the
/// dirty-tree rule ever catches the planted entry before the collision
/// refusal does.
const COLLISION_MESSAGE_MARKER: &str = "it never overwrites or removes anything it did not create";

#[test]
fn a_symlink_at_the_staging_name_is_never_followed_and_the_outside_file_survives()
-> support::TestOutcome {
    // A symlink is planted at `.plain.yml.skeletons-sync`, pointing at a real
    // file outside the workspace holding known, precious content — the
    // shape of a link planted to redirect a write onto a tracked file such
    // as `deny.toml`. `sync` must never follow it: the outside file's
    // content must survive exactly, `plain.yml` must never become a link
    // (or exist at all, since it never legitimately gets created), and
    // `sync` must refuse, naming the claimed path, with the
    // staging-collision message.
    const OUTSIDE_SENTINEL: &[u8] = b"precious content that exists nowhere else\n";

    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;

    let outside = TemporaryDirectory::new("sync-write-paths-outside-symlink-target")?;
    let outside_file = outside.path().join("precious-outside-file.txt");
    std::fs::write(&outside_file, OUTSIDE_SENTINEL)?;

    support::git::exclude_locally(fixture.root(), ".plain.yml.skeletons-sync")?;
    std::os::unix::fs::symlink(
        &outside_file,
        fixture.root().join(".plain.yml.skeletons-sync"),
    )?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_ne!(
        report.exit_code, 0,
        "sync must refuse rather than write through a symlink planted at its own staging name; \
         stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("plain.yml"),
        "the refusal must name the claimed path; combined output was: {combined}"
    );
    assert!(
        combined.contains(COLLISION_MESSAGE_MARKER),
        "the refusal must be the staging-collision message, not a dirty-worktree one; combined \
         output was: {combined}"
    );

    assert_eq!(
        std::fs::read(&outside_file)?,
        OUTSIDE_SENTINEL,
        "sync must never write through a symlink at its staging name to a file outside the \
         workspace"
    );

    let claimed_metadata = std::fs::symlink_metadata(fixture.root().join("plain.yml"));
    assert!(
        claimed_metadata.is_err(),
        "the claimed path must never come into existence at all from a refused sync (in \
         particular, never as a symlink); metadata was: {claimed_metadata:?}"
    );

    let staging_metadata =
        std::fs::symlink_metadata(fixture.root().join(".plain.yml.skeletons-sync"));
    assert!(
        staging_metadata
            .expect("the planted symlink must still be there")
            .is_symlink(),
        "sync never records something it did not itself create, so a refused sync must leave \
         the planted symlink exactly as planted"
    );
    Ok(())
}

#[test]
fn a_regular_file_at_the_staging_name_is_never_truncated_and_sync_refuses() -> support::TestOutcome
{
    // A plain, regular file — a leftover from some earlier interrupted run,
    // or simply a coincidentally named file — already sits at
    // `.plain.yml.skeletons-sync` before `sync` starts, holding its own
    // precious content. `File::create` would truncate whatever is already
    // there unconditionally; `sync` creates exclusively instead, so this
    // content must survive, `plain.yml` must
    // never be created from it, and `sync` must refuse, naming the claimed
    // path with the staging-collision message.
    const STAGING_SENTINEL: &[u8] = b"a leftover file that must never be truncated\n";

    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    support::git::exclude_locally(fixture.root(), ".plain.yml.skeletons-sync")?;
    fixture.write(".plain.yml.skeletons-sync", STAGING_SENTINEL)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_ne!(
        report.exit_code, 0,
        "sync must refuse rather than truncate a regular file already sitting at its own \
         staging name; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("plain.yml"),
        "the refusal must name the claimed path; combined output was: {combined}"
    );
    assert!(
        combined.contains(COLLISION_MESSAGE_MARKER),
        "the refusal must be the staging-collision message, not a dirty-worktree one; combined \
         output was: {combined}"
    );

    assert_eq!(
        fixture.read(".plain.yml.skeletons-sync")?,
        STAGING_SENTINEL,
        "sync must never truncate a file that already exists at its own staging name"
    );

    assert!(
        fixture.read("plain.yml").is_err(),
        "the claimed path must never come into existence from a refused sync"
    );
    Ok(())
}
