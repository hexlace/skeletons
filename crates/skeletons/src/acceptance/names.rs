//! Names that collide under case folding or Unicode normalisation, checked
//! per directory so a directory's own name is covered exactly like a file's,
//! are refused -- and `files/` and `partials/` are checked independently, so
//! the same name in each tree is never a collision at all.
//!
//! The two collisions this guards against most plainly -- `Dependabot.yml`
//! beside `dependabot.yml`, and an NFC `é.txt` beside an NFD `é.txt` --
//! cannot be committed as fixtures: a default macOS volume (APFS,
//! case-insensitive and normalisation-insensitive) collapses each pair to
//! one directory entry the moment it is written, so no checkout there could
//! hold the fixture. So each test here builds its skeleton at run time in a
//! temporary directory, after probing whether the filesystem it runs on can
//! hold both names; where it cannot, the test skips itself and asserts
//! nothing. End to end, the refusal is observed on a filesystem that keeps
//! the names apart, such as a typical Linux one; the `check_names` unit
//! tests in `skeleton/walk.rs` are handed a listing as text and so cover it
//! everywhere.

#[cfg(skeletons_checkout)]
use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

/// A fresh, uniquely named temporary directory for one test's skeleton, removed
/// by the caller once done. Named from the process id and an atomic counter,
/// not wall-clock time, so tests running in the same process never collide
/// even back to back.
fn scratch_directory() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "skeletons-names-test-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

/// Writes distinct marker bytes to `first` and `second` under `directory`,
/// then reads `first` back, to answer the one question that actually
/// matters: does writing `second` disturb `first` at all? A filesystem that
/// folds the two names together hands back `second`'s bytes when `first` is
/// read; a filesystem that keeps them apart hands back exactly what was
/// written to `first`. A read that fails, or returns anything else, is not
/// an answer about folding but a fault in the test's own scratch directory,
/// and fails the test rather than reading as a fold.
fn filesystem_keeps_these_names_apart(
    directory: &std::path::Path,
    first: &str,
    second: &str,
) -> bool {
    let first_path = directory.join(first);
    if let Some(parent) = first_path.parent() {
        std::fs::create_dir_all(parent).expect("create first's parent directory");
    }
    let second_path = directory.join(second);
    if let Some(parent) = second_path.parent() {
        std::fs::create_dir_all(parent).expect("create second's parent directory");
    }

    std::fs::write(&first_path, b"first-marker").expect("write the first name");
    std::fs::write(&second_path, b"second-marker").expect("write the second name");

    let read_back = std::fs::read(&first_path).expect("read the first name back");
    assert!(
        read_back == b"first-marker" || read_back == b"second-marker",
        "the first name read back as neither marker: {read_back:?}"
    );
    read_back == b"first-marker"
}

/// Writes a minimal skeleton manifest -- no options at all, since this test's
/// refusal is a property of the directory walk, well before any option
/// schema is linked against a partial mapping.
fn write_minimal_manifest(skeleton_directory: &std::path::Path, name: &str) {
    std::fs::write(
        skeleton_directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[package.metadata.skeletons]\n"
        ),
    )
    .expect("write Cargo.toml");
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn two_file_names_equal_under_case_folding_in_the_same_directory_are_refused_naming_both() {
    // The plainest collision: `Dependabot.yml` and
    // `dependabot.yml` in the same `files/` directory. `D` (0x44) sorts
    // before `d` (0x64), so in path order `files/Dependabot.yml` is the
    // first of the two -- that is what `error.file()` must report -- and
    // `files/dependabot.yml` is the other, which the reason must carry.
    let skeleton_directory = scratch_directory();
    write_minimal_manifest(&skeleton_directory, "case-fold-collision");
    let files = skeleton_directory.join("files");
    std::fs::create_dir_all(&files).expect("create files/");

    if !filesystem_keeps_these_names_apart(&files, "Dependabot.yml", "dependabot.yml") {
        // This filesystem already folds the two names into one
        // directory entry, exactly like the description above says a
        // default APFS volume does -- there is no skeleton left here to render,
        // so there is nothing this test can assert. Confirmed by hand with
        // `ls`: only one of the two names survives the writes above.
        std::fs::remove_dir_all(&skeleton_directory).expect("clean up scratch directory");
        eprintln!(
            "skipped: this filesystem folds `Dependabot.yml` and `dependabot.yml` into one name"
        );
        return;
    }

    let error = render(&skeleton_directory, &Choices::new())
        .expect_err("two names equal under case folding in one directory must be refused");

    assert_eq!(error.file(), Some("files/Dependabot.yml"));
    assert!(
        matches!(
            error.reason(),
            Reason::NamesCollide { other } if other == "files/dependabot.yml"
        ),
        "expected a names-collide refusal naming files/dependabot.yml as the other name, got {error:?}"
    );

    std::fs::remove_dir_all(&skeleton_directory).expect("clean up scratch directory");
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn two_directory_names_equal_under_case_folding_collide_exactly_like_two_file_names() {
    // Checking per directory covers directories too: `files/A/x.yml` and
    // `files/a/y.yml` collide at `A`/`a`, the two entries `files/` itself
    // lists -- not at the deeper `x.yml`/`y.yml`, which never even get
    // compared against each other. `A` (0x41) sorts before `a` (0x61), so
    // `files/A` is the first of the two in path order.
    let skeleton_directory = scratch_directory();

    // Probed in a directory of its own, with the *same* leaf filename
    // written through each candidate directory name -- so a filesystem
    // that folds `A` and `a` together makes the second write overwrite the
    // first, exactly like the file-name probe above, and never leaves a
    // stray marker file sitting inside the skeleton's own files/ tree once the
    // probe directory is discarded.
    let probe_directory = skeleton_directory.join("probe");
    std::fs::create_dir_all(&probe_directory).expect("create probe directory");
    let names_are_kept_apart =
        filesystem_keeps_these_names_apart(&probe_directory, "A/marker.txt", "a/marker.txt");
    std::fs::remove_dir_all(&probe_directory).expect("clean up probe directory");

    if !names_are_kept_apart {
        // Same limitation as the file-name case above, for a directory name
        // instead of a file name: this filesystem cannot hold `A` and `a`
        // as two separate entries of the same directory, so there is nothing
        // built here to render, and nothing to assert.
        std::fs::remove_dir_all(&skeleton_directory).expect("clean up scratch directory");
        eprintln!("skipped: this filesystem folds directories `A` and `a` into one name");
        return;
    }

    write_minimal_manifest(&skeleton_directory, "case-fold-directory-collision");
    let files = skeleton_directory.join("files");
    std::fs::create_dir_all(files.join("A")).expect("create files/A");
    std::fs::create_dir_all(files.join("a")).expect("create files/a");
    std::fs::write(files.join("A").join("x.yml"), b"x: yes\n").expect("write files/A/x.yml");
    std::fs::write(files.join("a").join("y.yml"), b"y: yes\n").expect("write files/a/y.yml");

    let error = render(&skeleton_directory, &Choices::new())
        .expect_err("two directory names equal under case folding must be refused");

    assert_eq!(error.file(), Some("files/A"));
    assert!(
        matches!(
            error.reason(),
            Reason::NamesCollide { other } if other == "files/a"
        ),
        "expected a names-collide refusal naming files/a as the other name, got {error:?}"
    );

    std::fs::remove_dir_all(&skeleton_directory).expect("clean up scratch directory");
}

#[cfg(skeletons_checkout)]
#[test]
fn the_same_name_in_files_and_partials_is_not_a_collision_at_all() {
    // `files/x` and `partials/x` never collide -- the two trees are checked
    // independently. Unlike the two tests above, this is a real, committed
    // skeleton: `files/settings.yml` and `partials/settings.yml` are the exact
    // same name in each tree, and both must render.
    let rendering = render(
        test_skeleton("renders/files-and-partials-share-a-name"),
        &Choices::new(),
    )
    .expect("the same name in files/ and partials/ must not be treated as a collision");

    assert_eq!(
        rendering.get("settings.yml"),
        Some(b"name: same\n".as_slice()),
        "files/settings.yml must render on its own"
    );
    assert_eq!(
        rendering.get("ci.yml"),
        Some(b"jobs:\nworkflow: yes\n".as_slice()),
        "the directive must still insert partials/settings.yml, the same name in the other tree"
    );
}
