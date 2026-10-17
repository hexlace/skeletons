//! Paths and files as the reader of a workspace without Cargo meets them.

use std::path::{Component, Path, PathBuf};

/// The file a package's manifest is always named.
pub(super) const MANIFEST_FILE_NAME: &str = "Cargo.toml";

/// The directory Cargo looks in for configuration, in every directory it walks and in the
/// user's home.
pub(super) const CARGO_DIRECTORY_NAME: &str = ".cargo";

/// The configuration file's name before Cargo had `.toml` on it. Cargo still reads it, and when
/// it exists Cargo uses it and ignores `config.toml` beside it.
pub(super) const LEGACY_CONFIGURATION_FILE_NAME: &str = "config";

/// The configuration file's name.
pub(super) const CONFIGURATION_FILE_NAME: &str = "config.toml";

/// `path` with every `.` removed and every `..` applied to the component before it, purely
/// lexically, as Cargo normalizes a path dependency's directory. A symlink is never followed and
/// nothing is read from the file system, so a `..` after a symlink still climbs the lexical parent.
pub(super) fn normalized(path: &Path) -> PathBuf {
    let mut kept: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                // The parent of the root is the root.
                Some(Component::RootDir) => {}
                Some(Component::ParentDir) | None => kept.push(component),
                Some(Component::CurDir | Component::Prefix(_)) => {
                    unreachable!("a current-directory or prefix component is never kept on Unix")
                }
            },
            Component::Normal(_) | Component::RootDir => kept.push(component),
            Component::Prefix(_) => unreachable!("a Unix path has no prefix component"),
        }
    }
    let result: PathBuf = kept.iter().collect();
    assert!(
        result.components().all(|part| part != Component::CurDir),
        "a normalized path holds no current-directory component"
    );
    // A `..` is kept only with nothing but `..` before it, so it is gone from an absolute path,
    // whose first component is the root.
    assert!(
        result
            .components()
            .skip_while(|part| *part == Component::ParentDir)
            .all(|part| part != Component::ParentDir),
        "a parent component survives only at the start of a path, before any other component"
    );
    if path.is_absolute() {
        assert!(
            result.is_absolute(),
            "the root is kept, so an absolute path stays absolute"
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::normalized;

    #[test]
    fn a_current_directory_component_is_removed() {
        assert_eq!(normalized(Path::new("/w/./a/./b")), Path::new("/w/a/b"));
    }

    #[test]
    fn a_parent_component_removes_the_one_before_it() {
        assert_eq!(normalized(Path::new("/w/root/../ci")), Path::new("/w/ci"));
        assert_eq!(normalized(Path::new("/w/a/b/../../c")), Path::new("/w/c"));
    }

    #[test]
    fn the_parent_of_the_root_is_the_root() {
        assert_eq!(normalized(Path::new("/../a")), Path::new("/a"));
    }

    #[test]
    fn a_relative_path_keeps_the_parents_it_cannot_apply() {
        assert_eq!(normalized(Path::new("../../a/./b")), Path::new("../../a/b"));
    }
}
