//! Optional dependencies of packages outside the workspace that ask a worn package for a version
//! its lock does not name.
//!
//! A registry holds many versions of a package, and Cargo keeps one of them for each range of
//! versions it calls compatible ([`compatible_range::CompatibleRange`]). When a feature enables an
//! optional dependency that asks a registry package for a version in the range a worn package is
//! locked in, and the locked version does not meet it, Cargo cannot resolve the workspace at all:
//! it keeps the version it has and finds it too old, and only updating the worn package helps. A
//! requirement in another range gets a second version beside the locked one, and a relock gives
//! Cargo's own document.
//!
//! A path or git source holds exactly one version of a package, so there is no second version to
//! give: any requirement the locked version does not meet fails once the dependency is enabled, in
//! whatever range it is written, and no relock gets past it ([`source_kind::SourceKind`]).
//!
//! Neither a registry requirement in the locked range nor an unmet requirement on a path or git
//! package is the stale lock this reader reads through, where a relock gives Cargo's own document,
//! so both are refused.
//!
//! The reader does not resolve features, so it cannot tell whether a feature enables the
//! dependency, and refuses whenever one could. A workspace where nothing does is refused too,
//! though Cargo reads it. What it can see is limited to manifests on disk: a requirement in a
//! registry package's manifest is not read, and a worn package that is not locked is never
//! compared.

mod compatible_range;
mod source_kind;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};

use super::cargo_cannot_load::CargoCannotLoad;
use super::error::ReadWithoutCargoError;
use super::file_system::{MANIFEST_FILE_NAME, normalized};
use super::layout::Layout;
use super::lock_entries::set_aside_entries;
use super::locked_source;
use super::lockfile::{LockEntry, Lockfile};
use super::manifest_failure::ManifestFailure;
use super::package_manifest::{Declaration, DeclaredSource, Unfollowed};
use super::path_package::{Inherits, read_manifest};
use super::worn_entries::worn_entries;
use source_kind::SourceKind;

/// Refuses a workspace where a package outside it, or a package reached by path from one of its
/// optional dependencies, asks for a worn package at a version the lock does not name and Cargo
/// has no way to give. For a registry package that is a version in the locked version's
/// compatible range, since a requirement in another range gets a second version beside it; for
/// a path or git package, which holds only the locked version, it is any version.
///
/// The declarations judged are every optional dependency of a package outside the workspace
/// that the layout holds, and every dependency of each package reached from those by path that
/// the layout does not hold, and of each package reached from those in turn. The layout holds
/// the packages the lock lists, and currency has judged their requirements already; the others
/// are the ones an optional dependency no feature has enabled yet leads to. A manifest Cargo
/// loads is read whole, a git URL's own query included, and its other requirements and path
/// dependencies are judged whether or not a feature enables the package: nothing here compares
/// the source of a git dependency read past, which can name no worn package. Exactly two
/// shapes are passed over, because Cargo fails on them too: a manifest that is not there, and a
/// dependency of the package that writes both `git` and `path`. The same entry in a workspace
/// root's `[workspace.dependencies]` is not one of them, since Cargo loads it until a member
/// inherits it. Any other manifest this reader cannot read is refused, naming the file, though
/// Cargo may never load the package. The packages read here do not join the layout, because
/// currency would then ask for them in the lock.
///
/// A requirement `semver` does not parse is passed over, as currency passes it over.
///
/// # Errors
///
/// Unsupported, naming the manifest, the dependency and the worn package. Unreadable or
/// Unsupported, naming the file, for a package reached that this reader cannot read in a way
/// that is neither of the two shapes above.
pub(super) fn refuse_unmet_optional_requirements(
    layout: &Layout,
    lockfile: &Lockfile,
) -> Result<(), ReadWithoutCargoError> {
    let worn = WornPackages::new(layout, lockfile);
    if worn.entries.is_empty() {
        return Ok(());
    }
    let mut unread: Vec<PathBuf> = Vec::new();
    for outsider in layout.packages.iter().filter(|package| !package.is_member) {
        for declaration in outsider
            .declarations
            .iter()
            .filter(|declaration| !declaration.is_dev && declaration.is_optional)
        {
            worn.refuse_if_unmet(&outsider.manifest_path, declaration)?;
            if let DeclaredSource::Path { directory } = &declaration.source {
                unread.push(normalized(directory));
            }
        }
    }
    refuse_in_packages_reached(&worn, unread)
}

/// Refuses an unmet requirement in a package at one of `pending`, or reached from one by path,
/// that the layout does not hold. Each directory is read once, so the loop is bounded by the
/// number of directories the packages read name.
fn refuse_in_packages_reached(
    worn: &WornPackages<'_>,
    mut pending: Vec<PathBuf>,
) -> Result<(), ReadWithoutCargoError> {
    let mut visited: BTreeSet<PathBuf> = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        if worn.layout_holds(&directory) {
            continue;
        }
        if !visited.insert(directory.clone()) {
            continue;
        }
        // A git URL with a query of its own is a manifest Cargo loads, so it is read past and
        // the package's other requirements and path dependencies are judged.
        let manifest = match read_manifest(&directory, Inherits::Ancestors, Unfollowed::ReadPast) {
            Ok(manifest) => manifest,
            Err(failure) if passed_over(&failure) => continue,
            Err(failure) => return Err(failure.into()),
        };
        let manifest_path = directory.join(MANIFEST_FILE_NAME);
        for declaration in manifest
            .declarations()
            .iter()
            .filter(|declaration| !declaration.is_dev)
        {
            worn.refuse_if_unmet(&manifest_path, declaration)?;
            if let DeclaredSource::Path { directory } = &declaration.source {
                pending.push(normalized(directory));
            }
        }
    }
    assert!(
        visited
            .iter()
            .all(|directory| !worn.layout_holds(directory)),
        "no directory the layout holds is read again"
    );
    Ok(())
}

/// Whether the walk passes over the package whose manifest read failed with `failure`, rather
/// than refusing the workspace.
///
/// A manifest is passed over only where Cargo fails to load it too, and only for the two shapes
/// [`CargoCannotLoad`] names, each measured against Cargo. The match has no wildcard: a failure
/// that carries no shape is refused by name, so a refusal added to the manifest reader later
/// refuses the workspace instead of silently hiding the packages that raise it.
const fn passed_over(failure: &ManifestFailure) -> bool {
    let Some(shape) = failure.cargo_cannot_load() else {
        return false;
    };
    match shape {
        CargoCannotLoad::Missing | CargoCannotLoad::GitAndPath => true,
    }
}

/// The lock entries a wearing can resolve to, and the packages on disk their declarations are
/// compared with.
struct WornPackages<'input> {
    layout: &'input Layout,
    entries: Vec<&'input LockEntry>,
}

impl<'input> WornPackages<'input> {
    fn new(layout: &'input Layout, lockfile: &'input Lockfile) -> Self {
        let set_aside = set_aside_entries(lockfile, &layout.packages);
        let indexes = worn_entries(layout, lockfile, &set_aside);
        assert!(
            indexes.is_disjoint(&set_aside),
            "an entry Cargo sets aside is never worn"
        );
        Self {
            layout,
            entries: indexes
                .into_iter()
                .map(|index| &lockfile.entries()[index])
                .collect(),
        }
    }

    /// Whether the layout holds a package in `directory`.
    fn layout_holds(&self, directory: &Path) -> bool {
        self.layout
            .packages
            .iter()
            .any(|package| package.directory == directory)
    }

    /// Refuses `declaration`, written in `manifest`, when it asks a worn package for a version
    /// that package's locked version does not meet and that Cargo cannot add beside it: for a
    /// registry package, one its compatible range can meet; for a path or git package, any.
    fn refuse_if_unmet(
        &self,
        manifest: &Path,
        declaration: &Declaration,
    ) -> Result<(), ReadWithoutCargoError> {
        let Some(text) = declaration.version_requirement.as_deref() else {
            return Ok(());
        };
        // Whether a requirement `semver` does not parse is one Cargo reads is not this reader's
        // to say; currency passes it over too.
        let Ok(requirement) = VersionReq::parse(text) else {
            return Ok(());
        };
        for entry in self
            .entries
            .iter()
            .filter(|entry| self.is_targeted_by(entry, declaration))
        {
            // A locked version that is no version is not compared: nothing meets it either.
            let Ok(locked) = Version::parse(entry.version()) else {
                continue;
            };
            // The declaration agrees with the lock on the source (`is_targeted_by`), so its
            // kind of source is the entry's.
            if let Some(reason) =
                SourceKind::of(&declaration.source).why_unmet(&locked, &requirement)
            {
                assert_eq!(
                    declaration.package,
                    entry.name(),
                    "a declaration is refused for the package it names"
                );
                return Err(ReadWithoutCargoError::unsupported(format!(
                    "{}: `{}` requires `{}` `{text}`, which the worn package's locked version \
                     {locked} does not meet{reason}, so cargo cannot resolve the workspace once \
                     a feature enables it",
                    manifest.display(),
                    declaration.key,
                    declaration.package,
                )));
            }
        }
        Ok(())
    }

    /// Whether `declaration` asks for the package `entry` is: it has the name, agrees with the
    /// lock on the source, and for a path is the directory of the package on disk that `entry`
    /// stands for.
    fn is_targeted_by(&self, entry: &LockEntry, declaration: &Declaration) -> bool {
        declaration.package == entry.name()
            && locked_source::agrees(entry.source(), &declaration.source)
            && match &declaration.source {
                DeclaredSource::Path { directory } => {
                    let directory = normalized(directory);
                    self.layout.packages.iter().any(|package| {
                        package.directory == directory
                            && package.name == entry.name()
                            && package.version == entry.version()
                    })
                }
                DeclaredSource::Git { .. } | DeclaredSource::Registry { .. } => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::passed_over;
    use crate::workspace::without_cargo::manifest_failure::ManifestFailure;
    use crate::workspace::without_cargo::text_error::{RefusedShape, TextError};

    const MANIFEST: &str = "/w/z/Cargo.toml";

    #[test]
    fn a_refusal_the_manifest_reader_gains_later_is_not_passed_over() {
        // The walk may pass over a manifest only where Cargo fails on it too, and a refusal
        // added to the manifest reader later has not been measured against Cargo. Wrap one of
        // each kind as `read_manifest` wraps a refused text, with no shape, and require that the
        // walk refuses it: a refusal the reader gains does not make the walk silently pass
        // over the packages that raise it.
        let path = Path::new(MANIFEST);
        let unreadable = TextError::unreadable("a refusal added to the manifest reader later");
        let unsupported = TextError::unsupported("a refusal added to the manifest reader later");

        assert!(!passed_over(&ManifestFailure::text(path, &unreadable)));
        assert!(!passed_over(&ManifestFailure::text(path, &unsupported)));
    }

    #[test]
    fn the_two_shapes_cargo_fails_on_too_are_passed_over() {
        // A manifest that is not there and a dependency that writes both `git` and `path` are
        // the shapes measured to fail in Cargo as well, so the walk passes over them.
        let path = Path::new(MANIFEST);
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        let ambiguous = TextError::unreadable_as(RefusedShape::GitAndPath, "ambiguous");

        assert!(passed_over(&ManifestFailure::file(path, &gone)));
        assert!(passed_over(&ManifestFailure::text(path, &ambiguous)));
    }

    #[test]
    fn a_file_error_other_than_not_found_is_not_passed_over() {
        // A manifest that exists but cannot be read, say for its permissions, is one Cargo may
        // read, so it is no shape the walk passes over.
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        assert!(!passed_over(&ManifestFailure::file(
            Path::new(MANIFEST),
            &denied
        )));
    }
}
