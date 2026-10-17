//! The shapes of workspace that Cargo reads and this reader does not follow, refused by name.
//!
//! Refusing one is cheaper and more honest than reproducing Cargo's resolution of it, and a
//! refusal can never be a report that silently differs from the one Cargo's own reading gives.

use std::collections::{BTreeMap, BTreeSet};

use super::error::ReadWithoutCargoError;
use super::file_system::MANIFEST_FILE_NAME;
use super::layout::Layout;
use super::path_package::PathPackage;

/// Refuses `layout` when it uses `[patch]` or `[replace]`, a `package.workspace` key, a member
/// that gives one package two dependency keys or one key two packages, or two packages with one
/// name and version.
///
/// # Errors
///
/// Unsupported, naming the manifest and the feature.
pub(super) fn check(layout: &Layout) -> Result<(), ReadWithoutCargoError> {
    let root_manifest = layout.root_directory.join(MANIFEST_FILE_NAME);
    if layout.root_writes_patch {
        return Err(ReadWithoutCargoError::unsupported(format!(
            "`[patch]` in {}",
            root_manifest.display()
        )));
    }
    if layout.root_writes_replace {
        return Err(ReadWithoutCargoError::unsupported(format!(
            "`[replace]` in {}",
            root_manifest.display()
        )));
    }

    for package in &layout.packages {
        if package.has_workspace_key {
            return Err(ReadWithoutCargoError::unsupported(format!(
                "`package.workspace` in {}",
                package.manifest_path.display()
            )));
        }
        if package.is_member {
            check_keys(package)?;
        }
    }
    check_identities(&layout.packages)
}

/// Refuses a member that declares one package under two keys, or one key for two packages. The
/// edges `cargo metadata` writes for such a member depend on how Cargo unifies the declarations,
/// which this reader does not reproduce.
fn check_keys(member: &PathPackage) -> Result<(), ReadWithoutCargoError> {
    let mut packages_of_key: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut keys_of_package: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for declaration in &member.declarations {
        packages_of_key
            .entry(declaration.key.as_str())
            .or_default()
            .insert(declaration.package.as_str());
        keys_of_package
            .entry(declaration.package.as_str())
            .or_default()
            .insert(declaration.key.as_str());
    }

    if let Some((key, packages)) = packages_of_key.iter().find(|(_, set)| set.len() > 1) {
        return Err(ReadWithoutCargoError::unsupported(format!(
            "{}: the key `{key}` names more than one package ({})",
            member.manifest_path.display(),
            joined(packages)
        )));
    }
    if let Some((package, keys)) = keys_of_package.iter().find(|(_, set)| set.len() > 1) {
        return Err(ReadWithoutCargoError::unsupported(format!(
            "{}: the package `{package}` is declared under more than one key ({})",
            member.manifest_path.display(),
            joined(keys)
        )));
    }
    Ok(())
}

/// Refuses two packages on disk that share a name and a version, which no package id this reader
/// holds could tell apart.
fn check_identities(packages: &[PathPackage]) -> Result<(), ReadWithoutCargoError> {
    let mut seen: BTreeMap<(&str, &str), &PathPackage> = BTreeMap::new();
    for package in packages {
        if let Some(first) = seen.insert((&package.name, &package.version), package) {
            return Err(ReadWithoutCargoError::unsupported(format!(
                "{} and {} are both the package `{}` {}",
                first.manifest_path.display(),
                package.manifest_path.display(),
                package.name,
                package.version
            )));
        }
    }
    Ok(())
}

/// The names in `names`, in backticks, separated by commas.
fn joined(names: &BTreeSet<&str>) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
