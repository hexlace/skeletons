//! Acceptance: an unreachable git remote's `undetermined` detail is git's
//! own *first* `fatal:` line, not its last stderr line.
//!
//! `git ls-remote` against a `file://` URL that no longer exists at all
//! prints a multi-line `fatal:` message: the first line names the actual
//! problem (the path git could not find), and the last is generic
//! boilerplate ("Please make sure you have the correct access rights / and
//! the repository exists.") that names nothing about the specific failure.
//! Reading the *last* non-blank line would show a wearer the boilerplate
//! tail instead of the line that actually tells them what went wrong, so
//! `behind`'s git remote queries quote `git::diagnostic`'s answer
//! (`crates/skeletons/src/git/diagnostic.rs`) instead.

mod support;

use support::git::SkeletonRepository;
use support::{Fixture, wearing_table, write_package_manifest};

/// Builds a fixture whose dependency `dependency_key` is locked against
/// `repository` at its tag `1.0.0`, then deletes the repository outright,
/// returning the fixture together with the raw filesystem path git's own
/// `fatal:` message names (`repository`'s `file://` URL with the scheme
/// stripped, which is exactly how git quotes a local path it could not
/// find) — captured before `remove` consumes the repository.
fn fixture_locked_then_remote_removed(
    dependency_key: &str,
    repository: SkeletonRepository,
) -> Result<(Fixture, String), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{dependency_key} = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table(dependency_key, ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"v1\n")?;

    let file_url = repository.file_url();
    let raw_path = file_url
        .strip_prefix("file://")
        .ok_or("SkeletonRepository::file_url must always return a file:// URL")?
        .to_owned();
    // Gone entirely: any further attempt to reach it fails loudly, with
    // git's own real multi-line "fatal:" message — nothing here fabricates
    // or mocks that message, it is git's real stderr.
    repository.remove()?;
    Ok((fixture, raw_path))
}

/// The generic boilerplate line git's own multi-line `fatal:` message ends
/// with — the wrong line to report, present in every one of git's "not a
/// repository" messages regardless of which real problem caused it, so it
/// can never actually tell a wearer what went wrong.
const GENERIC_TAIL_TEXT: &str = "and the repository exists";

#[test]
fn a_tag_pins_unreachable_detail_is_gits_first_fatal_line_not_its_last() -> support::TestOutcome {
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("detail-tag-unreachable", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;
    let (fixture, raw_path) =
        fixture_locked_then_remote_removed("detail-tag-unreachable", repository)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("unreachable")
    );

    let detail = support::json::behind_detail(&bones[0])?
        .ok_or("an unreachable remote must carry a behind.detail")?;

    // The positive fact: git's own first fatal line, naming the actual
    // path it could not find, without the `fatal: ` prefix `skeletons` strips.
    let first_fatal_line = format!("'{raw_path}' does not appear to be a git repository");
    assert!(
        detail.contains(&first_fatal_line),
        "the detail must contain git's own first `fatal:` line, naming the path that could not \
         be found, with the `fatal: ` prefix stripped; detail was: {detail:?}"
    );
    // The negative fact: not the generic tail line, which reading git's
    // last stderr line would report instead.
    assert!(
        !detail.contains(GENERIC_TAIL_TEXT),
        "the detail must not be git's own last stderr line, which names nothing about the \
         actual failure; detail was: {detail:?}"
    );
    Ok(())
}

#[test]
fn a_branch_pins_unreachable_detail_is_gits_first_fatal_line_not_its_last() -> support::TestOutcome
{
    // The same shape as the tag case, for the branch-head query: every
    // remote query reads its detail the same way, not just the tag one.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("detail-branch-unreachable", "thing.txt", "v1\n", "v1")?;
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ndetail-branch-unreachable = {{ git = \"{}\", branch = \"main\" }}\n\n{}",
        repository.file_url(),
        wearing_table("detail-branch-unreachable", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"v1\n")?;

    let file_url = repository.file_url();
    let raw_path = file_url
        .strip_prefix("file://")
        .ok_or("SkeletonRepository::file_url must always return a file:// URL")?
        .to_owned();
    repository.remove()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("unreachable")
    );

    let detail = support::json::behind_detail(&bones[0])?
        .ok_or("an unreachable remote must carry a behind.detail")?;
    let first_fatal_line = format!("'{raw_path}' does not appear to be a git repository");
    assert!(
        detail.contains(&first_fatal_line),
        "the detail must contain git's own first `fatal:` line; detail was: {detail:?}"
    );
    assert!(
        !detail.contains(GENERIC_TAIL_TEXT),
        "the detail must not be git's own last stderr line; detail was: {detail:?}"
    );
    Ok(())
}
