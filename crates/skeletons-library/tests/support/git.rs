//! A skeleton committed to a git repository the test creates, reached only
//! over a `file://` URL, so a git dependency on it needs no network.
//!
//! Git runs with a `HOME` of the test's own and no system configuration, never
//! signs, never starts background maintenance, and commits under a fixture
//! identity. Variables that redirect which repository git answers about are
//! removed, so the result is the same run from a shell as from inside a git
//! hook, which exports them.

use std::error::Error;
use std::path::Path;
use std::process::Command;

use super::require_success;

/// The variables that redirect which repository, work tree, index or object
/// store git answers about.
const REDIRECTING_VARIABLES: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_ATTR_SOURCE",
];

/// Makes `command` answer about nothing but the directory it is run in:
/// no redirecting variable, no global or system git configuration.
pub(crate) fn isolate(command: &mut Command) {
    for variable in REDIRECTING_VARIABLES {
        command.env_remove(variable);
    }
    command
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

fn run(directory: &Path, home: &Path, arguments: &[&str]) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("git");
    command
        .current_dir(directory)
        .env("HOME", home)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "user.name=Fixture",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments);
    isolate(&mut command);
    require_success(command, "git").map(drop)
}

/// The commit `HEAD` names in the repository at `directory`, as the full hex
/// hash.
pub(crate) fn head_commit(directory: &Path, home: &Path) -> Result<String, Box<dyn Error>> {
    let mut command = Command::new("git");
    command
        .current_dir(directory)
        .env("HOME", home)
        .args(["rev-parse", "HEAD"]);
    isolate(&mut command);
    let output = require_success(command, "git")?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Turns `directory`, which holds a skeleton crate, into a git repository with
/// one commit, and returns the `file://` URL a git dependency reaches it by.
pub(crate) fn commit_repository(directory: &Path, home: &Path) -> Result<String, Box<dyn Error>> {
    run(directory, home, &["init", "--quiet"])?;
    run(directory, home, &["add", "--all"])?;
    run(
        directory,
        home,
        &["commit", "--quiet", "--message", "the skeleton"],
    )?;
    Ok(format!("file://{}", directory.display()))
}
