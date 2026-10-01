//! The staging ledger: the only way `sync` ever creates a directory or a
//! staging file, so no failure path — including a panic — can lose track of
//! one (tested by `a_panic_after_creation_still_removes_the_staging_file`
//! and
//! `every_created_staging_file_is_removed_when_staging_fails_after_creation`,
//! below).
//!
//! Every staging file sits beside its own target, under a deterministic
//! name (no process id, no index): a leftover from an earlier interrupted
//! run, or from a `sync` running concurrently right now, is refused by that
//! name rather than raced around under a different one. A leftover is
//! something a person should see.
//!
//! Each thing the ledger creates is recorded with the device and inode it
//! was created as, and every removal — [`Staging::roll_back`], the [`Drop`]
//! backstop and [`Staging::linked`] — walks the path again and removes only
//! an entry that is still that one ([`remove_staging_file`] and
//! [`remove_directory`]). Anything else is
//! left in place and reported as a [`Leftover`] with its reason; nothing is
//! ever removed through a link. Tested by
//! `rolling_back_never_removes_a_file_through_a_directory_swapped_for_a_link`,
//! `dropping_the_ledger_never_removes_a_file_through_a_directory_swapped_for_a_link`
//! and `linked_never_removes_the_staging_name_through_a_link`, below.
//!
//! A staging file leaves the ledger one of two ways, each a single call that
//! both forgets it and says what became of it: [`Staging::handed_over`]
//! when a `rename` moved it onto its target (there is nothing left to
//! remove), and [`Staging::linked`] when a `hard_link` gave the target a
//! second name for the same file (the staging name is still there and is
//! removed, and a failure to remove it is reported, since it is a second
//! link to a file that is now the wearer's, and an untracked file that would
//! stop the next `sync`). Tested by `a_linked_staging_file_is_removed_and_forgotten` and
//! `a_linked_staging_file_that_cannot_be_removed_is_reported`, below.

use std::path::{Path, PathBuf};

use super::identity::FileIdentity;
use super::leftover::{Leftover, LeftoverReason};
use super::removal::{Created, DirectoryRemoval, Removal, remove_directory, remove_staging_file};
use crate::claim::ClaimPath;

/// Everything one `sync` run has created on disk and not yet handed over —
/// the only way `sync` creates a directory or a staging file: each is
/// recorded in the same call that creates it, so no failure path can lose
/// track of one — the same guarantee the module doc above names, tested by
/// `a_panic_after_creation_still_removes_the_staging_file` and
/// `every_created_staging_file_is_removed_when_staging_fails_after_creation`,
/// below. Kept in creation order.
#[derive(Debug)]
pub(super) struct Staging {
    root: PathBuf,
    created: Vec<Created>,
}

/// A staging file just created: the open handle, and the identity it was
/// created as, read from that handle once so every later question about which
/// file this is has the one answer.
#[derive(Debug)]
pub(super) struct StagedFile {
    pub(super) file: std::fs::File,
    pub(super) identity: FileIdentity,
}

/// Why creating a directory or a staging file failed.
#[derive(Debug)]
pub(super) enum StageError {
    /// Something is already at a path `sync` needed to create. Never
    /// overwritten, never removed.
    Collision {
        shown: String,
    },
    /// The operating system refused permission to create something in
    /// `directory` (`None` for the workspace root): the directory is not
    /// writable by this user.
    NotWritable {
        directory: Option<ClaimPath>,
        detail: String,
    },
    Io {
        detail: String,
    },
}

impl StageError {
    /// What creating `path` failing with `error` means: a path already taken
    /// is a [`Self::Collision`] where the caller says so, a permission refusal
    /// is [`Self::NotWritable`] for the directory `path` sits in, and
    /// anything else is the operating system's own failure.
    fn from_create(error: &std::io::Error, path: &ClaimPath) -> Self {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Self::NotWritable {
                directory: path.parent(),
                detail: error.to_string(),
            }
        } else {
            Self::Io {
                detail: error.to_string(),
            }
        }
    }
}

impl Staging {
    /// An empty ledger, rooted at the workspace root `shown` paths are made
    /// relative to.
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            created: Vec::new(),
        }
    }

    /// `std::fs::create_dir`. It never follows a link at `directory`
    /// (`create_dir` fails with `AlreadyExists` on anything already there,
    /// symbolic link included), so it never writes through one.
    ///
    /// The directory's identity is read from a `symlink_metadata` straight
    /// after the create. If what is there is not a directory, something
    /// replaced it in that instant: that is a [`StageError::Collision`], and
    /// it is not recorded, because it is not `sync`'s to remove. If the
    /// operating system will not describe it at all, the directory exists and
    /// cannot be identified, so it is a [`StageError::Io`] naming the path,
    /// for the person to remove by hand.
    pub(super) fn create_directory(&mut self, directory: &ClaimPath) -> Result<(), StageError> {
        let path = directory.to_path(&self.root);
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StageError::Collision {
                    shown: directory.to_string(),
                });
            }
            Err(error) => return Err(StageError::from_create(&error, directory)),
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                self.created.push(Created::Directory {
                    path: directory.clone(),
                    identity: FileIdentity::of(&metadata),
                });
                Ok(())
            }
            Ok(_replaced) => Err(StageError::Collision {
                shown: directory.to_string(),
            }),
            Err(error) => Err(StageError::Io {
                detail: format!(
                    "{directory} was created, but the operating system would not describe it \
                     ({error}), so sync cannot tell it from another directory at that path and \
                     will not remove it: remove it by hand"
                ),
            }),
        }
    }

    /// `OpenOptions::new().write(true).create_new(true).open(path)`:
    /// `O_CREAT|O_EXCL`, which fails on any existing entry — including a
    /// dangling symbolic link (the standard library's own documentation for
    /// `create_new`: "No file is allowed to exist at the target location,
    /// also no (dangling) symlink") — so it never follows a link and never
    /// truncates. Recorded before the [`std::fs::File`] is returned, so a
    /// panic on the very next line still leaves this ledger holding it, with
    /// the identity read from the open handle: the file `create_new` made,
    /// never a path looked up again. The identity is returned beside the
    /// handle, so nothing reads it a second time.
    ///
    /// If the operating system will not describe the descriptor it has just
    /// opened, the file exists and `sync` cannot identify it, so it cannot
    /// remove it safely: that is a [`StageError::Io`] naming the path, for
    /// the person to remove by hand.
    pub(super) fn create_staging_file(
        &mut self,
        staging: &ClaimPath,
    ) -> Result<StagedFile, StageError> {
        let path = staging.to_path(&self.root);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                let metadata = file.metadata().map_err(|error| StageError::Io {
                    detail: format!(
                        "{staging} was created, but the operating system would not describe it \
                         ({error}), so sync cannot tell it from another file at that path and \
                         will not remove it: remove it by hand"
                    ),
                })?;
                let identity = FileIdentity::of(&metadata);
                self.created.push(Created::StagingFile {
                    path: staging.clone(),
                    identity,
                });
                Ok(StagedFile { file, identity })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(StageError::Collision {
                    shown: staging.to_string(),
                })
            }
            Err(error) => Err(StageError::from_create(&error, staging)),
        }
    }

    /// The staging file at `path` was renamed onto its target and no longer
    /// exists: this ledger forgets it, so neither an explicit
    /// [`Self::roll_back`] nor the [`Drop`] backstop ever tries to remove
    /// it again.
    ///
    /// # Panics
    ///
    /// If `path` is not a staging file this same ledger created — a defect
    /// in `sync::write`'s own bookkeeping, never something a wearer's
    /// repository could cause.
    pub(super) fn handed_over(&mut self, path: &ClaimPath) {
        self.forget_staging_file(path);
    }

    /// The staging file at `path` was hard-linked onto its target, which
    /// now names the same file: this ledger forgets it and removes the
    /// staging name — after walking the path again, and only if it is still
    /// the file this ledger created — so neither [`Self::roll_back`] nor the
    /// [`Drop`] backstop ever tries again.
    ///
    /// Returns the staging file as a [`Leftover`] when it was left in place
    /// or could not be removed. The write it belongs to is finished and
    /// correct either way; the caller reports the leftover rather than
    /// treating the write as failed. A staging file already gone is not a
    /// leftover: the bytes are at the target.
    ///
    /// # Panics
    ///
    /// If `path` is not a staging file this same ledger created, as for
    /// [`Self::handed_over`].
    pub(super) fn linked(&mut self, path: &ClaimPath) -> Option<Leftover> {
        let identity = self.forget_staging_file(path);
        match remove_staging_file(&self.root, path, identity) {
            Removal::Removed | Removal::Gone => None,
            Removal::Left(reason) => Some(Leftover {
                path: path.to_string(),
                reason,
            }),
        }
    }

    /// Removes every staging file still recorded, then every recorded
    /// directory, newest first, each only if it is still what this ledger
    /// created. A directory that is not empty — it holds a finished write,
    /// or something else entirely — is kept, and that is not a failure.
    /// Returns everything it left in place or could not remove, sorted by
    /// path.
    pub(super) fn roll_back(mut self) -> Vec<Leftover> {
        let created = std::mem::take(&mut self.created);
        remove_all(&self.root, created)
    }

    /// Every staging file this run staged was handed over or linked already
    /// (each [`Self::handed_over`] and [`Self::linked`] call already forgot
    /// it), so nothing is left to roll back: the directories this run created
    /// stay, since they hold finished writes.
    pub(super) fn keep(mut self) {
        assert!(
            self.created
                .iter()
                .all(|created| matches!(created, Created::Directory { .. })),
            "keep is only ever called once every staging file has been handed over or linked"
        );
        self.created.clear();
    }

    /// Takes the staging file at `path` out of the ledger and returns the
    /// identity it was created as.
    fn forget_staging_file(&mut self, path: &ClaimPath) -> FileIdentity {
        let position = self.created.iter().position(|created| match created {
            Created::StagingFile { path: staged, .. } => staged == path,
            Created::Directory { .. } => false,
        });
        let position = position.unwrap_or_else(|| {
            unreachable!(
                "only a staging file this ledger itself created is ever handed over or linked"
            )
        });
        match self.created.remove(position) {
            Created::StagingFile { identity, .. } => identity,
            Created::Directory { .. } => {
                unreachable!("the position found above is a staging file")
            }
        }
    }
}

impl Drop for Staging {
    /// Best-effort backstop for a panic between creating something and this
    /// run's own explicit [`Staging::roll_back`] or [`Staging::keep`] — it
    /// never panics, and discards what it cannot fix, the same as every other
    /// best-effort cleanup in this crate. It removes through the same walk and
    /// identity check as a rollback, so it never follows a link either.
    fn drop(&mut self) {
        let created = std::mem::take(&mut self.created);
        if created.is_empty() {
            return;
        }
        let _leftovers = remove_all(&self.root, created);
    }
}

/// Removes every staging file in `created`, then every directory in it,
/// newest first (reverse creation order), so a directory is never removed
/// while it might still hold one of those staging files. Returns everything
/// left in place or not removable, sorted by path.
///
/// A staging file that is no longer at its path is reported: it may have
/// moved with its directory, so what `sync` prepared may be somewhere it
/// cannot name. A directory that is no longer there is not: nothing `sync`
/// made remains at that path, and a directory holds no prepared bytes.
fn remove_all(root: &Path, created: Vec<Created>) -> Vec<Leftover> {
    let mut leftovers = Vec::new();
    let mut directories = Vec::new();

    for entry in created {
        match entry {
            Created::StagingFile { path, identity } => {
                let reason = match remove_staging_file(root, &path, identity) {
                    Removal::Removed => None,
                    Removal::Gone => Some(LeftoverReason::Gone),
                    Removal::Left(reason) => Some(reason),
                };
                if let Some(reason) = reason {
                    leftovers.push(Leftover {
                        path: path.to_string(),
                        reason,
                    });
                }
            }
            Created::Directory { path, identity } => directories.push((path, identity)),
        }
    }

    for (path, identity) in directories.into_iter().rev() {
        match remove_directory(root, &path, identity) {
            // A directory still holding a finished write — or anything
            // else — is kept, and that is not a leftover: nothing this
            // ledger created is lost, only left where it already was.
            DirectoryRemoval::Removed | DirectoryRemoval::KeptNotEmpty | DirectoryRemoval::Gone => {
            }
            DirectoryRemoval::Left(reason) => leftovers.push(Leftover {
                path: path.to_string(),
                reason,
            }),
        }
    }

    leftovers.sort_by(|first, second| first.path.cmp(&second.path));
    leftovers
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::Staging;
    use crate::claim::{ClaimPath, UnsafePathCause};
    use crate::sync::write::identity::FileIdentity;
    use crate::sync::write::removal::Created;
    use crate::sync::write::{Leftover, LeftoverReason};

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    #[test]
    fn every_created_staging_file_is_removed_when_staging_fails_after_creation() {
        // Drives the ledger directly, without `sync::write::prepare` in the
        // way: one directory created for one write (beneath another made
        // beforehand), then its staging file, then a rollback — pinning that
        // `roll_back` honours the property every constructor already
        // establishes by recording what it creates.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());

        let directory_a = root.path().join("a");
        let directory_b = directory_a.join("b");
        std::fs::create_dir(&directory_a).expect("pre-create `a`");
        staging
            .create_directory(&claim_path("a/b"))
            .expect("create the nested directory through the ledger");
        let staging_file = directory_b.join(".thing.skeletons-sync");
        let file = staging
            .create_staging_file(&claim_path("a/b/.thing.skeletons-sync"))
            .expect("create the staging file through the ledger");
        drop(file);

        let leftovers = staging.roll_back();

        assert!(
            leftovers.is_empty(),
            "nothing here should fail to remove: {leftovers:?}"
        );
        assert!(!staging_file.exists(), "the staging file must be removed");
        assert!(
            !directory_b.exists(),
            "the directory the ledger created must be removed"
        );
        assert!(
            directory_a.exists(),
            "a directory the ledger did not itself create must survive"
        );
    }

    #[test]
    fn a_panic_after_creation_still_removes_the_staging_file() {
        // The `Drop` backstop: a closure that stages a file through the
        // ledger and then panics before ever calling `roll_back` or `keep`
        // explicitly. Unwinding still drops the ledger's own local
        // variable, which is `Staging`'s own best-effort cleanup.
        let root = TempDir::new().expect("scratch directory");
        let staging_file = root.path().join(".thing.skeletons-sync");
        let root_path = root.path().to_path_buf();
        let staging_claim = claim_path(".thing.skeletons-sync");

        let result = std::panic::catch_unwind(move || {
            let mut staging = Staging::new(&root_path);
            let _file = staging
                .create_staging_file(&staging_claim)
                .expect("staging file must be created");
            panic!("simulated failure between creation and handover");
        });

        assert!(result.is_err(), "the closure must have panicked");
        assert!(
            !staging_file.exists(),
            "Drop must remove a staging file left behind by a panic"
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_staging_file_that_cannot_be_removed_is_reported_as_a_leftover() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("scratch directory");
        let directory = root.path().join("locked");
        std::fs::create_dir_all(&directory).expect("directory to lock down");
        std::fs::write(directory.join("probe"), b"x").expect("probe file");

        let mut staging = Staging::new(root.path());
        let _file = staging
            .create_staging_file(&claim_path("locked/.thing.skeletons-sync"))
            .expect("staging file must be created");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
            .expect("lock down the directory");

        // A process that bypasses permission checks (root, as in many CI
        // containers) can still remove an entry from a 0o555 directory, so
        // there is nothing here for this test to exercise. The premise is
        // checked rather than assumed.
        let premise_holds = match std::fs::remove_file(directory.join("probe")) {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => true,
            Err(error) => panic!("removing the probe failed, but not on permission: {error}"),
        };
        if !premise_holds {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
                .expect("restore permissions so the temp directory can be cleaned up");
            eprintln!("skipped: this process can delete inside a 0o555 directory");
            return;
        }

        let leftovers = staging.roll_back();

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("restore permissions so the temp directory can be cleaned up");

        assert_eq!(
            leftovers.len(),
            1,
            "the staging file must be reported as one leftover: {leftovers:?}"
        );
        assert!(
            leftovers[0].path.ends_with(".thing.skeletons-sync"),
            "the leftover must name the staging file: {leftovers:?}"
        );
    }

    #[test]
    fn a_directory_still_holding_a_finished_write_is_kept_and_not_a_leftover() {
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let directory = root.path().join("d");
        staging
            .create_directory(&claim_path("d"))
            .expect("create the directory through the ledger");
        let file = staging
            .create_staging_file(&claim_path("d/.thing.skeletons-sync"))
            .expect("create the staging file through the ledger");
        drop(file);

        // Simulates an earlier write in the same run whose rename already
        // succeeded: a finished file sits in `d` that the ledger never
        // created and therefore never tracks.
        std::fs::write(directory.join("finished-write.txt"), b"already renamed")
            .expect("a finished write already sitting in the directory");

        let leftovers = staging.roll_back();

        assert!(
            leftovers.is_empty(),
            "a non-empty directory is not a leftover: {leftovers:?}"
        );
        assert!(
            !directory.join(".thing.skeletons-sync").exists(),
            "the staging file itself must still be removed"
        );
        assert!(
            directory.exists(),
            "the directory must survive, since it still holds the finished write"
        );
    }

    #[test]
    fn a_linked_staging_file_is_removed_and_forgotten() {
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let staging_file = root.path().join(".thing.skeletons-sync");
        let target = root.path().join("thing");
        let file = staging
            .create_staging_file(&claim_path(".thing.skeletons-sync"))
            .expect("create the staging file through the ledger");
        drop(file);
        std::fs::hard_link(&staging_file, &target).expect("link the staging file onto its target");

        assert_eq!(
            staging.linked(&claim_path(".thing.skeletons-sync")),
            None,
            "the staging name must be removable"
        );

        assert!(!staging_file.exists(), "the staging name must be removed");
        assert!(target.exists(), "the target keeps the file");
        staging.keep();
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_linked_staging_file_that_cannot_be_removed_is_reported() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("scratch directory");
        let directory = root.path().join("locked");
        std::fs::create_dir_all(&directory).expect("directory to lock down");
        std::fs::write(directory.join("probe"), b"x").expect("probe file");

        let mut staging = Staging::new(root.path());
        drop(
            staging
                .create_staging_file(&claim_path("locked/.thing.skeletons-sync"))
                .expect("staging file must be created"),
        );
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
            .expect("lock down the directory");

        // As in the roll-back test above: a process that bypasses
        // permission checks has nothing to exercise here.
        let premise_holds = match std::fs::remove_file(directory.join("probe")) {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => true,
            Err(error) => panic!("removing the probe failed, but not on permission: {error}"),
        };
        if !premise_holds {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
                .expect("restore permissions so the temp directory can be cleaned up");
            eprintln!("skipped: this process can delete inside a 0o555 directory");
            return;
        }

        let outcome = staging.linked(&claim_path("locked/.thing.skeletons-sync"));

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("restore permissions so the temp directory can be cleaned up");

        assert!(
            matches!(
                &outcome,
                Some(Leftover {
                    path,
                    reason: LeftoverReason::CouldNotRemove { .. },
                }) if path == "locked/.thing.skeletons-sync"
            ),
            "the staging name must be reported as one that could not be removed: {outcome:?}"
        );
        assert!(
            staging.roll_back().is_empty(),
            "a linked staging file is forgotten even when removing it failed"
        );
    }
    /// The staging file's name under the directory `x` in the rollback tests
    /// below.
    const STAGED_NAME: &str = ".f.yml.skeletons-sync";

    /// Swaps the real directory `x` under `root` for a symbolic link to
    /// `outside`, keeping the real one as `xreal`: what a racing process
    /// does between the moment `sync` staged a file and the moment it rolls
    /// back.
    fn swap_directory_for_a_link(root: &Path, outside: &Path) {
        std::fs::rename(root.join("x"), root.join("xreal")).expect("move the real directory");
        std::os::unix::fs::symlink(outside, root.join("x")).expect("link in its place");
    }

    #[test]
    fn rolling_back_never_removes_a_file_through_a_directory_swapped_for_a_link() {
        // `sync` staged `x/.f.yml.skeletons-sync`. Before it rolled back,
        // `x` was replaced by a link to a directory outside the workspace
        // that holds a file of the same name. Removing by name follows the
        // link and deletes the outside file, while the real staging file is
        // stranded and unreported. The outside file must survive, and the
        // staging file the rollback could not reach must be reported.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("a directory outside the workspace");
        std::fs::create_dir(root.path().join("x")).expect("the directory");
        let mut staging = Staging::new(root.path());
        drop(
            staging
                .create_staging_file(&claim_path(&format!("x/{STAGED_NAME}")))
                .expect("stage through the ledger"),
        );
        let precious = outside.path().join(STAGED_NAME);
        std::fs::write(&precious, b"precious").expect("the outside file");
        swap_directory_for_a_link(root.path(), outside.path());

        let leftovers = staging.roll_back();

        assert_eq!(
            std::fs::read(&precious).ok().as_deref(),
            Some(b"precious".as_slice()),
            "a rollback must never remove a file through a link: the outside file is gone or \
             changed"
        );
        assert!(
            leftovers
                .iter()
                .any(|left| left.path.ends_with(STAGED_NAME)),
            "the staging file the rollback could not reach must be reported: {leftovers:?}"
        );
    }

    #[test]
    fn rolling_back_never_removes_a_directory_through_a_directory_swapped_for_a_link() {
        // The ledger created `x/y`. Before the rollback `x` became a link
        // to an outside directory that holds an empty `y` of its own.
        // Removing `x/y` by name removes that one. It must survive, and the
        // directory the rollback could not reach must be reported.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("a directory outside the workspace");
        std::fs::create_dir(root.path().join("x")).expect("the directory");
        let mut staging = Staging::new(root.path());
        staging
            .create_directory(&claim_path("x/y"))
            .expect("create through the ledger");
        let outside_directory = outside.path().join("y");
        std::fs::create_dir(&outside_directory).expect("the outside directory");
        swap_directory_for_a_link(root.path(), outside.path());

        let leftovers = staging.roll_back();

        assert!(
            outside_directory.is_dir(),
            "a rollback must never remove a directory through a link"
        );
        assert!(
            leftovers.iter().any(|left| left.path.ends_with("/y")),
            "the directory the rollback could not reach must be reported: {leftovers:?}"
        );
    }

    #[test]
    fn dropping_the_ledger_never_removes_a_file_through_a_directory_swapped_for_a_link() {
        // The same swap, reached by the backstop rather than an explicit
        // rollback: the ledger is dropped without `roll_back` or `keep`,
        // as it is when a panic unwinds. The backstop must not follow the
        // link either.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("a directory outside the workspace");
        std::fs::create_dir(root.path().join("x")).expect("the directory");
        let mut staging = Staging::new(root.path());
        drop(
            staging
                .create_staging_file(&claim_path(&format!("x/{STAGED_NAME}")))
                .expect("stage through the ledger"),
        );
        let precious = outside.path().join(STAGED_NAME);
        std::fs::write(&precious, b"precious").expect("the outside file");
        swap_directory_for_a_link(root.path(), outside.path());

        drop(staging);

        assert_eq!(
            std::fs::read(&precious).ok().as_deref(),
            Some(b"precious".as_slice()),
            "the drop backstop must never remove a file through a link"
        );
    }

    #[test]
    fn rolling_back_leaves_a_file_that_is_no_longer_the_one_it_staged_and_reports_it() {
        // The staging file was removed and something else put at its path
        // (another process, or a leftover of another run). It is not the file
        // `sync` created, so `sync` must not remove it, and must say it is
        // there. The first file is kept alive under another name so the
        // second cannot inherit its inode: the identity check, not luck,
        // tells them apart.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let staged = root.path().join(STAGED_NAME);
        drop(
            staging
                .create_staging_file(&claim_path(STAGED_NAME))
                .expect("stage through the ledger"),
        );
        std::fs::rename(&staged, root.path().join("moved")).expect("the staging file moves");
        std::fs::write(&staged, b"someone else's file").expect("another file at the same path");

        let leftovers = staging.roll_back();

        assert_eq!(
            std::fs::read(&staged).ok().as_deref(),
            Some(b"someone else's file".as_slice()),
            "a rollback must remove only the file it staged"
        );
        assert!(
            leftovers
                .iter()
                .any(|left| left.path.ends_with(STAGED_NAME)),
            "the file it left must be reported: {leftovers:?}"
        );
    }

    /// The one leftover `leftovers` holds, whose path ends with `name`.
    fn the_leftover_named<'a>(leftovers: &'a [Leftover], name: &str) -> &'a Leftover {
        let mut matching = leftovers.iter().filter(|left| left.path.ends_with(name));
        let found = matching
            .next()
            .unwrap_or_else(|| panic!("no leftover named {name}: {leftovers:?}"));
        assert!(
            matching.next().is_none(),
            "more than one leftover named {name}: {leftovers:?}"
        );
        found
    }

    #[test]
    fn a_staging_file_under_a_directory_swapped_for_a_link_is_left_with_its_reason() {
        // The same swap as the first rollback test above, read for the reason
        // rather than for what survived: the walk refuses the path because
        // `x` above it is now a link, and says so.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("a directory outside the workspace");
        std::fs::create_dir(root.path().join("x")).expect("the directory");
        let mut staging = Staging::new(root.path());
        drop(
            staging
                .create_staging_file(&claim_path(&format!("x/{STAGED_NAME}")))
                .expect("stage through the ledger"),
        );
        swap_directory_for_a_link(root.path(), outside.path());

        let leftovers = staging.roll_back();

        assert_eq!(
            the_leftover_named(&leftovers, STAGED_NAME).reason,
            LeftoverReason::PathUnsafe(UnsafePathCause::SymbolicLinkAbove { at: "x".to_owned() })
        );
        assert!(
            root.path().join("xreal").join(STAGED_NAME).exists(),
            "the real staging file, in the moved directory, must be untouched"
        );
    }

    #[test]
    fn a_staging_file_replaced_by_another_file_is_left_as_something_else() {
        // The staging file goes and another file takes its path. The first
        // file is kept alive under another name so the second cannot inherit
        // its inode: the identity check, not luck, tells them apart.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let staged = root.path().join(STAGED_NAME);
        drop(
            staging
                .create_staging_file(&claim_path(STAGED_NAME))
                .expect("stage through the ledger"),
        );
        std::fs::rename(&staged, root.path().join("moved")).expect("the staging file moves");
        std::fs::write(&staged, b"another file").expect("another file at the path");

        let leftovers = staging.roll_back();

        assert_eq!(
            the_leftover_named(&leftovers, STAGED_NAME).reason,
            LeftoverReason::SomethingElseThere
        );
        assert!(staged.exists(), "the other file must be left in place");
    }

    #[test]
    fn a_staging_file_replaced_by_a_directory_is_left_as_something_else() {
        // The kind is checked as well as the identity: a directory at the
        // path is never removed as though it were the staging file.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let staged = root.path().join(STAGED_NAME);
        drop(
            staging
                .create_staging_file(&claim_path(STAGED_NAME))
                .expect("stage through the ledger"),
        );
        std::fs::remove_file(&staged).expect("the staging file goes");
        std::fs::create_dir(&staged).expect("a directory at its path");

        let leftovers = staging.roll_back();

        assert_eq!(
            the_leftover_named(&leftovers, STAGED_NAME).reason,
            LeftoverReason::SomethingElseThere
        );
        assert!(staged.is_dir(), "the directory must be left in place");
    }

    #[test]
    fn a_staging_file_that_vanished_is_reported_gone_by_roll_back_but_not_by_linked() {
        // A rollback cannot tell a staging file removed from one that moved
        // away with its directory, so it says it is no longer there; after a
        // link the bytes are at the target, so nothing is left to report.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let staged = root.path().join(STAGED_NAME);
        drop(
            staging
                .create_staging_file(&claim_path(STAGED_NAME))
                .expect("stage the first file"),
        );
        let linked_name = ".g.yml.skeletons-sync";
        drop(
            staging
                .create_staging_file(&claim_path(linked_name))
                .expect("stage the second file"),
        );
        std::fs::remove_file(&staged).expect("the first staging file vanishes");
        std::fs::remove_file(root.path().join(linked_name)).expect("the second vanishes");

        assert_eq!(staging.linked(&claim_path(linked_name)), None);
        let leftovers = staging.roll_back();

        assert_eq!(
            the_leftover_named(&leftovers, STAGED_NAME).reason,
            LeftoverReason::Gone
        );
        assert_eq!(
            leftovers.len(),
            1,
            "only the rolled-back file: {leftovers:?}"
        );
    }

    #[test]
    fn a_created_directory_replaced_by_another_directory_is_left() {
        // The first directory is kept alive under another name, so the second
        // cannot inherit its inode.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        staging
            .create_directory(&claim_path("d"))
            .expect("create through the ledger");
        std::fs::rename(root.path().join("d"), root.path().join("moved")).expect("move it");
        std::fs::create_dir(root.path().join("d")).expect("another directory of the same name");

        let leftovers = staging.roll_back();

        assert_eq!(
            the_leftover_named(&leftovers, "d").reason,
            LeftoverReason::SomethingElseThere
        );
        assert!(root.path().join("d").is_dir(), "it must be left in place");
    }

    #[test]
    fn a_directory_that_vanished_is_not_reported() {
        // Nothing `sync` made remains at the path, and a directory holds no
        // prepared bytes.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        staging
            .create_directory(&claim_path("d"))
            .expect("create through the ledger");
        std::fs::remove_dir(root.path().join("d")).expect("it goes");

        assert_eq!(staging.roll_back(), Vec::new());
    }

    #[test]
    fn linked_never_removes_the_staging_name_through_a_link() {
        // The staging file was linked onto its target, and before its name
        // was removed the directory above was swapped for a link to an
        // outside directory holding a same-named file. The outside file
        // survives, and the staging name is reported as left in place.
        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("a directory outside the workspace");
        std::fs::create_dir(root.path().join("x")).expect("the directory");
        let mut staging = Staging::new(root.path());
        let staging_claim = claim_path(&format!("x/{STAGED_NAME}"));
        drop(
            staging
                .create_staging_file(&staging_claim)
                .expect("stage through the ledger"),
        );
        let precious = outside.path().join(STAGED_NAME);
        std::fs::write(&precious, b"precious").expect("the outside file");
        swap_directory_for_a_link(root.path(), outside.path());

        let outcome = staging.linked(&staging_claim);

        assert_eq!(
            std::fs::read(&precious).ok().as_deref(),
            Some(b"precious".as_slice()),
            "linked must never remove a file through a link"
        );
        assert_eq!(
            outcome.map(|left| left.reason),
            Some(LeftoverReason::PathUnsafe(
                UnsafePathCause::SymbolicLinkAbove { at: "x".to_owned() }
            ))
        );
        staging.keep();
    }

    #[test]
    fn the_identity_recorded_is_the_handles_own() {
        // The identity is read from the open handle, so it is the file
        // `create_new` made: it equals what the handle says, and differs from
        // any other file's.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        let file = staging
            .create_staging_file(&claim_path(STAGED_NAME))
            .expect("stage through the ledger");
        std::fs::write(root.path().join("other"), b"another file").expect("another file");

        let from_handle =
            FileIdentity::of(&file.file.metadata().expect("the handle's own metadata"));
        let other = FileIdentity::of(
            &std::fs::symlink_metadata(root.path().join("other")).expect("metadata"),
        );

        let [Created::StagingFile { identity, .. }] = staging.created.as_slice() else {
            panic!("expected exactly one staging file: {:?}", staging.created)
        };
        assert_eq!(*identity, from_handle);
        assert_eq!(
            file.identity, *identity,
            "the one returned is the one recorded"
        );
        assert_ne!(*identity, other);
    }

    #[test]
    fn a_directory_created_through_the_ledger_records_its_own_identity() {
        let root = TempDir::new().expect("scratch directory");
        let mut staging = Staging::new(root.path());
        staging
            .create_directory(&claim_path("d"))
            .expect("create through the ledger");

        let on_disk =
            FileIdentity::of(&std::fs::symlink_metadata(root.path().join("d")).expect("metadata"));

        let [Created::Directory { identity, .. }] = staging.created.as_slice() else {
            panic!("expected exactly one directory: {:?}", staging.created)
        };
        assert_eq!(*identity, on_disk);
    }

    /// Makes `directory` unwritable for this user and says whether that took
    /// effect: a superuser can write anywhere, so there the test that needs
    /// the refusal says `skipped:` rather than pass without having tested it.
    fn make_unwritable(directory: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555))
            .expect("chmod the directory");
        // Only a refusal on permission makes the directory unwritable for the
        // test's purpose; any other failure is a fault in the scratch
        // directory and fails the test.
        match std::fs::write(directory.join("probe"), b"x") {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => true,
            Err(error) => panic!("the probe write failed, but not on permission: {error}"),
        }
    }

    fn make_writable(directory: &Path) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
            .expect("restore the directory");
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn creating_in_a_directory_this_user_cannot_write_is_not_writable_and_names_it() {
        let root = TempDir::new().expect("scratch directory");
        let locked = root.path().join("locked");
        std::fs::create_dir(&locked).expect("the directory to lock");
        if !make_unwritable(&locked) {
            make_writable(&locked);
            eprintln!("skipped: this process can write into a 0o555 directory");
            return;
        }
        let mut staging = Staging::new(root.path());

        let directory = staging.create_directory(&claim_path("locked/sub"));
        let file = staging.create_staging_file(&claim_path("locked/.x.yml.skeletons-sync"));
        make_writable(&locked);

        for outcome in [directory, file.map(|_file| ())] {
            let Err(super::StageError::NotWritable { directory, detail }) = outcome else {
                panic!("expected NotWritable, got {outcome:?}")
            };
            assert_eq!(directory, Some(claim_path("locked")));
            assert!(!detail.is_empty(), "the operating system's own words stay");
        }
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn creating_in_an_unwritable_workspace_root_names_no_directory() {
        let root = TempDir::new().expect("scratch directory");
        if !make_unwritable(root.path()) {
            make_writable(root.path());
            eprintln!("skipped: this process can write into a 0o555 directory");
            return;
        }
        let mut staging = Staging::new(root.path());

        let outcome = staging.create_staging_file(&claim_path(".x.yml.skeletons-sync"));
        make_writable(root.path());

        let Err(super::StageError::NotWritable { directory, .. }) = outcome else {
            panic!("expected NotWritable, got {outcome:?}")
        };
        assert_eq!(directory, None, "a file at the root is in no directory");
    }
}
