//! Acceptance: what counts as worn.
//!
//! Each scenario builds a fresh Cargo workspace, wears one or more test
//! skeletons in it, and asserts on `check`'s exit status and its output:
//! `--json` where the rows are the point, the plain output where a message is.

mod support;

use support::passthrough_plain::PASSTHROUGH_PLAIN_RENDER;
use support::{
    Fixture, path_dependency_on_test_skeleton, wearing_table, write_package_manifest,
    write_workspace_root,
};

#[test]
fn a_skeleton_dependency_with_no_wearing_table_produces_no_rows_and_the_repository_wears_nothing()
-> support::TestOutcome {
    // A dependency on a real skeleton, `passthrough-plain`, with no
    // `[package.metadata.skeletons.passthrough-plain]` table at all in the
    // wearer's own manifest, is not worn: it produces no rows, and since
    // it is the only dependency, the whole repository wears nothing.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        // Deliberately no `[package.metadata.skeletons.passthrough-plain]` table.
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(
        report.exit_code, 0,
        "an unworn skeleton dependency must not make check fail; stderr was: {}",
        report.stderr
    );
    // A do-nothing `check` also prints nothing on stdout, so the strongest
    // claim this test can make is that `--json` parses to a document whose
    // `skeletons` and `bones` arrays are both genuinely empty — never merely
    // that the word "plain.yml" is absent from an unparsed, possibly empty,
    // stream.
    let document = support::json::parse(&report.stdout)?;
    let skeletons = support::json::skeletons(&document)?;
    assert!(
        skeletons.is_empty(),
        "an unworn dependency must produce no worn-skeleton rows at all; skeletons were: \
         {skeletons:?}"
    );
    let bones = support::json::bones(&document)?;
    assert!(
        bones.is_empty(),
        "an unworn dependency must produce no bone rows at all; bones were: {bones:?}"
    );
    Ok(())
}

#[test]
fn a_repository_with_no_dependencies_at_all_wears_nothing_and_says_so() -> support::TestOutcome {
    // The simplest "wears nothing" shape: no dependencies whatsoever. This
    // is an exact answer, not an error, and must exit zero, printing the
    // one line `.docs/wearing.md` gives for exactly this case.
    let fixture = Fixture::new()?;
    write_package_manifest(fixture.root(), "", "wearer", "")?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 0,
        "a repository wearing no skeletons at all must exit zero; stderr was: {}",
        report.stderr
    );
    // `.docs/wearing.md` gives this exact line for a workspace wearing nothing —
    // a do-nothing stub prints nothing at all, so this is a positive fact
    // no stub can produce by accident.
    assert_eq!(
        report.stdout,
        "this workspace wears no skeletons; a manifest wears one with a \
         [package.metadata.skeletons.<dependency>] table beside the dependency\n"
    );
    Ok(())
}

#[test]
fn a_wearing_table_on_a_non_root_workspace_member_is_honoured_with_a_workspace_root_relative_path()
-> support::TestOutcome {
    // A two-member workspace where only the non-root member, `consumer`,
    // wears `passthrough-plain`. The claimed file's path must be relative
    // to the *workspace* root, not to `consumer`'s own directory: writing
    // the correctly-rendered bytes at the workspace root (never under
    // `consumer/`) must report matches.
    let fixture = Fixture::new()?;
    write_workspace_root(fixture.root(), &["consumer"])?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "consumer", "consumer", &extra)?;
    fixture.generate_lockfile()?;
    // At the workspace root, not under `consumer/`.
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(
        report.exit_code, 0,
        "a claimed file at the workspace-root-relative path must match, even though the \
         wearing table sits on a non-root member; stderr was: {}",
        report.stderr
    );
    // A do-nothing stub also exits 0 here, so the fact that carries the
    // test is the row itself: it must exist, name `consumer/Cargo.toml`
    // as the wearing manifest (not the workspace root's own `Cargo.toml`),
    // and read matches at the workspace-root-relative path.
    let document = support::json::parse(&report.stdout)?;
    let bones = support::json::bones(&document)?;
    assert_eq!(
        bones.len(),
        1,
        "expected exactly one bone row; bones were: {bones:?}"
    );
    let row = &bones[0];
    assert_eq!(support::json::bone_path(row)?, "plain.yml");
    assert_eq!(support::json::bone_manifest(row)?, "consumer/Cargo.toml");
    assert_eq!(support::json::bone_drift_state(row)?, "matches");
    Ok(())
}

#[test]
fn a_dependency_whose_own_crate_is_not_a_skeleton_is_refused_naming_the_dependency()
-> support::TestOutcome {
    // `plain-crate` has no `[package.metadata.skeletons]` table of its own —
    // it is an ordinary crate. Wearing it anyway (a wearing table naming
    // it in the wearer's own manifest) must refuse, naming the
    // dependency.
    let fixture = Fixture::new()?;
    write_package_manifest(fixture.root(), "plain-crate", "plain-crate", "")?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        support::path_dependency_on("plain-crate", &fixture.root().join("plain-crate")),
        wearing_table("plain-crate", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "wearing a dependency whose crate is not itself a skeleton must refuse; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("plain-crate"),
        "the refusal must name the offending dependency; combined output was: {combined}"
    );
    Ok(())
}

#[test]
fn two_worn_skeletons_claiming_the_same_path_produce_a_loud_named_error() -> support::TestOutcome {
    // `overlap-a` and `overlap-b` both ship `files/shared.txt`. Wearing
    // both at once must refuse loudly, naming the clashing path, rather
    // than silently picking one or merging them.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}{}\n{}{}",
        path_dependency_on_test_skeleton("overlap-a", "overlap-a"),
        path_dependency_on_test_skeleton("overlap-b", "overlap-b"),
        wearing_table("overlap-a", ""),
        wearing_table("overlap-b", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "two skeletons claiming the same path must refuse; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("shared.txt"),
        "the refusal must name the clashing path; combined output was: {combined}"
    );
    Ok(())
}

#[test]
fn one_skeleton_worn_by_two_members_claiming_the_same_path_produces_a_loud_named_error()
-> support::TestOutcome {
    // Both `member-a` and `member-b` wear `passthrough-plain`, whose one
    // file renders to the same workspace-root-relative path from either
    // member. Two members wearing the same skeleton this way must refuse the
    // same as two different skeletons claiming the same path would.
    let fixture = Fixture::new()?;
    write_workspace_root(fixture.root(), &["member-a", "member-b"])?;
    for member in ["member-a", "member-b"] {
        let extra = format!(
            "[dependencies]\n{}\n{}",
            path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
            wearing_table("passthrough-plain", ""),
        );
        write_package_manifest(fixture.root(), member, member, &extra)?;
    }
    fixture.generate_lockfile()?;

    let report = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        report.exit_code, 1,
        "one skeleton worn by two members, claiming the same path, must refuse; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("plain.yml"),
        "the refusal must name the clashing path; combined output was: {combined}"
    );
    Ok(())
}
