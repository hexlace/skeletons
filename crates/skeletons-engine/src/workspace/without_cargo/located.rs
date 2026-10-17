//! A locked package whose files the caller says where to find.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::error::ReadWithoutCargoError;
use super::file_system::MANIFEST_FILE_NAME;
use super::package_manifest::{Declaration, Unfollowed};
use super::targets::library_name;
use super::workspace_search::parse_inheriting;
use crate::workspace::LockedPackageId;

/// A locked package with a source, read from the directory the caller gave for it.
#[derive(Debug)]
pub(super) struct LocatedPackage {
    /// The `Cargo.toml` inside the directory given.
    pub(super) manifest_path: PathBuf,
    /// `[package.metadata]` as `cargo metadata` reports it.
    pub(super) metadata: serde_json::Value,
    /// Every dependency the manifest declares, of any kind.
    pub(super) declarations: Vec<Declaration>,
    /// The library's name, or `None` for a package with no library.
    pub(super) library: Option<String>,
}

/// Reads the package `identity` from the directory `locations` holds for it.
///
/// Returns `None` when the package has no files to read: no directory was given, the directory
/// holds no manifest Cargo reads, or the manifest names another package or another version. Such
/// a package locates nothing, and whether it is a skeleton is unknown.
///
/// A manifest Cargo loads is read whole, a git URL's own query included: the declarations are
/// kept for their names and renames and none is compared with a source, so a shape this reader
/// does not follow where it compares sources is no reason to call the package unlocated.
///
/// # Errors
///
/// Unreadable when the manifest names the package and Cargo would refuse the package for its
/// targets: it has none, its library's name or path is refused, or one of its target tables is.
pub(super) fn read(
    locations: &BTreeMap<LockedPackageId, PathBuf>,
    identity: &LockedPackageId,
) -> Result<Option<LocatedPackage>, ReadWithoutCargoError> {
    let Some(directory) = locations.get(identity) else {
        return Ok(None);
    };
    let manifest_path = directory.join(MANIFEST_FILE_NAME);
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return Ok(None);
    };
    let Ok(manifest) = parse_inheriting(&text, directory, Unfollowed::ReadPast) else {
        return Ok(None);
    };
    let Some(package) = manifest.package() else {
        return Ok(None);
    };
    if package.name != identity.name || package.version != identity.version {
        return Ok(None);
    }

    let library = library_name(directory, &package.name, manifest.targets())
        .map_err(|error| ReadWithoutCargoError::text(&manifest_path, &error))?;
    Ok(Some(LocatedPackage {
        manifest_path,
        metadata: package.metadata.clone(),
        declarations: manifest.declarations().to_vec(),
        library,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::read;
    use crate::workspace::LockedPackageId;
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;

    const SOURCE: &str = "git+https://example.test/skeleton";

    fn identity(name: &str, version: &str) -> LockedPackageId {
        LockedPackageId {
            name: name.to_owned(),
            version: version.to_owned(),
            source: SOURCE.to_owned(),
        }
    }

    /// A directory holding a library package `house-rules` 4.5.6 with `[package.metadata]`.
    fn written() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().to_owned();
        std::fs::create_dir(root.join("src")).expect("a directory");
        std::fs::write(root.join("src/lib.rs"), "").expect("a file");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"house-rules\"\nversion = \"4.5.6\"\n\
             [package.metadata.skeletons]\nx = 1\n[dependencies]\nserde = \"1\"\n",
        )
        .expect("a manifest");
        (directory, root)
    }

    fn located_at(path: &Path) -> BTreeMap<LockedPackageId, PathBuf> {
        BTreeMap::from([(identity("house-rules", "4.5.6"), path.to_owned())])
    }

    #[test]
    fn a_package_at_its_location_is_read_with_its_metadata_and_library() {
        let (_directory, root) = written();

        let package = read(&located_at(&root), &identity("house-rules", "4.5.6"))
            .expect("readable")
            .expect("located");

        assert_eq!(package.library.as_deref(), Some("house_rules"));
        assert_eq!(package.metadata["skeletons"]["x"], 1);
        assert_eq!(package.declarations.len(), 1);
        assert_eq!(package.manifest_path, root.join("Cargo.toml"));
    }

    #[test]
    fn a_package_whose_manifest_writes_a_git_query_in_a_dev_dependency_is_located() {
        // Cargo loads a manifest whose git URL has a query of its own, and the caller gave the
        // location, so the package is read, with that dependency among its declarations. Nothing
        // is compared with its source.
        let (_directory, root) = written();
        let mut manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("a manifest");
        manifest.push_str("[dev-dependencies]\nfoo = { git = \"https://example.test/foo?x=1\" }\n");
        std::fs::write(root.join("Cargo.toml"), manifest).expect("a manifest");

        let package = read(&located_at(&root), &identity("house-rules", "4.5.6"))
            .expect("readable")
            .expect("located");

        let keys: Vec<&str> = package
            .declarations
            .iter()
            .map(|declaration| declaration.key.as_str())
            .collect();
        assert_eq!(keys, ["serde", "foo"]);
    }

    #[test]
    fn a_package_given_no_location_is_unlocated() {
        let located = read(&BTreeMap::new(), &identity("house-rules", "4.5.6"));

        assert!(matches!(located, Ok(None)), "{located:?}");
    }

    #[test]
    fn a_location_with_no_manifest_is_unlocated() {
        let directory = tempfile::tempdir().expect("a temporary directory");

        let located = read(
            &located_at(directory.path()),
            &identity("house-rules", "4.5.6"),
        );

        assert!(matches!(located, Ok(None)), "{located:?}");
    }

    #[test]
    fn a_location_whose_manifest_names_another_package_or_version_is_unlocated() {
        // The files are `house-rules` 4.5.6, so the same files asked about as another name, or
        // the same name at another version, locate nothing.
        let (_directory, root) = written();
        let asked = |name: &str, version: &str| {
            let locations = BTreeMap::from([(identity(name, version), root.clone())]);
            read(&locations, &identity(name, version))
        };

        assert!(matches!(asked("other-rules", "4.5.6"), Ok(None)));
        assert!(matches!(asked("house-rules", "4.5.7"), Ok(None)));
    }

    #[test]
    fn a_location_whose_manifest_is_not_a_manifest_is_unlocated() {
        let (_directory, root) = written();
        std::fs::write(root.join("Cargo.toml"), "this is = not [toml").expect("a file");

        let located = read(&located_at(&root), &identity("house-rules", "4.5.6"));

        assert!(matches!(located, Ok(None)), "{located:?}");
    }

    #[test]
    fn a_named_package_with_no_target_is_refused_as_cargo_refuses_it() {
        let (_directory, root) = written();
        std::fs::remove_file(root.join("src/lib.rs")).expect("remove the library");

        let located = read(&located_at(&root), &identity("house-rules", "4.5.6"));

        assert!(
            matches!(
                located,
                Err(ReadWithoutCargoError::ManifestUnreadable { .. })
            ),
            "{located:?}"
        );
    }
}
