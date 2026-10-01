//! Acceptance: `behind` for a skeleton pinned by `tag =` inside a git
//! repository that holds more than one skeleton crate, each in its own
//! directory.
//!
//! This file proves, against a real repository (reached only over
//! `file://`, never the network), the rule that a tag-pinned skeleton
//! reads `<skeleton>-v<version>` tags — `<skeleton>` being the skeleton's
//! own package name — whenever the remote holds any tag of that shape for
//! it, whatever the repository's crate count. It falls back to plain
//! `v<version>`/`<version>` tags only when the remote holds none of the
//! prefixed shape for that skeleton.

mod support;

use support::git::SkeletonRepository;
use support::{Fixture, wearing_table, write_package_manifest};

/// A fixture wearing `crate-a` from `repository`, pinned by `pin_toml` (the
/// `tag = "…"` / `branch = "…"` / nothing at all fragment inside the
/// dependency's own `{ … }` table), with its manifest and lockfile written
/// and `thing-a.txt` written matching `"v1\n"` — the content
/// `repository`'s own initial `crate-a` commit carries, so `check` never
/// reports a claimed-file refusal or drift muddying a `behind`-only
/// assertion.
fn fixture_wearing_crate_a(
    repository: &SkeletonRepository,
    pin_toml: &str,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\ncrate-a = {{ git = \"{}\"{pin_toml} }}\n\n{}",
        repository.file_url(),
        wearing_table("crate-a", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing-a.txt", b"v1\n")?;
    Ok(fixture)
}

/// The one bone `check --json` reports for a fixture wearing only
/// `crate-a`, asserting there is exactly one.
fn only_bone(
    document: &serde_json::Value,
) -> Result<&serde_json::Value, Box<dyn std::error::Error>> {
    let bones = support::json::bones(document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    Ok(&bones[0])
}

#[test]
fn a_tag_pin_reads_behind_naming_the_newer_prefixed_tag_for_this_skeleton() -> support::TestOutcome
{
    // `crate-a-v0.1.0` is this crate's only tag at lock time; `crate-a-v0.2.0`
    // appears afterward. The pin must read behind, and name that exact
    // prefixed tag — not a bare version, and not any of `crate-b`'s own
    // tags, which this repository also carries: `crate-b-v0.9.0` is the
    // highest version of any tag here, so a reader that took it for this
    // skeleton's would name it instead.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;
    repository.tag("crate-a-v0.1.0")?;
    repository.tag("crate-b-v0.1.0")?;

    let fixture = fixture_wearing_crate_a(&repository, ", tag = \"crate-a-v0.1.0\"")?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "release crate-a v0.2.0",
    )?;
    repository.tag("crate-a-v0.2.0")?;
    repository.tag("crate-b-v0.9.0")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(support::json::behind_state(bone)?, "behind");
    assert_eq!(support::json::behind_newer_tag(bone)?, "crate-a-v0.2.0");
    Ok(())
}

#[test]
fn a_newer_tag_naming_only_the_other_crate_does_not_make_this_one_behind() -> support::TestOutcome {
    // A second, newer tag lands on the remote — but it is `crate-b`'s own
    // release, `crate-b-v0.9.0`, never `crate-a`'s. `crate-a`'s own pin
    // must keep reading current: a tag naming a sibling crate in the same
    // repository is not evidence this skeleton has anything newer.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;
    repository.tag("crate-a-v0.1.0")?;

    let fixture = fixture_wearing_crate_a(&repository, ", tag = \"crate-a-v0.1.0\"")?;

    repository.tag("crate-b-v0.9.0")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(
        support::json::behind_state(bone)?,
        "current",
        "a newer tag naming a different crate in the same repository must not read as this \
         skeleton being behind"
    );
    Ok(())
}

#[test]
fn a_tag_pin_falls_back_to_whole_repository_tags_when_none_are_prefixed_for_this_skeleton()
-> support::TestOutcome {
    // This repository carries no `crate-a-v*` tag at all, ever — only plain
    // `v0.1.0`/`v0.2.0`, the shape a repository tags itself as a whole with.
    // With no prefixed shape to prefer, the pin must fall back to reading
    // those directly, exactly as a single-crate repository already does.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;
    repository.tag("v0.1.0")?;

    let fixture = fixture_wearing_crate_a(&repository, ", tag = \"v0.1.0\"")?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "release v0.2.0",
    )?;
    repository.tag("v0.2.0")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(support::json::behind_state(bone)?, "behind");
    assert_eq!(support::json::behind_newer_tag(bone)?, "v0.2.0");
    Ok(())
}

#[test]
fn a_many_crate_remote_with_only_this_skeletons_tags_plain_falls_back_to_plain()
-> support::TestOutcome {
    // The edge of the tag rule that none of the other tests reaches: a
    // many-crate repository where the *other* crate uses the prefixed
    // shape (`crate-b-v…`) but this skeleton's own history only ever
    // carries plain tags. `release_tags` decides the scheme per skeleton,
    // by whether *this* package has any prefixed tag — a sibling's
    // prefixed tags must not pull this pin into the prefixed scheme (which
    // would then find none of its own and read undetermined), nor should
    // they be mistaken for this skeleton's own newer release.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton_at("crate-a", "crate-a", "thing-a.txt", "v1\n", "add crate-a")?;
    repository.commit_skeleton_at("crate-b", "crate-b", "thing-b.txt", "v1\n", "add crate-b")?;
    repository.tag("v0.1.0")?;
    repository.tag("crate-b-v0.1.0")?;

    let fixture = fixture_wearing_crate_a(&repository, ", tag = \"v0.1.0\"")?;

    repository.commit_skeleton_at(
        "crate-a",
        "crate-a",
        "thing-a.txt",
        "v2\n",
        "release v0.2.0",
    )?;
    repository.tag("v0.2.0")?;
    repository.tag("crate-b-v0.2.0")?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let document = support::json::parse(&report.stdout)?;
    let bone = only_bone(&document)?;
    assert_eq!(
        support::json::behind_state(bone)?,
        "behind",
        "crate-a has no prefixed tag of its own, so it must fall back to the plain tags rather \
         than being pulled into crate-b's prefixed scheme"
    );
    assert_eq!(support::json::behind_newer_tag(bone)?, "v0.2.0");
    Ok(())
}
