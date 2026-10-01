//! Acceptance: whether a worn skeleton is behind, for every pin kind that does
//! not require a registry this suite would have to stand up itself:
//! `tag =`, `branch =`, `rev =`, a local `path =`, and an unqualified
//! `git =` — including what a tag pin and an unqualified `git =` pin read
//! when the remote they would ask is gone entirely.
//!
//! Every skeleton here is synthesized fresh into a real git repository via
//! `support::git::SkeletonRepository`, reached only over a `file://` URL, so
//! determining whether it is behind — which needs the remote to actually
//! be asked — still makes no real network request.
//!
//! A registry-pinned skeleton's own `behind` answer, a skeleton from a registry
//! other than the default one, and a crates.io network that cannot be
//! reached are covered separately, in `check_behind_registry.rs`, through
//! the captured-index seam that infrastructure needs.

mod support;

use support::git::SkeletonRepository;
use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::{Fixture, wearing_table, write_package_manifest};

#[test]
fn a_tag_pin_reports_current_when_no_newer_tag_exists() -> support::TestOutcome {
    // Locked at the only tag that exists: nothing is newer, so the row
    // must read current, and the file (byte-identical) must match.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("tag-current", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ntag-current = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("tag-current", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"v1\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "current");
    Ok(())
}

#[test]
fn a_tag_pin_reports_behind_when_a_newer_tag_exists_on_the_remote() -> support::TestOutcome {
    // A second, newer tag appears on the remote only after the lock:
    // `check` never re-locks on its own, so seeing it at all means it
    // asked the remote.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("tag-behind", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ntag-behind = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("tag-behind", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("tag-behind", "thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    fixture.write("thing.txt", b"v1\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    Ok(())
}

#[test]
fn a_branch_pin_reports_behind_when_the_remote_branch_head_moves_past_the_locked_commit()
-> support::TestOutcome {
    // Locked against `feature`'s tip; a further commit lands on `feature`
    // on the remote afterward. `check` must notice the branch's own head
    // moved, without ever re-locking.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("branch-behind", "thing.txt", "v1\n", "v1")?;
    repository.branch_from_head("feature")?;
    repository.commit_skeleton("branch-behind", "thing.txt", "v2\n", "v2 on feature")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nbranch-behind = {{ git = \"{}\", branch = \"feature\" }}\n\n{}",
        repository.file_url(),
        wearing_table("branch-behind", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("branch-behind", "thing.txt", "v3\n", "v3 on feature")?;

    fixture.write("thing.txt", b"v2\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    Ok(())
}

#[test]
fn a_rev_pin_never_reports_behind_even_with_newer_commits_on_the_remote() -> support::TestOutcome {
    // A fixed commit was chosen on purpose: newer commits landing on the
    // remote afterward must never make this row read behind. It reads
    // pinned instead.
    let repository = SkeletonRepository::new()?;
    let locked_commit = repository.commit_skeleton("rev-pinned", "thing.txt", "v1\n", "v1")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nrev-pinned = {{ git = \"{}\", rev = \"{locked_commit}\" }}\n\n{}",
        repository.file_url(),
        wearing_table("rev-pinned", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("rev-pinned", "thing.txt", "v2\n", "v2")?;
    repository.tag("1.1.0")?;

    fixture.write("thing.txt", b"v1\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "pinned");
    Ok(())
}

#[test]
fn a_local_path_dependency_never_reports_behind_and_reads_pinned() -> support::TestOutcome {
    // The same rule as `rev =`, for the other pin kind that has no
    // "newer" to compare against: a `path =` dependency.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "pinned");
    Ok(())
}

#[test]
fn a_registry_dependency_patched_to_a_path_reads_pinned_and_names_the_patched_directory()
-> support::TestOutcome {
    // `patched-skeleton` is declared as a plain registry dependency
    // (`patched-skeleton = "1.0.0"`), then redirected to a local path
    // entirely through `[patch.crates-io]` — so the wearer's own declared
    // dependency carries no `path` at all; only the resolved package's own
    // manifest does. The pin must still read `path`, pinned, and its own
    // displayed directory must name the patched skeleton's real directory —
    // proved by canonicalizing both sides, since the display rule may
    // render it as a relative path with leading `..` components — never a
    // guess built from the dependency's declared name, which would name a
    // directory that does not exist at all.
    let fixture = Fixture::new()?;
    // `write_minimal_skeleton` (below) always writes version `0.0.0`, so the
    // vendored placeholder and the declared requirement both name that
    // same version: a patch only takes effect when its own version
    // satisfies the dependency's declared requirement.
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "patched-skeleton",
        "0.0.0",
        "unused.txt",
        "unused\n",
    )?;
    let patched_skeleton = support::TemporaryDirectory::new("patched-skeleton")?;
    support::write_minimal_skeleton(
        patched_skeleton.path(),
        "patched-skeleton",
        "claimed.txt",
        "patched\n",
    )?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n[patch.crates-io]\n{}",
        support::registry_dependency("patched-skeleton", "0.0.0"),
        wearing_table("patched-skeleton", ""),
        support::path_dependency_on("patched-skeleton", patched_skeleton.path()),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("claimed.txt", b"patched\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::pin_kind(&bones[0])?, "path");
    assert_eq!(support::json::behind_state(&bones[0])?, "pinned");

    let shown_path = support::json::pin_path(&bones[0])?;
    let resolved_directory = fixture.root().join(shown_path).canonicalize()?;
    let patched_skeleton_directory = patched_skeleton.path().canonicalize()?;
    assert_eq!(
        resolved_directory, patched_skeleton_directory,
        "pin.path must name the patched skeleton's own real directory"
    );
    Ok(())
}

#[test]
fn an_unqualified_git_dependency_reports_behind_the_same_way_a_branch_pin_would()
-> support::TestOutcome {
    // No `tag`/`branch`/`rev` at all: Cargo itself follows the remote's
    // default branch, and this is judged by the same rule as an explicit
    // `branch =` pin — the remote's own head, moving past the locked commit,
    // is what makes it behind.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("unqualified-git-behind", "thing.txt", "v1\n", "v1")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nunqualified-git-behind = {{ git = \"{}\" }}\n\n{}",
        repository.file_url(),
        wearing_table("unqualified-git-behind", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    repository.commit_skeleton("unqualified-git-behind", "thing.txt", "v2\n", "v2")?;

    fixture.write("thing.txt", b"v1\n")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "behind");
    Ok(())
}

#[test]
fn a_tag_pin_reads_undetermined_when_the_remote_is_unreachable_and_the_default_exit_is_unchanged()
-> support::TestOutcome {
    // Locks against a tag, then the remote repository itself disappears
    // before `check` runs. An attempt to reach it for a `behind` answer
    // must fail loudly, and that failure must read as undetermined, reason
    // `unreachable` — never as "not behind" — while the claimed file still
    // matches, so the default (no `--fail-behind`) exit status must stay 0:
    // an undetermined behind answer never fails the command on its own.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("tag-unreachable", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ntag-unreachable = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("tag-unreachable", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"v1\n")?;

    // The remote is now gone entirely: any further attempt to reach it, for
    // a `behind` answer or anything else, fails loudly — there is nothing
    // left at its `file://` URL to answer.
    repository.remove()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "an unreachable remote must not change the default exit status when nothing has \
         drifted; stderr was: {}",
        report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("unreachable"),
        "an unreachable remote must never be reported as though it were simply not behind"
    );
    Ok(())
}

#[test]
fn a_bare_git_pin_reads_undetermined_when_unreachable_and_keeps_the_default_exit()
-> support::TestOutcome {
    // The same shape as the tag-pin case above, for an unqualified `git =`
    // dependency (judged against the remote's own default branch): once the
    // remote is gone, the row must still read undetermined, reason
    // `unreachable`, and the default exit status must stay 0.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("git-unreachable", "thing.txt", "v1\n", "v1")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ngit-unreachable = {{ git = \"{}\" }}\n\n{}",
        repository.file_url(),
        wearing_table("git-unreachable", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"v1\n")?;

    repository.remove()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "an unreachable remote must not change the default exit status when nothing has \
         drifted; stderr was: {}",
        report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::behind_state(&bones[0])?, "undetermined");
    assert_eq!(
        support::json::behind_reason(&bones[0])?,
        Some("unreachable"),
        "an unreachable remote must never be reported as though it were simply not behind"
    );
    Ok(())
}
