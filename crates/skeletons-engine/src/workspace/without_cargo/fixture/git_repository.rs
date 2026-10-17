//! A git repository holding one package, for a test to depend on through a `file://` source.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// The variables removed from the environment of every git command: the ones that point git at a
/// configuration file or value, or at a repository, other than the fixture's own.
const REMOVED_VARIABLES: [&str; 6] = [
    "XDG_CONFIG_HOME",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
];

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

    /// A git command in the repository that takes nothing from the environment of the process
    /// that runs it: a `HOME` of its own, the global configuration the empty file `/dev/null`,
    /// and no system configuration, configuration directory, configuration value or repository
    /// location, each of which would bring in the machine's settings or another repository.
    fn command(&self) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(self.path())
            .env("HOME", self.home.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for variable in REMOVED_VARIABLES {
            command.env_remove(variable);
        }
        command
    }

    /// Runs git in the repository with `arguments`, and gives what it printed.
    fn git(&self, arguments: &[&str]) -> String {
        let output = self
            .command()
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;

    use super::GitRepository;

    #[test]
    fn the_git_command_takes_no_configuration_or_repository_from_the_environment() {
        // The command is built and inspected without being run, so the test does not depend on
        // what this process's environment holds: each variable that would bring in a
        // configuration file, a configuration value or another repository must be recorded as
        // removed (`None`), and the global configuration as the empty file `/dev/null`, so that
        // no `~/.gitconfig` or `$XDG_CONFIG_HOME/git/config` can be read.
        let repository = GitRepository::new("p");

        let command = repository.command();
        let environment: BTreeMap<&OsStr, Option<&OsStr>> = command.get_envs().collect();

        for removed in [
            "XDG_CONFIG_HOME",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_COUNT",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
        ] {
            assert_eq!(
                environment.get(OsStr::new(removed)),
                Some(&None),
                "{removed} must be removed from the environment of git"
            );
        }
        assert_eq!(
            environment.get(OsStr::new("GIT_CONFIG_GLOBAL")),
            Some(&Some(OsStr::new("/dev/null"))),
            "the global configuration is the empty file"
        );
        assert_eq!(
            environment.get(OsStr::new("GIT_CONFIG_NOSYSTEM")),
            Some(&Some(OsStr::new("1"))),
            "the system configuration is not read"
        );
    }
}
