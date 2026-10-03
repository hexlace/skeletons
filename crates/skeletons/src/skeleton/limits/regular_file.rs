//! A path judged, from its own metadata, to name a regular file, and the
//! bounded read of it. The judgement is the only way to obtain the type that
//! can be read, so [`RegularFile::read`] never opens a path that
//! [`RegularFile::judge`] has not passed.

use std::io::Read as _;
use std::path::Path;

use super::{ByteBudget, Reason};

/// A path known, from its own metadata, to name a regular file: not a symbolic
/// link, a directory, a FIFO, a socket or a device.
///
/// [`judge`](Self::judge) is the only way to make one, so
/// [`read`](Self::read) never opens a path whose kind has not been decided
/// first. The fields are private to this module, which is what makes that
/// true. It says nothing about code that opens a path without going through
/// this type, so a reader of a skeleton's files goes through it. The ordering
/// matters for a FIFO above all: opening one with nothing on its other end
/// blocks forever, which a skeleton directory must never be able to do to its
/// own render.
#[derive(Debug)]
pub(super) struct RegularFile<'path> {
    path: &'path Path,
    size_bytes: u64,
}

impl<'path> RegularFile<'path> {
    /// Decides, from `path`'s own [`std::fs::symlink_metadata`], whether
    /// `path` names a regular file.
    ///
    /// `symlink_metadata` is `lstat`: it reads the directory entry and never
    /// opens the file, so this cannot block. A symbolic link is refused rather
    /// than read through, and anything that is not a regular file is refused
    /// before it is ever opened.
    ///
    /// # Errors
    ///
    /// [`Reason::Unreadable`] when the metadata cannot be read,
    /// [`Reason::SymbolicLink`] for a symbolic link, and
    /// [`Reason::NotAFile`] for a directory, FIFO, socket or device.
    pub(super) fn judge(path: &'path Path) -> Result<Self, Reason> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|cause| Reason::Unreadable { cause })?;
        if metadata.is_symlink() {
            return Err(Reason::SymbolicLink);
        }
        if !metadata.is_file() {
            return Err(Reason::NotAFile);
        }
        Ok(Self {
            path,
            size_bytes: metadata.len(),
        })
    }

    /// Reads the file's bytes, reserving its declared size from `budget`
    /// before reading and reading through [`Read::take`](std::io::Read::take)
    /// so a file that grows between the size check and the read is still
    /// bounded to what was reserved.
    pub(super) fn read(self, budget: &mut ByteBudget) -> Result<Vec<u8>, Reason> {
        budget.reserve(self.size_bytes)?;

        let file = std::fs::File::open(self.path).map_err(|cause| Reason::Unreadable { cause })?;
        let mut buffer = Vec::new();
        file.take(self.size_bytes)
            .read_to_end(&mut buffer)
            .map_err(|cause| Reason::Unreadable { cause })?;

        // Postcondition: `Read::take(size_bytes)` never hands back more than
        // `size_bytes` bytes, which is exactly what was reserved above.
        assert!(
            buffer.len() as u64 <= self.size_bytes,
            "a bounded read never returns more than its own bound"
        );
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::{Reason, RegularFile};

    #[test]
    fn a_fifo_is_refused_from_its_own_metadata_without_being_opened() {
        // `RegularFile::judge` reads only `symlink_metadata`, which never
        // opens the file, so it has no way to block on a FIFO: a regression
        // that lets a FIFO through shows up as an `Ok` here rather than as a
        // hang.
        let directory = tempfile::tempdir().expect("scratch directory");
        let fifo_path = directory.path().join("pipe");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo_path)
            .status()
            .expect("run mkfifo");
        assert!(
            status.success(),
            "mkfifo must succeed for this test to mean anything"
        );

        let outcome = RegularFile::judge(&fifo_path);

        assert!(
            matches!(outcome, Err(Reason::NotAFile)),
            "a FIFO must be refused from its metadata alone, got {outcome:?}"
        );
    }
}
