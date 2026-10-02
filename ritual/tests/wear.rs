//! Acceptance: `cargo ritual skeletons wear <crate>[@<version>] [<key>]`
//! adopts a skeleton in one command, from every kind of source Cargo knows.
//!
//! Each scenario runs the real `ritual` binary in a disposable workspace whose
//! package is `skeletons-ritual`, the package that binary is built from, and
//! checks the same story end to end: the manifest gains the skeleton as a
//! dev-dependency under its key and an empty wearing table named for that key;
//! then `sync` (after a commit, as `sync` needs a clean tree) writes the
//! skeleton's files and `check` reports them matching. That last half is what
//! shows the two pieces `wear` wrote are the right two pieces: a table named
//! for the wrong key would leave `sync` with nothing to write.
//!
//! The registry case never reaches a real registry. It is Cargo's own source
//! replacement, in the sandbox's isolated `CARGO_HOME`, onto a vendored
//! directory source, with `CARGO_NET_OFFLINE` set as a guarantee, so `wear`
//! runs the registry code path with no source flag at all. `check`'s own
//! crates.io query is answered from the captured index, as in the `behind`
//! scenarios.

mod support;

use support::git::SkeletonRepository;
use support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER;
use support::wear::{
    assert_worn_as_a_dev_dependency, combined_output, commit_everything, fixture_for_wearing,
};
use support::{Fixture, TestOutcome, checked_in_test_skeleton};

/// Runs `sync` then `check` in `fixture`, after committing what `wear`
/// wrote, and asserts the claimed file `relative` holds `expected` and that
/// `check` agrees it matches.
fn assert_sync_writes_and_check_matches(
    fixture: &Fixture,
    relative: &str,
    expected: &[u8],
    extra_env: &[(&str, &str)],
) -> TestOutcome {
    commit_everything(fixture, "fixture: wear")?;

    let sync_report = fixture.run_with_env(&["skeletons", "sync"], extra_env)?;
    assert_eq!(
        sync_report.exit_code, 0,
        "sync must succeed on what wear wrote; stderr was: {}",
        sync_report.stderr
    );
    assert_eq!(
        fixture.read(relative)?,
        expected,
        "sync must write the skeleton's file"
    );

    let check_report = fixture.run_with_env(&["skeletons", "check"], extra_env)?;
    assert_eq!(
        check_report.exit_code, 0,
        "check must report the written file matching; stdout: {}; stderr: {}",
        check_report.stdout, check_report.stderr
    );
    Ok(())
}

#[test]
fn wearing_from_a_path_adds_the_dependency_and_an_empty_table_then_sync_and_check_agree()
-> TestOutcome {
    // The checked-in `passthrough-plain` skeleton, taken by `--path`, which
    // Cargo passes through. The crate name is the key, so the wearing table
    // is named for it.
    let fixture = fixture_for_wearing("")?;
    let skeleton_path = checked_in_test_skeleton("passthrough-plain");
    let skeleton_path = skeleton_path.to_str().ok_or("path must be UTF-8")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "--path",
        skeleton_path,
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    assert!(
        combined_output(&report).contains("sync"),
        "wear must say to run sync next; output was: {}",
        combined_output(&report)
    );
    assert_eq!(
        report.stdout,
        "added passthrough-plain 0.0.0 to Cargo.toml as the dev-dependency `passthrough-plain`, \
         with an empty [package.metadata.skeletons.passthrough-plain] table\n\
         commit Cargo.toml and Cargo.lock, then run the `sync` task to write its files\n",
        "wear must report what it wrote and what to run next, on stdout, as two lines"
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "passthrough-plain", "passthrough-plain")?;
    assert_sync_writes_and_check_matches(&fixture, "plain.yml", PASSTHROUGH_PLAIN_RENDER, &[])
}

#[test]
fn wearing_from_a_git_tag_pins_that_tag_then_sync_and_check_agree() -> TestOutcome {
    // A skeleton in a local repository reached over `file://`. `--tag` passes
    // through, so the dependency line names the tag, and `sync` writes the
    // file as it is at that tag, not as it is on the branch tip afterwards.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("tagged-skeleton", "thing.txt", "at the tag\n", "v1")?;
    repository.tag("1.0.0")?;
    repository.commit_skeleton("tagged-skeleton", "thing.txt", "after the tag\n", "v2")?;
    let fixture = fixture_for_wearing("")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "tagged-skeleton",
        "--git",
        &repository.file_url(),
        "--tag",
        "1.0.0",
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "tagged-skeleton", "tagged-skeleton")?;
    assert!(
        manifest.contains("tag = \"1.0.0\""),
        "the dependency must pin the tag; manifest was:\n{manifest}"
    );
    assert_sync_writes_and_check_matches(&fixture, "thing.txt", b"at the tag\n", &[])
}

#[test]
fn wearing_from_a_git_branch_follows_that_branch_then_sync_and_check_agree() -> TestOutcome {
    // `--branch` passes through: the dependency follows `feature`, whose tip
    // differs from the default branch's.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("branched-skeleton", "thing.txt", "on main\n", "main")?;
    repository.branch_from_head("feature")?;
    repository.commit_skeleton("branched-skeleton", "thing.txt", "on feature\n", "feature")?;
    let fixture = fixture_for_wearing("")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "branched-skeleton",
        "--git",
        &repository.file_url(),
        "--branch",
        "feature",
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "branched-skeleton", "branched-skeleton")?;
    assert!(
        manifest.contains("branch = \"feature\""),
        "the dependency must follow the branch; manifest was:\n{manifest}"
    );
    assert_sync_writes_and_check_matches(&fixture, "thing.txt", b"on feature\n", &[])
}

#[test]
fn wearing_from_a_git_rev_pins_that_commit_then_sync_and_check_agree() -> TestOutcome {
    // `--rev` passes through: the dependency pins the first commit although a
    // second one exists.
    let repository = SkeletonRepository::new()?;
    let first = repository.commit_skeleton("revved-skeleton", "thing.txt", "first\n", "first")?;
    repository.commit_skeleton("revved-skeleton", "thing.txt", "second\n", "second")?;
    let fixture = fixture_for_wearing("")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "revved-skeleton",
        "--git",
        &repository.file_url(),
        "--rev",
        &first,
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "revved-skeleton", "revved-skeleton")?;
    assert!(
        manifest.contains(&format!("rev = \"{first}\"")),
        "the dependency must pin the commit; manifest was:\n{manifest}"
    );
    assert_sync_writes_and_check_matches(&fixture, "thing.txt", b"first\n", &[])
}

#[test]
fn wearing_from_a_registry_with_a_version_adds_the_dependency_then_sync_and_check_agree()
-> TestOutcome {
    // No source flag: Cargo resolves `semver@1.0.7` through crates.io, which
    // the sandbox's `CARGO_HOME` replaces with a vendored directory source
    // (and `CARGO_NET_OFFLINE` guarantees nothing reaches the network). The
    // vendored crate is the only `semver` there is, at 1.0.7, so the
    // `@1.0.7` is what Cargo reads as the requirement.
    let fixture = fixture_for_wearing("")?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "thing.txt",
        "from the registry\n",
    )?;
    let index = support::captured_crates_io_index();
    let environment = [
        ("CARGO_NET_OFFLINE", "true"),
        (
            "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
            index.to_str().ok_or("index path must be UTF-8")?,
        ),
    ];

    let report = fixture.run_with_env(&["skeletons", "wear", "semver@1.0.7"], &environment)?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "semver", "semver")?;
    assert!(
        manifest.contains("1.0.7"),
        "the dependency must carry the requested version; manifest was:\n{manifest}"
    );
    assert_sync_writes_and_check_matches(
        &fixture,
        "thing.txt",
        b"from the registry\n",
        &environment,
    )
}

#[test]
fn a_custom_key_names_both_the_rename_and_the_wearing_table() -> TestOutcome {
    // `wear passthrough-plain tidy`: the dependency is renamed to `tidy` and
    // the table is `[package.metadata.skeletons.tidy]`, with nothing left
    // under the crate's own name. `sync` finding the file proves the table
    // and the rename agree.
    let fixture = fixture_for_wearing("")?;
    let skeleton_path = checked_in_test_skeleton("passthrough-plain");
    let skeleton_path = skeleton_path.to_str().ok_or("path must be UTF-8")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "tidy",
        "--path",
        skeleton_path,
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    let manifest = String::from_utf8(fixture.read("Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "tidy", "passthrough-plain")?;
    assert!(
        !manifest.contains("[package.metadata.skeletons.passthrough-plain]"),
        "no table may be named for the crate when the key differs; manifest was:\n{manifest}"
    );
    assert_sync_writes_and_check_matches(&fixture, "plain.yml", PASSTHROUGH_PLAIN_RENDER, &[])
}

#[test]
fn wear_writes_into_the_command_lines_own_package_in_a_workspace_of_several() -> TestOutcome {
    // A virtual workspace with two members; only `skeletons-ritual` is the
    // package the running command line names, so only its manifest may change
    // — not the workspace root's, not the bystander's — although `ritual` runs
    // at the workspace root.
    let fixture = Fixture::new()?;
    support::write_workspace_root(fixture.root(), &["tools/cli", "bystander"])?;
    support::wear::write_command_line_package(fixture.root(), "tools/cli", "")?;
    support::write_package_manifest(fixture.root(), "bystander", "bystander", "")?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    let root_before = fixture.read("Cargo.toml")?;
    let bystander_before = fixture.read("bystander/Cargo.toml")?;
    let skeleton_path = checked_in_test_skeleton("passthrough-plain");
    let skeleton_path = skeleton_path.to_str().ok_or("path must be UTF-8")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "--path",
        skeleton_path,
    ])?;

    assert_eq!(
        report.exit_code, 0,
        "wear must succeed; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        report.stdout,
        "added passthrough-plain 0.0.0 to tools/cli/Cargo.toml as the dev-dependency \
         `passthrough-plain`, with an empty [package.metadata.skeletons.passthrough-plain] \
         table\n\
         commit tools/cli/Cargo.toml and Cargo.lock, then run the `sync` task to write its \
         files\n",
        "the next step must name the manifest as the first line shows it, relative to the \
         workspace root"
    );
    let manifest = String::from_utf8(fixture.read("tools/cli/Cargo.toml")?)?;
    assert_worn_as_a_dev_dependency(&manifest, "passthrough-plain", "passthrough-plain")?;
    assert_eq!(
        fixture.read("Cargo.toml")?,
        root_before,
        "the workspace root manifest must be untouched"
    );
    assert_eq!(
        fixture.read("bystander/Cargo.toml")?,
        bystander_before,
        "a member that is not the command line's package must be untouched"
    );
    let lockfile = String::from_utf8(fixture.read("Cargo.lock")?)?;
    assert!(
        lockfile.contains("name = \"passthrough-plain\""),
        "the workspace lockfile must pin the skeleton; it was:\n{lockfile}"
    );
    Ok(())
}
