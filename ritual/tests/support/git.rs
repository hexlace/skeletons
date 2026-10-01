//! Git fixtures: a wearing workspace's own repository (for `sync`'s
//! dirty-worktree and index/stash rules), and a skeleton's own upstream
//! repository (the source of a `tag =` / `branch =` / `rev =` / unqualified
//! `git =` dependency, for the `behind` cases and any test that needs a
//! git-sourced skeleton).
//!
//! Every git invocation here passes `-c commit.gpgsign=false
//! -c tag.gpgSign=false` explicitly: a global git configuration
//! may sign every commit and tag, which is the right policy for real work
//! under a real name and has nothing to do with disposable fixture
//! history built in a temporary directory and never pushed anywhere. Every
//! repository is also given a fixed, local commit identity so a run never
//! depends on whatever `user.name`/`user.email` happen to be configured
//! globally on the machine running the suite.

use std::error::Error;
use std::path::Path;
use std::process::Command;

use super::{
    TemporaryDirectory, isolate_from_the_enclosing_repository, require_success, write,
    write_minimal_skeleton,
};

/// Runs `git`, in `directory`, with `arguments`, isolated from the running
/// machine's global git configuration and identity, using `home` as `HOME`
/// so no ambient `~/.gitconfig` is read either — and isolated from
/// whatever repository or git configuration happens to enclose the process
/// actually running this suite
/// ([`isolate_from_the_enclosing_repository`]).
fn git_command(directory: &Path, home: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(directory)
        .env("HOME", home)
        .args([
            // A detached maintenance child outliving the command that
            // spawned it, holding or removing a lock under `.git/` after
            // that command returned, would change a fixture's tree under a
            // test that believes it is quiescent (`git commit` starts one
            // with `git maintenance run --auto --detach`). Off here, so no
            // fixture command ever leaves a background process running
            // against its tree.
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "user.email=acceptance-fixture@example.invalid",
            "-c",
            "user.name=Acceptance Fixture",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments);
    isolate_from_the_enclosing_repository(&mut command);
    command
}

/// Runs `git`, in `directory`, with `arguments`, failing the calling test
/// if it did not exit successfully.
pub(crate) fn run(
    directory: &Path,
    home: &Path,
    arguments: &[&str],
    what: &str,
) -> Result<String, Box<dyn Error>> {
    let output = require_success(git_command(directory, home, arguments), what)?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// The same as [`run`], plus `extra_env` — environment variables set on the
/// `git` process after the isolation [`run`] applies, for a scenario whose
/// own point is what a child of `git` (a hook it runs, in particular) sees.
pub(crate) fn run_with_env(
    directory: &Path,
    home: &Path,
    arguments: &[&str],
    extra_env: &[(&str, &Path)],
    what: &str,
) -> Result<String, Box<dyn Error>> {
    let mut command = git_command(directory, home, arguments);
    for (name, value) in extra_env {
        command.env(name, value);
    }
    let output = require_success(command, what)?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Runs `git`, in `directory`, with `arguments`, regardless of whether it
/// exits successfully — for the handful of fixture-building steps whose
/// whole point is a non-zero exit: a merge deliberately left conflicted,
/// most notably. Unlike [`run`], this never fails the calling test on a
/// non-zero exit; the caller decides what a given exit code, stdout or
/// stderr means for the scenario it is building.
pub(crate) fn run_allow_failure(
    directory: &Path,
    home: &Path,
    arguments: &[&str],
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(git_command(directory, home, arguments).output()?)
}

/// A wearing repository's own git history: `git init`, plus one commit
/// holding every file the test wrote before calling this, so `sync`'s
/// dirty-worktree checks have a clean baseline to compare against.
///
/// The repository lives inside the fixture workspace's own root: `sync`
/// looks for a working tree containing the paths it is about to write, and
/// a wearing repository's own root is exactly where that repository lives.
pub(crate) fn init_wearing_repository(
    workspace_root: &Path,
    home: &Path,
) -> Result<(), Box<dyn Error>> {
    run(workspace_root, home, &["init", "--quiet"], "git init")?;
    run(workspace_root, home, &["add", "--all"], "git add")?;
    run(
        workspace_root,
        home,
        &["commit", "--quiet", "--message", "fixture: initial state"],
        "git commit",
    )?;
    Ok(())
}

/// A skeleton's own upstream repository, built fresh for one test: a
/// temporary directory holding real git history, reached only over a
/// `file://` URL, so a `tag =` / `branch =` / `rev =` / unqualified `git =`
/// dependency on it resolves without any network request.
pub(crate) struct SkeletonRepository {
    directory: TemporaryDirectory,
    home: TemporaryDirectory,
}

impl SkeletonRepository {
    /// Creates the repository and its own isolated `HOME`, both empty, and
    /// runs `git init` in it.
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let directory = TemporaryDirectory::new("skeleton-git-repository")?;
        let home = TemporaryDirectory::new("skeleton-git-repository-home")?;
        run(
            directory.path(),
            home.path(),
            &["init", "--quiet"],
            "git init",
        )?;
        Ok(Self { directory, home })
    }

    /// This repository's own working directory.
    fn path(&self) -> &Path {
        self.directory.path()
    }

    /// The `file://` URL a Cargo `git =` dependency reaches this repository
    /// through.
    pub(crate) fn file_url(&self) -> String {
        format!("file://{}", self.directory.path().display())
    }

    /// Writes a minimal skeleton crate holding `file_contents` at
    /// `file_relative`, replacing whatever was there before, and commits
    /// it with `message`, returning the new commit's full sha.
    ///
    /// `package_name` must be the same across every call for one
    /// repository — the crate's name cannot change between commits of the
    /// same history.
    pub(crate) fn commit_skeleton(
        &self,
        package_name: &str,
        file_relative: &str,
        file_contents: &str,
        message: &str,
    ) -> Result<String, Box<dyn Error>> {
        write_minimal_skeleton(self.path(), package_name, file_relative, file_contents)?;
        run(self.path(), self.home.path(), &["add", "--all"], "git add")?;
        run(
            self.path(),
            self.home.path(),
            &["commit", "--quiet", "--message", message],
            "git commit",
        )?;
        run(
            self.path(),
            self.home.path(),
            &["rev-parse", "HEAD"],
            "git rev-parse HEAD",
        )
    }

    /// The same as [`Self::commit_skeleton`], except the skeleton crate is
    /// written under `subdirectory` (relative to this repository's own
    /// root) rather than at the root itself — for a repository holding more
    /// than one skeleton crate, each in its own directory, the shape
    /// `check_behind_many_crates.rs`'s own tag- and branch-scoping tests
    /// need. Committing with `git add --all` from the repository's own
    /// root, as every commit here does, still picks up every crate's own
    /// files regardless of which one this particular call touched, so a
    /// commit written this way changes only `subdirectory`'s own tree
    /// entries — exactly "a commit past the locked one touches that
    /// skeleton's own directory" or does not.
    pub(crate) fn commit_skeleton_at(
        &self,
        subdirectory: &str,
        package_name: &str,
        file_relative: &str,
        file_contents: &str,
        message: &str,
    ) -> Result<String, Box<dyn Error>> {
        write_minimal_skeleton(
            &self.path().join(subdirectory),
            package_name,
            file_relative,
            file_contents,
        )?;
        run(self.path(), self.home.path(), &["add", "--all"], "git add")?;
        run(
            self.path(),
            self.home.path(),
            &["commit", "--quiet", "--message", message],
            "git commit",
        )?;
        run(
            self.path(),
            self.home.path(),
            &["rev-parse", "HEAD"],
            "git rev-parse HEAD",
        )
    }

    /// Tags the current `HEAD` as `name`, as a plain, unsigned, lightweight
    /// tag.
    pub(crate) fn tag(&self, name: &str) -> Result<(), Box<dyn Error>> {
        run(self.path(), self.home.path(), &["tag", name], "git tag")?;
        Ok(())
    }

    /// Creates a new branch named `name` at the current `HEAD` and switches
    /// to it, so the next [`Self::commit_skeleton`] call lands on it.
    pub(crate) fn branch_from_head(&self, name: &str) -> Result<(), Box<dyn Error>> {
        run(
            self.path(),
            self.home.path(),
            &["checkout", "--quiet", "-b", name],
            "git checkout -b",
        )?;
        Ok(())
    }

    /// Deletes this repository's own working directory, leaving nothing at
    /// its `file://` URL for a subsequent `git` invocation to reach.
    ///
    /// Once a fixture is locked against this repository, removing the
    /// repository itself means any further attempt to reach it — a `behind`
    /// check, in particular — fails loudly, with git's own error. A test uses
    /// that either to make a remote really unreachable for `check`'s
    /// `behind`, or to show a command never tried to reach it: a `sync` that
    /// still succeeds and writes the right bytes made no network request.
    pub(crate) fn remove(self) -> Result<(), Box<dyn Error>> {
        std::fs::remove_dir_all(self.directory.path())?;
        Ok(())
    }
}

/// Writes a file at `relative`, under `workspace_root`, and stages it, but
/// does not commit — leaving it exactly as an already-added-but-uncommitted
/// change, the staged case `sync`'s dirty-worktree refusal covers.
pub(crate) fn write_and_stage(
    workspace_root: &Path,
    home: &Path,
    relative: &str,
    content: &[u8],
) -> Result<(), Box<dyn Error>> {
    write(workspace_root, relative, content)?;
    run(workspace_root, home, &["add", "--", relative], "git add")?;
    Ok(())
}

/// The current git index, as `git status --porcelain` reports it — used to
/// prove `sync` never stages anything: read before and after, the two
/// reports must be identical.
pub(crate) fn status_porcelain(
    workspace_root: &Path,
    home: &Path,
) -> Result<String, Box<dyn Error>> {
    run(
        workspace_root,
        home,
        &["status", "--porcelain"],
        "git status",
    )
}

/// The current stash list, as `git stash list` reports it — used to prove
/// `sync` never stashes anything.
pub(crate) fn stash_list(workspace_root: &Path, home: &Path) -> Result<String, Box<dyn Error>> {
    run(workspace_root, home, &["stash", "list"], "git stash list")
}

/// Appends `pattern` to the `.gitignore` at `workspace_root`, creating it if
/// there is none, so any path matching it, existing or not, is ignored.
pub(crate) fn ignore(workspace_root: &Path, pattern: &str) -> Result<(), Box<dyn Error>> {
    let existing = std::fs::read_to_string(workspace_root.join(".gitignore")).unwrap_or_default();
    write(
        workspace_root,
        ".gitignore",
        format!("{existing}{pattern}\n").as_bytes(),
    )
}

/// Appends `pattern` to `.git/info/exclude` under `workspace_root`, so any
/// path matching it, existing or not, is ignored only in this one
/// repository's own local copy, rather than by a tracked `.gitignore`, and
/// the exclusion is never itself committed. Used to plant
/// an ignored entry a scenario needs `git status` to stay silent about
/// without also committing a pattern that would name it.
pub(crate) fn exclude_locally(workspace_root: &Path, pattern: &str) -> Result<(), Box<dyn Error>> {
    let existing =
        std::fs::read_to_string(workspace_root.join(".git/info/exclude")).unwrap_or_default();
    write(
        workspace_root,
        ".git/info/exclude",
        format!("{existing}{pattern}\n").as_bytes(),
    )
}
