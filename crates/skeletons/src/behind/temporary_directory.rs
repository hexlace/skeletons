//! One directory `behind` creates to fetch a single remote snapshot into,
//! and removes once that snapshot has been read. The directory is always one
//! this module made itself, new and empty, so removing it never reaches the
//! wearer's own repository or Cargo's own checkout: nothing here is ever
//! given the path of either.

use std::path::{Path, PathBuf};

/// The prefix every name [`temporary_name`] builds starts with — checked again
/// in [`Drop`] before a removal runs, so a [`TemporaryDirectory`] can never be
/// dropped holding a path this module did not itself build.
const NAME_PREFIX: &str = "skeletons-behind-";

/// The most attempts [`TemporaryDirectory::create`] makes at a fresh name
/// before giving up — generous enough that a real collision (a previous run's
/// own leftover, or another process racing this one) is vanishingly unlikely to
/// exhaust it.
const CREATE_ATTEMPTS_MAX: u32 = 16;

/// A directory this module created under some `parent` (production always
/// passes [`std::env::temp_dir`]), exclusively — [`create`](Self::create) is
/// the only constructor, and every path it hands back was just made by
/// [`create_exclusively`]'s `DirBuilder::create` (`mkdir(2)`, which fails with
/// `EEXIST` on anything already at that path — directory, file, or symbolic
/// link alike — rather than following or replacing it), the same
/// exclusive-creation guarantee `sync`'s own `Staging` relies on
/// (`crates/skeletons/src/claim/location.rs`'s module doc).
/// Tested here for the plain-collision case by
/// `a_name_already_taken_by_something_else_is_retried_under_the_next_attempt`,
/// which plants a file at the exact name `create`'s first attempt would
/// use and confirms it moves on rather than reusing it; the symbolic-link
/// case is structural — `mkdir` cannot distinguish a name already taken by
/// a symlink from one taken by anything else, so there is no separate
/// mechanism to test. [`Drop`] removes it, recursively, best-effort.
#[derive(Debug)]
pub(crate) struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    /// This directory's own path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Creates a fresh, empty, exclusively-owned directory under `parent`,
    /// named by [`temporary_name`] from this process's own id, `slot` (the
    /// snapshot query's own index, so two queries running concurrently never
    /// contend for the same name) and an attempt counter — retried, up to
    /// [`CREATE_ATTEMPTS_MAX`] times, only on an actual name collision.
    ///
    /// # Errors
    ///
    /// Whatever [`std::fs::DirBuilder::create`] returns for a cause other
    /// than the name already being taken, or [`std::io::ErrorKind::AlreadyExists`]
    /// itself once every attempt has collided.
    pub(crate) fn create(parent: &Path, slot: usize) -> std::io::Result<Self> {
        let pid = std::process::id();
        for attempt in 0..CREATE_ATTEMPTS_MAX {
            let path = parent.join(temporary_name(pid, slot, attempt));
            match create_exclusively(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "could not create a fresh temporary directory under {} after \
                 {CREATE_ATTEMPTS_MAX} attempts",
                parent.display()
            ),
        ))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // What makes `remove_dir_all` safe here: `self.path` is private and
        // set only by `create`, which made it as a new, empty directory
        // under a name only this module builds. Everything beneath it is
        // there because `behind`'s own git commands were pointed at it, and
        // `remove_dir_all` removes a symbolic link it finds rather than
        // following it. The assertion below re-checks the one property a
        // stray mutation of `path` could break: its own name still carries
        // the prefix `create` always gives it.
        let name = self.path.file_name().and_then(|name| name.to_str());
        assert!(
            name.is_some_and(|name| name.starts_with(NAME_PREFIX)),
            "TemporaryDirectory::drop only ever removes a path this module itself created \
             exclusively; path was {}",
            self.path.display()
        );
        // The assertion above is the only thing in this `drop` that can
        // panic, and only on a defect in this module. The removal itself is
        // best-effort: its error is discarded, because nothing `behind`
        // reports depends on the removal succeeding and the directory lives
        // in the operating system's own temporary area regardless.
        let _unused = std::fs::remove_dir_all(&self.path);
    }
}

/// `skeletons-behind-<pid>-<slot>-<attempt>` — pure, so this name is pinned by a
/// unit test rather than only ever observed through a real filesystem call.
fn temporary_name(pid: u32, slot: usize, attempt: u32) -> String {
    format!("{NAME_PREFIX}{pid}-{slot}-{attempt}")
}

fn create_exclusively(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    // `0o700`: only this process's own user may read, write or enter it —
    // the snapshot a wearer's own remote could hold is not this process's
    // to share with anyone else on the machine.
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(test)]
mod tests {
    use super::{TemporaryDirectory, temporary_name};

    #[test]
    fn temporary_name_is_the_prefix_then_pid_slot_and_attempt() {
        assert_eq!(temporary_name(123, 4, 0), "skeletons-behind-123-4-0");
        assert_eq!(temporary_name(123, 4, 2), "skeletons-behind-123-4-2");
    }

    #[test]
    fn create_makes_a_fresh_empty_directory_under_parent() {
        let parent = tempfile::tempdir().expect("scratch parent");
        let temporary = TemporaryDirectory::create(parent.path(), 0).expect("must create");
        assert!(temporary.path().is_dir());
        assert!(
            temporary
                .path()
                .parent()
                .is_some_and(|found| found == parent.path())
        );
    }

    #[test]
    fn drop_removes_the_directory_and_everything_under_it() {
        let parent = tempfile::tempdir().expect("scratch parent");
        let path = {
            let temporary = TemporaryDirectory::create(parent.path(), 0).expect("must create");
            std::fs::write(temporary.path().join("inner.txt"), b"snapshot")
                .expect("write a file inside it");
            temporary.path().to_owned()
        };
        assert!(!path.exists(), "the directory must be gone once dropped");
    }

    #[test]
    fn two_different_slots_never_collide() {
        let parent = tempfile::tempdir().expect("scratch parent");
        let first = TemporaryDirectory::create(parent.path(), 0).expect("must create");
        let second = TemporaryDirectory::create(parent.path(), 1).expect("must create");
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn a_name_already_taken_by_something_else_is_retried_under_the_next_attempt() {
        let parent = tempfile::tempdir().expect("scratch parent");
        // Plant the first attempt's own name ahead of time, as a plain file
        // rather than a directory `create` could ever have made itself, so
        // this proves the retry moves on rather than reusing what is there.
        std::fs::write(parent.path().join("skeletons-behind-0-0-0"), b"planted")
            .expect("plant a collision");
        let pid = std::process::id();
        std::fs::write(
            parent.path().join(format!("skeletons-behind-{pid}-0-0")),
            b"planted",
        )
        .expect("plant a collision at this process's own first attempt");

        let temporary = TemporaryDirectory::create(parent.path(), 0).expect("must still create");
        assert!(temporary.path().is_dir());
        assert_ne!(
            temporary.path().file_name().and_then(|name| name.to_str()),
            Some(format!("skeletons-behind-{pid}-0-0").as_str())
        );
    }
}
