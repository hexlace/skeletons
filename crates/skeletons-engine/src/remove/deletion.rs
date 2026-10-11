//! Deleting the files a skeleton claims, last, once the manifest and the
//! lockfile are written and read back.
//!
//! The files are deleted for real, one after another in path order, and the
//! directories the deletes left empty are removed after them. A deleted file
//! cannot be given back by the rollback, which restores only what it wrote, so
//! git is what holds them: the preparation has already shown that git can give
//! back each file that is deleted. A delete that fails stops the deleting, and
//! the failure carries which files were deleted and which were not, so that the
//! line the wearer reads names both. A directory that cannot be removed does
//! not fail the deletion: every file is already gone, an empty directory is
//! nothing git sees, and putting the manifest back would leave the project
//! wearing a skeleton whose files are all deleted. It is handed back as left.

use std::collections::BTreeSet;
use std::path::Path;

use super::refusal::RemoveRefusal;
use crate::claim::ClaimPath;

/// Which of the files to delete a failed deletion had deleted, and which it had
/// not, both in path order.
///
/// A file that failed to delete is among those not deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deletion {
    pub(crate) deleted: Vec<String>,
    pub(crate) undeleted: Vec<String>,
}

/// Why a deletion stopped, and what it had done by then. It is returned boxed,
/// so that the `Err` a function returns stays small to move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeletionFailed {
    pub(crate) refusal: RemoveRefusal,
    pub(crate) deletion: Deletion,
}

/// A directory the deletes emptied and that could not be removed, with the
/// operating system's words for why.
///
/// Only a refusal for a reason other than the directory holding something is
/// one: a directory that still holds something is the project's, and stays
/// without a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryLeft {
    pub(crate) directory: String,
    pub(crate) detail: String,
}

/// Deletes the file at `path` from the disk, which is what the command passes
/// to [`delete`] as its `delete_file`.
///
/// It is [`std::fs::remove_file`] with the argument spelled as a `&Path`, which
/// is the one shape [`delete`] takes: the function itself is generic over what
/// names the path, and a generic function cannot stand for a closure that takes
/// a borrow of any lifetime.
///
/// # Errors
///
/// Returns the operating system's error when the file cannot be deleted.
pub(crate) fn delete_from_disk(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// Deletes every file in `files` under `root` with `delete_file`, then removes
/// the directories the deletes left empty, and returns the ones it could not.
///
/// The files go in the order given, and the first delete that fails stops the
/// deleting: nothing after it is tried, and no directory is removed. Then each
/// directory above a deleted file is removed with [`std::fs::remove_dir`],
/// deepest first. A directory that still holds something stays, and so does
/// every directory above it; the workspace root is never reached. A directory
/// that cannot be removed for any other reason is returned in path order and
/// stays, and so does every directory above it, which holds it; the removal of
/// the rest goes on. With no files, nothing is done.
///
/// `root` is absolute and every file in `files` is at its claimed path under
/// it, which the survey has already shown. `delete_file` is
/// [`delete_from_disk`] in the command, and a test passes one that fails.
///
/// # Errors
///
/// Returns [`RemoveRefusal::DeleteFailed`] for the first file that cannot be
/// deleted, with the files before it as deleted and it and the files after it
/// as not. The files stay deleted: the caller's
/// [`rollback::attempt`](rituals_compose::rollback::attempt) puts back only
/// what it recorded.
pub(crate) fn delete(
    root: &Path,
    files: &[&ClaimPath],
    mut delete_file: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<Vec<DirectoryLeft>, Box<DeletionFailed>> {
    assert!(root.is_absolute(), "the workspace root is an absolute path");
    for (position, file) in files.iter().enumerate() {
        if let Err(error) = delete_file(&file.to_path(root)) {
            let (deleted, undeleted) = files.split_at(position);
            return Err(Box::new(DeletionFailed {
                refusal: RemoveRefusal::DeleteFailed {
                    path: file.as_str().to_owned(),
                    detail: error.to_string(),
                },
                deletion: Deletion {
                    deleted: names(deleted),
                    undeleted: names(undeleted),
                },
            }));
        }
    }
    Ok(remove_emptied_directories(root, files))
}

/// The claimed paths of `files`, as the messages show them.
fn names(files: &[&ClaimPath]) -> Vec<String> {
    files.iter().map(|file| file.as_str().to_owned()).collect()
}

/// Removes, deepest first, each directory above a file in `files` that the
/// deletes left empty, and returns those it could not remove, in path order.
fn remove_emptied_directories(root: &Path, files: &[&ClaimPath]) -> Vec<DirectoryLeft> {
    let mut directories: Vec<ClaimPath> = files
        .iter()
        .flat_map(|file| file.ancestors())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    directories.sort_by(|left, right| {
        right
            .depth()
            .cmp(&left.depth())
            .then_with(|| left.cmp(right))
    });

    let mut staying: Vec<ClaimPath> = Vec::new();
    let mut left: Vec<DirectoryLeft> = Vec::new();
    for directory in directories {
        // A directory above one that stays holds it, so asking would only be
        // told as much, and only the directory the removal was refused for is
        // reported.
        if staying
            .iter()
            .any(|stays| Path::new(stays.as_str()).starts_with(directory.as_str()))
        {
            continue;
        }
        match std::fs::remove_dir(directory.to_path(root)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                staying.push(directory);
            }
            Err(error) => {
                left.push(DirectoryLeft {
                    directory: directory.as_str().to_owned(),
                    detail: error.to_string(),
                });
                staying.push(directory);
            }
        }
    }
    left.sort_by(|first, second| first.directory.cmp(&second.directory));
    left
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Deletion, DeletionFailed, DirectoryLeft, delete, delete_from_disk};
    use crate::claim::ClaimPath;
    use crate::remove::refusal::RemoveRefusal;

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// Writes `bytes` at `relative` under `root`, creating the directories
    /// above it.
    fn fill(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a directory"))
            .expect("create the directories");
        std::fs::write(&path, bytes).expect("write the file");
    }

    /// Runs [`delete`] over `files` under `root` with `delete_file`.
    fn deleting(
        root: &Path,
        files: &[&str],
        delete_file: impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<Vec<DirectoryLeft>, Box<DeletionFailed>> {
        let claims: Vec<ClaimPath> = files.iter().map(|file| claim_path(file)).collect();
        let references: Vec<&ClaimPath> = claims.iter().collect();
        delete(root, &references, delete_file)
    }

    /// The error a delete that fails returns, the same on every run.
    fn denied() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied for the test")
    }

    /// A deleter that really deletes every file it is asked to except the
    /// `failing`th, counted from one, for which it returns [`denied`]. It
    /// counts calls rather than reading a clock, and it goes on deleting after
    /// the failure, so a deletion that did not stop would show.
    fn deleting_except_call(failing: usize) -> impl FnMut(&Path) -> std::io::Result<()> {
        let mut asked = 0_usize;
        move |path| {
            asked += 1;
            if asked == failing {
                Err(denied())
            } else {
                delete_from_disk(path)
            }
        }
    }

    fn names_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("read the directory")
            .map(|entry| {
                entry
                    .expect("read an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn every_file_is_deleted_and_emptied_directories_are_gone() {
        // Four files, one at the root, two sharing a directory that holds
        // nothing else, and one in a nested directory beside a file that
        // stays. After the deletes the files are gone, the directories they
        // emptied are gone all the way up, the one that still holds something
        // is there, so is the root, and the root holds nothing else.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "top.yml", b"top\n");
        fill(root, ".github/workflows/ci.yml", b"ci\n");
        fill(root, ".github/workflows/lint.yml", b"lint\n");
        fill(root, "docs/a/b.md", b"b\n");
        fill(root, "docs/keep.txt", b"kept\n");

        deleting(
            root,
            &[
                ".github/workflows/ci.yml",
                ".github/workflows/lint.yml",
                "docs/a/b.md",
                "top.yml",
            ],
            delete_from_disk,
        )
        .expect("every delete succeeds");

        for gone in [
            "top.yml",
            ".github/workflows/ci.yml",
            ".github/workflows/lint.yml",
            ".github/workflows",
            ".github",
            "docs/a/b.md",
            "docs/a",
        ] {
            assert!(
                root.join(gone).symlink_metadata().is_err(),
                "{gone} should be gone"
            );
        }
        assert_eq!(
            std::fs::read(root.join("docs/keep.txt")).expect("the file that stays"),
            b"kept\n"
        );
        assert!(root.is_dir(), "the workspace root is never removed");
        assert_eq!(names_in(root), ["docs"]);
        assert_eq!(names_in(&root.join("docs")), ["keep.txt"]);
    }

    #[test]
    fn the_files_are_deleted_in_the_order_given_by_their_paths_under_the_root() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "b.yml", b"b\n");
        fill(root, "a/c.yml", b"c\n");
        let mut asked = Vec::new();

        deleting(root, &["a/c.yml", "b.yml"], |path| {
            asked.push(path.to_path_buf());
            delete_from_disk(path)
        })
        .expect("every delete succeeds");

        assert_eq!(asked, [root.join("a/c.yml"), root.join("b.yml")]);
    }

    #[test]
    fn a_directory_that_still_holds_something_stays_and_so_does_everything_above_it() {
        // `a/b` is emptied and goes; `a` holds `a/other` and stays. `x/y/z`
        // empties a chain that goes right up to the root, and the root, which
        // is left holding only `a`, is not touched.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a/b/c.txt", b"c\n");
        fill(root, "a/other", b"other\n");
        fill(root, "x/y/z.txt", b"z\n");

        deleting(root, &["a/b/c.txt", "x/y/z.txt"], delete_from_disk)
            .expect("every delete succeeds");

        assert!(!root.join("a/b").exists());
        assert!(root.join("a/other").is_file());
        assert!(!root.join("x").exists());
        assert_eq!(names_in(root), ["a"]);
    }

    #[test]
    fn no_files_deletes_nothing_and_asks_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "theirs.txt", b"theirs\n");

        deleting(root, &[], |path| {
            panic!("no file is to be deleted, yet {} was asked", path.display())
        })
        .expect("nothing to delete is a success");

        assert_eq!(names_in(root), ["theirs.txt"]);
    }

    #[test]
    fn a_delete_failing_after_an_earlier_one_reports_which_were_deleted_and_which_were_not() {
        // Three files, each in a directory of its own. The second delete is
        // refused and every other one really happens, so the third file is
        // still there only if the deleting stopped at the second. The
        // directory the first file emptied is still there, because no
        // directory is removed once a file delete has failed.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a/one.yml", b"one\n");
        fill(root, "b/two.yml", b"two\n");
        fill(root, "c/three.yml", b"three\n");

        let failed = deleting(
            root,
            &["a/one.yml", "b/two.yml", "c/three.yml"],
            deleting_except_call(2),
        )
        .expect_err("the second delete is refused");

        assert_eq!(
            failed.refusal,
            RemoveRefusal::DeleteFailed {
                path: "b/two.yml".to_owned(),
                detail: "denied for the test".to_owned(),
            }
        );
        assert_eq!(
            failed.deletion,
            Deletion {
                deleted: strings(&["a/one.yml"]),
                undeleted: strings(&["b/two.yml", "c/three.yml"]),
            }
        );
        assert!(!root.join("a/one.yml").exists(), "the first is deleted");
        assert!(root.join("a").is_dir(), "no directory is removed");
        assert_eq!(names_in(&root.join("a")), Vec::<String>::new());
        assert!(root.join("b/two.yml").is_file());
        assert!(root.join("c/three.yml").is_file());
        assert_eq!(names_in(root), ["a", "b", "c"]);
    }

    #[test]
    fn the_first_delete_failing_reports_none_deleted() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a.yml", b"a\n");
        fill(root, "b.yml", b"b\n");

        let failed = deleting(root, &["a.yml", "b.yml"], deleting_except_call(1))
            .expect_err("the first delete is refused");

        assert_eq!(
            failed.deletion,
            Deletion {
                deleted: Vec::new(),
                undeleted: strings(&["a.yml", "b.yml"]),
            }
        );
        assert_eq!(names_in(root), ["a.yml", "b.yml"]);
    }

    #[test]
    fn a_directory_that_cannot_be_removed_for_another_reason_is_left_and_the_run_succeeds() {
        // `link` is a link to a directory, so removing it as a directory is
        // refused for a reason that is not that it holds something, and
        // whoever runs the test is refused alike. The file was deleted before
        // the removal was tried. The deletion still succeeds, and hands back
        // the directory it could not remove with the system's words.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "real/f.txt", b"f\n");
        std::os::unix::fs::symlink("real", root.join("link")).expect("create the link");

        let left = deleting(root, &["link/f.txt"], delete_from_disk)
            .expect("a directory that cannot be removed does not fail the deletion");

        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].directory, "link");
        assert!(!left[0].detail.is_empty());
        assert!(!root.join("real/f.txt").exists());
        assert_eq!(names_in(root), ["link", "real"]);
    }

    #[test]
    fn an_unrelated_emptied_directory_is_still_removed_after_another_was_refused() {
        // `outer/link` is a link to a directory, so removing it is refused,
        // while `other/x` and `other` are emptied by the delete of `other/x/g`
        // and are removed. Only the directory the removal was refused for is
        // reported: `outer`, which holds the link, stays and says nothing.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "real/f.txt", b"f\n");
        std::fs::create_dir(root.join("outer")).expect("create outer");
        std::os::unix::fs::symlink("../real", root.join("outer/link")).expect("create the link");
        fill(root, "other/x/g.txt", b"g\n");

        let left = deleting(
            root,
            &["outer/link/f.txt", "other/x/g.txt"],
            delete_from_disk,
        )
        .expect("a directory that cannot be removed does not fail the deletion");

        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].directory, "outer/link");
        assert!(!root.join("other").exists(), "the unrelated chain goes");
        assert_eq!(names_in(&root.join("outer")), ["link"]);
        assert_eq!(names_in(root), ["outer", "real"]);
    }

    #[test]
    fn a_run_where_every_directory_goes_leaves_none() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a/b/c.txt", b"c\n");

        let left = deleting(root, &["a/b/c.txt"], delete_from_disk).expect("deleted");

        assert_eq!(left, Vec::<DirectoryLeft>::new());
    }
}
