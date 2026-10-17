//! Whether `Cargo.lock` is current: the answer `cargo metadata --locked` gives, from the
//! manifests and the lockfile alone.
//!
//! `--locked` makes Cargo resolve the workspace again and refuse when the lockfile it would write
//! is not the one on disk. This module reproduces that verdict for the cases a reader of the
//! manifests can see, which is every case that involves only path packages and the declarations
//! and lock entries of every package. What it does not decide needs Cargo's feature resolution or
//! its configuration, and the library's readme names both.

mod locked_source;
mod reachability;

use std::collections::BTreeSet;

use semver::{Version, VersionReq};

use super::error::ReadWithoutCargoError;
use super::file_system::normalized;
use super::lock_entries::{path_entry_index, set_aside_entries};
use super::lockfile::{LockEntry, Lockfile};
use super::package_manifest::{Declaration, DeclaredSource};
use super::path_package::PathPackage;
use reachability::every_entry_is_reached;

/// Refuses a lockfile that Cargo would call out of date for the workspace whose packages on disk
/// are `packages`.
///
/// In order: an entry the lockfile still writes a `replace` on is out of date, because the root
/// manifest writes no `[replace]` by the time this runs (one that does is refused before), and
/// Cargo drops the replacement on a relock. Then the entries Cargo sets aside when it loads a
/// lockfile are set aside, with every dependency entry that names one. Then every package on disk
/// is locked as a package with no `source`, under its name and version. Then each package's
/// declarations are among the dependencies of its entry, and each of those dependencies is a
/// declared package, agreeing on source, git reference, version requirement and, for a path, the
/// manifest it names. Last, the members reach every entry that is left.
///
/// # Errors
///
/// [`ReadWithoutCargoError::Lockfile`] for every way the lockfile is out of date.
pub(super) fn verify_lockfile_is_current(
    lockfile: &Lockfile,
    packages: &[PathPackage],
) -> Result<(), ReadWithoutCargoError> {
    if lockfile.entries().iter().any(LockEntry::is_replaced) {
        return Err(ReadWithoutCargoError::Lockfile);
    }
    let set_aside = set_aside_entries(lockfile, packages);

    let mut entry_indexes = Vec::with_capacity(packages.len());
    for package in packages {
        let Some(index) = path_entry_index(lockfile, &package.name, &package.version) else {
            return Err(ReadWithoutCargoError::Lockfile);
        };
        entry_indexes.push(index);
    }
    assert_eq!(
        entry_indexes.len(),
        packages.len(),
        "every package on disk has one entry"
    );

    let mut member_indexes = Vec::new();
    for (package, &index) in packages.iter().zip(&entry_indexes) {
        let entry = &lockfile.entries()[index];
        let locked = dependencies_of(lockfile, entry, &set_aside);
        if !declarations_are_locked(packages, package, &locked) {
            return Err(ReadWithoutCargoError::Lockfile);
        }
        if !locked_dependencies_are_declared(packages, package, &locked) {
            return Err(ReadWithoutCargoError::Lockfile);
        }
        if package.is_member {
            member_indexes.push(index);
        }
    }

    if every_entry_is_reached(lockfile, &set_aside, &member_indexes) {
        Ok(())
    } else {
        Err(ReadWithoutCargoError::Lockfile)
    }
}

/// The entries `entry` depends on that Cargo keeps.
fn dependencies_of<'lock>(
    lockfile: &'lock Lockfile,
    entry: &LockEntry,
    set_aside: &BTreeSet<usize>,
) -> Vec<&'lock LockEntry> {
    entry
        .dependency_indexes()
        .iter()
        .filter(|index| !set_aside.contains(index))
        .map(|&index| &lockfile.entries()[index])
        .collect()
}

/// Whether every declaration of `package` that Cargo locks is among the `locked` dependencies of
/// its entry.
///
/// A member's declarations are all locked, of every kind and optional or not. A package outside
/// the workspace is locked only for what it needs: not its dev-dependencies, and not an optional
/// dependency until a feature enables one, which this reader does not decide.
fn declarations_are_locked(
    packages: &[PathPackage],
    package: &PathPackage,
    locked: &[&LockEntry],
) -> bool {
    package
        .declarations
        .iter()
        .filter(|declaration| {
            package.is_member || (!declaration.is_dev && !declaration.is_optional)
        })
        .all(|declaration| {
            locked
                .iter()
                .any(|candidate| declaration_resolves_to(packages, declaration, candidate))
        })
}

/// Whether every one of the `locked` dependencies of `package`'s entry is a package one of its
/// declarations resolves to, in any table.
///
/// An optional dependency of a package outside the workspace is in the lockfile once a feature
/// enables it, so it is declared and never extra.
fn locked_dependencies_are_declared(
    packages: &[PathPackage],
    package: &PathPackage,
    locked: &[&LockEntry],
) -> bool {
    locked.iter().all(|dependency| {
        package
            .declarations
            .iter()
            .any(|declaration| declaration_resolves_to(packages, declaration, dependency))
    })
}

/// Whether `candidate` is the package `declaration` resolves to: it has the name the declaration
/// names, and agrees with it on the rest.
fn declaration_resolves_to(
    packages: &[PathPackage],
    declaration: &Declaration,
    candidate: &LockEntry,
) -> bool {
    candidate.name() == declaration.package && candidate_agrees(packages, declaration, candidate)
}

/// Whether `candidate`, a lock entry named for what `declaration` names, agrees with the
/// declaration on source, version requirement and, for a path, the manifest it points at.
fn candidate_agrees(
    packages: &[PathPackage],
    declaration: &Declaration,
    candidate: &LockEntry,
) -> bool {
    locked_source::agrees(candidate.source(), &declaration.source)
        && requirement_is_met(
            declaration.version_requirement.as_deref(),
            candidate.version(),
        )
        && path_manifest_is_the_package(packages, declaration, candidate)
}

/// Whether the locked `version` meets the requirement text, when there is one that `semver`
/// parses.
///
/// A requirement `semver` does not parse is not judged here: the manifest reader does not refuse
/// it, and Cargo's own grammar for it is not this reader's to reproduce.
fn requirement_is_met(requirement: Option<&str>, version: &str) -> bool {
    let Some(requirement) = requirement.and_then(|text| VersionReq::parse(text).ok()) else {
        return true;
    };
    Version::parse(version).is_ok_and(|locked| requirement.matches(&locked))
}

/// Whether the manifest a path declaration points at is the package it names, at the locked
/// version. A declaration that is not a path one has no manifest here to disagree.
fn path_manifest_is_the_package(
    packages: &[PathPackage],
    declaration: &Declaration,
    candidate: &LockEntry,
) -> bool {
    let DeclaredSource::Path { directory } = &declaration.source else {
        return true;
    };
    let directory = normalized(directory);
    packages.iter().any(|package| {
        package.directory == directory
            && package.name == declaration.package
            && package.version == candidate.version()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::workspace::without_cargo::error::ReadWithoutCargoError;
    use crate::workspace::without_cargo::fixture::Fixture;
    use crate::workspace::without_cargo::read::read_document;

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A workspace of one member `app` with `app_extra` after its `[package]` table, a path
    /// package `dep` beside it, and the lockfile text given.
    fn workspace(app_extra: &str, lock: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package("app", "app", app_extra);
        fixture.package("dep", "dep", "version = \"1.2.0\"\n");
        fixture.write("Cargo.lock", lock);
        fixture
    }

    /// The lockfile of `app` depending on `dep`, with `more` entries after them.
    fn lock_of_app_and_dep(app_dependencies: &str, more: &str) -> String {
        format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [{app_dependencies}]\n\n[[package]]\nname = \"dep\"\n\
             version = \"1.2.0\"\n{more}"
        )
    }

    const APP_DEPENDS_ON_DEP: &str = "[dependencies]\ndep = { path = \"../dep\" }\n";

    fn is_current(fixture: &Fixture) -> bool {
        match read_document(&fixture.root, &BTreeMap::new()) {
            Ok(_) => true,
            Err(ReadWithoutCargoError::Lockfile) => false,
            Err(other) => panic!("expected a document or the lockfile refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_lockfile_matching_the_declarations_is_current() {
        let fixture = workspace(APP_DEPENDS_ON_DEP, &lock_of_app_and_dep("\"dep\"", ""));

        assert!(is_current(&fixture));
    }

    #[test]
    fn a_declared_dependency_the_lock_does_not_list_is_stale() {
        // `app` declares `dep`, but its lock entry lists no dependencies.
        let fixture = workspace(APP_DEPENDS_ON_DEP, &lock_of_app_and_dep("", ""));

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_locked_dependency_the_package_no_longer_declares_is_stale() {
        // `dep` is a member, so Cargo keeps its entry, and `app` declares nothing, so the
        // dependency the lock gives it is one Cargo would no longer write.
        let fixture = workspace("", &lock_of_app_and_dep("\"dep\"", ""));
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\", \"dep\"]\n");

        assert!(!is_current(&fixture));
    }

    #[test]
    fn an_entry_with_no_package_on_disk_is_set_aside_with_the_dependencies_naming_it() {
        // `gone` has no manifest, so Cargo drops its entry and the dependency on it.
        let lock = "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
                    dependencies = [\"gone\"]\n\n[[package]]\nname = \"gone\"\n\
                    version = \"1.0.0\"\n";
        let fixture = workspace("", lock);

        assert!(is_current(&fixture));
    }

    #[test]
    fn an_extra_entry_nobody_depends_on_is_set_aside_when_no_package_has_its_name() {
        let ghost = "\n[[package]]\nname = \"ghost\"\nversion = \"0.0.1\"\n";
        let fixture = workspace(APP_DEPENDS_ON_DEP, &lock_of_app_and_dep("\"dep\"", ghost));

        assert!(is_current(&fixture));
    }

    #[test]
    fn an_entry_still_replaced_after_the_root_stopped_writing_replace_is_stale() {
        // The root writes no `[replace]`, and the lock still redirects `r` to the path package
        // it once named. The same lock without the `replace` line is current, so the line is
        // what makes it stale.
        let lock = |replace: &str| {
            format!(
                "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
                 dependencies = [\"r 1.4.0 ({CRATES_IO})\"]\n\n\
                 [[package]]\nname = \"r\"\nversion = \"1.4.0\"\n\n\
                 [[package]]\nname = \"r\"\nversion = \"1.4.0\"\n\
                 source = \"{CRATES_IO}\"\n{replace}"
            )
        };
        let app = "[dependencies]\nr = \"1\"\n";

        assert!(is_current(&workspace(app, &lock(""))));
        assert!(!is_current(&workspace(
            app,
            &lock("replace = \"r 1.4.0\"\n")
        )));
    }

    #[test]
    fn an_extra_git_entry_nobody_depends_on_is_stale() {
        let extra = format!(
            "\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
             source = \"git+https://example.test/g#{COMMIT}\"\n"
        );
        let fixture = workspace(APP_DEPENDS_ON_DEP, &lock_of_app_and_dep("\"dep\"", &extra));

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_git_entry_only_a_departed_path_package_depended_on_is_stale() {
        // `gone` left the workspace and its entry is set aside, so the git package that only it
        // depended on is reached by no member.
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\n\
             [[package]]\nname = \"gone\"\nversion = \"1.0.0\"\ndependencies = [\"g\"]\n\n\
             [[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
             source = \"git+https://example.test/g#{COMMIT}\"\n"
        );
        let fixture = workspace("", &lock);

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_git_dependency_removed_from_the_manifest_is_stale() {
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"g\"]\n\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
             source = \"git+https://example.test/g#{COMMIT}\"\n"
        );
        let fixture = workspace("", &lock);

        assert!(!is_current(&fixture));
    }

    /// `app` declaring the git dependency `g` as written, with a lockfile whose source for it
    /// is `source`.
    fn git_workspace(declaration: &str, source: &str) -> Fixture {
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"g\"]\n\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
             source = \"{source}\"\n"
        );
        workspace(&format!("[dependencies]\ng = {declaration}\n"), &lock)
    }

    #[test]
    fn a_git_dependency_locked_at_its_declared_branch_is_current() {
        let fixture = git_workspace(
            "{ git = \"https://example.test/g\", branch = \"main\" }",
            &format!("git+https://example.test/g?branch=main#{COMMIT}"),
        );

        assert!(is_current(&fixture));
    }

    #[test]
    fn a_changed_branch_is_stale() {
        let fixture = git_workspace(
            "{ git = \"https://example.test/g\", branch = \"dev\" }",
            &format!("git+https://example.test/g?branch=main#{COMMIT}"),
        );

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_branch_with_a_slash_is_current_against_its_percent_encoded_lock() {
        let fixture = git_workspace(
            "{ git = \"https://example.test/g\", branch = \"feat/x\" }",
            &format!("git+https://example.test/g?branch=feat%2Fx#{COMMIT}"),
        );

        assert!(is_current(&fixture));
    }

    #[test]
    fn a_registry_requirement_the_locked_version_no_longer_meets_is_stale() {
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"r\"]\n\n[[package]]\nname = \"r\"\nversion = \"1.9.0\"\n\
             source = \"{CRATES_IO}\"\n"
        );
        let met = workspace("[dependencies]\nr = \"1\"\n", &lock);
        let unmet = workspace("[dependencies]\nr = \"2\"\n", &lock);

        assert!(is_current(&met));
        assert!(!is_current(&unmet));
    }

    #[test]
    fn a_source_kind_that_differs_from_the_declaration_is_stale() {
        // `g` is declared from crates.io but locked from a git source.
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"g\"]\n\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
             source = \"git+https://example.test/g#{COMMIT}\"\n"
        );
        let fixture = workspace("[dependencies]\ng = \"1\"\n", &lock);

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_path_declaration_locked_with_a_source_is_stale() {
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"dep\"]\n\n[[package]]\nname = \"dep\"\nversion = \"1.2.0\"\n\
             source = \"{CRATES_IO}\"\n"
        );
        let fixture = workspace(APP_DEPENDS_ON_DEP, &lock);

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_path_requirement_the_manifest_version_no_longer_meets_is_stale() {
        let fixture = workspace(
            "[dependencies]\ndep = { path = \"../dep\", version = \"2\" }\n",
            &lock_of_app_and_dep("\"dep\"", ""),
        );

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_path_declaration_pointing_at_another_package_is_stale() {
        // `dep` is declared at `../other`, whose manifest names `other`.
        let fixture = workspace(
            "[dependencies]\ndep = { path = \"../other\" }\n",
            &lock_of_app_and_dep("\"dep\"", ""),
        );
        fixture.package("other", "other", "version = \"1.2.0\"\n");

        assert!(!is_current(&fixture));
    }

    #[test]
    fn a_non_member_is_locked_for_what_it_needs_and_its_dev_dependency_is_not() {
        // `dep` is outside the workspace: its build and `cfg(windows)` dependencies are locked,
        // its dev-dependency and its optional dependency are not.
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package(
            "app",
            "app",
            "[dependencies]\ndep = { path = \"../../dep\" }\n",
        );
        fixture.package(
            "../dep",
            "dep",
            "[build-dependencies]\nbuilder = { path = \"../builder\" }\n\
             [target.'cfg(windows)'.dependencies]\nwindowed = { path = \"../windowed\" }\n\
             [dev-dependencies]\ndeveloper = { path = \"../developer\" }\n\
             [dependencies]\noptional = { path = \"../optional\", optional = true }\n",
        );
        for name in ["builder", "windowed"] {
            fixture.package(&format!("../{name}"), name, "");
        }
        fixture.write(
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
             dependencies = [\"dep\"]\n\n[[package]]\nname = \"builder\"\nversion = \"0.0.0\"\n\n\
             [[package]]\nname = \"dep\"\nversion = \"0.0.0\"\n\
             dependencies = [\"builder\", \"windowed\"]\n\n\
             [[package]]\nname = \"windowed\"\nversion = \"0.0.0\"\n",
        );

        assert!(is_current(&fixture));
    }
}
