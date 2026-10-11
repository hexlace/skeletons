//! Why a write could not be prepared, committed or confirmed: the failure and
//! outcome types [`super::plan`], [`super::prepare`],
//! [`super::Prepared::commit`] and [`super::verify`] return, and `sync`'s own
//! report reads.
//!
//! Every variant here is something the environment did (a file another
//! process touched, a path swapped for a link, a disk that filled) or
//! something the claims themselves say (two claims whose names a
//! filesystem cannot tell from a staging name). None is a defect in this
//! crate, so none panics: a defect in this crate panics, as the assertions
//! elsewhere in `write` do, and an environment or claim failure is one of
//! these.

use crate::claim::{ClaimPath, UnsafePathCause};

use crate::sync::fold_variant::FoldVariant;

use super::{Leftover, StagingRelation};

/// Why one path `sync` needed to create or write already held something
/// else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollisionAt {
    StagingFile,
    Directory,
}

/// How a claimed target no longer matches what `sync` proved about it,
/// found by [`super::reverify`] at any of the three sites it runs: staging
/// the write, the start of the commit, and immediately before the write's
/// own rename or link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetChange {
    /// A write that proved nothing was there now has something at its path.
    Appeared,
    /// A file `sync` proved it held is gone.
    Disappeared,
    /// A file `sync` proved it held is no longer a regular file: a symbolic
    /// link or a directory stands there now.
    NoLongerAFile,
    /// A file `sync` proved it held is still a regular file, but no longer
    /// holds the bytes git would check out for it.
    ContentChanged,
    /// The walk from the workspace root to the target, the one `check`
    /// makes, now refuses the path: a directory above it became a link, or
    /// was respelled, or cannot be read.
    PathUnsafe(UnsafePathCause),
}

/// Why a write could not be prepared, committed or confirmed.
#[derive(Debug)]
pub(crate) enum WriteFailure {
    /// A drifted write's own staging path is, to a filesystem that ignores
    /// case or Unicode normalization, the same name as, a directory above,
    /// or beneath a path some row claims: `relation` says which, and
    /// `claimed` is that path. A defect in how two of the worn skeletons
    /// name their own files, not something staging anything could ever
    /// recover from.
    StagingClaimed {
        claim: ClaimPath,
        staging: String,
        claimed: ClaimPath,
        relation: StagingRelation,
    },
    /// Something already sat at a path `sync` needed to create exclusively
    /// — a staging file or a directory. Never overwritten, never removed.
    Collision {
        claim: ClaimPath,
        shown: String,
        what: CollisionAt,
        leftovers: Vec<Leftover>,
    },
    /// The claimed target, or the path to it, no longer matched what `sync`
    /// proved about it while the write was being staged. Nothing had been
    /// renamed yet, so nothing was written.
    TargetChanged {
        claim: ClaimPath,
        what: TargetChange,
        leftovers: Vec<Leftover>,
    },
    /// While staging, the filesystem took an index entry git tracks for the
    /// staging file `sync` had just created: git holds `variant` hidden from
    /// the work tree under another spelling of `claim` (or of a directory
    /// above it), and would take what `sync` wrote for it. Nothing was
    /// written.
    FoldsOntoTracked {
        claim: ClaimPath,
        variant: FoldVariant,
        leftovers: Vec<Leftover>,
    },
    /// The operating system refused permission to create something in
    /// `directory` (`None` for the workspace root) while `path` was being
    /// staged: this user cannot write there. Nothing was written.
    DirectoryNotWritable {
        path: ClaimPath,
        directory: Option<ClaimPath>,
        detail: String,
        leftovers: Vec<Leftover>,
    },
    /// Staging a file failed for a reason other than a collision, a
    /// changed target or a directory that is not writable — an I/O error
    /// creating a directory or the staging file, reading the target's
    /// permissions, setting them on the staging file, writing, syncing, or
    /// asking the filesystem whether a spelling git tracks is the same name
    /// as the claim.
    Prepare {
        path: ClaimPath,
        detail: String,
        leftovers: Vec<Leftover>,
    },
    /// Committing a write failed: [`super::reverify`] refused it, or the rename
    /// or link itself did. Boxed: this is the one variant carrying three lists,
    /// a cause and a count at once, and boxing only it keeps every other
    /// `WriteFailure` — and therefore every `Result<_, WriteFailure>` this
    /// module returns — cheap to move.
    Commit(Box<CommitFailure>),
    /// Every write was committed, but reading one back afterwards found
    /// something other than what `sync` wrote: another process changed it
    /// in the instant between the commit and [`super::verify`]. The write
    /// itself completed; the paths are the ones that no longer read back as
    /// written.
    ChangedAfterWrite { paths: Vec<ClaimPath> },
}

/// The full detail of a [`WriteFailure::Commit`]. The paths in
/// `already_written` are exactly that — on disk, holding their own render —
/// and git's own history still holds whatever each one replaced.
/// `not_yet_written` is every write queued after the failed one that
/// `commit` never even attempted; the failed path itself is `failed_path`,
/// not repeated inside either list. When `already_written` is empty, nothing
/// was written at all.
#[derive(Debug)]
pub(crate) struct CommitFailure {
    pub(crate) failed_path: ClaimPath,
    pub(crate) already_written: Vec<ClaimPath>,
    pub(crate) not_yet_written: Vec<ClaimPath>,
    pub(crate) total: usize,
    pub(crate) cause: CommitCause,
    pub(crate) leftovers: Vec<Leftover>,
}

/// Why committing one write failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommitCause {
    /// `rename` over an existing file failed.
    Replace { detail: String },
    /// `hard_link` into an empty path failed for a reason other than
    /// `AlreadyExists`. `sync` creates a missing file by hard-linking it into
    /// place, and the error number is not read: `detail` is the operating
    /// system's own error, the whole account of why. Every link lands before
    /// any rename ([`super::landing`]), so a refusal that every file would
    /// meet is found before anything is written.
    Create { detail: String },
    /// [`super::reverify`] refused the write, or `hard_link` found the path
    /// already taken, which is the same refusal made by the filesystem.
    Changed(TargetChange),
}
