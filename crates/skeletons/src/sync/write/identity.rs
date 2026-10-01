//! Which file or directory an entry is, told apart from what merely has its
//! name: the device and inode it was created as.
//!
//! A path names whatever is at it now. `sync` removes only what it created,
//! so it records what each thing it creates *is* at the moment it creates it
//! and compares that with what it finds at the path when it comes to remove
//! it. Two entries with the same device and inode are one file; a different
//! file at the same path, or a directory with the same name, is not.

use std::os::unix::fs::MetadataExt as _;

/// The device and inode of one file or directory: what identifies it, apart
/// from its name, for as long as it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    /// The identity `metadata` describes. Read from the open handle for a
    /// file `sync` has just created, and from `symlink_metadata` for a
    /// directory it has just created, so it is the thing `sync` made and
    /// never a path looked up again.
    pub(super) fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::FileIdentity;

    fn identity_of(path: &std::path::Path) -> FileIdentity {
        FileIdentity::of(&std::fs::symlink_metadata(path).expect("metadata"))
    }

    #[test]
    fn a_file_is_the_same_file_under_another_name_and_not_the_same_as_a_copy() {
        let root = TempDir::new().expect("scratch directory");
        let original = root.path().join("original");
        std::fs::write(&original, b"bytes").expect("the file");
        std::fs::hard_link(&original, root.path().join("second-name")).expect("a second name");
        std::fs::write(root.path().join("copy"), b"bytes").expect("a copy");

        let identity = identity_of(&original);

        assert_eq!(identity, identity_of(&root.path().join("second-name")));
        assert_ne!(identity, identity_of(&root.path().join("copy")));
    }

    #[test]
    fn a_file_replaced_by_another_at_the_same_path_has_another_identity() {
        let root = TempDir::new().expect("scratch directory");
        let path = root.path().join("thing");
        std::fs::write(&path, b"first").expect("the file");
        // The first file stays alive under another name, so the second
        // cannot be given its inode back.
        std::fs::rename(&path, root.path().join("moved")).expect("move it away");
        let first = identity_of(&root.path().join("moved"));
        std::fs::write(&path, b"second").expect("another file at the path");

        assert_ne!(first, identity_of(&path));
    }
}
