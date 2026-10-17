//! Whether `Cargo.lock` is current: the answer `cargo metadata --locked` gives, from the
//! manifests and the lockfile alone.

use super::error::ReadWithoutCargoError;
use super::lock_entries::path_entry_index;
use super::lockfile::Lockfile;
use super::path_package::PathPackage;

/// Refuses a lockfile that Cargo would call out of date for the workspace whose packages on disk
/// are `packages`.
///
/// Today this checks that every package on disk is locked as a package with no `source`, under
/// its name and version.
///
/// # Errors
///
/// [`ReadWithoutCargoError::Lockfile`] for every way the lockfile is out of date.
pub(super) fn verify_lockfile_is_current(
    lockfile: &Lockfile,
    packages: &[PathPackage],
) -> Result<(), ReadWithoutCargoError> {
    for package in packages {
        if path_entry_index(lockfile, &package.name, &package.version).is_none() {
            return Err(ReadWithoutCargoError::Lockfile);
        }
    }
    Ok(())
}
