//! Listing the paths git's index holds at the depth of the writes and above,
//! so the ones a filesystem could take for a claim can be picked out
//! ([`super::fold_variants`]).
//!
//! Git's own case fold reaches ASCII case only, so a claim's Unicode variant
//! (a decomposed `é`, an `É`) cannot be asked of git by pathspec. It has to be
//! found by looking, and looking means reading the index's paths. They are
//! read with depth globs, `*`, `*/*`, `*/*/*` and so on, down to the deepest
//! write: a `glob` `*` does not cross a `/` and a pattern holding a wildcard
//! does not recurse (captured: git 2.53.0), so each pattern lists one level
//! and nothing deeper. Paths only, no mode or object id, is what keeps the
//! answer small: a repository's shallow paths are a few megabytes where the
//! full records for them would be several times that. The patterns carry no
//! text from a claim, so nothing a claim spells can change what they match.
//!
//! The listing is read whole or not at all, like every git answer here. One
//! past the cap is refused as [`SyncAbort::GitOutputTooLarge`] rather than
//! searched in part, since a variant in the part that was cut off is exactly
//! the one that would be missed.

use std::num::NonZeroUsize;

use crate::claim::ClaimPath;
use crate::git::{self, Locale};
use crate::subprocess::Truncated;

use super::super::abort::{GitQuestion, SyncAbort};
use super::super::work_tree::{WorkTree, run_local};

/// The paths of every index entry at depth 1 through the deepest of `writes`,
/// NUL-separated as `ls-files -z` prints them. Empty for no writes.
///
/// The patterns are one per depth, so their number is the deepest write's
/// depth: bounded by the claims, which are bounded by what a skeleton can
/// render.
pub(super) fn index_listing(
    work_tree: &WorkTree,
    writes: &[&ClaimPath],
) -> Result<Vec<u8>, SyncAbort> {
    let Some(deepest) = writes.iter().map(|claim| claim.depth()).max() else {
        return Ok(Vec::new());
    };
    assert!(deepest >= 1, "a claim path has at least one component");

    let mut ls_files = work_tree.git_for_pathspec_magic(Locale::Fixed);
    ls_files.args(["ls-files", "-z", "--"]);
    ls_files.args(
        (1..=deepest)
            .filter_map(NonZeroUsize::new)
            .map(git::pathspec_at_depth),
    );
    let finished = run_local(ls_files, GitQuestion::IndexListing)?;
    classify_listing(
        finished.success(),
        finished.stdout(),
        finished.stderr_head(),
    )
}

/// The pure classifier behind [`index_listing`], split out for the same
/// reason as `classify_ls_files`: a unit test hands it a truncated stream and
/// the same bytes uncut. The listing is stdout as written, never parsed here.
fn classify_listing(
    exit_ok: bool,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<Vec<u8>, SyncAbort> {
    if !exit_ok {
        return Err(SyncAbort::GitFailed {
            command: "ls-files",
            diagnostic: git::diagnostic(stderr),
        });
    }
    let stdout = stdout.map_err(|_truncated| SyncAbort::GitOutputTooLarge {
        command: "ls-files",
    })?;
    Ok(stdout.to_vec())
}

#[cfg(test)]
mod tests {
    use crate::subprocess::Truncated;
    use crate::sync::abort::SyncAbort;

    use super::classify_listing;

    #[test]
    fn a_truncated_listing_is_refused_as_too_large_never_searched_in_part() {
        // The control hands the same paths uncut, which must come back byte
        // for byte, so the refusal is the truncation and never the content.
        let refused = classify_listing(true, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("a truncated listing must be refused");
        assert!(matches!(
            refused,
            SyncAbort::GitOutputTooLarge {
                command: "ls-files"
            }
        ));

        let listing = classify_listing(true, Ok(b"a.yml\0d/b.yml\0"), b"")
            .expect("the same paths, uncut, are the listing");
        assert_eq!(listing, b"a.yml\0d/b.yml\0");
    }

    #[test]
    fn a_failed_listing_names_git_s_own_detail() {
        let refused = classify_listing(false, Ok(b"partial"), b"fatal: bad index file\n")
            .expect_err("a non-zero exit must be refused");
        let SyncAbort::GitFailed {
            command,
            diagnostic,
        } = refused
        else {
            panic!("expected GitFailed, got {refused:?}")
        };
        assert_eq!(command, "ls-files");
        assert_eq!(diagnostic, "bad index file");
    }
}
