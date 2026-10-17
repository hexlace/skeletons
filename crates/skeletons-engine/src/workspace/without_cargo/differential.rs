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
use super::targets::matrix;
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
pub(super) struct Shape {
    root: PathBuf,
    /// Every package in the graph as `(name, version, manifest path)`.
    packages: BTreeSet<(String, String, PathBuf)>,
    members: BTreeMap<Identity, MemberShape>,
}

impl Shape {
    /// This shape with its packages cut down to the members and the packages their resolve edges
    /// reach: what a report reads, as against the whole graph.
    pub(super) fn reached_by_members(mut self) -> Self {
        let mut reached: BTreeSet<Identity> = self.members.keys().cloned().collect();
        for member in self.members.values() {
            for (_, name, version) in &member.edges {
                reached.insert((name.clone(), version.clone()));
            }
        }
        self.packages
            .retain(|(name, version, _)| reached.contains(&(name.clone(), version.clone())));
        self
    }
}

/// The shape of `document`.
pub(super) fn shape(document: &Document) -> Shape {
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
        "[dependencies]\ninside = { path = \"../inside\" }\n\
         outside = { path = \"../../outside\" }\n",
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
fn an_optional_dependency_of_an_outsider_enabled_by_default_and_just_locked_agrees() {
    // `outside` enables its optional path dependency `enabled` by default, and cargo locks it
    // with `enabled` listed under `outside`. The reader takes that listing as the sign that the
    // feature is on, so both documents must hold `enabled`, with the same shape everywhere else.
    let fixture = workspace_with_app(
        "",
        "[dependencies]\noutside = { path = \"../../outside\" }\n",
    );
    fixture.package(
        "../outside",
        "outside",
        "[dependencies]\nenabled = { path = \"../enabled\", optional = true }\n\
         [features]\ndefault = [\"enabled\"]\n",
    );
    fixture.package("../enabled", "enabled", "");
    let toolchain = cargo_version();
    fixture
        .lock_with_cargo()
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));

    let own = read_document(&fixture.root, &BTreeMap::new())
        .unwrap_or_else(|error| panic!("{toolchain}: the reader refused the workspace: {error}"));
    let cargo = cargo_metadata::fetch(&fixture.root, Network::Refused)
        .unwrap_or_else(|error| panic!("{toolchain}: cargo metadata refused it: {error:?}"));

    for (side, shape) in [("the reader", shape(&own)), ("cargo", shape(&cargo))] {
        assert!(
            shape
                .packages
                .iter()
                .any(|(name, version, _)| name == "enabled" && version == "0.0.0"),
            "{toolchain}: {side} left `enabled` out of the graph: {:?}",
            shape.packages
        );
    }
    assert_eq!(shape(&own), shape(&cargo), "on {toolchain}");
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
        "[package]\nname = \"autolib-off\"\nautolib = false\n\
         [[bin]]\nname = \"x\"\npath = \"src/x.rs\"\n",
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
        "[dependencies]\ndep = { path = \"../../d1\" }\n\
         [dev-dependencies]\ndep = { path = \"../../d2\" }\n",
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
        "[dependencies]\ndep = { path = \"../../d1\" }\n\
         older = { path = \"../../d2\", package = \"dep\" }\n",
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
fn a_virtual_workspace_with_no_members_is_refused_by_cargo_and_by_the_reader() {
    // Three roots with no `[package]` and no member: an empty `members`, no `members` at all, and
    // a glob whose only match is excluded. Cargo locks each, then refuses to read it, and so must
    // the reader, whether or not a lockfile exists.
    let toolchain = cargo_version();
    let roots = [
        ("members = []", "[workspace]\nmembers = []\n"),
        ("no members key", "[workspace]\n"),
        (
            "an excluded glob",
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/a\"]\n",
        ),
    ];
    for (shape, manifest) in roots {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", manifest);
        fixture.package("crates/a", "a", "");
        fixture
            .lock_with_cargo()
            .unwrap_or_else(|stderr| panic!("{toolchain} could not lock {shape}: {stderr}"));

        let cargo = cargo_metadata::fetch(&fixture.root, Network::Refused);
        let read = read_document(&fixture.root, &BTreeMap::new());

        match cargo {
            Err(cargo_metadata::ReadWorkspaceError::CargoMetadataFailed { stderr }) => assert!(
                stderr.contains("the workspace has no members"),
                "{toolchain} refused {shape} for another reason: {stderr}"
            ),
            other => panic!("{toolchain} on {shape}: expected a refusal, got {other:?}"),
        }
        match read {
            Err(ReadWithoutCargoError::ManifestUnreadable { detail }) => assert!(
                detail.contains("has no members"),
                "{toolchain}: the reader refused {shape} as `{detail}`"
            ),
            other => panic!(
                "{toolchain}: the reader on {shape} gave {:?}",
                other.map(|_| ())
            ),
        }
    }
}

#[test]
fn a_member_above_the_root_is_refused_by_cargo_and_by_the_reader_with_the_same_sentence() {
    // `members = ["../far"]` names a package above the root that does not point back at it.
    // Cargo refuses to lock it, and the reader's detail is the sentence Cargo prints.
    let toolchain = cargo_version();
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[workspace]\nmembers = [\"../far\"]\n");
    fixture.package("../far", "far", "");

    let locked = fixture.lock_with_cargo();
    let read = read_document(&fixture.root, &BTreeMap::new());

    let stderr = locked.expect_err(&format!("{toolchain} locked a member above the root"));
    let Err(ReadWithoutCargoError::ManifestUnreadable { detail }) = read else {
        panic!("{toolchain}: the reader gave {:?}", read.map(|_| ()));
    };
    assert!(
        stderr.contains(&detail),
        "{toolchain} refused it as `{stderr}`, and the reader as `{detail}`"
    );
}

#[test]
fn a_member_above_the_root_that_names_the_root_is_read_by_cargo_and_refused_by_the_reader() {
    // With `package.workspace` pointing back at the root, Cargo accepts the member above it. The
    // reader does not follow that key, so it refuses by name.
    let toolchain = cargo_version();
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[workspace]\nmembers = [\"../far\"]\n");
    fixture.package("../far", "far", "workspace = \"../w\"\n");

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
fn a_root_package_with_empty_members_agrees() {
    // The root is a package, so it is its own member and the empty `members` refuses nothing.
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[package]\nname = \"solo\"\n[workspace]\nmembers = []\n",
    );
    fixture.write("src/lib.rs", "");

    assert_agree_at_root(&fixture);
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

#[test]
fn a_library_name_cargo_accepts_is_the_name_of_the_edge_on_both_sides() {
    // Cargo accepts a library name that is no Rust identifier, `1abc`, and names the edge to a
    // dependency after it. The reader takes the name as written, so the documents agree.
    let fixture = workspace_with_app("", "[dependencies]\nodd = { path = \"../odd\" }\n");
    fixture.package("odd", "odd", "[lib]\nname = \"1abc\"\n");

    assert_agree_at_root(&fixture);
}

#[test]
fn a_dependency_with_a_hyphen_in_its_library_name_is_refused_by_cargo_and_by_the_reader() {
    let toolchain = cargo_version();
    let fixture = workspace_with_app("", "[dependencies]\nodd = { path = \"../odd\" }\n");
    fixture.package("odd", "odd", "[lib]\nname = \"bad-name\"\n");

    let locked = fixture.lock_with_cargo();
    let read = read_document(&fixture.root, &BTreeMap::new());

    let stderr = locked.expect_err(&format!("{toolchain} locked a hyphenated library name"));
    assert!(
        stderr.contains("library target names cannot contain hyphens: bad-name"),
        "{toolchain} refused it for another reason: {stderr}"
    );
    match read {
        Err(ReadWithoutCargoError::ManifestUnreadable { detail }) => assert!(
            detail.ends_with("library target names cannot contain hyphens: bad-name"),
            "{toolchain}: the reader said {detail}"
        ),
        other => panic!("{toolchain}: the reader gave {:?}", other.map(|_| ())),
    }
}

/// Writes the package `row` describes as a workspace root, has Cargo lock it, and requires the
/// reader to say what Cargo said: the same documents when Cargo locks it, and a refusal that holds
/// Cargo's sentence when it does not.
fn assert_cargo_decides(row: &matrix::Row) {
    let toolchain = cargo_version();
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", &row.manifest);
    for file in row.files {
        fixture.write(file, "");
    }

    let locked = fixture.lock_with_cargo();
    let read = read_document(&fixture.root, &BTreeMap::new());

    match (row.expected, locked) {
        (Ok(_), Ok(())) => assert_reads_agree(&fixture.root),
        (Ok(_), Err(stderr)) => {
            panic!(
                "{toolchain} refused `{}`, which the matrix says it reads: {stderr}",
                row.name
            );
        }
        (Err(sentence), Err(stderr)) => {
            let cargo = matrix::cargo_sentence(sentence);
            assert!(
                stderr.contains(cargo),
                "{toolchain} refused `{}` another way than `{cargo}`: {stderr}",
                row.name
            );
            match read {
                Err(ReadWithoutCargoError::ManifestUnreadable { detail }) => assert!(
                    detail.contains(sentence),
                    "{toolchain}: for `{}` the reader said {detail}, not `{sentence}`",
                    row.name
                ),
                other => panic!(
                    "{toolchain}: for `{}` the reader gave {:?}",
                    row.name,
                    other.map(|_| ())
                ),
            }
        }
        (Err(sentence), Ok(())) => {
            panic!(
                "{toolchain} locked `{}`, which the matrix says it refuses with `{sentence}`",
                row.name
            );
        }
    }
}

#[test]
fn cargo_decides_every_library_binary_and_other_target_shape() {
    // Each row of the target matrix is written as a package and given to Cargo, which locks it or
    // refuses it with a sentence. The reader must do the same on every row, so the matrix's
    // expected values are Cargo's and not the reader's own.
    for row in matrix::rows() {
        assert_cargo_decides(&row);
    }
}

/// A workspace whose one member `app` takes its edition from `[workspace.package]`, where
/// `edition` is written, and declares a `[[bin]]` named `x` with no path beside only a
/// `src/main.rs`.
fn member_inheriting_edition(edition: &str) -> Fixture {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [\"app\"]\n[workspace.package]\nedition = \"{edition}\"\n"
        ),
    );
    fixture.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nedition.workspace = true\n[[bin]]\nname = \"x\"\n",
    );
    fixture.write("app/src/main.rs", "");
    fixture
}

#[test]
fn an_edition_inherited_from_the_workspace_decides_where_a_binary_is_found() {
    // In 2015 a `[[bin]]` with no path falls back to `src/main.rs`; in 2021 it does not. A member
    // that writes `edition.workspace = true` gets the root's, so the reader must follow it.
    let toolchain = cargo_version();

    let old = member_inheriting_edition("2015");
    assert_documents_agree(&old, &old.root);

    let newer = member_inheriting_edition("2021");
    let locked = newer.lock_with_cargo();
    let read = read_document(&newer.root, &BTreeMap::new());
    let stderr = locked.expect_err(&format!("{toolchain} locked a binary with no file"));
    assert!(
        stderr.contains("can't find `x` bin at `src/bin/x.rs`"),
        "{toolchain} refused it for another reason: {stderr}"
    );
    assert!(
        matches!(read, Err(ReadWithoutCargoError::ManifestUnreadable { .. })),
        "{toolchain}: the reader gave {:?}",
        read.map(|_| ())
    );
}
