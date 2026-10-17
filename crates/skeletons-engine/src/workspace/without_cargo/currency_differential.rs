//! Whether `Cargo.lock` is current, decided by this reader and by `cargo metadata --locked`.
//!
//! Each test writes a workspace of path packages into a temporary directory, has the cargo that
//! runs the tests lock it, changes the files so the lockfile may no longer fit them, and asks
//! both: cargo through the same fetch `check` uses, and this reader through `read_document`. The
//! verdicts must be the same fixed one, written in the test. Which cargo that is decides the
//! toolchain: `cargo test` and `cargo +1.85.0 test` each hand their own to the tests as `$CARGO`,
//! which `crate::cargo::command` runs, so the same suite checks each toolchain. Every failure
//! names the `cargo --version` it ran under.
//!
//! A git or registry package is locked in a Cargo home the test owns, which reaches nothing
//! outside the machine: the git sources are `file://` repositories the test builds, fetched into
//! that home before the workspace is changed, and the registry is a directory of vendored
//! crates that replaces crates.io. Cargo's verdict is read in that home, offline and `--locked`.
//! `currency.rs` tests the same shapes against fixed lockfile text.
//!
//! The cases for path packages are here, and those for git and registry packages are in the
//! files beside them.

mod git;
mod registry;

use std::collections::BTreeMap;

use super::differential::shape;
use super::error::ReadWithoutCargoError;
use super::fixture::{CargoHome, Fixture, cargo_version};
use super::read::read_document;
use crate::workspace::{Network, ReadWorkspaceError, cargo_metadata};

/// What a reader of a workspace says of its lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The lockfile is current: a document comes back.
    Current,
    /// The lockfile is out of date: the abort that `--locked` gives.
    Aborted,
    /// A manifest or a package is refused before the lockfile is looked at: the failure Cargo
    /// reports for a package it cannot read, against the reader's unreadable manifest.
    Refused,
}

/// What `cargo metadata --locked` says of the workspace at `fixture`'s root.
fn cargo_verdict(fixture: &Fixture) -> Verdict {
    cargo_verdict_of(cargo_metadata::fetch(&fixture.root, Network::Refused))
}

/// What `cargo metadata --locked` says of the workspace at `fixture`'s root, in the Cargo home
/// `home`.
fn cargo_verdict_in_home(fixture: &Fixture, home: &CargoHome) -> Verdict {
    cargo_verdict_of(cargo_metadata::fetch_in_cargo_home(
        &fixture.root,
        home.path(),
        home.home(),
    ))
}

/// The verdict that the outcome `result` of `cargo metadata --locked` stands for.
fn cargo_verdict_of<Output>(result: Result<Output, ReadWorkspaceError>) -> Verdict {
    match result {
        Ok(_) => Verdict::Current,
        Err(ReadWorkspaceError::Lockfile) => Verdict::Aborted,
        Err(ReadWorkspaceError::CargoMetadataFailed { .. }) => Verdict::Refused,
        Err(other) => panic!(
            "{}: cargo failed some other way: {other:?}",
            cargo_version()
        ),
    }
}

/// What this reader says of the workspace at `fixture`'s root.
fn reader_verdict(fixture: &Fixture) -> Verdict {
    match read_document(&fixture.root, &BTreeMap::new()) {
        Ok(_) => Verdict::Current,
        Err(ReadWithoutCargoError::Lockfile) => Verdict::Aborted,
        Err(ReadWithoutCargoError::ManifestUnreadable { .. }) => Verdict::Refused,
        Err(other) => panic!(
            "{}: the reader failed some other way: {other:?}",
            cargo_version()
        ),
    }
}

/// Requires cargo and the reader to both say `expected`.
fn assert_both_say(fixture: &Fixture, expected: Verdict) {
    let toolchain = cargo_version();
    assert_eq!(cargo_verdict(fixture), expected, "cargo, on {toolchain}");
    assert_eq!(
        reader_verdict(fixture),
        expected,
        "the reader, on {toolchain}"
    );
}

/// Requires cargo, in the Cargo home `home`, and the reader to both say `expected`.
fn assert_both_say_in_home(fixture: &Fixture, home: &CargoHome, expected: Verdict) {
    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict_in_home(fixture, home),
        expected,
        "cargo, on {toolchain}"
    );
    assert_eq!(
        reader_verdict(fixture),
        expected,
        "the reader, on {toolchain}"
    );
}

/// Has the cargo that runs the tests lock the workspace at `fixture`'s root.
fn lock(fixture: &Fixture) {
    let toolchain = cargo_version();
    fixture
        .lock_with_cargo()
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
}

/// A workspace rooted at `w` with the members `members`, each a library package with no
/// dependency, locked by cargo.
fn locked_members(members: &[&str]) -> Fixture {
    let fixture = Fixture::new();
    let listed: Vec<String> = members
        .iter()
        .map(|member| format!("\"{member}\""))
        .collect();
    fixture.write(
        "Cargo.toml",
        &format!("[workspace]\nmembers = [{}]\n", listed.join(", ")),
    );
    fixture
}

/// Writes the member `app`, a library package, with `extra` after its `[package]` table.
fn write_app(fixture: &Fixture, extra: &str) {
    fixture.package("app", "app", extra);
}

#[test]
fn a_lockfile_just_written_is_current() {
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "[dependencies]\nd = { path = \"../d\" }\n");
    fixture.package("d", "d", "");
    lock(&fixture);

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn a_removed_dependency_on_a_package_outside_the_root_is_current() {
    // Case A: `outside` leaves the graph altogether, so Cargo sets its entry aside.
    let fixture = locked_members(&["app"]);
    write_app(
        &fixture,
        "[dependencies]\noutside = { path = \"../../outside\" }\n",
    );
    fixture.package("../outside", "outside", "");
    lock(&fixture);
    write_app(&fixture, "");

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn a_removed_dependency_on_a_package_inside_the_root_is_current() {
    // Case rm1: `inside` was a member only because `app` depended on it; without the
    // dependency it is no package of the workspace and its entry is set aside.
    let fixture = locked_members(&["app"]);
    write_app(
        &fixture,
        "[dependencies]\ninside = { path = \"../inside\" }\n",
    );
    fixture.package("inside", "inside", "");
    lock(&fixture);
    write_app(&fixture, "");

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn a_dependency_removed_from_the_package_that_brought_in_another_is_current() {
    // A package that only `mid` kept is gone from the graph once `mid` stops depending on it.
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "[dependencies]\nmid = { path = \"../../mid\" }\n");
    fixture.package(
        "../mid",
        "mid",
        "[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    fixture.package("../leaf", "leaf", "");
    lock(&fixture);
    fixture.package("../mid", "mid", "");

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn an_extra_entry_nobody_depends_on_is_current_when_no_package_has_its_name() {
    // Case H2.
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "");
    lock(&fixture);
    let lockfile = std::fs::read_to_string(fixture.root.join("Cargo.lock")).expect("the lockfile");
    fixture.write(
        "Cargo.lock",
        &format!("{lockfile}\n[[package]]\nname = \"ghost\"\nversion = \"0.0.1\"\n"),
    );

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn a_dependency_removed_from_one_member_but_kept_by_another_is_aborted() {
    // Case B.
    let fixture = locked_members(&["one", "two"]);
    let depends = "[dependencies]\nshared = { path = \"../../shared\" }\n";
    fixture.package("one", "one", depends);
    fixture.package("two", "two", depends);
    fixture.package("../shared", "shared", "");
    lock(&fixture);
    fixture.package("one", "one", "");

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn a_dependency_on_a_listed_member_removed_is_aborted() {
    // Case C: `lib` is still a member, so its entry stays and `app` no longer declares it.
    let fixture = locked_members(&["app", "lib"]);
    write_app(&fixture, "[dependencies]\nlib = { path = \"../lib\" }\n");
    fixture.package("lib", "lib", "");
    lock(&fixture);
    write_app(&fixture, "");

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_added_dependency_is_aborted() {
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "");
    fixture.package("added", "added", "");
    lock(&fixture);
    write_app(
        &fixture,
        "[dependencies]\nadded = { path = \"../added\" }\n",
    );

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn a_path_dependency_whose_version_was_bumped_is_aborted() {
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "[dependencies]\nd = { path = \"../d\" }\n");
    fixture.package("d", "d", "version = \"0.1.0\"\n");
    lock(&fixture);
    fixture.package("d", "d", "version = \"0.2.0\"\n");

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_optional_dependency_added_with_its_feature_is_aborted() {
    // Case 4a.
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "");
    fixture.package("z", "z", "");
    lock(&fixture);
    write_app(
        &fixture,
        "[dependencies]\nz = { path = \"../z\", optional = true }\n[features]\nfz = [\"dep:z\"]\n",
    );

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_optional_dependency_added_with_only_its_implicit_feature_is_aborted() {
    // Case 4a prime.
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "");
    fixture.package("z", "z", "");
    lock(&fixture);
    write_app(
        &fixture,
        "[dependencies]\nz = { path = \"../z\", optional = true }\n",
    );

    assert_both_say(&fixture, Verdict::Aborted);
}

/// A member `app` depending on `y`, a package outside the root, with `y_extra` after `y`'s
/// `[package]` table.
fn app_depending_on_outsider(y_extra: &str) -> Fixture {
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "[dependencies]\ny = { path = \"../../y\" }\n");
    fixture.package("../y", "y", y_extra);
    fixture
}

#[test]
fn an_optional_dependency_of_an_outsider_that_no_feature_enables_is_current() {
    // Case H7: Cargo does not lock it.
    let fixture =
        app_depending_on_outsider("[dependencies]\nz = { path = \"../z\", optional = true }\n");
    fixture.package("../z", "z", "");
    lock(&fixture);

    assert_both_say(&fixture, Verdict::Current);
}

/// Requires that a workspace whose lock a feature change on a package outside it has made stale
/// gets, from the reader, the document `cargo metadata --locked` gives once the lock is
/// regenerated.
///
/// Cargo aborts on the stale lock, and deciding that takes feature resolution, which the reader
/// does not do, so it reads the lock as it stands. The document it gives is the one it reads
/// from members, their wearing tables and the packages their dependencies are locked to, and
/// the feature change touches none of those. The lock is regenerated in place, after the
/// reader has read the stale one, so that the paths in the two documents are the same.
fn assert_stale_report_is_the_relocked_one(fixture: &Fixture) {
    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict(fixture),
        Verdict::Aborted,
        "cargo, before the lock is regenerated, on {toolchain}"
    );
    let stale = read_document(&fixture.root, &BTreeMap::new())
        .unwrap_or_else(|error| panic!("{toolchain}: the reader refused the stale lock: {error}"));

    lock(fixture);
    let relocked = cargo_metadata::fetch(&fixture.root, Network::Refused)
        .unwrap_or_else(|error| panic!("{toolchain}: cargo refused the relocked lock: {error:?}"));

    // The whole graph is not compared: the lock the reader read lists `z` where the regenerated
    // one does not, or the other way round, and no member's edge reaches it.
    assert_eq!(
        shape(&stale).reached_by_members(),
        shape(&relocked).reached_by_members(),
        "on {toolchain}"
    );
}

#[test]
fn an_outsiders_optional_dependency_enabled_afterwards_is_reported_as_after_a_relock() {
    // Case H8: enabling `z` through a feature of `app` makes Cargo want it locked, which it is
    // not, so Cargo aborts.
    let fixture =
        app_depending_on_outsider("[dependencies]\nz = { path = \"../z\", optional = true }\n");
    fixture.package("../z", "z", "");
    lock(&fixture);
    write_app(
        &fixture,
        "[dependencies]\ny = { path = \"../../y\" }\n[features]\nf = [\"y/z\"]\n",
    );

    assert_stale_report_is_the_relocked_one(&fixture);
}

#[test]
fn an_outsiders_build_and_windows_dependencies_are_locked_and_its_dev_dependency_is_not() {
    // Case N1.
    let fixture = app_depending_on_outsider(
        "[build-dependencies]\nb = { path = \"../b\" }\n\
         [target.'cfg(windows)'.dependencies]\nt = { path = \"../t\" }\n\
         [dev-dependencies]\nd = { path = \"../d\" }\n",
    );
    for name in ["b", "t", "d"] {
        fixture.package(&format!("../{name}"), name, "");
    }
    lock(&fixture);

    assert_both_say(&fixture, Verdict::Current);
}

/// What `y` writes to depend on `z`, an optional path dependency.
const Y_OPTIONAL_Z: &str = "[dependencies]\nz = { path = \"../z\", optional = true }\n";

/// What `y` writes to depend on `z` and to enable it by default.
const Y_DEFAULT_ENABLES_Z: &str = "[dependencies]\nz = { path = \"../z\", optional = true }\n\
                                   [features]\ndefault = [\"z\"]\n";

/// The package `z` outside the root at version `version`.
fn write_z(fixture: &Fixture, version: &str) {
    fixture.package("../z", "z", &format!("version = \"{version}\"\n"));
}

/// A workspace whose member `app` depends on `y`, which enables its optional dependency `z`
/// by default, locked by cargo with `z` at 0.1.0.
fn locked_with_z_enabled_by_default() -> Fixture {
    let fixture = app_depending_on_outsider(Y_DEFAULT_ENABLES_Z);
    write_z(&fixture, "0.1.0");
    lock(&fixture);
    fixture
}

#[test]
fn an_enabled_optional_dependency_of_an_outsider_just_locked_is_current() {
    // Case G2 as locked: the lock lists `z` under `y`, and nothing has changed since.
    let fixture = locked_with_z_enabled_by_default();

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn an_enabled_optional_dependency_of_an_outsider_whose_version_was_bumped_is_aborted() {
    // Case G2a: `y` enables `z` by default and the lock lists it, so Cargo reads `z` and finds
    // a version the lock does not hold. The reader has to read `z` too to see it.
    let fixture = locked_with_z_enabled_by_default();
    write_z(&fixture, "0.2.0");

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_optional_dependency_enabled_by_the_member_whose_version_was_bumped_is_aborted() {
    // Case G2b: the member's own declaration of `y` enables `z`, with `features = ["z"]`.
    let fixture = locked_members(&["app"]);
    write_app(
        &fixture,
        "[dependencies]\ny = { path = \"../../y\", features = [\"z\"] }\n",
    );
    fixture.package("../y", "y", Y_OPTIONAL_Z);
    write_z(&fixture, "0.1.0");
    lock(&fixture);
    write_z(&fixture, "0.2.0");

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_enabled_optional_dependency_of_an_outsider_that_gains_a_dependency_is_aborted() {
    // Case G2c: `z` gains a path dependency `q` the lock does not list under `z`.
    let fixture = locked_with_z_enabled_by_default();
    fixture.package("../q", "q", "");
    fixture.package(
        "../z",
        "z",
        "version = \"0.1.0\"\n[dependencies]\nq = { path = \"../q\" }\n",
    );

    assert_both_say(&fixture, Verdict::Aborted);
}

#[test]
fn an_enabled_optional_dependency_of_an_outsider_that_is_gone_is_refused() {
    // Case G2e: the lock lists `z` and nothing is at its directory, so Cargo cannot load the
    // dependency of `y` and fails before it looks at the lockfile.
    let fixture = locked_with_z_enabled_by_default();
    std::fs::remove_dir_all(fixture.root.with_file_name("z")).expect("take z away");

    assert_both_say(&fixture, Verdict::Refused);
}

#[test]
fn an_enabled_optional_dependency_of_an_outsider_moved_to_another_directory_is_current() {
    // Case G2f: `y` points `z` at another directory that holds `z` 0.1.0, which is the same
    // package to the lockfile.
    let fixture = locked_with_z_enabled_by_default();
    fixture.package("../z2", "z", "version = \"0.1.0\"\n");
    fixture.package(
        "../y",
        "y",
        "[dependencies]\nz = { path = \"../z2\", optional = true }\n\
         [features]\ndefault = [\"z\"]\n",
    );

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn an_optional_dependency_of_an_outsider_that_is_not_enabled_and_was_bumped_is_current() {
    // Case G2g: nothing enables `z`, so Cargo never reads it and its version does not matter.
    let fixture =
        app_depending_on_outsider("[dependencies]\nz = { path = \"../z\", optional = true }\n");
    write_z(&fixture, "0.1.0");
    lock(&fixture);
    write_z(&fixture, "0.2.0");

    assert_both_say(&fixture, Verdict::Current);
}

#[test]
fn an_optional_dependency_of_an_outsider_that_stops_being_enabled_is_reported_as_after_a_relock() {
    // Case G2d, the mirror of H8: `y` drops its `default`, so `z` is no longer enabled, but the
    // lock still lists it, which Cargo calls out of date.
    let fixture = locked_with_z_enabled_by_default();
    fixture.package("../y", "y", Y_OPTIONAL_Z);

    assert_stale_report_is_the_relocked_one(&fixture);
}
