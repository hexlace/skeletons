//! A git repository holding one package, for a test to depend on through a `file://` source.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// A git repository with one package in it, in the shape the git source cases need: the branch
/// `main` at the first commit, the tag `v1` on it, and the branch `dev` one commit ahead.
///
/// It is isolated the way the repositories of `sync/test_repository.rs` are, and without the
/// `crate::git` that file builds on, which these tests cannot reach without the `commands`
/// feature: a `HOME` of its own, no system configuration, and no signing, maintenance or
/// identity taken from the machine.
pub(in crate::workspace::without_cargo) struct GitRepository {
    directory: TempDir,
    /// A `HOME` that holds no configuration.
    home: TempDir,
}

impl GitRepository {
    /// A repository of the package `package` at version 1.0.0.
    pub(in crate::workspace::without_cargo) fn new(package: &str) -> Self {
        let repository = Self {
            directory: tempfile::tempdir().expect("a temporary directory"),
            home: tempfile::tempdir().expect("a temporary directory"),
        };
        repository.git(&["init", "--quiet", "--initial-branch=main"]);
        repository.write(
            "Cargo.toml",
            &format!("[package]\nname = \"{package}\"\nversion = \"1.0.0\"\n"),
        );
        repository.write("src/lib.rs", "");
        repository.commit("one");
        repository.git(&["tag", "v1"]);
        repository.git(&["checkout", "--quiet", "-b", "dev"]);
        repository.write("src/lib.rs", "// two\n");
        repository.commit("two");
        repository.git(&["checkout", "--quiet", "main"]);
        repository
    }

    /// The URL a dependency names to use this repository as a git source.
    pub(in crate::workspace::without_cargo) fn url(&self) -> String {
        format!("file://{}", self.path().display())
    }

    /// The full commit that `reference` names.
    pub(in crate::workspace::without_cargo) fn revision(&self, reference: &str) -> String {
        self.git(&["rev-parse", reference]).trim().to_owned()
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, relative: &str, text: &str) {
        let path: PathBuf = self.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("a directory");
        std::fs::write(path, text).expect("a file");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
    }

    /// Runs git in the repository with `arguments`, and gives what it printed.
    fn git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(self.path())
            .env("HOME", self.home.path())
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_CONFIG_SYSTEM")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                // A background maintenance process would change the repository under a test
                // that believes it is quiescent.
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgSign=false",
                "-c",
                "user.email=skeletons-test@example.invalid",
                "-c",
                "user.name=Skeletons Test",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(arguments)
            .output()
            .expect("git can be run");
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}
