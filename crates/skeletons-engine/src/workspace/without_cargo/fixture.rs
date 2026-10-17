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
    /// The temporary directory itself, a real path.
    pub(super) top: PathBuf,
    /// The tree's root, a real path.
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let top = directory.path().canonicalize().expect("a real path");
        let root = top.join("w");
        std::fs::create_dir(&root).expect("the root directory");
        Self {
            _directory: directory,
            top,
            root,
        }
    }

    /// Writes the file `relative`, under the temporary directory's root, with `text`, and says
    /// where it is, as a real path.
    pub(super) fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self
            .directory_above(relative)
            .join(Path::new(relative).file_name().expect("a file has a name"));
        std::fs::write(&path, text).expect("a file");
        path
    }

    /// Makes the directory `relative`, under the temporary directory's root, and says where it
    /// is, as a real path.
    pub(super) fn directory(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(&path).expect("a directory");
        path.canonicalize().expect("a real path")
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

    /// The directory that holds the file `relative`, made, as a real path.
    fn directory_above(&self, relative: &str) -> PathBuf {
        let parent = Path::new(relative).parent().expect("a file has a parent");
        self.directory(&parent.display().to_string())
    }
}

/// Runs `cargo generate-lockfile --offline` in `directory`, with the cargo that runs the tests,
/// and says what it printed on failure. Cargo gets a `HOME` that holds no configuration.
pub(super) fn lock_with_cargo_in(directory: &Path) -> Result<(), String> {
    let home = tempfile::tempdir().expect("a temporary directory");
    let output = lock_command(directory, home.path())
        .output()
        .map_err(|error| format!("cargo could not be run: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// The command [`lock_with_cargo_in`] runs: `cargo generate-lockfile --offline` in `directory`,
/// with `home` as `HOME`, so that Cargo's git reads no configuration of the user who runs the
/// tests.
fn lock_command(directory: &Path, home: &Path) -> std::process::Command {
    let mut command = crate::cargo::command();
    command
        .current_dir(directory)
        .args(["generate-lockfile", "--offline"])
        .env("HOME", home);
    command
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::Path;

    use super::lock_command;

    #[test]
    fn the_lock_command_has_a_home_of_its_own() {
        // Cargo reads git's configuration through `HOME`, so the command a test runs to lock a
        // workspace must not inherit the running user's. The command is built and inspected
        // without being run: its `HOME` must be the directory it was given.
        let command = lock_command(Path::new("/workspace"), Path::new("/home"));

        let environment: BTreeMap<&OsStr, Option<&OsStr>> = command.get_envs().collect();

        assert_eq!(
            environment.get(OsStr::new("HOME")),
            Some(&Some(OsStr::new("/home"))),
            "HOME is the directory it was given"
        );
    }
}
