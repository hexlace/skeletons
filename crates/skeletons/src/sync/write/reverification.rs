//! Asking again whether a write is still about the thing `sync` proved.
//!
//! [`reverify`] is the one function that answers "is this still the thing
//! `sync` proved, at a path still safe to write". It walks the path from the
//! workspace root with [`crate::claim::resolve`], the walk `check` and the
//! survey use, so every directory above the target is checked as a real
//! directory under its claimed spelling, with no link, at the same moment
//! the file's content is read. No second walk exists.

use std::path::Path;

use crate::claim::{ClaimPath, OnDisk, UnsafePathCause, resolve};
use crate::sync::proof::Evidence;

use super::TargetChange;

/// Confirms `path` under `root` is still what `evidence` proved it was.
///
/// - [`Evidence::Absent`]: nothing is at the path. Anything there now,
///   whatever it is, has [`TargetChange::Appeared`].
/// - [`Evidence::Held`]: a regular file is there, holding exactly the bytes
///   git would check out for it. Gone is [`TargetChange::Disappeared`], a
///   link or a directory in its place is [`TargetChange::NoLongerAFile`],
///   and other bytes are [`TargetChange::ContentChanged`].
///
/// A refusal from the walk itself, a directory above the target that became
/// a link or was respelled, is [`TargetChange::PathUnsafe`]: the answer is
/// about the path, not about the bytes at the end of it.
///
/// The file is read up to the proven length plus one byte, so a file that
/// grew is caught without ever reading more of it than deciding that needs.
///
/// # Errors
///
/// The [`TargetChange`] described above.
pub(super) fn reverify(
    root: &Path,
    path: &ClaimPath,
    evidence: &Evidence,
) -> Result<(), TargetChange> {
    let bound = match evidence {
        Evidence::Absent => 0,
        Evidence::Held(held) => held.checkout().len(),
    };
    match (evidence, resolve(root, path, bound)) {
        (Evidence::Absent, Ok(OnDisk::Missing)) => Ok(()),
        (Evidence::Absent, Ok(OnDisk::File(_))) => Err(TargetChange::Appeared),
        (Evidence::Absent, Err(UnsafePathCause::Symlink | UnsafePathCause::NotAFile)) => {
            Err(TargetChange::Appeared)
        }
        (Evidence::Held(_), Ok(OnDisk::Missing)) => Err(TargetChange::Disappeared),
        (Evidence::Held(held), Ok(OnDisk::File(bytes))) => {
            if bytes == held.checkout() {
                Ok(())
            } else {
                Err(TargetChange::ContentChanged)
            }
        }
        (Evidence::Held(_), Err(UnsafePathCause::Symlink | UnsafePathCause::NotAFile)) => {
            Err(TargetChange::NoLongerAFile)
        }
        // Every other refusal is about the path above or around the target,
        // whatever the evidence: the same cause `check` would report.
        (Evidence::Absent | Evidence::Held(_), Err(cause)) => Err(TargetChange::PathUnsafe(cause)),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{TargetChange, reverify};
    use crate::claim::{ClaimPath, UnsafePathCause};
    use crate::git::ObjectId;
    use crate::sync::proof::{Evidence, HeldFile, IndexMode};

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    fn held(checkout: &[u8]) -> Evidence {
        Evidence::Held(HeldFile::for_test(
            ObjectId::parse("0000000000000000000000000000000000000000")
                .expect("well-formed test object id"),
            IndexMode::Regular,
            checkout.to_vec(),
        ))
    }

    #[test]
    fn a_path_still_absent_is_confirmed() {
        let root = TempDir::new().expect("scratch directory");
        assert_eq!(
            reverify(root.path(), &claim_path("a/b.yml"), &Evidence::Absent),
            Ok(())
        );
    }

    #[test]
    fn an_absent_path_that_now_holds_a_file_has_appeared() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("b.yml"), b"x").expect("appeared file");
        assert_eq!(
            reverify(root.path(), &claim_path("b.yml"), &Evidence::Absent),
            Err(TargetChange::Appeared)
        );
    }

    #[test]
    fn an_absent_path_that_now_holds_a_link_or_a_directory_has_appeared() {
        let root = TempDir::new().expect("scratch directory");
        std::os::unix::fs::symlink(root.path().join("nowhere"), root.path().join("link.yml"))
            .expect("dangling link");
        std::fs::create_dir(root.path().join("directory.yml")).expect("directory");
        for name in ["link.yml", "directory.yml"] {
            assert_eq!(
                reverify(root.path(), &claim_path(name), &Evidence::Absent),
                Err(TargetChange::Appeared),
                "{name}"
            );
        }
    }

    #[test]
    fn a_held_file_with_the_proven_bytes_is_confirmed() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("b.yml"), b"proven").expect("file");
        assert_eq!(
            reverify(root.path(), &claim_path("b.yml"), &held(b"proven")),
            Ok(())
        );
    }

    #[test]
    fn a_held_file_with_other_bytes_has_changed_content() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("b.yml"), b"precious").expect("file");
        assert_eq!(
            reverify(root.path(), &claim_path("b.yml"), &held(b"proven")),
            Err(TargetChange::ContentChanged)
        );
    }

    #[test]
    fn a_held_file_that_grew_has_changed_content() {
        // The proven bytes are a prefix of what is there now: the read
        // bound of the proven length plus one is what makes that visible.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("b.yml"), b"proven and then some").expect("file");
        assert_eq!(
            reverify(root.path(), &claim_path("b.yml"), &held(b"proven")),
            Err(TargetChange::ContentChanged)
        );
    }

    #[test]
    fn a_held_file_that_is_gone_has_disappeared() {
        let root = TempDir::new().expect("scratch directory");
        assert_eq!(
            reverify(root.path(), &claim_path("b.yml"), &held(b"proven")),
            Err(TargetChange::Disappeared)
        );
    }

    #[test]
    fn a_held_file_replaced_by_a_link_or_a_directory_is_no_longer_a_file() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("real.yml"), b"proven").expect("link target");
        std::os::unix::fs::symlink(root.path().join("real.yml"), root.path().join("link.yml"))
            .expect("link");
        std::fs::create_dir(root.path().join("directory.yml")).expect("directory");
        for name in ["link.yml", "directory.yml"] {
            assert_eq!(
                reverify(root.path(), &claim_path(name), &held(b"proven")),
                Err(TargetChange::NoLongerAFile),
                "{name}"
            );
        }
    }

    #[test]
    fn a_directory_above_the_target_that_became_a_link_is_a_path_refusal_whatever_the_bytes() {
        // The link leads to a directory whose `one.yml` holds exactly the
        // proven bytes, so only the walk, never the content, can refuse it.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("outside directory");
        std::fs::write(outside.path().join("one.yml"), b"proven").expect("outside file");
        std::os::unix::fs::symlink(outside.path(), root.path().join("x")).expect("link");

        assert_eq!(
            reverify(root.path(), &claim_path("x/one.yml"), &held(b"proven")),
            Err(TargetChange::PathUnsafe(
                UnsafePathCause::SymbolicLinkAbove { at: "x".to_owned() }
            ))
        );
        assert!(matches!(
            reverify(root.path(), &claim_path("x/one.yml"), &Evidence::Absent),
            Err(TargetChange::PathUnsafe(
                UnsafePathCause::SymbolicLinkAbove { .. }
            ))
        ));
    }

    #[test]
    fn a_file_where_a_directory_above_the_target_belongs_is_a_path_refusal() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("a"), b"a file").expect("file");
        assert_eq!(
            reverify(root.path(), &claim_path("a/b.yml"), &Evidence::Absent),
            Err(TargetChange::PathUnsafe(
                UnsafePathCause::NotADirectoryAbove { at: "a".to_owned() }
            ))
        );
    }
}
