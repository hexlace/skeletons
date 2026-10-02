//! Acceptance: `wear` refuses what it must, says what is wrong, and leaves
//! the command line crate's manifest and the workspace `Cargo.lock`
//! byte-identical to what they were.
//!
//! Every scenario reads both files before and after, as bytes, and compares
//! them, never trusting an exit code alone. Only one refusal comes after
//! `cargo add` has already changed both files (a crate that is not a
//! skeleton is only found out once Cargo has resolved it), which is what
//! makes the comparison mean something there; the others come before it, and
//! leave the files identical because nothing touched them.
//!
//! Byte-identical files cannot tell those two apart, so each scenario also
//! holds stderr to the refusal's whole line, word for word, filled in from
//! the fixture. The refusals that come before any change must not say the
//! project was put back; the one that undid a change must say so.

mod support;

use support::wear::{
    ManifestAndLockfile, assert_nothing_was_undone, assert_refused_with_line, fixture_for_wearing,
};
use support::{
    Fixture, TemporaryDirectory, TestOutcome, checked_in_test_skeleton, wearing_table,
    write_package_manifest,
};

fn skeleton_path(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(checked_in_test_skeleton(name)
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned())
}

/// A workspace that already wears `passthrough-plain`, as a dev-dependency,
/// committed clean.
fn fixture_already_wearing_passthrough_plain() -> Result<Fixture, Box<dyn std::error::Error>> {
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        support::path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    fixture_for_wearing(&extra)
}

#[test]
fn a_skeleton_that_is_already_worn_is_refused_and_nothing_changes() -> TestOutcome {
    // `passthrough-plain` is already a dependency with its wearing table.
    // Cargo alone would accept the second `add` and quietly rewrite the line,
    // so `wear` has to be the one to say it is already worn.
    let fixture = fixture_already_wearing_passthrough_plain()?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "passthrough-plain is already worn, as `passthrough-plain` in Cargo.toml; a workspace \
         wears a skeleton once, so to change its options, edit \
         [package.metadata.skeletons.passthrough-plain] there",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_key_already_taken_by_another_dependency_is_refused_and_nothing_changes() -> TestOutcome {
    // `taken` is a dependency on a different crate. Wearing a skeleton under
    // that key would overwrite it, so `wear` must refuse, naming the key.
    let other = skeleton_path("enum-fill")?;
    let extra =
        format!("[dependencies]\ntaken = {{ package = \"enum-fill\", path = \"{other}\" }}\n");
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "taken",
        "--path",
        &path,
    ])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "`taken` is taken in Cargo.toml: its dependency on enum-fill is declared under `taken`; \
         give the `wear` task another key as its second argument",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_crate_that_is_not_a_skeleton_is_refused_after_cargo_ran_and_nothing_changes() -> TestOutcome {
    // `plain-crate` resolves fine but has no `[package.metadata.skeletons]`
    // table. That is only known once Cargo has added it (changing both the
    // manifest and the lockfile), so this is the refusal that has something
    // to undo: both files must come back byte for byte.
    let crate_directory = TemporaryDirectory::new("not-a-skeleton")?;
    write_package_manifest(crate_directory.path(), "", "plain-crate", "")?;
    let fixture = fixture_for_wearing("")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = crate_directory
        .path()
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned();

    let report = fixture.run(&["skeletons", "wear", "plain-crate", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "plain-crate 0.1.0 is not a skeleton: its manifest has no [package.metadata.skeletons] \
         table, so it cannot be worn; wear a crate that is one; ritual put the project back as \
         it found it",
    );
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must restore the manifest and Cargo.lock byte for byte"
    );
    Ok(())
}

#[test]
fn a_work_tree_with_uncommitted_changes_is_refused_and_nothing_changes() -> TestOutcome {
    // The command line crate's manifest has an uncommitted edit. `wear` is
    // about to write to that file, so it must refuse as `sync` does, listing
    // the file as a line of its own and then summing up, and must leave the uncommitted edit and the lockfile exactly
    // as they were.
    let fixture = fixture_for_wearing("")?;
    let mut edited = fixture.read("Cargo.toml")?;
    edited.extend_from_slice(b"\n# an edit nobody committed\n");
    fixture.write("Cargo.toml", &edited)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert!(
        report
            .stdout
            .lines()
            .any(|line| line == "Cargo.toml has uncommitted changes"),
        "stdout must list the dirty path as its own line; stdout was:\n{}",
        report.stdout
    );
    assert_refused_with_line(
        &report,
        "the working tree has 1 uncommitted change, so wear wrote nothing: it writes only into a \
         clean working tree, where git holds everything it changes; commit, stash or move it, \
         then run the `wear` task again",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the uncommitted manifest and Cargo.lock byte-identical"
    );
    assert_eq!(
        fixture.read("Cargo.toml")?,
        edited,
        "the uncommitted edit must survive, as it was written"
    );
    Ok(())
}

#[test]
fn a_skeleton_already_depended_on_under_another_key_is_refused_and_nothing_changes() -> TestOutcome
{
    // `passthrough-plain` is already a dependency, under the key `other`, and
    // nothing wears it. Wearing it under its own name would give the manifest
    // a second dependency on one crate, which Cargo refuses only after
    // `cargo add` has written it, in words that offer no remedy. `wear` has to
    // see the dependency by its crate, and name the key it is under, since
    // that is the key the wearing table goes at.
    let path = skeleton_path("passthrough-plain")?;
    let extra = format!(
        "[dependencies]\nother = {{ package = \"passthrough-plain\", path = \"{path}\" }}\n"
    );
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = fixture.run(&["skeletons", "wear", "passthrough-plain", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "Cargo.toml already depends on passthrough-plain under the key `other`, without wearing \
         it; to wear it, add an empty [package.metadata.skeletons.other] table to Cargo.toml",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_dependency_whose_wearing_is_refused_is_sent_to_check_and_nothing_changes() -> TestOutcome {
    // `plain-crate` is a dependency with a wearing table at its key, but it is
    // no skeleton, so the reader refuses the wearing and nothing is worn. The
    // table is not missing, so telling the wearer to add one would be false:
    // the one thing that helps is the `check` task, which says why the
    // wearing is refused.
    let crate_directory = TemporaryDirectory::new("refused-wearing")?;
    write_package_manifest(crate_directory.path(), "", "plain-crate", "")?;
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        support::path_dependency_on("plain-crate", crate_directory.path()),
        wearing_table("plain-crate", ""),
    );
    let fixture = fixture_for_wearing(&extra)?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = crate_directory
        .path()
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned();

    let report = fixture.run(&["skeletons", "wear", "plain-crate", "--path", &path])?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    assert_refused_with_line(
        &report,
        "Cargo.toml already depends on plain-crate under the key `plain-crate`, and has a \
         [package.metadata.skeletons.plain-crate] table for it, but that wearing is refused; \
         the `check` task says why, so run it and fix what it names in Cargo.toml",
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

#[test]
fn a_key_that_names_a_crate_the_compiler_provides_is_refused_and_nothing_changes() -> TestOutcome {
    // `std`, `test` and `proc-macro` are crates rustc supplies to every
    // build, so a dependency renamed to one of them would shadow, or be
    // shadowed by, the compiler's own. `proc-macro` is typed with its hyphen
    // and is the compiler's `proc_macro`: the two spellings are one name to
    // rustc, so the refusal must see through the hyphen. Each is refused
    // before `cargo add` runs, naming the key, and the files stay as they were.
    let fixture = fixture_for_wearing("")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let path = skeleton_path("passthrough-plain")?;

    for key in ["std", "test", "proc-macro"] {
        let report = fixture.run(&[
            "skeletons",
            "wear",
            "passthrough-plain",
            key,
            "--path",
            &path,
        ])?;

        assert_ne!(report.exit_code, 0, "key {key}; stdout: {}", report.stdout);
        assert_refused_with_line(
            &report,
            &format!(
                "`{key}` cannot be worn as a dependency key: it names a crate the compiler \
                 provides; give the `wear` task another key as its second argument"
            ),
        );
        assert_nothing_was_undone(&report);
        assert_eq!(
            ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
            before,
            "a wear refused for the key `{key}` must leave the manifest and Cargo.lock \
             byte-identical"
        );
    }
    Ok(())
}
