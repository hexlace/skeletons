//! The targets Cargo infers from the files beside a manifest: what is in `src/bin`, `tests`,
//! `examples` and `benches`, and `src/main.rs`.

use std::path::{Path, PathBuf};

use super::super::package_manifest::TargetKind;

/// Where Cargo looks for the one binary named for the package.
const MAIN_BINARY_PATH: &str = "src/main.rs";

/// A target Cargo finds in the files: the name it gets, and its path from the package's
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Inferred {
    pub(super) name: String,
    pub(super) path: PathBuf,
}

/// The directory Cargo infers the targets of `kind` from, relative to the package.
fn directory_of(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Binary => "src/bin",
        TargetKind::Test => "tests",
        TargetKind::Example => "examples",
        TargetKind::Benchmark => "benches",
        TargetKind::Library => {
            unreachable!("a library is inferred from src/lib.rs, not a directory")
        }
    }
}

/// Every target of `kind` Cargo infers in the package at `directory`.
///
/// A Rust file in the kind's directory is a target named for its stem, and a subdirectory with a
/// `main.rs` is one named for the subdirectory. A name that starts with a dot is skipped, and so
/// is one that is not UTF-8. For binaries, `src/main.rs` is a target named for the package.
/// A directory that cannot be read holds none: Cargo goes on without it.
pub(super) fn inferred(directory: &Path, kind: TargetKind, package_name: &str) -> Vec<Inferred> {
    let mut found = Vec::new();
    if kind == TargetKind::Binary && directory.join(MAIN_BINARY_PATH).exists() {
        found.push(Inferred {
            name: package_name.to_owned(),
            path: PathBuf::from(MAIN_BINARY_PATH),
        });
    }

    let relative = Path::new(directory_of(kind));
    let Ok(entries) = std::fs::read_dir(directory.join(relative)) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }
        let is_directory = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        let path = relative.join(&file_name);
        if is_directory {
            if directory.join(&path).join("main.rs").exists() {
                found.push(Inferred {
                    name: file_name,
                    path: path.join("main.rs"),
                });
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let stem = path.file_stem().and_then(|stem| stem.to_str());
            if let Some(stem) = stem {
                found.push(Inferred {
                    name: stem.to_owned(),
                    path: path.clone(),
                });
            }
        }
    }
    // The main binary's path is a constant ending in `.rs`, a stem target was kept only for its
    // `.rs` extension, and a subdirectory's is `main.rs`, so no file name can break this.
    for target in &found {
        assert!(
            target
                .path
                .extension()
                .is_some_and(|extension| extension == "rs"),
            "an inferred target is a Rust file: {}",
            target.path.display()
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{TargetKind, inferred};

    /// Writes empty `files` under a new directory and infers the targets of `kind` there for the
    /// package `p`, as `(name, path)` sorted by name.
    fn infer(kind: TargetKind, files: &[&str]) -> Vec<(String, PathBuf)> {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for file in files {
            let path = directory.path().join(file);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("a directory");
            std::fs::write(path, "").expect("a file");
        }
        let mut found: Vec<_> = inferred(directory.path(), kind, "p")
            .into_iter()
            .map(|target| (target.name, target.path))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_rust_file_or_a_directory_with_a_main_is_a_target() {
        // Cargo infers `tests/x.rs` and `tests/y/main.rs` alike, and nothing else.
        let found = infer(
            TargetKind::Test,
            &[
                "tests/x.rs",
                "tests/y/main.rs",
                "tests/z/lib.rs",
                "tests/readme.md",
            ],
        );

        assert_eq!(
            found,
            [
                ("x".to_owned(), PathBuf::from("tests/x.rs")),
                ("y".to_owned(), PathBuf::from("tests/y/main.rs")),
            ]
        );
    }

    #[test]
    fn each_kind_is_inferred_from_its_own_directory() {
        let files = [
            "src/bin/b.rs",
            "tests/t.rs",
            "examples/e.rs",
            "benches/n.rs",
        ];

        assert_eq!(infer(TargetKind::Binary, &files).len(), 1);
        assert_eq!(infer(TargetKind::Test, &files)[0].0, "t");
        assert_eq!(infer(TargetKind::Example, &files)[0].0, "e");
        assert_eq!(infer(TargetKind::Benchmark, &files)[0].0, "n");
    }

    #[test]
    fn src_main_is_a_binary_named_for_the_package() {
        assert_eq!(
            infer(TargetKind::Binary, &["src/main.rs"]),
            [("p".to_owned(), PathBuf::from("src/main.rs"))]
        );
        assert_eq!(infer(TargetKind::Test, &["src/main.rs"]), []);
    }

    #[test]
    fn a_name_that_starts_with_a_dot_is_skipped() {
        let found = infer(
            TargetKind::Test,
            &["tests/.hidden.rs", "tests/.dir/main.rs"],
        );

        assert_eq!(found, []);
    }

    #[test]
    fn a_directory_that_does_not_exist_holds_no_target() {
        assert_eq!(infer(TargetKind::Example, &["readme.md"]), []);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_directory_that_cannot_be_read_holds_no_target() {
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
        let found = inferred(directory.path(), TargetKind::Test, "p");
        // Restored before any assertion, so a failure leaves nothing the temporary directory
        // cannot remove.
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o755))
            .expect("permissions given back");

        if may_read_anyway {
            eprintln!("skipped: this process can read a directory with no permissions");
            return;
        }
        assert!(found.is_empty());
    }
}
