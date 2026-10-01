//! Acceptance: a directory above a claimed file is replaced by a symbolic
//! link to somewhere outside the workspace after `sync` has walked the
//! claims, and before it writes.
//!
//! `create_new` (`O_EXCL`) and `rename` guard only the last component of a
//! path; an intermediate component that has become a link is followed. So a
//! claimed file `x/one.yml` whose directory `x` is swapped for a link to an
//! outside directory is "written" outside the workspace.
//!
//! The swap here is done deterministically by a content filter on a *later*
//! claimed path (`z.yml`): filters run while `sync` proves that path, which
//! is after `x/one.yml` was proven and before anything is staged. That is
//! the same lever the re-verification tests use (`sync_reverify.rs`), so no
//! timing is involved.
//!
//! `sync` refuses both cases, before it creates a staging file anywhere,
//! because every write walks its path again immediately before it happens
//! and refuses a directory above the target that is now a link. The walk is
//! the same one `check` and the survey use, so the answer is about the path,
//! not about the bytes at the end of it. Every git process `sync` runs has
//! exited before staging starts, so a swap a content filter makes while git
//! runs it is made before that walk. A process the filter leaves running can
//! still swap later; that is the window `.docs/design.md`'s "Known limits"
//! section describes.
//!
//! - The first test has the outside file differ from the file that was
//!   proven. Re-verifying a file's *content* right before its rename would
//!   refuse it too, because the bytes behind the link are not the proven
//!   bytes.
//! - The second has the outside file hold exactly the proven bytes, so its
//!   content is indistinguishable from the real file's. Only refusing to
//!   traverse a link catches it: a check on the bytes alone passes and the
//!   write lands outside the workspace.
//!
//! Platform: macOS and Linux alike.

mod support;

use support::sync::{assert_refused_naming, fixture_wearing, settle_index};
use support::{Fixture, TemporaryDirectory};

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

const ONE_COMMITTED: &[u8] = b"old-one\n";

/// Builds the scenario and runs `sync` in it: a workspace claiming
/// `x/one.yml` and `z.yml`, both committed, clean, and drifted, with a
/// filter on `z.yml` that, once armed, replaces the directory `x` with a
/// link to `outside`. `outside` holds a `one.yml` with `outside_content`.
/// Returns the report of `sync`, the fixture, and the temporary directory
/// holding the filter script, which the caller keeps alive while it
/// inspects both sides.
fn sync_with_an_ancestor_swapped(
    outside: &TemporaryDirectory,
    outside_content: &[u8],
) -> Result<
    (
        support::Report,
        support::sync::WearingFixture,
        TemporaryDirectory,
    ),
    Box<dyn std::error::Error>,
> {
    let wearing = fixture_wearing(&[(
        "swap-target",
        &[("x/one.yml", "render-one\n"), ("z.yml", "render-z\n")],
    )])?;
    let fixture = &wearing.fixture;
    support::write(outside.path(), "one.yml", outside_content)?;

    let scripts = TemporaryDirectory::new("sync-ancestor-swap-filter")?;
    let armed = scripts.path().join("armed");
    let script = format!(
        "#!/bin/sh\n\
         if [ -e '{armed}' ] && [ -d '{root}/x' ] && [ ! -L '{root}/x' ]; then\n\
         mv '{root}/x' '{root}/x.moved' && ln -s '{outside}' '{root}/x'\n\
         fi\n\
         cat\n",
        armed = armed.display(),
        root = fixture.root().display(),
        outside = outside.path().display(),
    );
    support::write(scripts.path(), "filter.sh", script.as_bytes())?;
    let command = format!("sh {}", scripts.path().join("filter.sh").display());

    git(fixture, &["init", "--quiet"])?;
    git(fixture, &["config", "filter.swap.clean", &command])?;
    git(fixture, &["config", "filter.swap.smudge", &command])?;
    fixture.write(".gitattributes", b"z.yml filter=swap\n")?;
    fixture.write("x/one.yml", ONE_COMMITTED)?;
    fixture.write("z.yml", b"old-z\n")?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    settle_index(fixture, &["x/one.yml", "z.yml"])?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git must call the work tree clean"
    );
    std::fs::write(armed, b"")?;

    let report = fixture.run(&["skeletons", "sync"])?;
    Ok((report, wearing, scripts))
}

#[test]
fn a_directory_swapped_for_a_link_over_a_different_outside_file_never_writes_through_it()
-> support::TestOutcome {
    // `x` becomes a link to an outside directory whose `one.yml` is some
    // other file entirely. `sync` must not replace that file.
    //
    // Guards that a link above a claimed file is never followed for a write,
    // and, as a second line, that a write's content is re-verified against
    // what was proven (the bytes behind the link are not the proven bytes).
    //
    // A check that only asks whether `x/one.yml` is a regular file follows
    // the link and says yes, so the write would create its staging file and
    // rename over the outside file. Each write must instead re-walk its path
    // right before it happens and refuse the link.
    const OUTSIDE_FILE: &[u8] = b"an unrelated file outside the workspace\n";
    let outside = TemporaryDirectory::new("sync-ancestor-swap-outside")?;

    let (report, wearing, _scripts) = sync_with_an_ancestor_swapped(&outside, OUTSIDE_FILE)?;

    assert!(
        std::fs::symlink_metadata(wearing.fixture.root().join("x"))?.is_symlink(),
        "precondition: the filter swapped x for a link while sync ran; stdout was: {}, stderr \
         was: {}",
        report.stdout,
        report.stderr
    );
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(outside.path().join("one.yml"))?),
        String::from_utf8_lossy(OUTSIDE_FILE),
        "sync wrote outside the workspace, through a directory that became a link; stdout was: \
         {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_refused_naming(&report, &["x/one.yml"]);
    Ok(())
}

#[test]
fn a_directory_swapped_for_a_link_to_an_outside_file_with_the_proven_bytes_never_writes_through_it()
-> support::TestOutcome {
    // The same swap, but the outside `one.yml` holds exactly the bytes
    // that were proven for `x/one.yml`. Nothing about its content
    // distinguishes it from the real file. `sync` must still not write
    // outside the workspace root.
    //
    // Guards that the refusal is a check on the path, not on the bytes: a
    // re-verification of content alone would find the outside file equal to
    // the proven one and let the write through, so only refusing to
    // traverse a link closes this case.
    //
    // As in its sibling, each write re-walks its path right before it
    // happens and refuses the link, whatever the bytes behind it are.
    let outside = TemporaryDirectory::new("sync-ancestor-swap-outside-identical")?;

    let (report, wearing, _scripts) = sync_with_an_ancestor_swapped(&outside, ONE_COMMITTED)?;

    assert!(
        std::fs::symlink_metadata(wearing.fixture.root().join("x"))?.is_symlink(),
        "precondition: the filter swapped x for a link while sync ran; stdout was: {}, stderr \
         was: {}",
        report.stdout,
        report.stderr
    );
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(outside.path().join("one.yml"))?),
        String::from_utf8_lossy(ONE_COMMITTED),
        "sync wrote outside the workspace, through a directory that became a link; stdout was: \
         {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_refused_naming(&report, &["x/one.yml"]);
    Ok(())
}
