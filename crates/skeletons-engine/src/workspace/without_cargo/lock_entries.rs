//! Questions about the packages of a lockfile that depend on which packages are on disk.

use std::collections::BTreeSet;

use super::lockfile::{LockEntry, Lockfile};
use super::path_package::PathPackage;

/// The position, in [`Lockfile::entries`], of the entry with no `source` that has this name and
/// version: how a path package is locked.
pub(super) fn path_entry_index(lockfile: &Lockfile, name: &str, version: &str) -> Option<usize> {
    lockfile.entries().iter().position(|entry| {
        entry.source().is_none() && entry.name() == name && entry.version() == version
    })
}

/// The names of the path packages the entry with no `source` of this name and version lists as
/// its dependencies: what the lockfile says that package needs, whichever of its optional
/// dependencies a feature enabled when it was written. Empty when no such entry exists.
pub(super) fn locked_path_dependency_names<'lock>(
    lockfile: &'lock Lockfile,
    name: &str,
    version: &str,
) -> BTreeSet<&'lock str> {
    let Some(index) = path_entry_index(lockfile, name, version) else {
        return BTreeSet::new();
    };
    let dependency_indexes = lockfile.entries()[index].dependency_indexes();
    let names: BTreeSet<&str> = dependency_indexes
        .iter()
        .map(|&dependency| &lockfile.entries()[dependency])
        .filter(|dependency| dependency.source().is_none())
        .map(LockEntry::name)
        .collect();
    assert!(
        names.len() <= dependency_indexes.len(),
        "every name comes from one of the entry's own dependencies"
    );
    names
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::locked_path_dependency_names;
    use crate::workspace::without_cargo::lockfile::Lockfile;

    /// `y` 0.0.0 lists a path package `z`, a registry package `r` and a git package `g`; `z`
    /// lists nothing.
    const LOCK: &str = "version = 4\n\n\
        [[package]]\nname = \"g\"\nversion = \"1.0.0\"\n\
        source = \"git+https://example.test/g#0123456789abcdef\"\n\n\
        [[package]]\nname = \"r\"\nversion = \"1.0.0\"\n\
        source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
        checksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n\n\
        [[package]]\nname = \"y\"\nversion = \"0.0.0\"\ndependencies = [\"g\", \"r\", \"z\"]\n\n\
        [[package]]\nname = \"z\"\nversion = \"0.1.0\"\n";

    fn lock() -> Lockfile {
        Lockfile::parse(LOCK).expect("a lockfile")
    }

    #[test]
    fn only_the_path_packages_an_entry_lists_are_named() {
        // The registry and git packages `y` lists have a source, so they are not path packages.
        assert_eq!(
            locked_path_dependency_names(&lock(), "y", "0.0.0"),
            BTreeSet::from(["z"])
        );
    }

    #[test]
    fn an_entry_that_lists_nothing_names_nothing() {
        assert_eq!(
            locked_path_dependency_names(&lock(), "z", "0.1.0"),
            BTreeSet::new()
        );
    }

    #[test]
    fn an_entry_at_another_version_or_of_another_name_is_absent() {
        // A package whose version changed since locking has no entry, which currency refuses.
        assert_eq!(
            locked_path_dependency_names(&lock(), "y", "0.0.1"),
            BTreeSet::new()
        );
        assert_eq!(
            locked_path_dependency_names(&lock(), "absent", "0.0.0"),
            BTreeSet::new()
        );
    }

    #[test]
    fn an_entry_with_a_source_is_not_the_path_package_of_its_name() {
        // `r` is locked from a registry, so no path package `r` is locked.
        assert_eq!(
            locked_path_dependency_names(&lock(), "r", "1.0.0"),
            BTreeSet::new()
        );
    }
}
