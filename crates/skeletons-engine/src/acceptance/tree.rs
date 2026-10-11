//! Tree: every file under `files/` is rendered, keyed by its path relative
//! to `files/`, at any depth and including dotfiles and dot-directories, in
//! an order that never depends on how the filesystem listed them. A symlink
//! anywhere under `files/` or `partials/` is refused outright, and a file
//! that cannot be decoded as UTF-8 text is refused rather than copied
//! through unexamined, unless it is declared verbatim.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

#[test]
fn every_file_under_files_is_rendered_keyed_by_its_relative_path_including_dotfiles() {
    // The render yields every file under `files/`, at any depth and
    // including files and directories whose names begin with `.` (here,
    // `.github/dependabot.yml`), keyed by its path relative to `files/`, in
    // an order that does not depend on the order the filesystem lists them.
    // `Rendering::iter` is documented to
    // yield ascending byte order of the path string regardless of
    // filesystem order, which is exercised here directly against a tree
    // with a dot-directory, a plain file, and a nested file.
    let rendering = render(test_skeleton("renders/nested-dotfiles"), &Choices::new())
        .expect("a skeleton with a nested tree including a dot-directory must render");

    assert_eq!(
        rendering.get(".github/dependabot.yml"),
        Some(b"dependabot: true\n".as_slice()),
        "a file under a dot-directory must be rendered and reachable by its relative path"
    );
    assert_eq!(
        rendering.get("root.yml"),
        Some(b"root-file: yes\n".as_slice()),
        "a top-level file must be rendered"
    );
    assert_eq!(
        rendering.get("a/b/deep.yml"),
        Some(b"nested: yes\n".as_slice()),
        "a file nested several directories deep must be rendered"
    );

    let paths: Vec<&str> = rendering.iter().map(|(path, _bytes)| path).collect();
    assert_eq!(
        paths,
        vec![".github/dependabot.yml", "a/b/deep.yml", "root.yml"],
        "iteration order must be ascending byte order of the path string, not filesystem order"
    );
}

#[test]
fn a_symbolic_link_file_under_files_is_refused_naming_it() {
    // A file or directory under `files/` or `partials/` that is a symbolic
    // link is refused, naming it: a skeleton is the bytes it ships, and a link
    // is a pointer to bytes it does not. `files/link.yml` is a link pointing
    // at `target.txt`.
    let error = render(
        test_skeleton("refused/symlink-file-in-files"),
        &Choices::new(),
    )
    .expect_err("a symlinked file under files/ must be refused");

    assert_eq!(error.file(), Some("files/link.yml"));
    assert!(
        matches!(error.reason(), Reason::SymbolicLink),
        "expected a symbolic-link refusal, got {error:?}"
    );
}

#[test]
fn a_symbolic_link_directory_under_files_is_refused_naming_it() {
    // Same rule as above, for a symlinked directory rather than a
    // symlinked file. `files/linked-dir` is a link pointing at `real-dir`.
    let error = render(
        test_skeleton("refused/symlink-dir-in-files"),
        &Choices::new(),
    )
    .expect_err("a symlinked directory under files/ must be refused");

    assert_eq!(error.file(), Some("files/linked-dir"));
    assert!(
        matches!(error.reason(), Reason::SymbolicLink),
        "expected a symbolic-link refusal, got {error:?}"
    );
}

#[test]
fn a_symbolic_link_under_partials_is_refused_naming_it() {
    // Same refusal, the `partials/` half of the rule above (the `files/`
    // half is covered there). The skeleton's one set value maps to
    // `lint.yml`, which is itself a symlink to a sibling `real.yml`.
    // `partials/lint.yml` is a link pointing at `real.yml`.
    let error = render(test_skeleton("refused/symlink-partial"), &Choices::new())
        .expect_err("a symlinked partial must be refused");

    assert_eq!(error.file(), Some("partials/lint.yml"));
    assert!(
        matches!(error.reason(), Reason::SymbolicLink),
        "expected a symbolic-link refusal, got {error:?}"
    );
}

#[test]
fn a_file_that_cannot_be_read_as_text_is_refused_naming_the_skeleton_and_file() {
    // A skeleton file that cannot be read as text is refused, naming the skeleton
    // and the file, rather than being copied through unexamined. The fixture
    // contains the bytes `377 376` (0xFF 0xFE), which is not valid UTF-8 in
    // any position.
    let error = render(test_skeleton("refused/not-utf8-file"), &Choices::new())
        .expect_err("a file that is not valid UTF-8 must be refused");

    assert_eq!(
        error.skeleton(),
        &crate::skeleton::SkeletonIdentity::Named("not-utf8-file".to_owned())
    );
    assert_eq!(error.file(), Some("files/binary.yml"));
    assert!(
        matches!(error.reason(), Reason::NotUtf8),
        "expected a not-UTF-8 refusal, got {error:?}"
    );
}
