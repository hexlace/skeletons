//! Why a command that writes could not even ask git the questions it needs
//! answered before it writes anything.

use crate::claim::ClaimPath;

/// The question a git command was answering, so that a command killed for
/// running too long can say what it was doing: which path, and what to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitQuestion {
    /// Whether the directory is inside a git work tree at all.
    WorkTree,
    /// Whether the whole work tree is clean.
    Status,
    /// What git's index holds for one claimed path.
    IndexEntry(ClaimPath),
    /// What git's index tracks at one directory above a claim.
    IndexAbove(ClaimPath),
    /// Which paths git's index lists at the depth of the writes.
    IndexListing,
    /// What git would check out for one claimed path.
    Checkout(ClaimPath),
    /// Whether git's ignore rules would ignore one claimed path, and which
    /// rule does.
    Ignored(ClaimPath),
}

/// Why a writing command could not establish either rule (a) (the whole work tree is
/// clean) or rule (b) (positive proof per path) at all — as opposed to
/// either rule answering "no", which is a refusal the command reports in full
/// (dirty paths, or unproven ones), not an abort.
#[derive(Debug)]
pub(crate) enum WorkTreeAbort {
    /// One or more of the variables that redirect which repository, work
    /// tree, index, object store or attribute source git answers from is
    /// set in this process's own environment.
    RedirectedGit { variables: Vec<&'static str> },
    /// `git rev-parse --is-inside-work-tree` exited zero with an answer
    /// other than `true`, or exited non-zero saying the directory is not a
    /// git repository. Any other non-zero exit is
    /// [`WorkTreeAbort::DubiousOwnership`] or [`WorkTreeAbort::GitFailed`], and an
    /// answer too large to read is [`WorkTreeAbort::GitOutputTooLarge`].
    NotAWorkTree,
    /// git refused to read the repository at all because another user owns
    /// it (`safe.directory`).
    DubiousOwnership { diagnostic: String },
    /// `git` itself could not be run at all.
    GitUnavailable { detail: String },
    /// A git command was still running once [`crate::git::local_timeout`]
    /// had elapsed, and was killed. A content filter the repository configures,
    /// another process holding git's index, or a slow filesystem can each
    /// cause it.
    GitTimedOut { question: GitQuestion },
    /// A git command ran and exited non-zero, for a reason the command does not
    /// otherwise recognise.
    GitFailed {
        command: &'static str,
        diagnostic: String,
    },
    /// A git command's own stdout ran past the cap this crate reads a
    /// subprocess's output under.
    GitOutputTooLarge { command: &'static str },
}
