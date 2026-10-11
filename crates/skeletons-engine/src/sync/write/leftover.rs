//! Something `sync` created and did not remove, and why it is still there.

use crate::claim::UnsafePathCause;

/// A path `sync` created and left where it is, with the reason. `sync`
/// names each one to the person running it, since each is theirs to find and
/// remove by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Leftover {
    /// Workspace-relative and `/`-separated.
    pub(crate) path: String,
    pub(crate) reason: LeftoverReason,
}

/// Why `sync` left something it created in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LeftoverReason {
    /// The removal itself failed.
    CouldNotRemove { detail: String },
    /// The walk from the workspace root to the path refused it — a link, a
    /// file or a respelled directory above it — so `sync` did not touch it,
    /// since removing it would have been done through that link or under
    /// that other name.
    PathUnsafe(UnsafePathCause),
    /// The entry at the path is not the file or directory `sync` created:
    /// its device, inode or kind differs.
    SomethingElseThere,
    /// Nothing is at the path now: a staging file `sync` prepared has moved
    /// or been removed by something else, so its bytes are somewhere `sync`
    /// cannot say.
    Gone,
}
