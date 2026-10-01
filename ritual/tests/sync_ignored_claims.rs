//! Acceptance: `sync` refuses to create a claim that git would ignore.
//!
//! A file git ignores is one `git status` never shows and `git add` refuses
//! without `-f`. If `sync` created one, the wearer would hold a bone that no
//! commit ever records, and the next drift would find it untracked and
//! refuse to update it. `skeletons` does not serve one-shot files that are meant
//! to stay out of git, so `sync` says so before it writes anything: it
//! names the rule that ignores the claim, exactly where git's own
//! `check-ignore -v` finds it, and the two ways out.
//!
//! Each refusal test asserts what the refusal said (the claim, the rule and
//! its source, the remedy) and reads the disk back, never the exit code
//! alone. The guards at the end are the cases a refusal drawn too wide would
//! break: a sibling no rule ignores, a rule a later `!` pattern undoes, and
//! the remedy itself.
//!
//! Platforms: every test here runs on macOS and Linux. Nothing in them
//! depends on the filesystem's case or normalisation rules.

mod support;

use support::Fixture;
use support::sync::{WearingFixture, assert_refused_naming, fixture_wearing};

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

/// The words that name the two ways out of a refused ignored claim: git's
/// own forced add, and taking the rule away.
const FORCE_ADD: &[&str] = &["git add -f", "git add --force"];
const REMOVE_THE_RULE: &[&str] = &[
    "remove the ignore rule",
    "remove the rule",
    "remove that rule",
    "remove the ignore",
];

/// Asserts git itself ignores `path` (`check-ignore` exits 0), so a test
/// whose story is "git ignores this" is not resting on a wrong pattern.
fn assert_git_ignores(fixture: &Fixture, path: &str) -> support::TestOutcome {
    let output = support::git::run_allow_failure(
        fixture.root(),
        fixture.sandbox().home(),
        &["check-ignore", "--no-index", "--quiet", "--", path],
    )?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "precondition: git must ignore {path}"
    );
    Ok(())
}

/// Asserts git does not ignore `path` (`check-ignore` exits 1).
fn assert_git_does_not_ignore(fixture: &Fixture, path: &str) -> support::TestOutcome {
    let output = support::git::run_allow_failure(
        fixture.root(),
        fixture.sandbox().home(),
        &["check-ignore", "--no-index", "--quiet", "--", path],
    )?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "precondition: git must not ignore {path}"
    );
    Ok(())
}

/// Asserts the refusal text says what an ignored claim needs told: the
/// claim, the rule's source and pattern as `check-ignore -v` reports them,
/// and both remedies. `sync` must also have created nothing at the claim.
fn assert_refused_as_ignored(
    fixture: &Fixture,
    report: &support::Report,
    claim: &str,
    source: &str,
    pattern: &str,
) {
    assert_refused_naming(report, &[claim, source, pattern]);
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        mentions_any(&combined, FORCE_ADD),
        "the refusal must name `git add -f` as a way out; combined output was: {combined:?}"
    );
    assert!(
        mentions_any(&combined, REMOVE_THE_RULE),
        "the refusal must name removing the ignore rule as a way out; combined output was: \
         {combined:?}"
    );
    assert!(
        !combined.contains("created "),
        "sync must not report creating a file it refused; combined output was: {combined:?}"
    );
    assert!(
        std::fs::symlink_metadata(fixture.root().join(claim)).is_err(),
        "sync must not create the ignored claim {claim}"
    );
}

/// A clean, committed workspace whose skeleton claims `claims`, with
/// `cfg/.gitignore` holding two lines: a comment, then `*.local.yml`, so
/// the rule git reports is the second line of a nested file, not the
/// repository's root `.gitignore`.
fn workspace_ignoring_local_yml(
    claims: &[(&str, &str)],
) -> Result<WearingFixture, Box<dyn std::error::Error>> {
    let wearing = fixture_wearing(&[("claims-cfg", claims)])?;
    wearing.fixture.write(
        "cfg/.gitignore",
        b"# machine-local settings stay out of git\n*.local.yml\n",
    )?;
    wearing.fixture.init_git_repository()?;
    Ok(wearing)
}

#[test]
fn a_claim_a_gitignore_rule_ignores_is_refused_naming_the_rule_and_the_way_out()
-> support::TestOutcome {
    // The skeleton claims `cfg/editor.local.yml`, and `cfg/.gitignore` says
    // `*.local.yml`. Creating it would leave a file `git status` never shows
    // and `git add` refuses. `sync` must refuse before writing, naming the
    // claim, the rule's source (`cfg/.gitignore`) and pattern (`*.local.yml`)
    // as `git check-ignore -v` reports them, and both ways out: remove the
    // ignore rule, or create the file by hand and `git add -f` it. Nothing
    // may be created.
    //
    // What this proves: the refusal for a rule in a tracked `.gitignore`.
    // Platforms: macOS and Linux.
    let wearing = workspace_ignoring_local_yml(&[("cfg/editor.local.yml", RENDER)])?;
    let fixture = &wearing.fixture;
    assert_git_ignores(fixture, "cfg/editor.local.yml")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_as_ignored(
        fixture,
        &report,
        "cfg/editor.local.yml",
        "cfg/.gitignore",
        "*.local.yml",
    );
    Ok(())
}

#[test]
fn refusing_a_file_sync_would_create_claims_nothing_is_replaced() -> support::TestOutcome {
    // The only refusal is an ignored claim, a file `sync` would create where
    // nothing is. Its closing summary must speak of what `sync` would create,
    // and must not say that git holds "what would be replaced", since nothing
    // at all would be.
    //
    // What this proves: the summary is worded by what the refused paths are.
    // Platforms: macOS and Linux.
    let wearing = workspace_ignoring_local_yml(&[("cfg/editor.local.yml", RENDER)])?;
    let fixture = &wearing.fixture;
    assert_git_ignores(fixture, "cfg/editor.local.yml")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["cfg/editor.local.yml"]);
    assert!(
        report.stderr.contains(
            "sync cannot show that git would see the 1 file it would create, so it wrote \
             nothing: the line above says why"
        ),
        "the summary must speak of the file sync would create; stderr was: {}",
        report.stderr
    );
    assert!(
        !report.stderr.contains("replace"),
        "nothing would be replaced, so the summary must not say so; stderr was: {}",
        report.stderr
    );
    Ok(())
}

const HELD_BY_GIT: &[u8] = b"old\n";
const ONLY_COPY: &[u8] = b"old\nlocal: only-copy-of-this-line\n";

/// Commits `tracked.yml` holding `HELD_BY_GIT` under a clean filter that
/// strips `local:` lines, then stages `ONLY_COPY` over it: the work tree holds
/// a line the filter drops, and git still calls the tree clean.
fn stage_line_git_never_stores(fixture: &Fixture) -> support::TestOutcome {
    git(
        fixture,
        &["config", "filter.strip.clean", "sed '/^local:/d'"],
    )?;
    git(fixture, &["config", "filter.strip.smudge", "cat"])?;
    fixture.write(".gitattributes", b"tracked.yml filter=strip\n")?;
    fixture.write("tracked.yml", HELD_BY_GIT)?;
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    fixture.write("tracked.yml", ONLY_COPY)?;
    git(fixture, &["add", "tracked.yml"])?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git must call this work tree clean, or the refusal of tracked.yml \
         could come from the whole-tree rule"
    );
    Ok(())
}

#[test]
fn refusing_one_replacement_and_one_creation_names_each_in_the_summary() -> support::TestOutcome {
    // Two claims are refused for different reasons, and a third is fine.
    // `tracked.yml` is tracked under a clean filter that strips a line
    // present on disk, so `sync` cannot show git holds what it would replace;
    // `cfg/editor.local.yml` is ignored, so `sync` cannot show git would see
    // the file it would create; `cfg/settings.yml` would be created and
    // nothing stops it. The summary must count one of each kind, and count
    // only the refused paths: nothing is written, the proven claim included.
    //
    // What this proves: the summary's wording follows each refused path's
    // own kind through the whole run, in both directions at once.
    // Platforms: macOS and Linux; the filter is `sed`.
    let wearing = workspace_ignoring_local_yml(&[
        ("tracked.yml", RENDER),
        ("cfg/editor.local.yml", RENDER),
        ("cfg/settings.yml", RENDER),
    ])?;
    let fixture = &wearing.fixture;
    stage_line_git_never_stores(fixture)?;
    assert_git_ignores(fixture, "cfg/editor.local.yml")?;
    assert_git_does_not_ignore(fixture, "cfg/settings.yml")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_naming(&report, &["tracked.yml"]);
    assert_refused_as_ignored(
        fixture,
        &report,
        "cfg/editor.local.yml",
        "cfg/.gitignore",
        "*.local.yml",
    );
    assert!(
        report.stderr.contains(
            "sync cannot show that git holds what 1 file would replace, or that it would see \
             the 1 file it would create, so it wrote nothing: the lines above say why"
        ),
        "the summary must name one replacement and one creation; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        fixture.read("tracked.yml")?,
        ONLY_COPY,
        "a line git never stored must survive a refused sync"
    );
    assert!(
        std::fs::symlink_metadata(fixture.root().join("cfg/settings.yml")).is_err(),
        "a refused sync creates nothing, not even the claim it could prove"
    );
    Ok(())
}

#[test]
fn a_claim_the_local_exclude_file_ignores_is_refused_naming_that_file() -> support::TestOutcome {
    // The rule is in `.git/info/exclude`, which no commit carries, so the
    // wearer's own machine ignores the claim and a checkout elsewhere does
    // not. The refusal must name the rule where `check-ignore -v` finds it:
    // `.git/info/exclude` and the pattern `envrc.local`.
    //
    // What this proves: the refusal does not depend on the rule living in a
    // `.gitignore` file. Platforms: macOS and Linux.
    let wearing = fixture_wearing(&[("claims-envrc", &[("envrc.local", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.init_git_repository()?;
    support::git::exclude_locally(fixture.root(), "envrc.local")?;
    assert_git_ignores(fixture, "envrc.local")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_refused_as_ignored(
        fixture,
        &report,
        "envrc.local",
        ".git/info/exclude",
        "envrc.local",
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Guards: ordinary cases that a refusal drawn too wide would also catch.
// ---------------------------------------------------------------------

#[test]
fn a_claim_beside_an_ignored_pattern_that_no_rule_ignores_is_still_created() -> support::TestOutcome
{
    // Guard: the same workspace and the same `cfg/.gitignore` rule
    // (`*.local.yml`), with a claim `cfg/plain.yml` in the same directory
    // that the rule does not match. A refusal keyed on "a `.gitignore` is
    // nearby" or "the directory has rules" would refuse it; only a claim
    // git itself ignores may be refused.
    //
    // What this proves: sync consults git for the claim, not for the
    // directory. Platforms: macOS and Linux.
    let wearing = workspace_ignoring_local_yml(&[("cfg/plain.yml", RENDER)])?;
    let fixture = &wearing.fixture;
    assert_git_does_not_ignore(fixture, "cfg/plain.yml")?;
    assert_git_ignores(fixture, "cfg/other.local.yml")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a claim no rule ignores must be created; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("cfg/plain.yml")?, RENDER.as_bytes());
    Ok(())
}

#[test]
fn a_claim_a_later_negated_rule_un_ignores_is_still_created() -> support::TestOutcome {
    // Guard: `cfg/.gitignore` says `*.local.yml` and then `!keep.local.yml`,
    // so git does not ignore `cfg/keep.local.yml` although an earlier rule
    // matches it, while `cfg/other.local.yml` stays ignored. A refusal that
    // stops at the first matching pattern would refuse the claim; git's
    // last-match-wins answer says it is not ignored, and `sync` must
    // create it.
    //
    // What this proves: sync follows git's verdict, negation included,
    // rather than reading patterns itself. Platforms: macOS and Linux.
    let wearing = fixture_wearing(&[("claims-keep", &[("cfg/keep.local.yml", RENDER)])])?;
    let fixture = &wearing.fixture;
    fixture.write("cfg/.gitignore", b"*.local.yml\n!keep.local.yml\n")?;
    fixture.init_git_repository()?;
    assert_git_does_not_ignore(fixture, "cfg/keep.local.yml")?;
    assert_git_ignores(fixture, "cfg/other.local.yml")?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a claim a `!` rule un-ignores must be created; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("cfg/keep.local.yml")?, RENDER.as_bytes());
    Ok(())
}

#[test]
fn a_claim_added_with_force_despite_an_ignore_rule_is_still_updated() -> support::TestOutcome {
    // Guard: the refusal's own remedy must lead somewhere. The claim
    // `cfg/editor.local.yml` is ignored by `cfg/.gitignore`, but the wearer
    // created it by hand and `git add -f`ed it, so git tracks it. It then
    // drifts from the render. Git sees a tracked file whatever the rule
    // says, so `sync` must update it and not refuse it as ignored.
    //
    // What this proves: the `git add -f` remedy the refusal names actually
    // ends the refusal. Platforms: macOS and Linux.
    let wearing = workspace_ignoring_local_yml(&[("cfg/editor.local.yml", RENDER)])?;
    let fixture = &wearing.fixture;
    fixture.write("cfg/editor.local.yml", b"committed: bytes\n")?;
    git(fixture, &["add", "-f", "--", "cfg/editor.local.yml"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: forced add"],
    )?;
    assert_git_ignores(fixture, "cfg/editor.local.yml")?;
    assert!(
        git(fixture, &["ls-files", "--", "cfg/editor.local.yml"])?.contains("cfg/editor.local.yml"),
        "precondition: git tracks the forced file"
    );

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a tracked file must be updated whatever a rule says; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("cfg/editor.local.yml")?, RENDER.as_bytes());
    Ok(())
}
