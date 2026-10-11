//! Deleting files so that a failure can give every one of them back, mode and
//! all.
//!
//! The rollback records a file it writes, and restores it from its bytes, so a
//! file it restored would lose its mode, and it records no deletion at all. A
//! file it *moves* comes back whole. So the files are not deleted inside the
//! run but moved into a directory the run reserves in the workspace root, and
//! the directories the moves emptied are removed; if the run then fails, the
//! undo puts the directories back and moves every file home. Only when the run
//! has succeeded is the holding directory deleted, and then nothing is left to
//! undo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rituals_compose::rollback::Changes;

use super::HOLDING_DIRECTORY;
use super::refusal::RemoveRefusal;
use crate::claim::ClaimPath;

/// Whether nothing is at the holding path in the workspace root, so that it can
/// be reserved.
///
/// Anything but a plain absence counts as taken: a path that cannot be
/// examined cannot be reserved either.
pub(crate) fn holding_path_is_free(root: &Path) -> bool {
    matches!(
        std::fs::symlink_metadata(root.join(HOLDING_DIRECTORY)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

/// What [`stage`] did, which is the only way to [`purge`]: the holding
/// directory is deleted only if the run reserved it, so a directory that was
/// already there under that name, when no file was to be deleted and so none
/// was reserved, is never touched.
#[derive(Debug)]
#[must_use = "the holding directory is deleted by purging it"]
pub(crate) struct Staged {
    holding: Option<PathBuf>,
}

/// Moves every file in `files` out of the work tree, into the holding directory
/// under `root`, and removes the directories the moves left empty.
///
/// The directory is reserved through `changes` and each file is moved into it
/// with [`Changes::rename`], under its position in `files` as its name, so no
/// two collide whatever they are called. Then each directory above a moved
/// file is removed with [`Changes::remove_empty_directory`], deepest first. A
/// directory that still holds something stays, and so does every directory
/// above it; the workspace root is never reached. With no files, nothing is
/// reserved and nothing is moved.
///
/// `root` is absolute and every file in `files` is at its claimed path under
/// it, which the survey has already shown.
///
/// # Errors
///
/// Returns [`RemoveRefusal::HoldingNotCreated`] when the holding directory
/// cannot be reserved, [`RemoveRefusal::MoveFailed`] for the first file that
/// cannot be moved, and [`RemoveRefusal::DirectoryNotRemoved`] for the first
/// emptied directory that cannot be removed for a reason other than holding
/// something. The undo of every step already taken is the caller's
/// [`rollback::attempt`](rituals_compose::rollback::attempt).
pub(crate) fn stage(
    changes: &mut Changes,
    root: &Path,
    files: &[&ClaimPath],
) -> Result<Staged, RemoveRefusal> {
    assert!(root.is_absolute(), "the workspace root is an absolute path");
    if files.is_empty() {
        return Ok(Staged { holding: None });
    }
    let holding = root.join(HOLDING_DIRECTORY);
    changes
        .reserve_directory(&holding)
        .map_err(|failure| RemoveRefusal::HoldingNotCreated {
            detail: failure.to_string(),
        })?;
    for (position, file) in files.iter().enumerate() {
        changes
            .rename(&file.to_path(root), &holding.join(position.to_string()))
            .map_err(|failure| RemoveRefusal::MoveFailed {
                path: file.as_str().to_owned(),
                detail: failure.to_string(),
            })?;
    }
    remove_emptied_directories(changes, root, files)?;
    Ok(Staged {
        holding: Some(holding),
    })
}

/// Removes, deepest first, each directory above a file in `files` that the
/// moves left empty.
fn remove_emptied_directories(
    changes: &mut Changes,
    root: &Path,
    files: &[&ClaimPath],
) -> Result<(), RemoveRefusal> {
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
    for directory in directories {
        // A directory above one that stays holds it, so asking would only be
        // told as much.
        if staying
            .iter()
            .any(|stays| Path::new(stays.as_str()).starts_with(directory.as_str()))
        {
            continue;
        }
        match changes.remove_empty_directory(&directory.to_path(root)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                staying.push(directory);
            }
            Err(error) => {
                return Err(RemoveRefusal::DirectoryNotRemoved {
                    directory: directory.as_str().to_owned(),
                    detail: error.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Deletes the holding directory with everything in it, once the run that
/// reserved it has succeeded and the files it holds are no longer to be given
/// back.
///
/// `key` is the wearing that was stopped, which the failure names.
///
/// # Errors
///
/// Returns [`RemoveRefusal::HoldingNotDeleted`] when it could not be deleted.
pub(crate) fn purge(staged: Staged, key: &str) -> Result<(), RemoveRefusal> {
    staged.holding.map_or(Ok(()), |holding| {
        std::fs::remove_dir_all(&holding).map_err(|error| RemoveRefusal::HoldingNotDeleted {
            key: key.to_owned(),
            detail: error.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use rituals::Failure;
    use rituals_compose::rollback::{self, Changes, Wording};

    use super::{HOLDING_DIRECTORY, Staged, holding_path_is_free, purge, stage};
    use crate::claim::ClaimPath;
    use crate::remove::refusal::RemoveRefusal;

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// Writes `bytes` at `relative` under `root`, creating the directories
    /// above it, and gives the file `mode`.
    fn fill(root: &Path, relative: &str, bytes: &[u8], mode: u32) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a directory"))
            .expect("create the directories");
        std::fs::write(&path, bytes).expect("write the file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("set the file's mode");
    }

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path)
            .expect("the file is there")
            .permissions()
            .mode()
            & 0o7777
    }

    fn wording(root: &Path) -> Wording<'_> {
        Wording::project(root, "running the `remove` task again")
    }

    /// Runs `stage` over `files` under `root` inside an attempt that then
    /// carries on with `after`, and hands back whatever the attempt returned.
    fn staged_then(
        root: &Path,
        files: &[&str],
        after: impl FnOnce(&mut Changes) -> Result<(), Failure>,
    ) -> Result<Staged, Failure> {
        let claims: Vec<ClaimPath> = files.iter().map(|file| claim_path(file)).collect();
        let references: Vec<&ClaimPath> = claims.iter().collect();
        rollback::attempt(wording(root), |changes| {
            let staged = stage(changes, root, &references).map_err(RemoveRefusal::into_failure)?;
            after(changes)?;
            Ok(staged)
        })
    }

    /// [`staged_then`] with nothing after it, so the run succeeds.
    fn staged(root: &Path, files: &[&str]) -> Result<Staged, Failure> {
        staged_then(root, files, |_changes| Ok(()))
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

    #[test]
    fn every_file_is_moved_into_the_holding_directory_and_emptied_directories_are_gone() {
        // Four files, one at the root, two sharing a directory that holds
        // nothing else, and one in a nested directory beside a file that
        // stays. After the run the files are out of the work tree with their
        // bytes in the holding directory, the directories the moves emptied
        // are gone all the way up, the one that still holds something is
        // there, and so is the root.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "top.yml", b"top\n", 0o644);
        fill(root, ".github/workflows/ci.yml", b"ci\n", 0o644);
        fill(root, ".github/workflows/lint.yml", b"lint\n", 0o644);
        fill(root, "docs/a/b.md", b"b\n", 0o644);
        fill(root, "docs/keep.txt", b"kept\n", 0o644);

        let staged = staged(
            root,
            &[
                ".github/workflows/ci.yml",
                ".github/workflows/lint.yml",
                "docs/a/b.md",
                "top.yml",
            ],
        )
        .expect("the run succeeds");

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
        let holding = root.join(HOLDING_DIRECTORY);
        assert_eq!(names_in(&holding), ["0", "1", "2", "3"]);
        for (name, bytes) in [
            ("0", &b"ci\n"[..]),
            ("1", b"lint\n"),
            ("2", b"b\n"),
            ("3", b"top\n"),
        ] {
            assert_eq!(std::fs::read(holding.join(name)).expect(name), bytes);
        }

        purge(staged, "ci").expect("the holding directory is deleted");

        assert!(holding.symlink_metadata().is_err());
        assert_eq!(names_in(root), ["docs"]);
    }

    #[test]
    fn a_directory_that_still_holds_something_stays_and_so_does_everything_above_it() {
        // `a/b` is emptied and goes; `a` holds `a/other` and stays. `x/y/z`
        // empties a chain that goes right up to the root, and the root, which
        // now holds only the holding directory, is not touched.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a/b/c.txt", b"c\n", 0o644);
        fill(root, "a/other", b"other\n", 0o644);
        fill(root, "x/y/z.txt", b"z\n", 0o644);

        let _staged = staged(root, &["a/b/c.txt", "x/y/z.txt"]).expect("the run succeeds");

        assert!(!root.join("a/b").exists());
        assert!(root.join("a/other").is_file());
        assert!(!root.join("x").exists());
        assert_eq!(names_in(root), [".skeletons-remove", "a"]);
    }

    #[test]
    fn two_files_of_one_name_in_different_directories_do_not_collide() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a/ci.yml", b"from a\n", 0o644);
        fill(root, "b/ci.yml", b"from b\n", 0o644);

        let _staged = staged(root, &["a/ci.yml", "b/ci.yml"]).expect("the run succeeds");

        let holding = root.join(HOLDING_DIRECTORY);
        assert_eq!(std::fs::read(holding.join("0")).expect("0"), b"from a\n");
        assert_eq!(std::fs::read(holding.join("1")).expect("1"), b"from b\n");
    }

    #[test]
    fn a_second_file_that_is_not_there_fails_the_run_and_the_undo_restores_the_first() {
        // The weak ground of moving files rather than deleting them: the
        // first file is executable and has already been moved when the second
        // move fails. The undo must put it back with the bytes and the mode
        // it had, leave its directory, and leave no holding directory behind.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "tools/first.sh", b"#!/bin/sh\necho first\n", 0o755);
        std::fs::create_dir_all(root.join("tools")).expect("the directory the second would be in");

        let failure = staged(root, &["tools/first.sh", "tools/second.sh"])
            .expect_err("the second file is not there to move");

        let message = failure.to_string();
        assert!(
            message.starts_with("remove could not move tools/second.sh out of the work tree: "),
            "{message}"
        );
        assert!(
            message.ends_with("; ritual put the project back as it found it"),
            "{message}"
        );
        let first = root.join("tools/first.sh");
        assert_eq!(
            std::fs::read(&first).expect("the first file is back"),
            b"#!/bin/sh\necho first\n"
        );
        assert_eq!(mode_of(&first), 0o755, "the executable bit is back");
        assert_eq!(names_in(root), ["tools"]);
        assert_eq!(names_in(&root.join("tools")), ["first.sh"]);
    }

    #[test]
    fn a_later_failure_puts_back_every_file_its_mode_and_every_emptied_directory() {
        // The moves and the directory removals all succeed and the run fails
        // afterwards, as it does when the read back finds the wearing still
        // there. Every file is back with its bytes and its own mode, the
        // directories the moves emptied exist again, and the holding
        // directory is gone.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, ".github/workflows/ci.yml", b"ci\n", 0o644);
        fill(root, ".github/workflows/run.sh", b"run\n", 0o755);
        fill(root, "docs/a/b.md", b"b\n", 0o444);
        fill(root, "top.yml", b"top\n", 0o600);
        std::fs::set_permissions(root.join(".github"), std::fs::Permissions::from_mode(0o750))
            .expect("set a directory's mode");

        let failure = staged_then(
            root,
            &[
                ".github/workflows/ci.yml",
                ".github/workflows/run.sh",
                "docs/a/b.md",
                "top.yml",
            ],
            |_changes| Err(Failure::new("the project no longer loads")),
        )
        .expect_err("the run fails after the moves");

        assert_eq!(
            failure.to_string(),
            "the project no longer loads; ritual put the project back as it found it"
        );
        for (path, bytes, mode) in [
            (".github/workflows/ci.yml", &b"ci\n"[..], 0o644),
            (".github/workflows/run.sh", b"run\n", 0o755),
            ("docs/a/b.md", b"b\n", 0o444),
            ("top.yml", b"top\n", 0o600),
        ] {
            let file = root.join(path);
            assert_eq!(std::fs::read(&file).expect(path), bytes, "{path}");
            assert_eq!(mode_of(&file), mode, "{path}");
        }
        assert_eq!(mode_of(&root.join(".github")), 0o750);
        assert_eq!(names_in(root), [".github", "docs", "top.yml"]);
    }

    #[test]
    fn no_files_reserves_nothing_and_purging_it_touches_nothing() {
        // With nothing to delete the holding path is never reserved, so a
        // directory the wearer has under that name is not the run's, and
        // purging what the run did must not delete it.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, ".skeletons-remove/theirs.txt", b"theirs\n", 0o644);

        let staged = staged(root, &[]).expect("the run succeeds");
        purge(staged, "ci").expect("nothing to purge");

        assert_eq!(
            std::fs::read(root.join(".skeletons-remove/theirs.txt")).expect("still there"),
            b"theirs\n"
        );
    }

    #[test]
    fn something_at_the_holding_path_fails_the_run_and_is_left_as_it_was() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a.yml", b"a\n", 0o644);
        fill(root, ".skeletons-remove", b"not a directory\n", 0o644);

        let failure = staged(root, &["a.yml"]).expect_err("the holding path is taken");

        assert!(
            failure.to_string().starts_with(
                "remove could not create .skeletons-remove to move the files it deletes into: "
            ),
            "{failure}"
        );
        assert_eq!(
            std::fs::read(root.join(".skeletons-remove")).expect("the thing in the way"),
            b"not a directory\n"
        );
        assert!(root.join("a.yml").is_file());
    }

    #[test]
    fn a_directory_that_cannot_be_removed_for_another_reason_fails_the_run_and_the_file_is_back() {
        // `link` is a link to a directory, so removing it as a directory is
        // refused for a reason that is not that it holds something, and
        // whoever runs the test is refused alike. The file was moved before
        // the removal was tried, and is back after the undo.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "real/f.txt", b"f\n", 0o644);
        std::os::unix::fs::symlink("real", root.join("link")).expect("create the link");

        let failure =
            staged(root, &["link/f.txt"]).expect_err("a link is not a directory to remove");

        assert!(
            failure
                .to_string()
                .starts_with("remove could not remove link, which deleting its files left empty: "),
            "{failure}"
        );
        assert_eq!(
            std::fs::read(root.join("real/f.txt")).expect("the file is back"),
            b"f\n"
        );
        assert_eq!(names_in(root), ["link", "real"]);
    }

    #[test]
    fn the_holding_path_is_free_only_when_nothing_is_there() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        let holding = root.join(HOLDING_DIRECTORY);
        assert!(holding_path_is_free(root), "the control: nothing is there");

        std::fs::write(&holding, b"a file").expect("write a file");
        assert!(!holding_path_is_free(root), "a file");
        std::fs::remove_file(&holding).expect("remove it");

        std::fs::create_dir(&holding).expect("create a directory");
        assert!(!holding_path_is_free(root), "a directory");
        std::fs::remove_dir(&holding).expect("remove it");

        std::os::unix::fs::symlink("nowhere", &holding).expect("create a link to nothing");
        assert!(!holding_path_is_free(root), "a link to nothing");
    }

    #[test]
    fn purging_deletes_the_holding_directory_with_what_it_holds() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a.yml", b"a\n", 0o644);
        let staged = staged(root, &["a.yml"]).expect("the run succeeds");
        assert_eq!(names_in(root), [".skeletons-remove"]);

        purge(staged, "ci").expect("purged");

        assert!(names_in(root).is_empty());
    }

    #[test]
    fn a_holding_directory_that_cannot_be_deleted_names_the_wearing_and_the_error() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        fill(root, "a.yml", b"a\n", 0o644);
        let staged = staged(root, &["a.yml"]).expect("the run succeeds");
        // Something else removed it first, which leaves the delete nothing to
        // delete and the operating system's own answer.
        std::fs::remove_dir_all(root.join(HOLDING_DIRECTORY)).expect("remove it");

        let refusal = purge(staged, "ci").expect_err("there is nothing to delete");

        let RemoveRefusal::HoldingNotDeleted { key, detail } = refusal else {
            panic!("expected HoldingNotDeleted, got {refusal:?}");
        };
        assert_eq!(key, "ci");
        assert!(!detail.is_empty());
    }
}
