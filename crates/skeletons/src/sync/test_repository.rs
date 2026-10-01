//! Building a real git repository for `clean`'s and `proof`'s own unit
//! tests to run real `git` against — the shapes those two modules need
//! (submodules, content filters, sparse-checkout bits, conflicts) are real
//! git behaviour no hand-built fixture could stand in for.
//!
//! Isolated the same way `ritual/tests/support/git.rs` isolates the `ritual`
//! crate's integration-test fixtures: a fresh `HOME` per repository, so no
//! ambient `~/.gitconfig` is ever read, `commit.gpgsign=false
//! -c tag.gpgSign=false` so the running user's signing policy never applies
//! to disposable test history, and a fixed local commit identity so a run
//! never depends on what is configured globally.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::git::{self, Locale};

use super::work_tree::{WorkTree, open};

/// A real git repository, with its own isolated `HOME` (always owned, and
/// removed on drop). Its own working directory is either a fresh temporary
/// one this value owns and removes too ([`TestRepository::new`]), or an
/// existing directory a test already owns the cleanup of —
/// [`TestRepository::new_at`], for a repository nested inside another
/// [`TestRepository`]'s own tree, which must not be removed twice.
pub(super) struct TestRepository {
    directory: PathBuf,
    _owned_directory: Option<TempDir>,
    home: TempDir,
}

impl TestRepository {
    /// `git init`, in a fresh temporary directory this value owns, with no
    /// repository history yet.
    pub(super) fn new() -> Self {
        let directory = TempDir::new().expect("scratch repository directory");
        let path = directory.path().to_path_buf();
        Self::init(path, Some(directory))
    }

    /// `git init` at `directory`, an already-existing directory this
    /// value's own cleanup does not own — for a repository nested inside
    /// another [`TestRepository`]'s own working directory.
    pub(super) fn new_at(directory: PathBuf) -> Self {
        Self::init(directory, None)
    }

    fn init(directory: PathBuf, owned_directory: Option<TempDir>) -> Self {
        let home = TempDir::new().expect("scratch HOME directory");
        let repository = Self {
            directory,
            _owned_directory: owned_directory,
            home,
        };
        repository.git(&["init", "--quiet"]).run_ok();
        repository
    }

    pub(super) fn path(&self) -> &Path {
        &self.directory
    }

    /// Writes `content` at `relative`, under this repository's own root,
    /// creating parent directories as needed.
    pub(super) fn write(&self, relative: &str, content: &[u8]) {
        let full_path = self.path().join(relative);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent directories");
        }
        std::fs::write(full_path, content).expect("write fixture file");
    }

    /// `git add --all` then `git commit`.
    pub(super) fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]).run_ok();
        self.git(&["commit", "--quiet", "--message", message])
            .run_ok();
    }

    /// A [`Command`] builder, isolated the way this whole module's own doc
    /// comment describes, with `arguments` appended.
    pub(super) fn git(&self, arguments: &[&str]) -> TestCommand {
        let mut command = git::command(Locale::Fixed);
        command
            .current_dir(self.path())
            .env("HOME", self.home.path())
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_CONFIG_SYSTEM")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                // A detached maintenance child outliving the command that
                // spawned it, holding or removing a lock under `.git/` after
                // that command returned, would change the tree under a test
                // that believes it is quiescent (`git commit` starts one
                // with `git maintenance run --auto --detach`). Whether the
                // lock is still there when the command returns depends on
                // the git version. Off here, so no fixture command ever
                // leaves a background process running against its tree.
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
            .args(arguments);
        TestCommand { command }
    }

    /// Opens this repository's own root as a [`WorkTree`] — the same call
    /// `sync`'s own orchestration makes, so `clean`'s and `proof`'s own unit
    /// tests exercise the real seam rather than a hand-built [`WorkTree`].
    pub(super) fn work_tree(&self) -> WorkTree {
        open(self.path(), |_name| false).expect("a freshly built repository must open")
    }

    /// Every byte under `.git/`, except `lfs/objects/` (a git-lfs filter,
    /// clean or smudge, genuinely writes there, and that write is not what
    /// this snapshot is proving) — the mechanism behind design.md's guarantee
    /// that none of the git commands `sync` runs writes git's own index,
    /// refs, config or object database.
    pub(super) fn snapshot_dot_git(&self) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut snapshot = std::collections::BTreeMap::new();
        let root = self.path().join(".git");
        let mut pending = vec![root.clone()];
        while let Some(current) = pending.pop() {
            let Ok(file_type) = std::fs::symlink_metadata(&current).map(|meta| meta.file_type())
            else {
                continue;
            };
            if file_type.is_dir() {
                if current
                    .strip_prefix(&root)
                    .is_ok_and(|relative| relative == Path::new("lfs/objects"))
                {
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(&current) else {
                    continue;
                };
                for entry in entries.flatten() {
                    pending.push(entry.path());
                }
            } else if file_type.is_file() {
                let relative = current
                    .strip_prefix(&root)
                    .unwrap_or(&current)
                    .to_path_buf();
                if let Ok(bytes) = std::fs::read(&current) {
                    snapshot.insert(relative, bytes);
                }
            }
        }
        snapshot
    }
}

/// Names what differs between two [`TestRepository::snapshot_dot_git`]
/// snapshots, by path and size: paths only in `before`, paths only in
/// `after`, and paths in both whose contents differ. Empty when the two are
/// equal.
///
/// Never prints a file's bytes. Asserting two whole snapshots equal dumps
/// every file's contents, which is enough output to cut a CI log off before
/// it reaches the path that differs.
pub(super) fn describe_difference(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> String {
    let mut lines = Vec::new();
    for (path, bytes) in before {
        match after.get(path) {
            None => lines.push(format!(
                "only in before: {} ({} bytes)",
                path.display(),
                bytes.len()
            )),
            Some(after_bytes) if after_bytes != bytes => lines.push(format!(
                "different contents: {} ({} bytes before, {} bytes after)",
                path.display(),
                bytes.len(),
                after_bytes.len()
            )),
            Some(_) => {}
        }
    }
    for (path, bytes) in after {
        if !before.contains_key(path) {
            lines.push(format!(
                "only in after: {} ({} bytes)",
                path.display(),
                bytes.len()
            ));
        }
    }
    lines.join("\n")
}

/// A [`Command`] built through [`TestRepository::git`], run to completion
/// with a simple pass/fail contract this module's own fixture-building code
/// needs — never the contract `clean`/`proof`'s own production code reads
/// (that goes through [`crate::subprocess::run`], exercised through
/// [`TestRepository::work_tree`] instead).
pub(super) struct TestCommand {
    command: Command,
}

impl TestCommand {
    /// Runs the command, panicking with its own stdout and stderr if it did
    /// not exit successfully — fixture setup has no contract of its own to
    /// assert on, so a failure here is this test's own fixture being wrong.
    pub(super) fn run_ok(mut self) {
        let output = self.command.output().expect("git must be runnable");
        assert!(
            output.status.success(),
            "git fixture setup failed ({status}); stdout: {stdout}; stderr: {stderr}",
            status = output.status,
            stdout = String::from_utf8_lossy(&output.stdout),
            stderr = String::from_utf8_lossy(&output.stderr),
        );
    }

    /// Runs the command, returning its raw `Output` regardless of whether it
    /// exited successfully — for fixture steps whose own point is a
    /// non-zero exit (a deliberately conflicted merge, in particular).
    pub(super) fn run_allow_failure(mut self) -> std::process::Output {
        self.command.output().expect("git must be runnable")
    }

    /// Runs the command, panicking on a non-zero exit, and returns its own
    /// stdout, trimmed — for a fixture step whose whole point is reading
    /// back a value git printed (an object id, in particular).
    pub(super) fn output_ok(mut self) -> String {
        let output = self.command.output().expect("git must be runnable");
        assert!(
            output.status.success(),
            "git fixture setup failed ({status}); stdout: {stdout}; stderr: {stderr}",
            status = output.status,
            stdout = String::from_utf8_lossy(&output.stdout),
            stderr = String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8(output.stdout)
            .expect("git output must be valid UTF-8")
            .trim()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{TestRepository, describe_difference};

    fn snapshot(entries: &[(&str, &[u8])]) -> BTreeMap<PathBuf, Vec<u8>> {
        entries
            .iter()
            .map(|(path, bytes)| (PathBuf::from(path), bytes.to_vec()))
            .collect()
    }

    #[test]
    fn equal_snapshots_describe_no_difference() {
        let one = snapshot(&[("HEAD", b"ref: refs/heads/main\n"), ("config", b"x")]);
        assert_eq!(describe_difference(&one, &one.clone()), "");
    }

    #[test]
    fn a_difference_names_each_path_and_size_in_the_three_ways_it_can_differ() {
        // One path in each of the three categories, with contents that
        // would be recognisable if they were printed, so the assertion
        // that they are not is meaningful.
        let before = snapshot(&[
            ("objects/maintenance.lock", b""),
            ("HEAD", b"SECRET-BEFORE"),
            ("same", b"unchanged"),
        ]);
        let after = snapshot(&[
            ("HEAD", b"SECRET-AFTER-LONGER"),
            ("same", b"unchanged"),
            ("index", b"12345"),
        ]);

        let description = describe_difference(&before, &after);

        assert!(
            description.contains("only in before: objects/maintenance.lock (0 bytes)"),
            "{description}"
        );
        assert!(
            description.contains("only in after: index (5 bytes)"),
            "{description}"
        );
        assert!(
            description.contains("different contents: HEAD (13 bytes before, 19 bytes after)"),
            "{description}"
        );
        assert!(!description.contains("same"), "{description}");
        assert!(!description.contains("SECRET"), "{description}");
    }

    #[test]
    fn every_fixture_git_command_has_background_maintenance_turned_off() {
        // `git commit` spawns `git maintenance run --auto --detach`, whose
        // child outlives the command and holds or removes a lock under
        // `.git/` after the command returned. That cannot be observed
        // directly: a process listing differs by platform and the detached
        // child may already have exited by the time it is looked for, so a
        // check that finds nothing proves nothing. What the builder
        // controls is the effective configuration, so this reads both
        // settings back through it with `git config --get`.
        let repository = TestRepository::new();

        let maintenance_auto = repository
            .git(&["config", "--get", "maintenance.auto"])
            .output_ok();
        let gc_auto = repository.git(&["config", "--get", "gc.auto"]).output_ok();

        assert_eq!(
            maintenance_auto, "false",
            "maintenance.auto must be off for fixture git commands"
        );
        assert_eq!(gc_auto, "0", "gc.auto must be off for fixture git commands");
    }
}
