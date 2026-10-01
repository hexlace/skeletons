//! Acceptance: whenever `sync` decides that a path it is about to create is
//! free, it decides with the fold the filesystem or git applies, not with
//! the exact spelling.
//!
//! Three kinds of question are asked of a path `sync` will create: is it
//! taken by another claim or by a staging file `sync` itself will make; is
//! it hidden in git's index under another case; is it under a directory
//! `sync` creates. Where the answer comes from an exact-spelling comparison
//! and the filesystem or git folds case, two names that are one file to the
//! filesystem read as two, and `sync` writes the wrong bytes to a file, or
//! writes a file git then ignores, and reports success or panics.
//!
//! The claim-overlap check folds unconditionally (`FoldedName`, on every
//! platform), so a pair of claims that could collide on any filesystem is
//! refused everywhere. The staging-name comparison uses the same fold, so
//! those refusals are expected on macOS and on Linux, before anything is
//! written.
//!
//! Each test says in a comment in its body what each platform proves:
//! macOS/APFS folds case for real; Linux ext4 does not, so a test that
//! needs a folding filesystem either asserts a refusal that does not depend
//! on the filesystem or exercises git's own fold (`core.ignorecase`, set by
//! hand).

mod support;

use support::Fixture;
use support::sync::{any_staging_file_remains, assert_refused_naming, fixture_wearing};

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

/// Commits `held_path` with `held_contents`, marks it skip-worktree and
/// removes it (and its now-empty directory) from disk: a path git tracks and
/// reports nothing about, that is not on the file system. Git's own
/// `core.ignorecase` is set by hand, so a case-sensitive filesystem (Linux)
/// exercises git's fold and a case-insensitive one (macOS, where `git init`
/// already sets it) is unchanged.
fn commit_then_hide_under_ignorecase(
    fixture: &Fixture,
    held_path: &str,
    held_contents: &[u8],
) -> support::TestOutcome {
    git(fixture, &["init", "--quiet"])?;
    git(fixture, &["config", "core.ignorecase", "true"])?;
    fixture.write(held_path, held_contents)?;
    commit_everything(fixture)?;
    git(fixture, &["update-index", "--skip-worktree", held_path])?;
    std::fs::remove_file(fixture.root().join(held_path))?;
    if let Some((directory, _file)) = held_path.rsplit_once('/') {
        std::fs::remove_dir(fixture.root().join(directory))?;
    }
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git reports nothing for a removed skip-worktree path"
    );
    assert!(
        git(fixture, &["ls-files", "-v"])?.contains(&format!("S {held_path}")),
        "precondition: git's index still tracks {held_path}, hidden by skip-worktree"
    );
    Ok(())
}

#[test]
fn a_staging_name_that_folds_onto_another_claim_is_refused_before_anything_is_written()
-> support::TestOutcome {
    // The skeleton claims `a/b` and `a/.B.skeletons-sync`, both missing. `sync`
    // stages `a/b` at `a/.b.skeletons-sync`, which is the same name as the
    // second claim to anything that ignores case. It must refuse, name both
    // paths, and write nothing: no file, no directory, no staging file.
    //
    // Without the fold the two platforms go wrong differently:
    // - macOS/APFS: the names are one file; `a/b` would receive the other
    //   bone's bytes and `sync` would panic reading it back.
    // - Linux ext4: the names are two files, so `sync` would find no
    //   collision, write both and exit 0.
    // The staging-name comparison uses the same fold as the claim-overlap
    // check, which is unconditional, so the refusal is the same on both
    // platforms and comes before anything is written.
    let wearing = fixture_wearing(&[(
        "staging-fold",
        &[
            ("a/b", "first bone\n"),
            ("a/.B.skeletons-sync", "second bone\n"),
        ],
    )])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["a/b"]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("a/.B.skeletons-sync") || combined.contains("a/.b.skeletons-sync"),
        "the refusal must name the other path, in either spelling; combined output was: \
         {combined:?}"
    );
    assert!(
        !fixture.root().join("a").exists(),
        "a refusal that comes before any write must not even create the directory"
    );
    assert!(
        !any_staging_file_remains(fixture.root())?,
        "no staging file may be left behind"
    );
    Ok(())
}

#[test]
fn a_path_git_hides_under_another_case_is_refused_not_created() -> support::TestOutcome {
    // Git's index holds `a.yml` (skip-worktree, so absent from disk and
    // invisible to `git status`), and the skeleton claims `A.yml`. Under
    // `core.ignorecase` git takes `A.yml` for the hidden entry and ignores
    // it, so writing it would print `created` and leave the committed bytes
    // in force. `sync` must refuse, naming both spellings, and create
    // nothing.
    //
    // An absent-path proof that asks git only about the literal spelling
    // `A.yml` finds nothing, and `sync` would create the file and exit 0.
    // The proof must also refuse a path git would fold onto a tracked entry.
    //
    // Platform: on macOS the filesystem and git both fold, and `git init`
    // sets `core.ignorecase` itself. On Linux the filesystem is
    // case-sensitive and `core.ignorecase` is set by hand, so the Linux run
    // proves the half that is git's fold alone.
    let wearing = fixture_wearing(&[("absent-fold", &[("A.yml", "render\n")])])?;
    let fixture = &wearing.fixture;
    commit_then_hide_under_ignorecase(fixture, "a.yml", b"committed\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["A.yml", "a.yml"]);
    assert!(
        fixture.read("A.yml").is_err(),
        "sync must not create A.yml, which git would ignore"
    );
    assert!(
        fixture.read("a.yml").is_err(),
        "sync must not create a.yml either"
    );
    assert_eq!(
        git(fixture, &["show", "HEAD:a.yml"])?,
        "committed",
        "HEAD must still hold the committed bytes"
    );
    Ok(())
}

#[test]
fn a_path_git_hides_under_a_directory_of_another_case_is_refused_not_created()
-> support::TestOutcome {
    // The same hazard, one level down. Git's index holds `D/f.yml`
    // (skip-worktree, its directory gone from disk) and the skeleton claims
    // `d/f.yml`. Under `core.ignorecase` git folds the directory too, takes
    // the new file for the hidden entry, and ignores it. Comparing only the
    // final component, or only the parent's own listing, misses it. `sync`
    // must refuse, name both paths, and create nothing.
    //
    // An absent-path proof that asks git only about the literal `d/f.yml`
    // finds it absent from the index under that spelling, so `sync` would
    // create it and exit 0.
    //
    // Platform: as the test above: real folding on macOS, git's fold alone
    // (`core.ignorecase` set by hand) on Linux.
    let wearing = fixture_wearing(&[("directory-fold", &[("d/f.yml", "render\n")])])?;
    let fixture = &wearing.fixture;
    commit_then_hide_under_ignorecase(fixture, "D/f.yml", b"committed\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["d/f.yml", "D/f.yml"]);
    assert!(
        !fixture.root().join("d").exists(),
        "sync must not create the directory git would fold onto a hidden entry"
    );
    assert_eq!(
        git(fixture, &["show", "HEAD:D/f.yml"])?,
        "committed",
        "HEAD must still hold the committed bytes"
    );
    Ok(())
}

#[test]
fn a_success_never_leaves_a_claim_under_a_differently_spelled_directory() -> support::TestOutcome {
    // Two worn skeletons claim `a/x` and `A/y`. Their directories, `a`
    // and `A`, fold to one name, so the claim walk that `check` (and the
    // survey `sync` starts from) does refuses a claimed path under a
    // directory whose case variant is also there. `sync` creates the
    // directories itself and never asks the same question of them, so it
    // reports every bone matching and leaves a state `check` refuses.
    //
    // The property is that `sync` never reports success for a state `check`
    // refuses: either it refuses up front (naming a path, writing nothing,
    // not panicking), or, if it succeeds, every claim really is where its
    // skeleton says and `check` agrees. Whether `sync` refuses the pair
    // on every platform (as it does for two claims that fold onto each
    // other) or handles it another way, both satisfy this test.
    //
    // The two platforms break differently without it. On macOS/APFS `a` and
    // `A` are one directory: `sync` would create `A`, see `a` as already
    // there, put `a/x` under `A`, and `check` would say `a/x is under A on
    // disk`. On Linux ext4 they are two directories, both would be created,
    // and `check` would say `a/x is under a, which is also present on disk
    // as A`.
    let wearing = fixture_wearing(&[
        ("lower-directory", &[("a/x", "one\n")]),
        ("upper-directory", &[("A/y", "two\n")]),
    ])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let sync = fixture.run(&["skeletons", "sync"])?;

    if sync.exit_code == 0 {
        let check = fixture.run(&["skeletons", "check"])?;
        assert_eq!(
            check.exit_code, 0,
            "sync reported success but check refuses what it wrote; sync said: {}; check said: \
             {}{}",
            sync.stdout, check.stdout, check.stderr
        );
    } else {
        assert_refused_naming(&sync, &[]);
        let combined = format!("{}{}", sync.stdout, sync.stderr);
        assert!(
            combined.contains("a/x") || combined.contains("A/y"),
            "a refusal must name a claimed path; combined output was: {combined:?}"
        );
        assert!(
            !fixture.root().join("a").exists(),
            "a refusal must leave nothing written"
        );
    }
    Ok(())
}

#[test]
fn two_worn_skeletons_claiming_files_under_one_identically_spelled_directory_are_both_written()
-> support::TestOutcome {
    // Guard, not a refusal: two worn skeletons claim `a/x` and `a/y`. Their
    // directory is one directory, spelled the same way by both, so nothing
    // collides: `sync` must create both files with their own bytes and
    // report success, and `check` must then agree.
    //
    // It stops an overlap rule from treating every pair of claims that share
    // a directory as a collision. Two claims under a directory that folds to
    // one name are only in conflict when they spell it differently (see the
    // test above); the same spelling is the ordinary case and must never be
    // refused, on either platform.
    //
    let wearing = fixture_wearing(&[
        ("first-in-directory", &[("a/x", "one\n")]),
        ("second-in-directory", &[("a/y", "two\n")]),
    ])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;

    let sync = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        sync.exit_code, 0,
        "two claims under one identically spelled directory do not collide; stdout was: {}, \
         stderr was: {}",
        sync.stdout, sync.stderr
    );
    assert_eq!(fixture.read("a/x")?, b"one\n");
    assert_eq!(fixture.read("a/y")?, b"two\n");
    let check = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        check.exit_code, 0,
        "check must agree with what sync wrote; stdout was: {}, stderr was: {}",
        check.stdout, check.stderr
    );
    Ok(())
}
