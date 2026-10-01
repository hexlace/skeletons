//! Acceptance: a claim git refuses to track is refused when the claim is
//! built, so `check` reports it and `sync` never creates it.
//!
//! `skeletons` never writes into `.git`, and git's own rule for what counts as
//! `.git` is wider than the spelling `.git`. Git refuses to add a path whose
//! component is any of a set of synonyms, and says `invalid path`: a file
//! named that way can be created on disk but never committed, so a bone at
//! such a name would sit untracked for ever.
//!
//! The set is git's, read from the source of git v2.53.0 rather than taken
//! from memory:
//!
//! - `read-cache.c`, `verify_path_internal` and `verify_dotfile`: `.git` in
//!   any ASCII case, and it calls the two functions below when
//!   `core.protectNTFS` and `core.protectHFS` are on;
//! - `path.c`, `is_ntfs_dotgit`: `.git` or `git~1` in any ASCII case,
//!   followed by nothing, a run of spaces and dots, `:` and anything after
//!   it (a named stream), or a backslash, which it reads as a directory
//!   separator;
//! - `utf8.c`, `next_hfs_char` and `is_hfs_dotgit`: `.git` in any ASCII
//!   case with any of sixteen code points skipped anywhere, including
//!   before the dot and after the `t` (U+200C to U+200F, U+202A to U+202E,
//!   U+206A to U+206F and U+FEFF).
//!
//! Every test asks git itself first, by trying to add the name to a
//! scratch repository with both protections on, so this file's list cannot
//! drift from what git refuses: a guard name git would refuse, or a refused
//! name git would take, stops the test before it says anything about
//! `skeletons`.
//!
//! Each `check` refusal test reads `check --json` and asserts the refusal's
//! own fields: its kind, the claimed path and the skeleton, and that the path
//! is not also reported as a bone to write; the `sync` test asserts that
//! `sync` refuses, naming the claim, and creates nothing. The guards at the
//! end are mostly the names an over-wide rule would refuse and git accepts;
//! the exception, `.GIT`, is a name git refuses that a narrower rule would
//! let through.
//!
//! Platforms: every test here runs on macOS and Linux. Names are only ever
//! created inside a disposable directory, and none of them needs the
//! filesystem to fold anything.

mod support;

use std::collections::BTreeSet;

use serde_json::Value;

use support::sync::{WearingFixture, assert_refused_naming, fixture_wearing};
use support::{Fixture, TemporaryDirectory};

const RENDER: &str = "rendered: bytes\n";

/// What git prints when it refuses a name.
const INVALID_PATH: &str = "nvalid path";

/// Asks git whether it would track a regular file at `name`: adds the
/// empty blob at that path in a scratch repository, with both protections
/// switched on, and returns whether git accepted it. The scratch
/// repository is never the fixture's, so nothing here dirties a workspace
/// a test goes on to run in.
fn git_accepts(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let repository = TemporaryDirectory::new("untrackable-names-oracle")?;
    let home = TemporaryDirectory::new("untrackable-names-oracle-home")?;
    support::git::run(
        repository.path(),
        home.path(),
        &["init", "--quiet"],
        "git init",
    )?;
    let blob = support::git::run(
        repository.path(),
        home.path(),
        &["hash-object", "-w", "/dev/null"],
        "git hash-object",
    )?;
    let output = support::git::run_allow_failure(
        repository.path(),
        home.path(),
        &[
            "-c",
            "core.protectNTFS=true",
            "-c",
            "core.protectHFS=true",
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{},{name}", blob.trim()),
        ],
    )?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(INVALID_PATH),
        "precondition: git must refuse {name:?} as an invalid path, not for another reason; \
         stderr was: {stderr:?}"
    );
    Ok(false)
}

/// A clean, committed workspace whose one skeleton claims each of `claims`.
fn workspace_claiming(claims: &[&str]) -> Result<WearingFixture, Box<dyn std::error::Error>> {
    let files: Vec<(&str, &str)> = claims.iter().map(|claim| (*claim, RENDER)).collect();
    let wearing = fixture_wearing(&[("claims-names", &files)])?;
    wearing.fixture.init_git_repository()?;
    Ok(wearing)
}

/// Runs `check --json` in `fixture` and returns its parsed document. It
/// makes no claim about the exit code, which is non-zero for a drifted
/// claim as well as for a refused one.
fn check_document(fixture: &Fixture) -> Result<Value, Box<dyn std::error::Error>> {
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_ne!(
        report.exit_code, 101,
        "check must not panic; stderr was: {}",
        report.stderr
    );
    support::json::parse(&report.stdout)
}

/// The claimed paths `check` refused as `unsafe-path`, sorted. Each refusal
/// is also asserted to name the skeleton `claims-names` and to say why.
fn refused_paths(document: &Value) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut paths = BTreeSet::new();
    for refusal in support::json::refusals(document)? {
        assert_eq!(
            support::json::refusal_kind(refusal)?,
            "unsafe-path",
            "a claim git refuses to track is refused as an unsafe path; refusal was: {refusal}"
        );
        assert_eq!(
            support::json::refusal_skeleton(refusal)?,
            Some("claims-names"),
            "the refusal must name the skeleton that claims the path; refusal was: {refusal}"
        );
        let path = support::json::refusal_path(refusal)?
            .ok_or_else(|| format!("an unsafe-path refusal names its path; was: {refusal}"))?;
        assert!(
            !support::json::refusal_message(refusal)?.is_empty(),
            "the refusal must say why; refusal was: {refusal}"
        );
        paths.insert(path.to_owned());
    }
    Ok(paths)
}

/// The claimed paths `check` reports as bones.
fn bone_paths(document: &Value) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut paths = BTreeSet::new();
    for row in support::json::bones(document)? {
        paths.insert(support::json::bone_path(row)?.to_owned());
    }
    Ok(paths)
}

/// Asserts git refuses `claim` and `check` refuses it too: one refusal,
/// for exactly that path, and no bone to write at it.
fn assert_check_refuses(claim: &str) -> support::TestOutcome {
    assert!(
        !git_accepts(claim)?,
        "precondition: git must refuse {claim:?}"
    );
    let wearing = workspace_claiming(&[claim])?;

    let document = check_document(&wearing.fixture)?;

    assert_eq!(
        refused_paths(&document)?,
        BTreeSet::from([claim.to_owned()]),
        "check must refuse the claim git refuses to track; document was: {document}"
    );
    assert!(
        bone_paths(&document)?.is_empty(),
        "a refused claim must not also be a bone to write; document was: {document}"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// One test per shape of name git refuses to track.
// ---------------------------------------------------------------------

#[test]
fn check_refuses_dotgit_followed_by_a_period() -> support::TestOutcome {
    // `.git.` is a synonym of `.git`: git's rule strips trailing periods
    // and spaces before comparing. `check` must refuse it, not report
    // `drifted (missing)` for a file that could never be committed.
    assert_check_refuses(".git.")
}

#[test]
fn check_refuses_dotgit_followed_by_a_space() -> support::TestOutcome {
    // `.git ` with a trailing space, the same synonym.
    assert_check_refuses(".git ")
}

#[test]
fn check_refuses_dotgit_followed_by_a_run_of_spaces_and_periods() -> support::TestOutcome {
    // `.git . .`: git allows any run of spaces and periods after the name,
    // in any order, not just one.
    assert_check_refuses(".git . .")
}

#[test]
fn check_refuses_the_short_name_git_tilde_one() -> support::TestOutcome {
    // `git~1` is the short form a filesystem gives `.git`, and git
    // refuses it as the same name.
    assert_check_refuses("git~1")
}

#[test]
fn check_refuses_the_short_name_in_capitals() -> support::TestOutcome {
    // `GIT~1`: git compares the short name without regard to case.
    assert_check_refuses("GIT~1")
}

#[test]
fn check_refuses_the_short_name_followed_by_a_space_and_a_period() -> support::TestOutcome {
    // `git~1 .`: the trailing run of spaces and periods applies to the
    // short name as well.
    assert_check_refuses("git~1 .")
}

#[test]
fn check_refuses_dotgit_followed_by_a_named_stream() -> support::TestOutcome {
    // `.git::$INDEX_ALLOCATION`: a colon ends the name for git's
    // purposes, and everything after it is a stream of `.git` itself.
    assert_check_refuses(".git::$INDEX_ALLOCATION")
}

#[test]
fn check_refuses_dotgit_followed_by_a_backslash() -> support::TestOutcome {
    // `.git\config`: git reads a backslash as a directory separator when
    // it applies its refusal rules, so this is a file inside `.git`.
    assert_check_refuses(".git\\config")
}

#[test]
fn check_refuses_dotgit_after_a_backslash() -> support::TestOutcome {
    // `sub\.git`: the same reading, with the name after the
    // separator.
    assert_check_refuses("sub\\.git")
}

#[test]
fn check_refuses_dotgit_with_an_ignorable_character_inside_it() -> support::TestOutcome {
    // `.g` ZERO WIDTH NON-JOINER `it`: git skips code points a filesystem
    // ignores, so this reads as `.git`. It looks like `.git` on screen
    // too.
    assert_check_refuses(".g\u{200c}it")
}

#[test]
fn check_refuses_dotgit_with_an_ignorable_character_before_the_dot() -> support::TestOutcome {
    // ZERO WIDTH JOINER, then `.git`: the skipped code points count before
    // the first character as well.
    assert_check_refuses("\u{200d}.git")
}

#[test]
fn check_refuses_dotgit_with_an_ignorable_character_after_it() -> support::TestOutcome {
    // `.git` then ZERO WIDTH NO-BREAK SPACE: and after the last.
    assert_check_refuses(".git\u{feff}")
}

#[test]
fn check_refuses_dotgit_in_capitals_with_an_ignorable_character() -> support::TestOutcome {
    // `.G` ZERO WIDTH NON-JOINER `IT`: the ASCII case rule and the
    // ignorable-character rule combine.
    assert_check_refuses(".G\u{200c}IT")
}

#[test]
fn check_refuses_every_ignorable_character_git_skips() -> support::TestOutcome {
    // All sixteen code points `utf8.c` lists, each placed between `.g` and
    // `it`, in one skeleton. Each must be refused as its own path, and none
    // may remain a bone. A list of only the well-known ones (U+200C,
    // U+200D, U+FEFF) misses the directional marks and the
    // symmetric-swapping controls.
    let ignorable = [
        '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
        '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}', '\u{feff}',
    ];
    let names: Vec<String> = ignorable
        .iter()
        .map(|character| format!(".g{character}it"))
        .collect();
    for name in &names {
        assert!(
            !git_accepts(name)?,
            "precondition: git must refuse {name:?}"
        );
    }
    let claims: Vec<&str> = names.iter().map(String::as_str).collect();
    let wearing = workspace_claiming(&claims)?;

    let document = check_document(&wearing.fixture)?;

    assert_eq!(
        refused_paths(&document)?,
        names.iter().cloned().collect::<BTreeSet<_>>(),
        "check must refuse every one of git's ignorable characters; document was: {document}"
    );
    assert!(
        bone_paths(&document)?.is_empty(),
        "a refused claim must not also be a bone to write; document was: {document}"
    );
    Ok(())
}

#[test]
fn check_refuses_a_directory_git_would_take_for_dotgit() -> support::TestOutcome {
    // The name is not the file but a directory above it:
    // `.git./hooks/pre-commit.yml`. Git checks every component of a path,
    // so a claim inside such a directory is refused as well.
    assert_check_refuses(".git./hooks/pre-commit.yml")
}

#[test]
fn check_refuses_a_short_name_directory() -> support::TestOutcome {
    // `git~1/config.yml`: the same, for the short name as a
    // directory.
    assert_check_refuses("git~1/config.yml")
}

#[test]
fn sync_refuses_a_name_git_refuses_to_track_and_creates_nothing() -> support::TestOutcome {
    // `sync` must not write the file either: it would be a file no commit
    // can ever record. It refuses, naming the claim, and creates nothing
    // under that name.
    let claim = ".git.";
    assert!(
        !git_accepts(claim)?,
        "precondition: git must refuse {claim:?}"
    );
    let wearing = workspace_claiming(&[claim])?;
    let fixture = &wearing.fixture;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &[claim]);
    assert!(
        std::fs::symlink_metadata(fixture.root().join(claim)).is_err(),
        "sync must not create {claim:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Guards: names git accepts, which a rule that is too wide would refuse, and
// `.GIT`, which git refuses and a rule that is too narrow would let through.
// ---------------------------------------------------------------------

#[test]
fn check_refuses_dotgit_in_capitals() -> support::TestOutcome {
    // Guard: `.GIT` is in git's set (`verify_dotfile` matches `.git` in any
    // case), so it must stay refused. This stops a rewrite of the rule from
    // losing that spelling.
    assert_check_refuses(".GIT")
}

#[test]
fn check_keeps_names_that_only_look_like_dotgit() -> support::TestOutcome {
    // Guard: `.github`, `.gitignore`, `git`, `.gitattributes`, `a.git`,
    // `gitx` and `.git-blame-ignore-revs` all start with, end with or
    // contain `git`, and git tracks every one of them. All seven must stay
    // valid claims: reported as bones to write, and none refused.
    let names = [
        ".github",
        ".gitignore",
        "git",
        ".gitattributes",
        "a.git",
        "gitx",
        ".git-blame-ignore-revs",
    ];
    for name in names {
        assert!(git_accepts(name)?, "precondition: git must accept {name:?}");
    }
    let wearing = workspace_claiming(&names)?;

    let document = check_document(&wearing.fixture)?;

    assert_eq!(
        refused_paths(&document)?,
        BTreeSet::new(),
        "check must refuse none of them; document was: {document}"
    );
    assert_eq!(
        bone_paths(&document)?,
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "check must report all of them as bones; document was: {document}"
    );
    Ok(())
}

#[test]
fn check_keeps_near_misses_git_accepts() -> support::TestOutcome {
    // Guard: the neighbours of the refused shapes that git takes:
    // `git~2` and `git~10` (only `~1` is the short name), `.git~1` (a dot
    // before the short name), `..git`, `.gitx`, `a:b` and `x\y` (a colon or
    // backslash only matters after `.git` or `git~1`), and `.g it` (a space
    // inside the name). A rule that refuses on any colon, backslash or
    // tilde would break them.
    let names = [
        "git~2", "git~10", ".git~1", "..git", ".gitx", "a:b", "x\\y", ".g it",
    ];
    for name in names {
        assert!(git_accepts(name)?, "precondition: git must accept {name:?}");
    }
    let wearing = workspace_claiming(&names)?;

    let document = check_document(&wearing.fixture)?;

    assert_eq!(
        refused_paths(&document)?,
        BTreeSet::new(),
        "check must refuse none of them; document was: {document}"
    );
    assert_eq!(
        bone_paths(&document)?,
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "check must report all of them as bones; document was: {document}"
    );
    Ok(())
}

#[test]
fn a_claim_whose_staging_name_reads_as_dotgit_is_still_written() -> support::TestOutcome {
    // Guard: `git:x.yml` and `git\x.yml` are names git tracks, but the
    // name `sync` stages them under, `.git:x.yml.skeletons-sync` and
    // `.git\x.yml.skeletons-sync`, begins `.git` and then a colon or a
    // backslash, which git would refuse. A staging file is never tracked,
    // so that harms nothing: `check` reports each claim as a bone to write
    // and refuses neither, and `sync` creates both and exits 0 without
    // panicking.
    let names = ["git:x.yml", "git\\x.yml"];
    for name in names {
        assert!(git_accepts(name)?, "precondition: git must accept {name:?}");
    }
    let wearing = workspace_claiming(&names)?;
    let fixture = &wearing.fixture;

    let document = check_document(fixture)?;

    assert_eq!(
        refused_paths(&document)?,
        BTreeSet::new(),
        "check must refuse neither; document was: {document}"
    );
    assert_eq!(
        bone_paths(&document)?,
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "check must report both as bones; document was: {document}"
    );
    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "sync must create both without panicking; stderr was: {}",
        report.stderr
    );
    for name in names {
        assert!(
            fixture.root().join(name).is_file(),
            "sync must create {name:?}"
        );
    }
    Ok(())
}
