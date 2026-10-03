//! No tracked Rust source line is wider than the limit `rustfmt.toml` sets.
//!
//! `rustfmt` wraps code to `max_width`, but it leaves a string literal or a
//! comment alone when wrapping it would change what it says, so a line of
//! either can stand over the limit while `cargo fmt --check` passes. This test
//! is the check that notices: it lists every tracked `.rs` file through
//! `git ls-files`, and fails naming each line over the limit as
//! `file:line (N columns)`.
//!
//! Columns are characters (Unicode scalar values), not bytes, because the
//! limit is about how wide a line reads and several lines hold non-ASCII text
//! on purpose.
//!
//! `crates/skeletons/test-skeletons/` is skipped, a deliberate deviation from
//! `TS-LINE-LIMIT` ("no exceptions"). Those crates are fixture data whose
//! exact bytes are what the tests check (a hidden-marker case holds invisible
//! characters, for instance), so their lines are whatever the case under test
//! needs, and re-wrapping one would change what is tested.
//!
//! The files come from `git`, run at the repository root, so a checkout is
//! required. `xtask` is never published (`publish = false`), so it is only
//! ever built from a checkout; when `git` cannot list the files this test
//! fails and says why, because a guard that found no files to read would pass
//! having checked nothing.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The directory whose lines are exempt from `TS-LINE-LIMIT`, relative to the
/// repository root; the module documentation says why.
const FIXTURE_DIRECTORY: &str = "crates/skeletons/test-skeletons/";

/// The repository root: `xtask` sits directly under it.
fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask's manifest directory has no parent")?;
    Ok(root.to_path_buf())
}

/// The `max_width` that `rustfmt.toml` sets, read from there so that the limit
/// is stated in one place.
fn width_limit(root: &Path) -> Result<usize, Box<dyn Error>> {
    let text = std::fs::read_to_string(root.join("rustfmt.toml"))?;
    let document: toml_edit::DocumentMut = text.parse()?;
    let limit = document
        .get("max_width")
        .and_then(toml_edit::Item::as_integer)
        .ok_or("rustfmt.toml does not set an integer `max_width`")?;
    Ok(usize::try_from(limit)?)
}

/// Every tracked `.rs` file outside the fixture directory, as a path relative
/// to the repository root, in the order `git` lists them.
fn tracked_rust_files(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .map_err(|error| {
            format!(
                "could not run `git ls-files` in {}: {error}",
                root.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "`git ls-files` failed in {} ({}): {}",
            root.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    // A name that is not UTF-8 cannot be reported as `file:line`, and the
    // repository does not hold one, so it is passed over rather than handled.
    let files = output
        .stdout
        .split(|&byte| byte == 0)
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter(|name| !name.is_empty() && !name.starts_with(FIXTURE_DIRECTORY))
        .map(str::to_owned)
        .collect();
    Ok(files)
}

/// The 1-based line number and width in characters of every line of `text`
/// wider than `limit`.
fn lines_over(limit: usize, text: &str) -> Vec<(usize, usize)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.chars().count()))
        .filter(|&(_, columns)| columns > limit)
        .collect()
}

/// One `file:line (N columns)` entry per line over `limit` in the tracked
/// files under `root`, plus how many files were read.
fn offenders(root: &Path, limit: usize) -> Result<(Vec<String>, usize), Box<dyn Error>> {
    let files = tracked_rust_files(root)?;
    let mut found = Vec::new();
    let mut read = 0;
    for file in &files {
        let text = match std::fs::read_to_string(root.join(file)) {
            Ok(text) => text,
            // Listed by git but removed from the working tree: there are no
            // lines to be wide.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("could not read {file}: {error}").into()),
        };
        read += 1;
        for (line, columns) in lines_over(limit, &text) {
            found.push(format!("{file}:{line} ({columns} columns)"));
        }
    }
    Ok((found, read))
}

/// Reads every tracked source file and requires that none holds a line wider
/// than `rustfmt.toml`'s `max_width`. The count of files read must be
/// non-zero, so that a listing that came back empty cannot pass as clean.
#[test]
fn no_tracked_rust_line_is_wider_than_the_rustfmt_limit() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let limit = width_limit(&root)?;

    let (found, read) = offenders(&root, limit)?;

    assert!(
        read > 0,
        "`git ls-files` listed no readable .rs files in {}",
        root.display()
    );
    assert!(
        found.is_empty(),
        "{} line(s) wider than {limit} columns (rustfmt.toml `max_width`); rustfmt cannot wrap \
         string literals or comments, so wrap these by hand (a `\\` continuation keeps a \
         literal's value):\n{}",
        found.len(),
        found.join("\n")
    );
    Ok(())
}

/// The limit is read from `rustfmt.toml` rather than restated, so this pins
/// that the file still says 100 and that the reading finds it.
#[test]
fn the_limit_is_read_from_rustfmt_toml() -> Result<(), Box<dyn Error>> {
    assert_eq!(width_limit(&repository_root()?)?, 100);
    Ok(())
}

/// A line of exactly the limit passes and one character more fails, counting
/// ASCII, where characters and bytes agree.
#[test]
fn a_line_is_over_only_past_the_limit_in_ascii() {
    assert_eq!(lines_over(100, &"a".repeat(100)), []);
    assert_eq!(lines_over(100, &"a".repeat(101)), [(1, 101)]);
}

/// Columns are characters, not bytes: 100 two-byte characters are 200 bytes
/// and still pass, 101 do not, and a four-byte character counts once.
#[test]
fn columns_count_characters_not_bytes() {
    let exactly_the_limit = "é".repeat(100);
    assert_eq!(exactly_the_limit.len(), 200);
    assert_eq!(lines_over(100, &exactly_the_limit), []);
    assert_eq!(lines_over(100, &"é".repeat(101)), [(1, 101)]);

    let astral = format!("{}{}", "a".repeat(99), "\u{1F600}");
    assert_eq!(astral.len(), 103);
    assert_eq!(lines_over(100, &astral), []);
}

/// Each offending line is reported at its own 1-based number, and a short
/// line between two long ones is not reported.
#[test]
fn every_offending_line_is_reported_at_its_own_number() {
    let text = format!("{}\nshort\n{}\n", "a".repeat(101), "b".repeat(120));
    assert_eq!(lines_over(100, &text), [(1, 101), (3, 120)]);
}

/// The fixture directory is skipped and nothing else is: no listed file is
/// under it, and the listing is not empty.
#[test]
fn the_fixture_directory_is_not_listed() -> Result<(), Box<dyn Error>> {
    let files = tracked_rust_files(&repository_root()?)?;
    assert!(!files.is_empty(), "no tracked .rs files were listed");
    assert!(
        files
            .iter()
            .all(|file| !file.starts_with(FIXTURE_DIRECTORY)),
        "a fixture file was listed"
    );
    assert!(
        files.iter().any(|file| file == "xtask/src/main.rs"),
        "the listing must reach files below the root, and xtask/src/main.rs is one"
    );
    Ok(())
}
