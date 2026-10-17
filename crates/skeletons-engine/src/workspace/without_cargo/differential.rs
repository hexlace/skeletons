//! The document a workspace read without Cargo amounts to, compared with the one `cargo metadata`
//! prints for the same files.
//!
//! Each test writes a workspace of path packages into a temporary directory, has the cargo that
//! runs the tests lock it, and reads it both ways. Which cargo that is decides the toolchain:
//! `cargo test` and `cargo +1.85.0 test` each hand their own to the tests as `$CARGO`, which
//! `crate::cargo::command` runs, so the same suite checks each toolchain. Every failure names the
//! `cargo --version` it ran under.
//!
//! The documents are compared on what the readers of a workspace take from them, and keyed by a
//! package's name and version, never by its id: the ids differ by construction. That is the
//! packages in the graph, the members, each member's declared dependencies and metadata, and each
//! member's resolve edges as the edge's name and the name and version of the package it reaches.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::error::ReadWithoutCargoError;
use super::fixture::{Fixture, cargo_version, lock_with_cargo_in};
use super::read::read_document;
use crate::workspace::schema::Document;
use crate::workspace::{Network, cargo_metadata};

/// A package as `(name, version)`.
type Identity = (String, String);

/// What the readers of a workspace take from one member of a document.
#[derive(Debug, PartialEq, Eq)]
struct MemberShape {
    manifest_path: PathBuf,
    /// Each declared dependency as `(package, rename)`.
    dependencies: BTreeSet<(String, Option<String>)>,
    /// `[package.metadata]` as JSON text.
    metadata: String,
    /// Each resolve edge as `(edge name, package name, package version)`.
    edges: BTreeSet<(String, String, String)>,
}

/// What the readers of a workspace take from a document.
#[derive(Debug, PartialEq, Eq)]
struct Shape {
    root: PathBuf,
    /// Every package in the graph as `(name, version, manifest path)`.
    packages: BTreeSet<(String, String, PathBuf)>,
    members: BTreeMap<Identity, MemberShape>,
}

/// The shape of `document`.
fn shape(document: &Document) -> Shape {
    let identity_of = |id: &str| -> Identity {
        let located = document
            .packages
            .iter()
            .find(|package| package.id == id)
            .map(|package| (package.name.clone(), package.version.clone()));
        let unlocated = || {
            document
                .unlocated
                .iter()
                .find(|package| package.id == id)
                .map(|package| (package.name.clone(), package.version.clone()))
        };
        located
            .or_else(unlocated)
            .unwrap_or_else(|| panic!("the document names a package it does not hold: {id}"))
    };

    let mut members = BTreeMap::new();
    for member_id in &document.workspace_members {
        let package = document
            .packages
            .iter()
            .find(|package| &package.id == member_id)
            .unwrap_or_else(|| panic!("a member the document does not hold: {member_id}"));
        let node = document
            .resolve
            .nodes
            .iter()
            .find(|node| &node.id == member_id)
            .unwrap_or_else(|| panic!("a member with no resolve node: {member_id}"));
        let edges = node
            .deps
            .iter()
            .map(|edge| {
                let (name, version) = identity_of(&edge.pkg);
                (edge.name.clone(), name, version)
            })
            .collect();
        members.insert(
            (package.name.clone(), package.version.clone()),
            MemberShape {
                manifest_path: package.manifest_path.clone(),
                dependencies: package
                    .dependencies
                    .iter()
                    .map(|dependency| (dependency.name.clone(), dependency.rename.clone()))
                    .collect(),
                metadata: package.metadata.to_string(),
                edges,
            },
        );
    }

    Shape {
        root: document.workspace_root.clone(),
        packages: document
            .packages
            .iter()
            .map(|package| {
                (
                    package.name.clone(),
                    package.version.clone(),
                    package.manifest_path.clone(),
                )
            })
            .collect(),
        members,
    }
}

/// Locks the fixture with cargo, then reads it from `start` both ways and requires the same shape.
fn assert_documents_agree(fixture: &Fixture, start: &Path) {
    let toolchain = cargo_version();
    fixture
        .lock_with_cargo()
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
    assert_reads_agree(start);
}

/// Reads the locked workspace from `start` both ways and requires the same shape.
fn assert_reads_agree(start: &Path) {
    let toolchain = cargo_version();
    let own = read_document(start, &BTreeMap::new())
        .unwrap_or_else(|error| panic!("{toolchain}: the reader refused the workspace: {error}"));
    let cargo = cargo_metadata::fetch(start, Network::Refused)
        .unwrap_or_else(|error| panic!("{toolchain}: cargo metadata refused it: {error:?}"));

    assert_eq!(shape(&own), shape(&cargo), "on {toolchain}");
}

/// The matrix's tests read from the root.
fn assert_agree_at_root(fixture: &Fixture) {
    assert_documents_agree(fixture, &fixture.root);
}

/// A skeleton-like wearer `app` in a workspace rooted at `w`, with `app_extra` after its
/// `[package]` table.
fn workspace_with_app(root_extra: &str, app_extra: &str) -> Fixture {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        &format!("[workspace]\nmembers = [\"app\"]\n{root_extra}"),
    );
    fixture.package("app", "app", app_extra);
    fixture
}

#[test]
fn glob_members_with_a_hidden_directory_and_a_file_beside_them_agree() {
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    fixture.package("crates/a", "a", "");
    fixture.package("crates/.hidden", "hidden", "");
    fixture.write("crates/file.txt", "not a member");

    assert_agree_at_root(&fixture);
}

#[test]
fn an_excluded_directory_and_an_excluded_path_dependency_agree() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/old\", \"tools\"]\n",
    );
    fixture.package(
        "crates/a",
        "a",
        "[dependencies]\ntool = { path = \"../../tools/tool\" }\n",
    );
    fixture.package("crates/old", "old", "");
    fixture.package("tools/tool", "tool", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn an_in_root_path_dependency_becomes_a_member_and_an_out_of_root_one_does_not() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\ninside = { path = \"../inside\" }\noutside = { path = \"../../outside\" }\n",
    );
    fixture.package("inside", "inside", "");
    fixture.package("../outside", "outside", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_path_dependency_of_an_outsider_is_in_the_graph_only_when_cargo_puts_it_there() {
    // `outside` brings along its normal and build path dependencies, not its dev-dependency and
    // not the optional one no feature enables.
    let fixture = workspace_with_app(
        "",
        "[dependencies]\noutside = { path = \"../../outside\" }\n",
    );
    fixture.package(
        "../outside",
        "outside",
        "[dependencies]\nneeded = { path = \"../needed\" }\n\
         optional = { path = \"../optional\", optional = true }\n\
         [dev-dependencies]\ndeveloping = { path = \"../developing\" }\n\
         [build-dependencies]\nbuilding = { path = \"../building\" }\n",
    );
    for name in ["needed", "optional", "developing", "building"] {
        fixture.package(&format!("../{name}"), name, "");
    }

    assert_agree_at_root(&fixture);
}

#[test]
fn a_renamed_dependency_agrees() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\nrenamed = { path = \"../lib-x\", package = \"lib-x\" }\n",
    );
    fixture.package("lib-x", "lib-x", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn dev_build_and_cfg_windows_dependencies_agree() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\nnormal = { path = \"../normal\" }\n\
         [dev-dependencies]\ndeveloping = { path = \"../developing\" }\n\
         [build-dependencies]\nbuilding = { path = \"../building\" }\n\
         [target.'cfg(windows)'.dependencies]\nwindowed = { path = \"../windowed\" }\n",
    );
    for name in ["normal", "developing", "building", "windowed"] {
        fixture.package(name, name, "");
    }

    assert_agree_at_root(&fixture);
}

#[test]
fn workspace_true_dependencies_and_versions_agree() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"app\"]\n[workspace.package]\nversion = \"2.3.4\"\n\
         [workspace.dependencies]\nshared = { path = \"shared\" }\n",
    );
    fixture.package(
        "app",
        "app",
        "version.workspace = true\n[dependencies]\nshared = { workspace = true }\n",
    );
    fixture.package("shared", "shared", "version.workspace = true\n");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_datetime_in_metadata_agrees() {
    let fixture = workspace_with_app(
        "",
        "[package.metadata.skeletons.x]\nwhen = 1979-05-27T07:32:00Z\nday = 1979-05-27\n\
         [package.metadata]\nplain = \"text\"\n",
    );

    assert_agree_at_root(&fixture);
}

#[test]
fn a_lib_name_differing_from_the_package_name_agrees_plain_and_renamed() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\nlib-a = { path = \"../lib-a\" }\n\
         ren-b = { path = \"../lib-b\", package = \"lib-b\" }\n",
    );
    fixture.package("lib-a", "lib-a", "[lib]\nname = \"zed\"\n");
    fixture.package("lib-b", "lib-b", "[lib]\nname = \"yarn\"\n");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_lib_path_with_no_lib_name_agrees() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\ncustom-path = { path = \"../custom-path\" }\n",
    );
    fixture.write(
        "custom-path/Cargo.toml",
        "[package]\nname = \"custom-path\"\n[lib]\npath = \"src/other.rs\"\n",
    );
    fixture.write("custom-path/src/other.rs", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_package_with_no_library_gets_no_edge_on_either_side() {
    // `bin-only` has only a binary, `autolib-off` has a `src/lib.rs` it turned off, and
    // `tests-only` has only an integration test: Cargo writes no edge for any of them.
    let fixture = workspace_with_app(
        "",
        "[dependencies]\nbin-only = { path = \"../bin-only\" }\n\
         autolib-off = { path = \"../autolib-off\" }\ntests-only = { path = \"../tests-only\" }\n\
         renamed-bin = { path = \"../renamed-bin\", package = \"renamed-bin\" }\n",
    );
    fixture.write("bin-only/Cargo.toml", "[package]\nname = \"bin-only\"\n");
    fixture.write("bin-only/src/main.rs", "fn main() {}\n");
    fixture.write(
        "autolib-off/Cargo.toml",
        "[package]\nname = \"autolib-off\"\nautolib = false\n[[bin]]\nname = \"x\"\npath = \"src/x.rs\"\n",
    );
    fixture.write("autolib-off/src/lib.rs", "");
    fixture.write("autolib-off/src/x.rs", "fn main() {}\n");
    fixture.write(
        "tests-only/Cargo.toml",
        "[package]\nname = \"tests-only\"\n",
    );
    fixture.write("tests-only/tests/t.rs", "");
    fixture.write(
        "renamed-bin/Cargo.toml",
        "[package]\nname = \"renamed-bin\"\n",
    );
    fixture.write("renamed-bin/src/main.rs", "fn main() {}\n");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_hyphenated_package_name_agrees_plain_and_renamed() {
    let fixture = workspace_with_app(
        "",
        "[dependencies]\nhy-phen = { path = \"../hy-phen\" }\n\
         ren-h = { path = \"../hy-phen-two\", package = \"hy-phen-two\" }\n",
    );
    fixture.package("hy-phen", "hy-phen", "");
    fixture.package("hy-phen-two", "hy-phen-two", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn reading_from_a_directory_below_a_member_agrees() {
    let fixture = workspace_with_app("", "[dependencies]\nd = { path = \"../d\" }\n");
    fixture.package("d", "d", "");
    std::fs::create_dir_all(fixture.root.join("app/src/deeper")).expect("a directory");

    assert_documents_agree(&fixture, &fixture.root.join("app/src/deeper"));
}

#[test]
fn a_lone_package_with_a_path_dependency_agrees() {
    // Without a `[workspace]`, the dependency under the root is in the graph and no member.
    let fixture = Fixture::new();
    fixture.package("", "solo", "[dependencies]\ninner = { path = \"inner\" }\n");
    fixture.package("inner", "inner", "");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_package_the_root_above_excludes_agrees_as_its_own_workspace() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"listed\"]\nexclude = [\"apart\"]\n",
    );
    fixture.package("listed", "listed", "");
    fixture.package(
        "apart",
        "apart",
        "[dependencies]\nd = { path = \"../d\" }\n",
    );
    fixture.package("d", "d", "");

    // `apart` is its own workspace, so cargo locks it where it stands.
    let apart = fixture.root.join("apart");
    lock_with_cargo_in(&apart)
        .unwrap_or_else(|stderr| panic!("{} could not lock it: {stderr}", cargo_version()));
    assert_reads_agree(&apart);
}

#[test]
fn a_dependency_key_with_two_source_paths_is_refused_by_cargo_and_by_the_reader() {
    let toolchain = cargo_version();
    let fixture = workspace_with_app(
        "",
        "[dependencies]\ndep = { path = \"../../d1\" }\n[dev-dependencies]\ndep = { path = \"../../d2\" }\n",
    );
    fixture.package("../d1", "dep", "version = \"1.0.0\"\n");
    fixture.package("../d2", "dep", "version = \"2.0.0\"\n");

    let locked = fixture.lock_with_cargo();
    let read = read_document(&fixture.root, &BTreeMap::new());

    let stderr = locked.expect_err(&format!("{toolchain} locked a key with two paths"));
    assert!(
        stderr.contains("has different source paths depending on the build target"),
        "{toolchain} refused it for another reason: {stderr}"
    );
    assert!(
        matches!(read, Err(ReadWithoutCargoError::ManifestUnreadable { .. })),
        "{toolchain}: the reader gave {:?}",
        read.map(|_| ())
    );
}

#[test]
fn one_package_under_two_keys_is_read_by_cargo_and_refused_as_unsupported_by_the_reader() {
    // Cargo reads two versions of one package under two keys; this reader does not follow how it
    // names their edges, so it refuses by name.
    let toolchain = cargo_version();
    let fixture = workspace_with_app(
        "",
        "[dependencies]\ndep = { path = \"../../d1\" }\nolder = { path = \"../../d2\", package = \"dep\" }\n",
    );
    fixture.package("../d1", "dep", "version = \"1.0.0\"\n");
    fixture.package("../d2", "dep", "version = \"2.0.0\"\n");

    fixture
        .lock_with_cargo()
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock it: {stderr}"));
    cargo_metadata::fetch(&fixture.root, Network::Refused)
        .unwrap_or_else(|error| panic!("{toolchain} refused it: {error:?}"));
    let read = read_document(&fixture.root, &BTreeMap::new());

    assert!(
        matches!(read, Err(ReadWithoutCargoError::Unsupported { .. })),
        "{toolchain}: the reader gave {:?}",
        read.map(|_| ())
    );
}

#[test]
fn a_dependency_left_with_only_a_build_script_is_refused_by_cargo_and_by_the_reader() {
    let toolchain = cargo_version();
    let fixture = workspace_with_app("", "[dependencies]\ndep = { path = \"../dep\" }\n");
    fixture.package("dep", "dep", "");
    fixture
        .lock_with_cargo()
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock it: {stderr}"));
    std::fs::remove_file(fixture.root.join("dep/src/lib.rs")).expect("remove the library");
    fixture.write("dep/build.rs", "fn main() {}\n");

    let cargo = cargo_metadata::fetch(&fixture.root, Network::Refused);
    let read = read_document(&fixture.root, &BTreeMap::new());

    assert!(cargo.is_err(), "{toolchain} read a package with no target");
    assert!(
        matches!(read, Err(ReadWithoutCargoError::ManifestUnreadable { .. })),
        "{toolchain}: the reader gave {:?}",
        read.map(|_| ())
    );
}
