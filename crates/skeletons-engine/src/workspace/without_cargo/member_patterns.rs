//! Expanding a workspace's `members` entries into directories, as Cargo does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::error::ReadWithoutCargoError;
use super::file_system::normalized;

/// The directories `patterns`, a workspace's `members`, name under `root`, sorted and without
/// repeats.
///
/// Each entry is a glob (`*`, `?`, `[…]`, `**`) relative to the root; the root's own path is
/// matched literally, whatever characters it holds. Only directories are kept, so a file beside
/// them is passed over, and a directory is kept whether or not its name is hidden. An entry that
/// matches nothing is kept as the literal path it names, so a member that does not exist is
/// found missing when its manifest is read, as Cargo finds it.
///
/// # Errors
///
/// Unreadable for an entry that is not a glob, for a match that cannot be read, and for a root
/// that is not UTF-8.
pub(super) fn expand(
    root: &Path,
    patterns: &[String],
) -> Result<Vec<PathBuf>, ReadWithoutCargoError> {
    let Some(root_text) = root.to_str() else {
        return Err(ReadWithoutCargoError::unreadable(format!(
            "{} is not valid UTF-8, which a members glob needs",
            root.display()
        )));
    };
    let escaped_root = glob::Pattern::escape(root_text);

    let mut directories = BTreeSet::new();
    for pattern in patterns {
        let matches = glob::glob(&format!("{escaped_root}/{pattern}")).map_err(|error| {
            ReadWithoutCargoError::unreadable(format!(
                "members entry `{pattern}` is not a valid glob: {error}"
            ))
        })?;
        let mut matched_anything = false;
        for found in matches {
            let path = found.map_err(|error| {
                ReadWithoutCargoError::unreadable(format!(
                    "members entry `{pattern}` could not be read: {error}"
                ))
            })?;
            matched_anything = true;
            if path.is_dir() {
                directories.insert(normalized(&path));
            }
        }
        if !matched_anything {
            directories.insert(normalized(&root.join(pattern)));
        }
    }
    Ok(directories.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::expand;
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;

    /// A real root holding the directories `crates/a`, `crates/.hidden` and `crates/b`, and the
    /// file `crates/file`.
    fn root() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().canonicalize().expect("a real path");
        for name in ["a", ".hidden", "b"] {
            std::fs::create_dir_all(root.join("crates").join(name)).expect("a directory");
        }
        std::fs::write(root.join("crates/file"), "").expect("a file");
        (directory, root)
    }

    #[test]
    fn a_glob_keeps_directories_hidden_ones_included_and_skips_a_file() {
        let (_directory, root) = root();

        let found = expand(&root, &["crates/*".to_owned()]).expect("a glob");

        assert_eq!(
            found,
            [
                root.join("crates/.hidden"),
                root.join("crates/a"),
                root.join("crates/b")
            ]
        );
    }

    #[test]
    fn entries_naming_one_directory_twice_give_it_once() {
        let (_directory, root) = root();

        let found =
            expand(&root, &["crates/a".to_owned(), "crates/*".to_owned()]).expect("two entries");

        assert_eq!(found.iter().filter(|path| path.ends_with("a")).count(), 1);
    }

    #[test]
    fn an_entry_matching_nothing_is_kept_as_the_literal_path() {
        let (_directory, root) = root();

        assert_eq!(
            expand(&root, &["none/*".to_owned()]).expect("an entry"),
            [root.join("none/*")]
        );
        assert_eq!(
            expand(&root, &["./crates/../crates/a".to_owned()]).expect("a literal"),
            [root.join("crates/a")]
        );
    }

    #[test]
    fn a_root_holding_glob_characters_is_matched_literally() {
        // A root named like a glob must not be read as one: its brackets are part of its name.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("a real path")
            .join("[ab]*");
        std::fs::create_dir_all(root.join("m")).expect("a directory");

        assert_eq!(
            expand(&root, &["*".to_owned()]).expect("a glob"),
            [root.join("m")]
        );
    }

    #[test]
    fn an_entry_that_is_not_a_glob_is_unreadable() {
        let (_directory, root) = root();

        let error = expand(&root, &["crates/[".to_owned()]).expect_err("an open bracket");

        assert!(
            matches!(error, ReadWithoutCargoError::ManifestUnreadable { .. }),
            "{error:?}"
        );
    }
}
