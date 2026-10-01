//! Reading the locked side of a branch or default-branch pin's own
//! directory-scoped `behind` comparison, straight out of Cargo's own
//! checkout — the same checkout `cargo metadata` already resolved a worn
//! skeleton's own `skeleton_directory` from. Read-only: [`locked_directory`]
//! is the only entry point this module has, and it runs exactly one git
//! command, `rev-parse --show-prefix HEAD HEAD:./` — a local plumbing read
//! with no fetch, write, or network-capable flag among its arguments — so
//! there is nothing else this module could ask of the checkout or a remote
//! even if it tried (tested by
//! `ritual/tests/check_behind_branch_directory.rs` →
//! `a_branch_pin_reads_behind_when_a_commit_past_the_lock_touches_this_crates_own_directory`,
//! which snapshots `CARGO_HOME` before and after a run that reads this
//! checkout and asserts it is byte-for-byte unchanged).

use std::path::Path;
use std::process::Command;

use crate::git::{self, Locale, ObjectId, RepositoryPrefix};
use crate::subprocess::Finished;

/// The skeleton's own package directory's tree, as Cargo's checkout holds it
/// at the locked commit, together with that directory's own prefix relative
/// to the repository's top level — the same prefix a fetched remote
/// snapshot's own matching directory is read back out by
/// (`behind::snapshot::directory_trees`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockedDirectory {
    prefix: RepositoryPrefix,
    tree: ObjectId,
}

impl LockedDirectory {
    pub(crate) const fn prefix(&self) -> &RepositoryPrefix {
        &self.prefix
    }

    pub(crate) const fn tree(&self) -> &ObjectId {
        &self.tree
    }

    /// Builds a [`LockedDirectory`] directly, at whatever prefix and tree a
    /// sibling module's own unit test wants — `branch_directory.rs`'s own
    /// tests of `finalize_branch_directory` need one without spawning a real
    /// git checkout to read it from.
    #[cfg(test)]
    pub(crate) const fn for_test(prefix: RepositoryPrefix, tree: ObjectId) -> Self {
        Self { prefix, tree }
    }
}

/// Why [`locked_directory`] could not read the checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckoutFailure {
    /// The checkout's own `HEAD` is not the pin's own locked commit — a
    /// stale checkout, or one that resolved a different commit than the one
    /// this pin actually locked.
    NotAtLockedCommit { actual: ObjectId },
    /// The checkout could not be read as a git repository at all, or the
    /// git process asking it failed outright.
    Unreadable { diagnostic: String },
}

/// Reads `skeleton_directory`'s own prefix and tree at `locked`, from
/// Cargo's own checkout: one local `git rev-parse --show-prefix HEAD
/// HEAD:./`, run with `skeleton_directory` itself as the working directory.
///
/// # Errors
///
/// [`CheckoutFailure::NotAtLockedCommit`] when the checkout's own `HEAD`
/// differs from `locked`. [`CheckoutFailure::Unreadable`] when git could not
/// answer at all — spawn failure, a non-zero exit, output past this crate's
/// own cap, or an answer this crate cannot parse.
pub(crate) fn locked_directory(
    skeleton_directory: &Path,
    locked: &ObjectId,
) -> Result<LockedDirectory, CheckoutFailure> {
    let mut command = git::command(Locale::Fixed);
    command
        .current_dir(skeleton_directory)
        .args(["rev-parse", "--show-prefix", "HEAD", "HEAD:./"]);
    let finished =
        run_local(command).map_err(|diagnostic| CheckoutFailure::Unreadable { diagnostic })?;

    if !finished.success() {
        return Err(CheckoutFailure::Unreadable {
            diagnostic: git::diagnostic(finished.stderr_head()),
        });
    }
    let Ok(stdout) = finished.stdout() else {
        return Err(CheckoutFailure::Unreadable {
            diagnostic: "printed more than 16 MiB, the most `skeletons` reads".to_owned(),
        });
    };
    let Some((prefix, actual_head, tree)) = parse_rev_parse_three(stdout) else {
        return Err(CheckoutFailure::Unreadable {
            diagnostic: "printed an answer `skeletons` cannot read".to_owned(),
        });
    };
    if actual_head != *locked {
        return Err(CheckoutFailure::NotAtLockedCommit {
            actual: actual_head,
        });
    }
    Ok(LockedDirectory { prefix, tree })
}

/// The pure parser behind [`locked_directory`]'s own reading of
/// `rev-parse --show-prefix HEAD HEAD:./`'s three-line answer: the prefix
/// line (empty for a package at the repository root), then `HEAD`'s own
/// commit, then the tree `HEAD:./` names.
fn parse_rev_parse_three(stdout: &[u8]) -> Option<(RepositoryPrefix, ObjectId, ObjectId)> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines();
    let prefix = RepositoryPrefix::parse(lines.next()?)?;
    let head = ObjectId::parse(lines.next()?)?;
    let tree = ObjectId::parse(lines.next()?)?;
    Some((prefix, head, tree))
}

/// Runs `command` under the shared local bounds — the one git question
/// this module ever asks — with a failure to run it worded as text.
fn run_local(command: Command) -> Result<Finished, String> {
    git::run_local(command).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        CheckoutFailure, ObjectId, RepositoryPrefix, git, locked_directory, parse_rev_parse_three,
    };
    use crate::git::Locale;

    fn oid(text: &str) -> ObjectId {
        ObjectId::parse(text).expect("a well-formed test object id")
    }

    const HEAD: &str = "53106b505adcdf4e83fdb7ec18058b7e30e4b797";
    const TREE: &str = "8a9a2221e545e17fae7676301faca75dd3d411f1";

    #[test]
    fn a_nested_package_reads_its_own_prefix_head_and_tree() {
        let stdout = format!("crates/alpha/\n{HEAD}\n{TREE}\n");
        let (prefix, head, tree) = parse_rev_parse_three(stdout.as_bytes())
            .expect("a well-formed three-line answer must parse");
        assert_eq!(
            prefix,
            RepositoryPrefix::parse("crates/alpha/").expect("well-formed prefix")
        );
        assert_eq!(head, oid(HEAD));
        assert_eq!(tree, oid(TREE));
    }

    #[test]
    fn a_package_at_the_repository_root_reads_an_empty_prefix() {
        let stdout = format!("\n{HEAD}\n{TREE}\n");
        let (prefix, _head, _tree) = parse_rev_parse_three(stdout.as_bytes()).expect("must parse");
        assert_eq!(prefix, RepositoryPrefix::parse("").expect("empty prefix"));
    }

    #[test]
    fn two_lines_instead_of_three_fails_to_parse() {
        let stdout = format!("crates/alpha/\n{HEAD}\n");
        assert!(parse_rev_parse_three(stdout.as_bytes()).is_none());
    }

    #[test]
    fn a_bad_object_id_line_fails_to_parse() {
        let stdout = format!("crates/alpha/\n{HEAD}\nnot-an-object-id\n");
        assert!(parse_rev_parse_three(stdout.as_bytes()).is_none());
    }

    /// Runs `git` with `arguments` in `directory`, under an isolated `HOME`,
    /// with signing off and a fixed identity so the running user's own
    /// configuration never reaches disposable test history; returns its
    /// trimmed standard output.
    ///
    /// Background maintenance is off (`maintenance.auto=false`,
    /// `gc.auto=0`) for the reason the other fixture builders give: `git
    /// commit` otherwise starts a detached `git maintenance run` that can
    /// outlive this call and still hold or remove a lock under `.git/`
    /// while the test reads the repository or removes its directory.
    fn git_output(home: &Path, directory: &Path, arguments: &[&str]) -> String {
        let output = git::command(Locale::Fixed)
            .current_dir(directory)
            .env("HOME", home)
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_CONFIG_SYSTEM")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.email=skeletons-test@example.invalid",
                "-c",
                "user.name=Skeletons Test",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
            ])
            .args(arguments)
            .output()
            .expect("git must run");
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git printed UTF-8")
            .trim()
            .to_owned()
    }

    /// A real repository holding a package at `crates/alpha`, with two
    /// commits that both change it, and its work tree left checked out at
    /// the first.
    struct Checkout {
        repository: TempDir,
        _home: TempDir,
        first: ObjectId,
        second: ObjectId,
    }

    impl Checkout {
        fn new() -> Self {
            let repository = TempDir::new().expect("scratch repository");
            let home = TempDir::new().expect("scratch HOME");
            let root = repository.path();
            git_output(home.path(), root, &["init", "--quiet"]);
            let mut commits = Vec::new();
            for content in ["first", "second"] {
                let file = root.join("crates/alpha/file.txt");
                std::fs::create_dir_all(file.parent().expect("a nested file")).expect("mkdir");
                std::fs::write(&file, content).expect("write fixture file");
                git_output(home.path(), root, &["add", "--all"]);
                git_output(
                    home.path(),
                    root,
                    &["commit", "--quiet", "--message", content],
                );
                commits.push(oid(&git_output(home.path(), root, &["rev-parse", "HEAD"])));
            }
            git_output(
                home.path(),
                root,
                &["checkout", "--quiet", commits[0].as_str()],
            );
            let [first, second]: [ObjectId; 2] =
                commits.try_into().expect("exactly two commits were made");
            assert_ne!(first, second, "two commits that change a file differ");
            Self {
                repository,
                _home: home,
                first,
                second,
            }
        }

        fn package_directory(&self) -> std::path::PathBuf {
            self.repository.path().join("crates/alpha")
        }
    }

    /// The checkout's `HEAD` is the first commit while the pin locked the
    /// second, so `locked_directory` must refuse with the commit the
    /// checkout is really at. Builds a real repository, checks out the
    /// first commit and asks for the second; without the head comparison
    /// the tree of the wrong commit would be returned as the locked side.
    #[test]
    fn a_checkout_at_another_commit_than_the_locked_one_is_refused_naming_its_own_head() {
        let checkout = Checkout::new();

        let result = locked_directory(&checkout.package_directory(), &checkout.second);

        assert_eq!(
            result,
            Err(CheckoutFailure::NotAtLockedCommit {
                actual: checkout.first
            })
        );
    }

    /// The control for the refusal above: the same checkout asked for the
    /// commit it really is at answers with the package's own prefix and
    /// the tree git names for that directory.
    #[test]
    fn a_checkout_at_the_locked_commit_reads_its_prefix_and_tree() {
        let checkout = Checkout::new();
        let home = TempDir::new().expect("scratch HOME");
        let expected_tree = oid(&git_output(
            home.path(),
            checkout.repository.path(),
            &["rev-parse", "HEAD:crates/alpha"],
        ));

        let directory = locked_directory(&checkout.package_directory(), &checkout.first)
            .expect("a checkout at the locked commit must read");

        assert_eq!(directory.prefix().as_str(), "crates/alpha/");
        assert_eq!(directory.tree(), &expected_tree);
    }

    /// A directory that is not inside any git repository cannot answer
    /// `rev-parse`, so the read fails as `Unreadable` carrying git's own
    /// diagnostic rather than as a commit mismatch.
    #[test]
    fn a_directory_that_is_not_a_repository_is_unreadable() {
        let directory = TempDir::new().expect("scratch directory");

        let result = locked_directory(directory.path(), &oid(HEAD));

        assert!(
            matches!(&result, Err(CheckoutFailure::Unreadable { diagnostic }) if !diagnostic.is_empty()),
            "expected Unreadable with a diagnostic, got {result:?}"
        );
    }

    /// `git commit` spawns `git maintenance run --auto --detach`, whose
    /// child outlives the command and holds or removes a lock under
    /// `.git/` after the command returned. That cannot be observed
    /// directly: a process listing differs by platform and the detached
    /// child may already have exited by the time it is looked for, so a
    /// check that finds nothing proves nothing. What `git_output` controls
    /// is the effective configuration, so this reads both settings back
    /// through it with `git config --get`, inside a repository it built.
    #[test]
    fn every_fixture_git_command_has_background_maintenance_turned_off() {
        let checkout = Checkout::new();
        let home = TempDir::new().expect("scratch HOME");
        let root = checkout.repository.path();

        let maintenance_auto =
            git_output(home.path(), root, &["config", "--get", "maintenance.auto"]);
        let gc_auto = git_output(home.path(), root, &["config", "--get", "gc.auto"]);

        assert_eq!(
            maintenance_auto, "false",
            "maintenance.auto must be off for fixture git commands"
        );
        assert_eq!(gc_auto, "0", "gc.auto must be off for fixture git commands");
    }

    proptest! {
        /// However the three lines are shaped, `parse_rev_parse_three`
        /// never panics.
        #[test]
        fn never_panics(stdout in ".*") {
            let _result = parse_rev_parse_three(stdout.as_bytes());
        }
    }
}
