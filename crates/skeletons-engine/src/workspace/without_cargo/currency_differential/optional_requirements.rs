//! The cases where an optional dependency of a package outside the workspace asks a worn package
//! for a version the lock does not name.
//!
//! Cargo keeps one version of a package for each range of versions it calls compatible, and the
//! workspace's `house-rules = "4"` is locked at 4.5.6. A package outside the workspace that a
//! feature enables later, and that asks for `house-rules` `4.6`, is in the range 4.5.6 is in and
//! not met by it: Cargo cannot resolve the workspace until the worn package is updated, however
//! it is asked. That is no stale lock for the reader to report through, and the reader refuses
//! it. A requirement in another range, or one the locked version meets, gives Cargo a lock to
//! rewrite, and the reader reports as it always did. Every case runs in a Cargo home the test
//! owns, which vendors `house-rules` 4.5.6 as the replacement for crates.io, and 4.6.0 and 5.0.0
//! after the lock where a case needs a version to ask for.
//!
//! The package outside the workspace is `y`, in a directory beside the root, and its optional
//! dependency is `z`. A test writes `z` and then changes `y` so that a feature enables `z`, so
//! the lock was made with `z` off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::cargo_configuration::HomeVariables;
use super::super::fixture::{CargoHome, Fixture, cargo_version};
use super::super::read::read_document;
use super::{
    Verdict, Y_DEFAULT_ENABLES_Z, Y_OPTIONAL_Z, assert_both_say_in_home, assert_reader_refuses,
    assert_stale_report_is_the_relocked_one_in_home, cargo_verdict_in_home, locked_members,
    write_app,
};
use crate::workspace::crates_io_source::CRATES_IO_SOURCE;
use crate::workspace::{LockedPackageId, Network};

/// What the member `app` writes to depend on `house-rules` and to wear it, and on `y`.
const APP_WEARS_HOUSE_RULES_AND_DEPENDS_ON_Y: &str = "[dependencies]\nhouse-rules = \"4\"\n\
     y = { path = \"../../y\" }\n[package.metadata.skeletons.house-rules]\n";

/// A workspace whose member `app` wears `house-rules` and depends on `y` and `z` beside the root,
/// each written as its tail after its `[package]` table, locked in a home that vendors
/// `house-rules` 4.5.6 and nothing newer. The lock holds whatever `y` enables, which is `z` only
/// when `y_tail` says so.
fn locked_workspace(y_tail: &str, z_tail: &str) -> (Fixture, CargoHome) {
    let home = CargoHome::new();
    home.vendor("house-rules", "4.5.6");
    let fixture = locked_members(&["app"]);
    write_app(&fixture, APP_WEARS_HOUSE_RULES_AND_DEPENDS_ON_Y);
    fixture.package("../y", "y", y_tail);
    fixture.package("../z", "z", z_tail);
    let toolchain = cargo_version();
    home.lock(&fixture.root, Network::Refused)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
    (fixture, home)
}

/// [`locked_workspace`] with a `y` whose optional `z` nothing enables.
fn locked_with_optional_z(z_tail: &str) -> (Fixture, CargoHome) {
    locked_workspace(Y_OPTIONAL_Z, z_tail)
}

/// Has `y`'s default feature enable `z`, after the lock was made without it.
fn enable_z(fixture: &Fixture) {
    fixture.package("../y", "y", Y_DEFAULT_ENABLES_Z);
}

/// The manifest of the package `name` beside the root.
fn manifest_of(fixture: &Fixture, name: &str) -> PathBuf {
    fixture.root.with_file_name(name).join("Cargo.toml")
}

/// What the reader says when `requirement` of `house-rules` in `manifest` is one 4.5.6 cannot
/// meet and a later version in its range can.
fn refusal_of(manifest: &Path, requirement: &str) -> String {
    format!(
        "{}: `house-rules` requires `house-rules` `{requirement}`, which the worn package's \
         locked version 4.5.6 does not meet within its compatible range, so cargo cannot resolve \
         the workspace once a feature enables it",
        manifest.display()
    )
}

/// Requires that Cargo, in `home`, cannot resolve the workspace at `fixture`'s root and that the
/// reader, given that home, refuses it as something only Cargo follows, with the detail `detail`.
fn assert_cargo_cannot_resolve_and_reader_refuses(
    fixture: &Fixture,
    home: &CargoHome,
    detail: &str,
) {
    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict_in_home(fixture, home),
        Verdict::Refused,
        "cargo, on {toolchain}"
    );
    assert_reader_refuses(fixture, &HomeVariables::cargo_home(home.path()), detail);
}

/// Where the reader is to find `house-rules` 4.5.6 from crates.io: the vendored directory.
fn house_rules_at(home: &CargoHome) -> BTreeMap<LockedPackageId, PathBuf> {
    BTreeMap::from([(
        LockedPackageId {
            name: "house-rules".to_owned(),
            version: "4.5.6".to_owned(),
            source: CRATES_IO_SOURCE.to_owned(),
        },
        home.vendored_directory("house-rules", "4.5.6"),
    )])
}

#[test]
fn an_optional_dependency_enabled_after_the_lock_that_needs_a_newer_worn_version_is_refused() {
    // The direct shape: `app` locks `house-rules` at 4.5.6 through `^4`. `z` asks for `^4.6`,
    // which is in the range 4.5.6 is in, and 4.6.0 is in the registry once the lock is made.
    // Once `y` enables `z`, Cargo keeps the one 4.x it has, finds it too old, and fails to
    // select a version, and a relock does not help. The reader cannot read that from the lock,
    // and says so, naming `z`'s manifest.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    enable_z(&fixture);
    home.vendor("house-rules", "4.6.0");

    assert_cargo_cannot_resolve_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of(&manifest_of(&fixture, "z"), "4.6"),
    );
}

#[test]
fn a_dependency_of_an_optional_package_that_needs_a_newer_worn_version_is_refused() {
    // The indirect shape: `z` takes the requirement through `x`, a package it depends on by
    // path, which asks for `^4.6`. The reader follows the path from `z` and names `x`'s
    // manifest. (The directory `w` is the workspace's own, so the package is not called that.)
    let (fixture, home) = locked_with_optional_z("[dependencies]\nx = { path = \"../x\" }\n");
    fixture.package("../x", "x", "[dependencies]\nhouse-rules = \"4.6\"\n");
    enable_z(&fixture);
    home.vendor("house-rules", "4.6.0");

    assert_cargo_cannot_resolve_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of(&manifest_of(&fixture, "x"), "4.6"),
    );
}

#[test]
fn an_optional_dependency_that_is_never_enabled_is_refused_though_cargo_reads_the_workspace() {
    // The cost of refusing: nothing enables `z`, so Cargo never reads its requirement and says
    // the lock is current. The reader cannot see which features are on, and refuses
    // whenever one could break resolution, which is a workspace Cargo reads.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");

    assert_eq!(
        cargo_verdict_in_home(&fixture, &home),
        Verdict::Current,
        "cargo, on {}",
        cargo_version()
    );
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &refusal_of(&manifest_of(&fixture, "z"), "4.6"),
    );
}

#[test]
fn an_optional_dependency_of_the_outsider_that_is_the_worn_package_is_refused() {
    // `y` declares `house-rules` itself as an optional dependency asking for `^4.6`, a
    // declaration in a manifest the reader already holds. The reader names `y`'s manifest.
    let optional_house_rules =
        "[dependencies]\nhouse-rules = { version = \"4.6\", optional = true }\n";
    let (fixture, home) = locked_workspace(optional_house_rules, "");
    fixture.package(
        "../y",
        "y",
        &format!("{optional_house_rules}[features]\ndefault = [\"house-rules\"]\n"),
    );
    home.vendor("house-rules", "4.6.0");

    assert_cargo_cannot_resolve_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of(&manifest_of(&fixture, "y"), "4.6"),
    );
}

#[test]
fn a_requirement_the_locked_worn_version_meets_is_reported_as_after_a_relock() {
    // The control against over-refusing a met requirement: `z` asks for `^4.5`, which 4.5.6
    // meets. Enabling `z` only makes the lock lack it, so Cargo's `--locked` aborts and a relock
    // fixes it, and the reader reads the lock as it stands, which is the report once Cargo
    // brings it up to date. A requirement the locked version meets is no reason to refuse.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.5\"\n");
    enable_z(&fixture);

    assert_stale_report_is_the_relocked_one_in_home(&fixture, &home, &house_rules_at(&home));
}

#[test]
fn a_requirement_in_another_range_than_the_locked_one_is_reported_as_after_a_relock() {
    // The control against over-refusing another range: `z` asks for `^5`. Cargo keeps one
    // version for each range, so it adds 5.0.0 beside 4.5.6 and leaves the worn package alone,
    // and a relock fixes the lock. A requirement in a range the lock does not hold is no reason
    // to refuse.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"5\"\n");
    enable_z(&fixture);
    home.vendor("house-rules", "5.0.0");

    assert_stale_report_is_the_relocked_one_in_home(&fixture, &home, &house_rules_at(&home));
}

#[test]
fn a_requirement_of_a_registry_package_is_not_read_and_the_report_is_the_locked_version() {
    // The limit that stays: `y` enables `r`, a crate from the registry whose own manifest asks
    // for `^4.6`. Cargo fails to resolve it as it does for a manifest the reader holds, and the
    // reader has no manifest of `r` to read, so it reports `house-rules` at the version the
    // lock names. The lock is current to the reader and holds 4.5.6, which is what its report
    // shows.
    let (fixture, home) = locked_workspace(
        "[dependencies]\nr = { version = \"1\", optional = true }\n",
        "",
    );
    fixture.package(
        "../y",
        "y",
        "[dependencies]\nr = { version = \"1\", optional = true }\n\
         [features]\ndefault = [\"r\"]\n",
    );
    home.vendor_with_dependencies("r", "1.0.0", "[dependencies]\nhouse-rules = \"4.6\"\n");
    home.vendor("house-rules", "4.6.0");

    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict_in_home(&fixture, &home),
        Verdict::Refused,
        "cargo, on {toolchain}"
    );
    let lockfile = std::fs::read_to_string(fixture.root.join("Cargo.lock")).expect("the lockfile");
    assert!(
        lockfile.contains("name = \"house-rules\"\nversion = \"4.5.6\""),
        "the lock names 4.5.6, on {toolchain}:\n{lockfile}"
    );
    let document = read_document(
        &fixture.root,
        &BTreeMap::new(),
        &HomeVariables::cargo_home(home.path()),
    )
    .unwrap_or_else(|error| panic!("{toolchain}: the reader refused the workspace: {error}"));
    let unlocated: Vec<(&str, &str)> = document
        .unlocated
        .iter()
        .map(|package| (package.name.as_str(), package.version.as_str()))
        .collect();
    assert_eq!(unlocated, [("house-rules", "4.5.6")], "on {toolchain}");
}

#[test]
fn an_optional_dependency_whose_directory_is_gone_is_read_past() {
    // Measured on cargo 1.85.0 and 1.95.0: with `z` off, Cargo does not load `z`, so a `z`
    // whose directory is removed after the lock leaves the workspace current, and it fails
    // only once a feature enables `z`. The reader cannot know the feature is off, and a refusal
    // here would fire on workspaces Cargo reads, so it passes over a package it cannot read, as
    // it does a requirement `semver` cannot parse.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    std::fs::remove_dir_all(fixture.root.with_file_name("z")).expect("take z away");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}

#[test]
fn a_dependency_of_an_optional_package_whose_directory_is_gone_is_read_past() {
    // The same one step further: `z` is there and reads, and `x`, which it depends on by path,
    // was never written. Cargo does not load `z` while it is off, and the reader passes over `x`.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nx = { path = \"../x\" }\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}

#[test]
fn a_dev_dependency_of_an_optional_package_is_not_a_requirement_on_the_worn_package() {
    // Cargo resolves the dev-dependencies of the workspace's members and of no package outside
    // it, so `z`'s `^4.6` there is no requirement on anything. Both say the lock is current.
    let (fixture, home) = locked_with_optional_z("[dev-dependencies]\nhouse-rules = \"4.6\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}
