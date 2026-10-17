//! The cases where Cargo's configuration files change what Cargo reads, which the reader
//! refuses by name: `paths`, `[patch]`, and a source replacement it reads past.
//!
//! Cargo reads each of these without complaint and gives a document, so the verdict the reader is
//! held to is its own, `Unsupported`, and Cargo's is the control that shows the configuration
//! did change the document: the package it reads is the one the configuration names. Every case
//! runs in a Cargo home the test owns, which vendors the registry crate `house-rules` 1.1.0.

use std::path::{Path, PathBuf};

use super::super::cargo_configuration::HomeVariables;
use super::super::file_system::normalized;
use super::super::fixture::{CargoHome, Fixture, GitRepository, cargo_version};
use super::{
    Verdict, assert_both_say_in_home, assert_reader_refuses, cargo_verdict_in_home, locked_members,
    write_app,
};
use crate::workspace::schema::{Document, Package};
use crate::workspace::{Network, cargo_metadata};

/// A workspace whose member `app` depends on `house-rules = "1"`, in a Cargo home that vendors
/// `house-rules` 1.1.0 as the replacement for crates.io. Nothing is locked yet.
fn unlocked_app_on_house_rules() -> (Fixture, CargoHome) {
    let home = CargoHome::new();
    home.vendor("house-rules", "1.1.0");
    let fixture = locked_members(&["app"]);
    write_app(&fixture, "[dependencies]\nhouse-rules = \"1\"\n");
    (fixture, home)
}

/// Has Cargo lock the workspace at `fixture`'s root in `home`.
fn lock_in(home: &CargoHome, fixture: &Fixture, network: Network) {
    let toolchain = cargo_version();
    home.lock(&fixture.root, network)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
}

/// A copy of `house-rules` 1.1.0 in the directory `name` beside the workspace's root, which a
/// configuration file can point at, and the directory.
fn write_copy_of_house_rules(fixture: &Fixture, name: &str) -> PathBuf {
    fixture.package(
        &format!("../{name}/house-rules"),
        "house-rules",
        "version = \"1.1.0\"\n",
    );
    fixture.root.with_file_name(name).join("house-rules")
}

/// Cargo's document for the workspace at `fixture`'s root, read in `home`.
fn cargo_document_in_home(fixture: &Fixture, home: &CargoHome) -> Document {
    let toolchain = cargo_version();
    cargo_metadata::fetch_in_cargo_home(&fixture.root, home.path(), home.home())
        .unwrap_or_else(|error| panic!("{toolchain}: cargo refused the workspace: {error:?}"))
}

/// The package `house-rules` of `document`.
fn house_rules(document: &Document) -> &Package {
    let mut named = document
        .packages
        .iter()
        .filter(|package| package.name == "house-rules");
    let package = named.next().expect("the document holds house-rules");
    assert!(named.next().is_none(), "the document holds it once");
    package
}

/// Requires that Cargo, in `home`, says the lockfile is current and reads `house-rules` from
/// `directory`, the package a configuration file named, and not from the registry.
fn assert_cargo_reads_house_rules_from(fixture: &Fixture, home: &CargoHome, directory: &Path) {
    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict_in_home(fixture, home),
        Verdict::Current,
        "cargo, on {toolchain}"
    );
    assert_eq!(
        house_rules(&cargo_document_in_home(fixture, home)).manifest_path,
        directory.join("Cargo.toml"),
        "cargo's package, on {toolchain}"
    );
}

/// The text of a configuration file that overrides with the directory `directory`.
fn paths_to(directory: &Path) -> String {
    format!("paths = [\"{}\"]\n", directory.display())
}

#[test]
fn a_paths_override_in_the_workspaces_own_configuration_is_refused() {
    // `paths` puts `house-rules` at another directory, gets past `--locked` and leaves the lock
    // as it was, so the lock is current to Cargo and the document it gives has the package
    // somewhere else. The reader cannot see that without doing what Cargo does, and refuses.
    let (fixture, home) = unlocked_app_on_house_rules();
    lock_in(&home, &fixture, Network::Refused);
    let copy = write_copy_of_house_rules(&fixture, "override");
    fixture.write(".cargo/config.toml", &paths_to(&copy));

    assert_cargo_reads_house_rules_from(&fixture, &home, &copy);
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &format!(
            "`paths` in {}",
            fixture.root.join(".cargo/config.toml").display()
        ),
    );
}

#[test]
fn a_paths_override_in_the_directory_above_the_root_is_refused() {
    // Cargo reads the configuration of every directory above the one it starts in, so the file
    // beside the workspace's directory changes the document as much as the workspace's own.
    let (fixture, home) = unlocked_app_on_house_rules();
    lock_in(&home, &fixture, Network::Refused);
    let copy = write_copy_of_house_rules(&fixture, "override");
    fixture.write("../.cargo/config.toml", &paths_to(&copy));

    assert_cargo_reads_house_rules_from(&fixture, &home, &copy);
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &format!(
            "`paths` in {}",
            normalized(&fixture.root.join("../.cargo/config.toml")).display()
        ),
    );
}

#[test]
fn a_paths_override_in_the_cargo_home_is_refused() {
    // The Cargo home's configuration is the user's, and Cargo reads it last. The reader reads it
    // through the home it is given, the one the test's Cargo runs with.
    let (fixture, home) = unlocked_app_on_house_rules();
    lock_in(&home, &fixture, Network::Refused);
    let copy = write_copy_of_house_rules(&fixture, "override");
    let configuration = home.path().join("config.toml");
    let vendored = std::fs::read_to_string(&configuration).expect("the vendored replacement");
    std::fs::write(&configuration, format!("{}{vendored}", paths_to(&copy)))
        .expect("the home's configuration");

    assert_cargo_reads_house_rules_from(&fixture, &home, &copy);
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &format!("`paths` in {}", configuration.display()),
    );
}

/// Requires that a `[patch.crates-io]` of `house-rules` to a copy at a path, written to the
/// configuration file `location` under the root, locked with the patch, is current to Cargo,
/// which reads the patched package, and is refused by the reader.
fn assert_path_patch_is_refused(location: &str) {
    let (fixture, home) = unlocked_app_on_house_rules();
    let copy = write_copy_of_house_rules(&fixture, "patched");
    fixture.write(
        location,
        &format!(
            "[patch.crates-io]\nhouse-rules = {{ path = \"{}\" }}\n",
            copy.display()
        ),
    );
    lock_in(&home, &fixture, Network::Refused);

    let toolchain = cargo_version();
    assert_cargo_reads_house_rules_from(&fixture, &home, &copy);
    assert!(
        house_rules(&cargo_document_in_home(&fixture, &home))
            .id
            .starts_with("path+file://"),
        "cargo reads a patched package with no source, on {toolchain}"
    );
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &format!(
            "`[patch.crates-io]` in {}",
            normalized(&fixture.root.join(location)).display()
        ),
    );
}

#[test]
fn a_patch_by_path_in_the_workspaces_own_configuration_is_refused() {
    // The patched package is locked with no source, which the reader would call a stale lock for
    // a dependency declared from a registry. It is Cargo's resolution, so it is refused instead.
    assert_path_patch_is_refused(".cargo/config.toml");
}

#[test]
fn a_patch_by_path_in_the_directory_above_the_root_is_refused() {
    assert_path_patch_is_refused("../.cargo/config.toml");
}

/// Requires that a `[patch.crates-io]` of `house-rules` to a git repository, written to the
/// configuration file `location` under the root, locked with the patch, is current to Cargo,
/// which reads the package from the repository, and is refused by the reader.
fn assert_git_patch_is_refused(location: &str) {
    let (fixture, home) = unlocked_app_on_house_rules();
    let repository = GitRepository::new("house-rules");
    fixture.write(
        location,
        &format!(
            "[patch.crates-io]\nhouse-rules = {{ git = \"{}\" }}\n",
            repository.url()
        ),
    );
    // Locking fetches the repository into the home, which is what lets Cargo read the patched
    // package offline afterwards.
    lock_in(&home, &fixture, Network::Allowed);

    let toolchain = cargo_version();
    assert_eq!(
        cargo_verdict_in_home(&fixture, &home),
        Verdict::Current,
        "cargo, on {toolchain}"
    );
    assert!(
        house_rules(&cargo_document_in_home(&fixture, &home))
            .id
            .starts_with("git+file://"),
        "cargo reads the package from the repository, on {toolchain}"
    );
    assert_reader_refuses(
        &fixture,
        &HomeVariables::cargo_home(home.path()),
        &format!(
            "`[patch.crates-io]` in {}",
            normalized(&fixture.root.join(location)).display()
        ),
    );
}

#[test]
fn a_patch_by_git_in_the_workspaces_own_configuration_is_refused() {
    assert_git_patch_is_refused(".cargo/config.toml");
}

#[test]
fn a_patch_by_git_in_the_directory_above_the_root_is_refused() {
    assert_git_patch_is_refused("../.cargo/config.toml");
}

#[test]
fn a_source_replacement_in_the_workspaces_own_configuration_is_read_past() {
    // The control against over-refusing: the replacement of crates.io lives in the workspace's
    // own `.cargo` and the Cargo home holds none. It moves where Cargo reads `house-rules` and
    // never which package the lock names, so both say the lock is current: reading the
    // configuration must not turn a replacement into a refusal.
    let (fixture, home) = unlocked_app_on_house_rules();
    home.move_configuration_into(&fixture.root.join(".cargo"));
    lock_in(&home, &fixture, Network::Refused);

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}
