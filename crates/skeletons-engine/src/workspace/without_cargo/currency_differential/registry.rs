//! The cases where a package comes from a registry, which is a directory of vendored crates that
//! replaces crates.io in a Cargo home the test owns.

use super::super::fixture::{CargoHome, Fixture, cargo_version};
use super::{Verdict, assert_both_say_in_home, locked_members, write_app};
use crate::workspace::Network;

/// A workspace whose member `app` depends on the vendored crates in `dependencies`, locked
/// offline in a home that vendors `r` 1.4.0 and 2.1.0 and `s` 0.3.0, with the home.
fn locked_vendored(dependencies: &str) -> (Fixture, CargoHome) {
    let home = CargoHome::new();
    home.vendor("r", "1.4.0");
    home.vendor("r", "2.1.0");
    home.vendor("s", "0.3.0");
    let fixture = locked_members(&["app"]);
    write_app(&fixture, &format!("[dependencies]\n{dependencies}"));
    let toolchain = cargo_version();
    home.lock(&fixture.root, Network::Refused)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
    (fixture, home)
}

#[test]
fn a_registry_dependency_just_locked_is_current() {
    let (fixture, home) = locked_vendored("r = \"1\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}

#[test]
fn a_registry_requirement_changed_to_one_the_lock_does_not_meet_is_aborted() {
    let (fixture, home) = locked_vendored("r = \"1\"\n");
    write_app(&fixture, "[dependencies]\nr = \"2\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Aborted);
}

#[test]
fn a_registry_requirement_narrowed_to_one_the_lock_still_meets_is_current() {
    let (fixture, home) = locked_vendored("r = \"1\"\n");
    write_app(&fixture, "[dependencies]\nr = \"1.4\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}

#[test]
fn an_added_registry_dependency_is_aborted() {
    let (fixture, home) = locked_vendored("r = \"1\"\n");
    write_app(&fixture, "[dependencies]\nr = \"1\"\ns = \"0.3\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Aborted);
}

#[test]
fn a_removed_registry_dependency_is_aborted() {
    let (fixture, home) = locked_vendored("r = \"1\"\n");
    write_app(&fixture, "");

    assert_both_say_in_home(&fixture, &home, Verdict::Aborted);
}
