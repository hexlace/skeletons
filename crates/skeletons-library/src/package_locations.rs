//! Where the files of each locked package that is not a path on disk are.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skeletons_engine::LockedPackageId;

use crate::locked_package::LockedPackage;

/// Where the files of each locked package are, for the packages
/// [`check_without_cargo`](crate::check_without_cargo) cannot find by itself.
///
/// A package with a `source` in `Cargo.lock`, from a registry or a git
/// repository, is not a path on disk, so the caller says where its files are.
/// Each location is the directory holding that package's own `Cargo.toml`, as
/// the source `Cargo.lock` names has it: for a git package, at the commit
/// after `#`. Only the manifest's name and version are checked against the
/// key, so the files of another commit at the same version are read as given. A
/// path package is found from the manifests and needs none. A relative
/// directory is taken from the process's current directory when
/// `check_without_cargo` reads it, not when it is inserted.
///
/// A location for a package no wearing resolves to is never read, and is not
/// an error.
///
/// # Examples
///
/// ```
/// use skeletons_library::{LockedPackage, PackageLocations};
///
/// let package = LockedPackage {
///     name: "house-rules".to_owned(),
///     version: "4.5.6".to_owned(),
///     source: "registry+https://github.com/rust-lang/crates.io-index".to_owned(),
/// };
/// let mut locations = PackageLocations::new();
/// assert_eq!(locations.insert(package.clone(), "vendor/house-rules-4.5.6"), None);
///
/// assert_eq!(
///     locations.get(&package),
///     Some(std::path::Path::new("vendor/house-rules-4.5.6"))
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PackageLocations {
    directories: BTreeMap<LockedPackage, PathBuf>,
}

impl PackageLocations {
    /// No locations: enough for a workspace whose skeletons are all path
    /// dependencies. Equal to [`default`](Default::default).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that the files of `package` are in `directory`, and returns the
    /// directory it replaced, as [`BTreeMap::insert`] does.
    pub fn insert(
        &mut self,
        package: LockedPackage,
        directory: impl Into<PathBuf>,
    ) -> Option<PathBuf> {
        self.directories.insert(package, directory.into())
    }

    /// The directory recorded for `package`, if one was.
    #[must_use]
    pub fn get(&self, package: &LockedPackage) -> Option<&Path> {
        self.directories.get(package).map(PathBuf::as_path)
    }

    /// How many packages have a location.
    #[must_use]
    pub fn len(&self) -> usize {
        self.directories.len()
    }

    /// Whether no package has a location.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }

    /// The locations in the form the engine reads them.
    pub(crate) fn to_engine(&self) -> BTreeMap<LockedPackageId, PathBuf> {
        let converted: BTreeMap<LockedPackageId, PathBuf> = self
            .directories
            .iter()
            .map(|(package, directory)| (package.to_engine(), directory.clone()))
            .collect();
        assert_eq!(
            converted.len(),
            self.directories.len(),
            "two packages are two identities, whichever type holds them"
        );
        converted
    }
}

/// Collects pairs of a package and its directory. A later pair for the same
/// package replaces the earlier one.
impl<P: Into<PathBuf>> FromIterator<(LockedPackage, P)> for PackageLocations {
    fn from_iter<I: IntoIterator<Item = (LockedPackage, P)>>(pairs: I) -> Self {
        let mut locations = Self::new();
        locations.extend(pairs);
        locations
    }
}

/// Adds pairs of a package and its directory. A later pair for the same
/// package replaces the earlier one.
impl<P: Into<PathBuf>> Extend<(LockedPackage, P)> for PackageLocations {
    fn extend<I: IntoIterator<Item = (LockedPackage, P)>>(&mut self, pairs: I) {
        for (package, directory) in pairs {
            self.insert(package, directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PackageLocations;
    use crate::locked_package::LockedPackage;

    fn package(name: &str, version: &str) -> LockedPackage {
        LockedPackage {
            name: name.to_owned(),
            version: version.to_owned(),
            source: "registry+https://example.test/index".to_owned(),
        }
    }

    #[test]
    fn the_engine_form_holds_every_location_under_its_identity() {
        // Two versions of one name keep their own directories through the
        // conversion, and the directory stays with the package it was given for.
        let mut locations = PackageLocations::new();
        locations.insert(package("a", "1.0.0"), "/one");
        locations.insert(package("a", "1.0.1"), "/two");

        let engine = locations.to_engine();

        assert_eq!(engine.len(), 2);
        assert_eq!(
            engine.get(&package("a", "1.0.0").to_engine()),
            Some(&PathBuf::from("/one"))
        );
        assert_eq!(
            engine.get(&package("a", "1.0.1").to_engine()),
            Some(&PathBuf::from("/two"))
        );
    }

    #[test]
    fn no_locations_convert_to_none() {
        assert!(PackageLocations::new().to_engine().is_empty());
    }
}
