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
//! The cases that need a git or registry package are not here, because locking them needs the
//! network or a populated Cargo home; `currency.rs` tests them against fixed lockfile text.

use std::collections::BTreeMap;

use super::error::ReadWithoutCargoError;
use super::fixture::{Fixture, cargo_version};
use super::read::read_document;
use crate::workspace::{Network, ReadWorkspaceError, cargo_metadata};

/// What a reader of a workspace says of its lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The lockfile is current: a document comes back.
    Current,
    /// The lockfile is out of date: the abort that `--locked` gives.
    Aborted,
}

/// What `cargo metadata --locked` says of the workspace at `fixture`'s root.
fn cargo_verdict(fixture: &Fixture) -> Verdict {
    match cargo_metadata::fetch(&fixture.root, Network::Refused) {
        Ok(_) => Verdict::Current,
        Err(ReadWorkspaceError::Lockfile) => Verdict::Aborted,
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

#[test]
fn an_outsiders_optional_dependency_enabled_afterwards_is_a_gap_the_reader_cannot_see() {
    // Case H8, the known gap: enabling `z` through a feature of `app` makes Cargo want it
    // locked, which it is not, so Cargo aborts. Deciding that takes feature resolution, which
    // this reader does not do, so it gives a document where Cargo aborts.
    let fixture =
        app_depending_on_outsider("[dependencies]\nz = { path = \"../z\", optional = true }\n");
    fixture.package("../z", "z", "");
    lock(&fixture);
    write_app(
        &fixture,
        "[dependencies]\ny = { path = \"../../y\" }\n[features]\nf = [\"y/z\"]\n",
    );

    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict(&fixture),
        Verdict::Aborted,
        "cargo, on {toolchain}"
    );
    assert_eq!(
        reader_verdict(&fixture),
        Verdict::Current,
        "the reader, on {toolchain}"
    );
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
