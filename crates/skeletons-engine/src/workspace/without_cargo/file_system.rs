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

/// Whether `directory` holds at least one Rust file or one subdirectory with a `main.rs`: how
/// Cargo discovers the targets of `src/bin`, `tests`, `examples` and `benches`.
///
/// A directory that cannot be read, and an entry in it that cannot be read, hold no target source:
/// Cargo discovers nothing from them either, and goes on to read the package without them.
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
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_directory_that_cannot_be_read_holds_no_target_source() {
        // `cargo metadata` lists no target from a `tests` it cannot read and still succeeds, so
        // the reader treats the directory as holding none. Takes every permission from a
        // directory holding a Rust file, then asks. A process that may read it anyway, such as
        // root, cannot show this, and the test says so.
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("a temporary directory");
        let tests = directory.path().join("tests");
        std::fs::create_dir(&tests).expect("a directory");
        std::fs::write(tests.join("t.rs"), "").expect("a file");
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o000))
            .expect("permissions taken away");

        let may_read_anyway = std::fs::read_dir(&tests).is_ok();
        let holds = holds_a_target_source(&tests);
        // Restored before any assertion, so a failure leaves nothing the temporary directory
        // cannot remove.
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o755))
            .expect("permissions given back");

        if may_read_anyway {
            eprintln!("skipped: this process can read a directory with no permissions");
            return;
        }
        assert!(!holds);
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
