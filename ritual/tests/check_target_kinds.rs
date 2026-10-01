//! Acceptance: a skeleton whose own crate is not a plain `lib` still
//! resolves, in both `check` and `sync`.
//!
//! `skeletons` locates a worn skeleton's own resolved package by matching a
//! member's declared dependency against the workspace's resolved graph
//! (`workspace/locate.rs`). A library has *several* possible Cargo target
//! kinds — `lib`, `rlib`, `dylib`, `proc-macro`, `cdylib`, `staticlib`, … —
//! and a skeleton whose crate declares any of them must still resolve,
//! rather than being refused with a message that claims cargo found nothing
//! at all when it plainly did.
//!
//! Every fixture here is a from-scratch skeleton crate (not one of the
//! checked-in `crates/skeletons/test-skeletons/`, none of which declare a
//! non-`lib` crate type), built with `support::write` directly rather than
//! `write_minimal_skeleton`, since that helper's manifest carries no `[lib]`
//! table at all.

mod support;

use std::path::Path;

use support::{
    Fixture, TemporaryDirectory, path_dependency_on, wearing_table, write_package_manifest,
};

/// The bytes `files/thing.txt` holds for every skeleton this file builds —
/// the one claim every fixture below makes.
const RENDER: &[u8] = b"kind-resolves\n";

/// Builds a skeleton crate at a fresh temporary directory, naming
/// `package_name` and carrying `lib_table` (a whole `[lib]\n...\n` section,
/// or an empty string for a plain default `lib` target — never reached by
/// this file's own tests, which exist to cover the *non*-`lib` kinds) ahead
/// of `[package.metadata.skeletons]`.
fn build_skeleton(
    package_name: &str,
    lib_table: &str,
) -> Result<TemporaryDirectory, Box<dyn std::error::Error>> {
    let directory = TemporaryDirectory::new(&format!("target-kind-skeleton-{package_name}"))?;
    support::write(
        directory.path(),
        "Cargo.toml",
        format!(
            "[package]\n\
             name = \"{package_name}\"\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             {lib_table}\
             [package.metadata.skeletons]\n"
        )
        .as_bytes(),
    )?;
    support::write(
        directory.path(),
        "src/lib.rs",
        b"// nothing: a skeleton's own crate is never built by a render.\n",
    )?;
    support::write(directory.path(), "files/thing.txt", RENDER)?;
    Ok(directory)
}

/// A fixture wearing the skeleton built at `skeleton_directory`, under
/// `dependency_key`, with its manifest and lockfile written but
/// `thing.txt` not yet created.
fn fixture_wearing(
    dependency_key: &str,
    skeleton_directory: &Path,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on(dependency_key, skeleton_directory),
        wearing_table(dependency_key, ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

/// Asserts `report` carries no `unresolved` refusal at all — the specific,
/// false claim ("cargo resolved no package for it") that must never be made
/// for a kind cargo did in fact resolve.
fn assert_not_unresolved(report: &support::Report, dependency_key: &str) {
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        !combined.contains("cargo resolved no package"),
        "{dependency_key} must not be refused as unresolved when cargo did resolve it; output \
         was: {combined}"
    );
}

#[test]
fn a_crate_type_rlib_skeleton_resolves_in_check() -> support::TestOutcome {
    // `[lib]\ncrate-type = ["rlib"]` is a real, if unusual, Cargo library
    // target: cargo reports its own target kind as `["rlib"]`, never
    // `["lib"]`. `check` must still find and compare its one claimed file,
    // not refuse the whole skeleton as though cargo resolved nothing.
    let skeleton = build_skeleton("rlib-skeleton", "[lib]\ncrate-type = [\"rlib\"]\n\n")?;
    let fixture = fixture_wearing("rlib-skeleton", skeleton.path())?;
    fixture.write("thing.txt", RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_not_unresolved(&report, "rlib-skeleton");
    assert_eq!(
        report.exit_code, 0,
        "an rlib skeleton whose claimed file already matches must report clean; stdout was: {}, \
         stderr was: {}",
        report.stdout, report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let skeletons = support::json::skeletons(&document)?;
    assert_eq!(skeletons.len(), 1, "skeletons were: {skeletons:?}");
    assert!(
        !support::json::skeleton_refused(&skeletons[0])?,
        "an rlib skeleton must not be reported refused; row was: {:?}",
        skeletons[0]
    );
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn a_crate_type_rlib_skeleton_resolves_in_sync() -> support::TestOutcome {
    // The same skeleton kind, through `sync` instead: the claimed file is
    // missing, so `sync` must actually create it, which it can only do once
    // the skeleton resolves at all.
    let skeleton = build_skeleton("rlib-skeleton-sync", "[lib]\ncrate-type = [\"rlib\"]\n\n")?;
    let fixture = fixture_wearing("rlib-skeleton-sync", skeleton.path())?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_not_unresolved(&report, "rlib-skeleton-sync");
    assert_eq!(
        report.exit_code, 0,
        "sync must succeed once an rlib skeleton resolves; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        fixture.read("thing.txt")?,
        RENDER,
        "sync must actually create the rlib skeleton's claimed file"
    );
    Ok(())
}

#[test]
fn a_proc_macro_skeleton_resolves_in_check() -> support::TestOutcome {
    // `[lib]\nproc-macro = true` is the least plain library kind:
    // cargo reports the target's own kind as `["proc-macro"]`.
    let skeleton = build_skeleton("proc-macro-skeleton", "[lib]\nproc-macro = true\n\n")?;
    let fixture = fixture_wearing("proc-macro-skeleton", skeleton.path())?;
    fixture.write("thing.txt", RENDER)?;

    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_not_unresolved(&report, "proc-macro-skeleton");
    assert_eq!(
        report.exit_code, 0,
        "a proc-macro skeleton whose claimed file already matches must report clean; stdout \
         was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let skeletons = support::json::skeletons(&document)?;
    assert_eq!(skeletons.len(), 1, "skeletons were: {skeletons:?}");
    assert!(
        !support::json::skeleton_refused(&skeletons[0])?,
        "a proc-macro skeleton must not be reported refused; row was: {:?}",
        skeletons[0]
    );
    let bones = support::json::bones(&document)?;
    assert_eq!(bones.len(), 1, "bones were: {bones:?}");
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn a_proc_macro_skeleton_resolves_in_sync() -> support::TestOutcome {
    let skeleton = build_skeleton("proc-macro-skeleton-sync", "[lib]\nproc-macro = true\n\n")?;
    let fixture = fixture_wearing("proc-macro-skeleton-sync", skeleton.path())?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_not_unresolved(&report, "proc-macro-skeleton-sync");
    assert_eq!(
        report.exit_code, 0,
        "sync must succeed once a proc-macro skeleton resolves; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        fixture.read("thing.txt")?,
        RENDER,
        "sync must actually create the proc-macro skeleton's claimed file"
    );
    Ok(())
}
