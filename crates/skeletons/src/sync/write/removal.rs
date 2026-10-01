//! Removing something `sync` created — and only that.
//!
//! `remove_file` and `remove_dir` resolve every directory above their path by
//! name when they run, so a directory swapped for a symbolic link after `sync`
//! created something under it would make a removal by name unlink whatever
//! has that name where the link points. So a removal is never by name alone:
//! [`remove_staging_file`] and [`remove_directory`] first walk the path again
//! from the workspace root with the claim walk ([`look_up`]), which refuses a
//! link, a file or a respelled directory above it, and then remove the entry
//! only when it is still the device and inode `sync` recorded when it created
//! it. They differ only in what each may report: only a directory can be kept
//! for holding something.

use std::path::Path;

use super::identity::FileIdentity;
use super::leftover::LeftoverReason;
use crate::claim::{ClaimPath, Located, look_up};

/// One thing this run created and has not handed over, with what it was when
/// it was created.
#[derive(Debug)]
pub(super) enum Created {
    Directory {
        path: ClaimPath,
        identity: FileIdentity,
    },
    StagingFile {
        path: ClaimPath,
        identity: FileIdentity,
    },
}

/// What became of the removal of a staging file.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Removal {
    /// The file `sync` created was there, and is gone.
    Removed,
    /// Nothing is at the path now.
    Gone,
    /// Left in place, never removed, with the reason.
    Left(LeftoverReason),
}

/// What became of the removal of a directory.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DirectoryRemoval {
    /// The directory `sync` created was there, and is gone.
    Removed,
    /// A directory that still holds something: kept, and not a leftover,
    /// since nothing `sync` created is lost, only left where it already was.
    KeptNotEmpty,
    /// Nothing is at the path now.
    Gone,
    /// Left in place, never removed, with the reason.
    Left(LeftoverReason),
}

/// Which kind of entry a removal is for: what the entry found must be, and
/// which `std::fs` call removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Directory,
}

/// What the walk and the identity check made of the path, before any remove
/// call, or what the remove call itself made of it.
enum Attempt {
    Removed,
    Gone,
    Left(LeftoverReason),
    /// The operating system refused the remove call.
    Refused(std::io::Error),
}

/// Removes the staging file `sync` created at `path` under `root` if, and
/// only if, the path still leads through real directories to that very file.
pub(super) fn remove_staging_file(
    root: &Path,
    path: &ClaimPath,
    identity: FileIdentity,
) -> Removal {
    match attempt(root, path, identity, Kind::File) {
        Attempt::Removed => Removal::Removed,
        Attempt::Gone => Removal::Gone,
        Attempt::Left(reason) => Removal::Left(reason),
        // Any refusal, whatever its kind, is one more thing the operating
        // system would not remove, so it is a leftover like any other.
        Attempt::Refused(error) => Removal::Left(could_not_remove(&error)),
    }
}

/// Removes the directory `sync` created at `path` under `root` if, and only
/// if, the path still leads through real directories to that very directory.
pub(super) fn remove_directory(
    root: &Path,
    path: &ClaimPath,
    identity: FileIdentity,
) -> DirectoryRemoval {
    match attempt(root, path, identity, Kind::Directory) {
        Attempt::Removed => DirectoryRemoval::Removed,
        Attempt::Gone => DirectoryRemoval::Gone,
        Attempt::Left(reason) => DirectoryRemoval::Left(reason),
        Attempt::Refused(error) => match error.kind() {
            std::io::ErrorKind::DirectoryNotEmpty => DirectoryRemoval::KeptNotEmpty,
            _ => DirectoryRemoval::Left(could_not_remove(&error)),
        },
    }
}

fn could_not_remove(error: &std::io::Error) -> LeftoverReason {
    LeftoverReason::CouldNotRemove {
        detail: error.to_string(),
    }
}

/// The walk ([`look_up`]) runs first. If it refuses the path, nothing is
/// removed. If it finds an entry, that entry is removed only when its device,
/// inode and kind are the ones recorded at creation.
fn attempt(root: &Path, path: &ClaimPath, identity: FileIdentity, kind: Kind) -> Attempt {
    let metadata = match look_up(root, path) {
        Err(cause) => return Attempt::Left(LeftoverReason::PathUnsafe(cause)),
        Ok(Located::Missing) => return Attempt::Gone,
        Ok(Located::Entry(metadata)) => metadata,
    };
    let kind_is_right = match kind {
        Kind::File => metadata.is_file(),
        Kind::Directory => metadata.is_dir(),
    };
    match (kind_is_right, FileIdentity::of(&metadata) == identity) {
        (true, true) => {}
        (true | false, false) | (false, true) => {
            return Attempt::Left(LeftoverReason::SomethingElseThere);
        }
    }

    let absolute = path.to_path(root);
    let outcome = match kind {
        Kind::File => std::fs::remove_file(&absolute),
        Kind::Directory => std::fs::remove_dir(&absolute),
    };
    match outcome {
        Ok(()) => Attempt::Removed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Attempt::Gone,
        Err(error) => Attempt::Refused(error),
    }
}
