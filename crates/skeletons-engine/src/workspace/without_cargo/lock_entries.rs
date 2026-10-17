//! Questions about the packages of a lockfile that depend on which packages are on disk.

use std::collections::BTreeSet;

use super::lockfile::Lockfile;
use super::path_package::PathPackage;

/// The position, in [`Lockfile::entries`], of the entry with no `source` that has this name and
/// version: how a path package is locked.
pub(super) fn path_entry_index(lockfile: &Lockfile, name: &str, version: &str) -> Option<usize> {
    lockfile.entries().iter().position(|entry| {
        entry.source().is_none() && entry.name() == name && entry.version() == version
    })
}

/// The positions of the entries Cargo sets aside when it loads the lockfile: those with no
/// `source` whose name no package on disk has. They are path packages that have left the
/// workspace, and Cargo drops them, and every dependency entry that names one, rather than
/// reading them as stale.
pub(super) fn set_aside_entries(lockfile: &Lockfile, packages: &[PathPackage]) -> BTreeSet<usize> {
    let names: BTreeSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    lockfile
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.source().is_none() && !names.contains(entry.name()))
        .map(|(index, _)| index)
        .collect()
}
