//! Workspaces written into a temporary directory, for the tests of reading one without Cargo.

mod cargo_home;
mod git_repository;

use std::path::{Path, PathBuf};

pub(super) use cargo_home::CargoHome;
pub(super) use git_repository::GitRepository;

/// A temporary directory holding one test's packages, with a real path. The tree's root is the
/// directory `w` in it, so that a package can sit beside the root and still be inside the
/// temporary directory.
pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    /// The tree's root, a real path.
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("a real path")
            .join("w");
        std::fs::create_dir(&root).expect("the root directory");
        Self {
            _directory: directory,
            root,
        }
    }

    /// Writes the file `relative`, under the temporary directory's root, with `text`.
    pub(super) fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("a directory");
        std::fs::write(path, text).expect("a file");
    }

    /// A library package named `name` at `relative`, with `extra` after its `[package]` table.
    pub(super) fn package(&self, relative: &str, name: &str, extra: &str) {
        let directory = Path::new(relative);
        self.write(
            &directory.join("Cargo.toml").display().to_string(),
            &format!("[package]\nname = \"{name}\"\n{extra}"),
        );
        self.write(&directory.join("src/lib.rs").display().to_string(), "");
    }

    /// Runs `cargo generate-lockfile --offline` in the root, with the cargo that runs the tests,
    /// and says what it printed on failure.
    pub(super) fn lock_with_cargo(&self) -> Result<(), String> {
        lock_with_cargo_in(&self.root)
    }
}

/// Runs `cargo generate-lockfile --offline` in `directory`, with the cargo that runs the tests,
/// and says what it printed on failure.
pub(super) fn lock_with_cargo_in(directory: &Path) -> Result<(), String> {
    let output = crate::cargo::command()
        .current_dir(directory)
        .args(["generate-lockfile", "--offline"])
        .output()
        .map_err(|error| format!("cargo could not be run: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// What `cargo --version` prints, for a failure message to name the toolchain it ran on.
pub(super) fn cargo_version() -> String {
    crate::cargo::command()
        .arg("--version")
        .output()
        .map_or_else(
            |error| format!("cargo could not be run: {error}"),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )
}
