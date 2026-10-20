//! The cases where an optional dependency of a package outside the workspace asks a worn package
//! for a version the lock does not name.
//!
//! Cargo keeps one version of a package for each range of versions it calls compatible, and the
//! workspace's `house-rules = "4"` is locked at 4.5.6. A package outside the workspace that a
//! feature enables later, and that asks for `house-rules` `4.6`, is in the range 4.5.6 is in and
//! not met by it: Cargo cannot resolve the workspace until the worn package is updated, however it
//! is asked. That is no stale lock for the reader to report through, and the reader refuses it. A
//! requirement in another range, or one the locked version meets, gives Cargo a lock to rewrite,
//! and the reader reports as it always did. A worn package from a path or a git repository holds
//! one version, so there Cargo cannot resolve a requirement in any range the locked version does
//! not meet, and the reader refuses it. Every case runs in a Cargo home the test owns, which
//! vendors `house-rules` 4.5.6 as the replacement for crates.io, and 4.6.0 and 5.0.0 after the lock
//! where a case needs a version to ask for.
//!
//! The package outside the workspace is `y`, in a directory beside the root, and its optional
//! dependency is `z`. A test writes `z` and then changes `y` so that a feature enables `z`, so
//! the lock was made with `z` off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::cargo_configuration::{ConfigurationRead, HomeVariables};
use super::super::error::ReadWithoutCargoError;
use super::super::fixture::{CargoHome, Fixture, GitRepository, cargo_version};
use super::super::read::read_document;
use super::{
    Verdict, Y_DEFAULT_ENABLES_Z, Y_OPTIONAL_Z, assert_both_say_in_home, assert_reader_refuses,
    assert_stale_report_is_the_relocked_one_in_home, cargo_metadata, cargo_verdict_in_home,
    locked_members, write_app,
};
use crate::workspace::crates_io_source::CRATES_IO_SOURCE;
use crate::workspace::{LockedPackageId, Network, ReadWorkspaceError};

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

/// What the reader says when `requirement` of `house-rules` in `manifest` is one that the worn
/// 1.0.0 does not meet, and its `source` kind (`path` or `git`) holds no other version.
fn refusal_of_the_only_version(manifest: &Path, requirement: &str, source: &str) -> String {
    format!(
        "{}: `house-rules` requires `house-rules` `{requirement}`, which the worn package's \
         locked version 1.0.0 does not meet, and its {source} source holds only that version, so \
         cargo cannot resolve the workspace once a feature enables it",
        manifest.display()
    )
}

/// A workspace whose member `app` wears `house-rules` written as `app_dependency` and depends on
/// `y`, whose optional dependency `z` asks for `house-rules` written as `z_dependency`. Not
/// locked: the caller writes the package `house-rules` first, where the dependencies say it is.
fn workspace_of_one_worn_version(app_dependency: &str, z_dependency: &str) -> Fixture {
    let fixture = locked_members(&["app"]);
    write_app(
        &fixture,
        &format!(
            "[dependencies]\nhouse-rules = {app_dependency}\ny = {{ path = \"../../y\" }}\n\
             [package.metadata.skeletons.house-rules]\n"
        ),
    );
    fixture.package("../y", "y", Y_OPTIONAL_Z);
    fixture.package(
        "../z",
        "z",
        &format!("[dependencies]\nhouse-rules = {z_dependency}\n"),
    );
    fixture
}

/// Locks `fixture` in `home`, which Cargo may not fetch into.
fn lock_offline(fixture: &Fixture, home: &CargoHome) {
    let toolchain = cargo_version();
    home.lock(&fixture.root, Network::Refused)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
}

/// A workspace like [`workspace_of_one_worn_version`], with `house-rules` 1.0.0 worn by path,
/// where `z` writes a git dependency `foo` whose URL has a query of its own (`?x=1`) and then
/// `z_dependencies`. `foo` is a repository Cargo has fetched into the home, so that the only
/// thing Cargo can fail on once `z` is enabled is a version of `house-rules`. Locked with `z`
/// off. The repository is returned too, because it is removed with it.
fn locked_with_a_git_query_in_z(z_dependencies: &str) -> (Fixture, CargoHome, GitRepository) {
    let repository = GitRepository::in_directory_named("foo", "repo?x=1");
    let query_dependency = format!("foo = {{ git = \"{}\" }}", repository.url());
    let home = CargoHome::new();
    home.prime(std::slice::from_ref(&query_dependency));
    let fixture = workspace_of_one_worn_version(
        "{ path = \"../../house-rules\" }",
        "{ path = \"../house-rules\" }",
    );
    fixture.package("../house-rules", "house-rules", "version = \"1.0.0\"\n");
    fixture.package(
        "../z",
        "z",
        &format!("[dependencies]\n{query_dependency}\n{z_dependencies}"),
    );
    let toolchain = cargo_version();
    home.lock(&fixture.root, Network::Allowed)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
    (fixture, home, repository)
}

/// Requires that Cargo, in `home`, fails to resolve the workspace at `fixture`'s root because no
/// version of `house-rules` meets a requirement, and not because a source could not be fetched,
/// and that the reader refuses it with the detail `detail`.
fn assert_cargo_cannot_select_a_version_and_reader_refuses(
    fixture: &Fixture,
    home: &CargoHome,
    detail: &str,
) {
    let toolchain = cargo_version();
    match cargo_metadata::fetch_in_cargo_home(&fixture.root, home.path(), home.home()) {
        Err(ReadWorkspaceError::CargoMetadataFailed { stderr }) => assert!(
            stderr.contains("failed to select a version for the requirement `house-rules"),
            "cargo, on {toolchain}, failed some other way:\n{stderr}"
        ),
        other => panic!("cargo, on {toolchain}: expected a failed resolution, got {other:?}"),
    }
    assert_cargo_cannot_resolve_and_reader_refuses(fixture, home, detail);
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
fn a_worn_path_package_asked_for_outside_its_compatible_range_is_refused() {
    // A path package holds one version. `house-rules` 1.0.0 sits in a directory, `app` wears it
    // by path, and `z` asks for it at `^2` by the same path. Cargo cannot keep a second version
    // beside it as it would from a registry, so once `y` enables `z` it fails to select one,
    // and a relock does not help. `^2` is outside the range 1.0.0 is in, and the reader refuses
    // it all the same, naming `z`'s manifest.
    let home = CargoHome::new();
    let fixture = workspace_of_one_worn_version(
        "{ path = \"../../house-rules\" }",
        "{ path = \"../house-rules\", version = \"2\" }",
    );
    fixture.package("../house-rules", "house-rules", "version = \"1.0.0\"\n");
    lock_offline(&fixture, &home);
    enable_z(&fixture);

    assert_cargo_cannot_resolve_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of_the_only_version(&manifest_of(&fixture, "z"), "2", "path"),
    );
}

#[test]
fn a_worn_git_package_asked_for_outside_its_compatible_range_is_refused() {
    // The same for a git source, which also holds one version: `house-rules` 1.0.0 in a
    // `file://` repository, worn by `app` and asked for at `^2` from that repository by `z`.
    // Cargo fails to select a version once `y` enables `z`, and the reader refuses it.
    let repository = GitRepository::new("house-rules");
    let dependency = format!("{{ git = \"{}\" }}", repository.url());
    let home = CargoHome::new();
    home.prime(&[format!("house-rules = {dependency}")]);
    let fixture = workspace_of_one_worn_version(
        &dependency,
        &format!("{{ git = \"{}\", version = \"2\" }}", repository.url()),
    );
    let toolchain = cargo_version();
    home.lock(&fixture.root, Network::Allowed)
        .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
    enable_z(&fixture);

    assert_cargo_cannot_resolve_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of_the_only_version(&manifest_of(&fixture, "z"), "2", "git"),
    );
}

#[test]
fn a_git_url_with_a_query_beside_an_unmet_requirement_on_a_worn_path_package_is_refused() {
    // Cargo loads `z` with `foo = { git = "…/repo?x=1" }`, a URL with a query of its own, so
    // `z`'s other requirements bind once `y` enables it: `house-rules` 1.0.0, worn by path,
    // does not meet `^1.1`, and Cargo fails to select a version. The query is no reason for the
    // reader to pass `z` over as a manifest Cargo refuses: it reads `z` whole and refuses the
    // requirement, naming `z`'s manifest.
    let (fixture, home, _repository) = locked_with_a_git_query_in_z(
        "house-rules = { path = \"../house-rules\", version = \"1.1\" }\n",
    );
    enable_z(&fixture);

    assert_cargo_cannot_select_a_version_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of_the_only_version(&manifest_of(&fixture, "z"), "1.1", "path"),
    );
}

#[test]
fn a_git_url_with_a_query_beside_a_requirement_in_another_range_of_a_worn_path_package_is_refused()
{
    // The same with `^2`, outside the range 1.0.0 is in. A path package holds one version, so
    // Cargo fails here too, and the reader refuses it all the same.
    let (fixture, home, _repository) = locked_with_a_git_query_in_z(
        "house-rules = { path = \"../house-rules\", version = \"2\" }\n",
    );
    enable_z(&fixture);

    assert_cargo_cannot_select_a_version_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of_the_only_version(&manifest_of(&fixture, "z"), "2", "path"),
    );
}

#[test]
fn a_git_url_with_a_query_above_a_package_that_asks_for_a_newer_worn_version_is_refused() {
    // The query is in `z`, and the unmet requirement is in `x`, a package `z` depends on by
    // path. A package passed over is never followed, so a query in `z` would hide every package
    // below it; the reader reads `z`, follows the path and names `x`'s manifest.
    let (fixture, home, _repository) = locked_with_a_git_query_in_z("x = { path = \"../x\" }\n");
    fixture.package(
        "../x",
        "x",
        "[dependencies]\nhouse-rules = { path = \"../house-rules\", version = \"1.1\" }\n",
    );
    enable_z(&fixture);

    assert_cargo_cannot_select_a_version_and_reader_refuses(
        &fixture,
        &home,
        &refusal_of_the_only_version(&manifest_of(&fixture, "x"), "1.1", "path"),
    );
}

#[test]
fn a_git_url_with_a_query_beside_a_requirement_the_worn_path_package_meets_is_reported() {
    // The control against over-refusing: with the query beside `^1`, which 1.0.0 meets, enabling
    // `z` only makes the lock lack `z` and `foo`, and the report is the one a relock gives.
    let (fixture, home, _repository) = locked_with_a_git_query_in_z(
        "house-rules = { path = \"../house-rules\", version = \"1\" }\n",
    );
    enable_z(&fixture);

    assert_stale_report_is_the_relocked_one_in_home(&fixture, &home, &BTreeMap::new());
}

#[test]
fn a_git_and_path_in_the_workspace_dependencies_of_a_reached_root_is_refused_naming_it() {
    // `z` is a workspace root of its own, and its `[workspace.dependencies]` writes
    // `foo = { git, path }`, which nothing in `z` inherits. Cargo refuses the entry only when a
    // member inherits it, so it loads `z`, and `z`'s other requirements bind once `y` enables
    // it: `house-rules` 1.0.0, worn by path, does not meet `^1.1`, and Cargo fails to select a
    // version, not on the ambiguous specification. The reader does not follow the entry and
    // cannot pass `z` over as a manifest Cargo refuses, so it refuses the workspace, naming
    // `z`'s manifest, rather than report a lock Cargo cannot resolve.
    let fixture = workspace_of_one_worn_version(
        "{ path = \"../../house-rules\" }",
        "{ path = \"../house-rules\", version = \"1.1\" }",
    );
    fixture.package("../house-rules", "house-rules", "version = \"1.0.0\"\n");
    fixture.package(
        "../z",
        "z",
        "[workspace]\n[workspace.dependencies]\n\
         foo = { git = \"file:///nowhere/foo\", path = \"../foo\" }\n\
         [dependencies]\nhouse-rules = { path = \"../house-rules\", version = \"1.1\" }\n",
    );
    let home = CargoHome::new();
    lock_offline(&fixture, &home);
    enable_z(&fixture);

    assert_cargo_cannot_select_a_version_and_reader_refuses(
        &fixture,
        &home,
        &format!(
            "{}: [workspace.dependencies] `foo` writes both `git` and `path`, which Cargo loads \
             until a member inherits it and this reader does not follow",
            manifest_of(&fixture, "z").display()
        ),
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
fn a_worn_registry_package_asked_for_outside_its_range_is_not_refused() {
    // The control against over-refusing another range of a registry package: `z` asks for `^5`.
    // Cargo keeps one version for each range, so it adds 5.0.0 beside 4.5.6 and leaves the worn
    // package alone, and the stale report is the one a relock gives. A requirement in a range the
    // lock does not hold is no reason to refuse here. A path or git package holds one version,
    // so it is refused there, as the two tests named for those sources hold.
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
        &ConfigurationRead::AsCargoReadsIt(HomeVariables::cargo_home(home.path())),
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
    // here would fire on workspaces Cargo reads, so it passes over a manifest that is not
    // there, one of the two shapes it knows Cargo fails on too.
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

/// What Cargo, in `home`, says when it cannot resolve the workspace at `fixture`'s root.
fn cargo_failure_in_home(fixture: &Fixture, home: &CargoHome) -> String {
    let toolchain = cargo_version();
    match cargo_metadata::fetch_in_cargo_home(&fixture.root, home.path(), home.home()) {
        Err(ReadWorkspaceError::CargoMetadataFailed { stderr }) => stderr,
        other => panic!("cargo, on {toolchain}: expected a failed resolution, got {other:?}"),
    }
}

#[test]
fn an_optional_dependency_whose_directory_is_gone_fails_in_cargo_once_enabled() {
    // The reason the reader may pass over a package whose directory is gone: Cargo fails on it
    // too, as soon as a feature enables it. `z` is locked while off and removed, `y` then enables
    // it, and Cargo cannot load it.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    std::fs::remove_dir_all(fixture.root.with_file_name("z")).expect("take z away");
    enable_z(&fixture);

    let stderr = cargo_failure_in_home(&fixture, &home);

    assert!(
        stderr.contains("failed to load source for dependency `z`"),
        "cargo, on {}, failed some other way:\n{stderr}",
        cargo_version()
    );
}

#[test]
fn a_git_and_path_dependency_of_an_optional_package_is_read_past_while_it_is_off() {
    // Cargo does not load `z` while a feature leaves it off, so a `z` that writes
    // `foo = { git, path }` leaves the lock current, and the reader, which passes over exactly
    // this manifest, agrees.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    fixture.package(
        "../z",
        "z",
        "[dependencies]\nfoo = { git = \"file:///nowhere/foo\", path = \"../foo\" }\n",
    );

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}

#[test]
fn a_git_and_path_dependency_of_an_optional_package_fails_in_cargo_once_enabled() {
    // The reason the reader may pass over `{ git, path }`: Cargo fails on it too, as soon as a
    // feature enables the package, because the dependency is ambiguous.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    fixture.package(
        "../z",
        "z",
        "[dependencies]\nfoo = { git = \"file:///nowhere/foo\", path = \"../foo\" }\n",
    );
    enable_z(&fixture);

    let stderr = cargo_failure_in_home(&fixture, &home);

    assert!(
        stderr.contains("specification is ambiguous"),
        "cargo, on {}, failed some other way:\n{stderr}",
        cargo_version()
    );
}

#[test]
fn an_optional_package_whose_manifest_cargo_refuses_in_another_way_is_refused_naming_it() {
    // A manifest the reader cannot read that is neither missing nor `{ git, path }` is no
    // shape the reader knows Cargo fails on, so it is not passed over: the reader refuses the
    // workspace, naming `z`'s manifest, though `z` is off and Cargo says the lock is current.
    // That is the cost of passing over only the two named shapes: `version = "one"` is a
    // manifest Cargo refuses too, but only once something enables `z`.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    fixture.package(
        "../z",
        "z",
        "version = \"one\"\n[dependencies]\nhouse-rules = \"4.6\"\n",
    );
    let toolchain = cargo_version();

    assert_eq!(
        cargo_verdict_in_home(&fixture, &home),
        Verdict::Current,
        "cargo, on {toolchain}"
    );
    let Err(ReadWithoutCargoError::ManifestUnreadable { detail }) = read_document(
        &fixture.root,
        &BTreeMap::new(),
        &ConfigurationRead::AsCargoReadsIt(HomeVariables::cargo_home(home.path())),
    ) else {
        panic!("the reader, on {toolchain}: expected an unreadable manifest");
    };
    assert!(
        detail.starts_with(&manifest_of(&fixture, "z").display().to_string()),
        "the reader, on {toolchain}, named another file: {detail}"
    );
}

#[test]
fn an_optional_package_with_a_version_cargo_refuses_fails_in_cargo_once_enabled() {
    // The same manifest with `y` enabling `z`: Cargo fails, and the reader refuses it, naming
    // `z`'s manifest, so a refusal that fires early is at least one Cargo would give.
    let (fixture, home) = locked_with_optional_z("[dependencies]\nhouse-rules = \"4.6\"\n");
    fixture.package(
        "../z",
        "z",
        "version = \"one\"\n[dependencies]\nhouse-rules = \"4.6\"\n",
    );
    enable_z(&fixture);

    assert_both_say_in_home(&fixture, &home, Verdict::Refused);
}

#[test]
fn a_dev_dependency_of_an_optional_package_is_not_a_requirement_on_the_worn_package() {
    // Cargo resolves the dev-dependencies of the workspace's members and of no package outside
    // it, so `z`'s `^4.6` there is no requirement on anything. Both say the lock is current.
    let (fixture, home) = locked_with_optional_z("[dev-dependencies]\nhouse-rules = \"4.6\"\n");

    assert_both_say_in_home(&fixture, &home, Verdict::Current);
}
