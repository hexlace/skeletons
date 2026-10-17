//! Paths and files as the reader of a workspace without Cargo meets them.

use std::path::{Component, Path, PathBuf};

/// The file a package's manifest is always named.
pub(super) const MANIFEST_FILE_NAME: &str = "Cargo.toml";

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
    result
}

/// Whether `directory` holds at least one Rust file or one subdirectory with a `main.rs`: how
/// Cargo discovers the targets of `src/bin`, `tests`, `examples` and `benches`.
pub(super) fn holds_a_target_source(directory: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            path.join("main.rs").is_file()
        } else {
            path.extension().is_some_and(|extension| extension == "rs")
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{holds_a_target_source, normalized};

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

    #[test]
    fn a_directory_that_does_not_exist_holds_no_target_source() {
        let directory = tempfile::tempdir().expect("a temporary directory");

        assert!(!holds_a_target_source(&directory.path().join("absent")));
    }

    #[test]
    fn a_rust_file_or_a_directory_with_a_main_is_a_target_source() {
        // Cargo discovers `tests/x.rs` and `tests/x/main.rs` alike, and nothing else.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        std::fs::write(root.join("readme.md"), "").expect("a file");
        std::fs::create_dir(root.join("empty")).expect("a directory");
        assert!(!holds_a_target_source(root));

        std::fs::write(root.join("empty").join("lib.rs"), "").expect("a file");
        assert!(!holds_a_target_source(root), "only a main.rs counts below");

        std::fs::write(root.join("empty").join("main.rs"), "").expect("a file");
        assert!(holds_a_target_source(root));
    }

    #[test]
    fn a_top_level_rust_file_is_a_target_source() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(directory.path().join("t.rs"), "").expect("a file");

        assert!(holds_a_target_source(directory.path()));
    }
}
