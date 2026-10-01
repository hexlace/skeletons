//! Rule (a): the whole work tree, not only the paths `sync` is about to
//! write, must be clean — as git itself defines clean, under flags that
//! defeat the configuration that would otherwise hide dirt from a plain
//! `git status`. It is rule (a) in `.docs/design.md`, "The whole work tree is
//! clean"; rule (b), "Positive proof, per path", is `super::proof`'s.

use crate::git::{self, Locale, RepositoryPrefix};
use crate::subprocess::Truncated;

use super::abort::{GitQuestion, SyncAbort};
use super::work_tree::{WorkTree, run_local};

/// Proof that `git status` reported nothing at all for the whole work
/// tree: no tracked change (staged or unstaged, mode included), untracked
/// file, conflict, intent-to-add entry or submodule change. Ignored files
/// do not count. The field is private: only [`check_clean`] builds one, so
/// a `&CleanWorkTree` argument elsewhere in this crate is a type-level
/// proof that this check already ran and found nothing.
#[derive(Debug)]
pub(crate) struct CleanWorkTree(());

impl CleanWorkTree {
    /// A witness for `proof`'s own tests that need to call `prove` without a
    /// genuinely clean tree behind it — exclusively for exercising `prove`'s
    /// mode-based refusals (`SymbolicLinkInIndex`, `SubmoduleInIndex`), which a
    /// real, clean git state can never reach: a hand-crafted index entry whose
    /// mode disagrees with what is really on disk is a type change
    /// (`git status --porcelain=v2` reports it `.T`, as it does for a committed
    /// symbolic link replaced by a regular file), which this module's own
    /// parser reads as `Dirt::Unstaged` (tested by
    /// `a_type_change_with_a_blank_index_column_is_read_as_unstaged`, below),
    /// so rule (a) refuses first in every real run.
    #[cfg(test)]
    pub(crate) const fn assume_clean_for_test() -> Self {
        Self(())
    }
}

/// What the whole-tree status question found.
#[derive(Debug)]
pub(crate) enum Cleanliness {
    Clean(CleanWorkTree),
    Dirty(Vec<DirtyPath>),
}

/// One path the whole-tree status reported anything at all for, and why.
#[derive(Debug)]
pub(crate) struct DirtyPath {
    pub(crate) shown: String,
    pub(crate) dirt: Dirt,
}

/// Why one reported path counts as dirty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dirt {
    /// Modified in the work tree, from the index or `HEAD` (`.M`), or any
    /// other status this classifier does not otherwise recognise.
    Unstaged,
    /// Staged — added, modified, deleted, mode-changed, or type-changed in
    /// the index.
    Staged,
    /// Deleted in the work tree, index and `HEAD` otherwise unchanged.
    Deleted,
    /// In the work tree and not in the index, and not ignored (`?`).
    Untracked,
    /// A `git add -N` placeholder, with no real committed content behind
    /// it.
    IntentToAdd,
    /// Unmerged: a merge, rebase or similar left the entry conflicted (`u`).
    Conflicted,
    /// A change inside a submodule, or the submodule's own gitlink entry
    /// itself changed.
    Submodule,
    /// A status record this parser could not classify at all — reported
    /// dirty regardless, since an unrecognised status is never read as
    /// clean.
    Unreadable,
}

/// Asks whether the whole work tree `work_tree` sits in is clean, under the
/// flags that defeat `status.showUntrackedFiles`, `diff.ignoreSubmodules`
/// and `submodule.<name>.ignore`, with `core.fsmonitor=false` so no
/// fsmonitor answer is trusted and no daemon is started under `.git/`.
pub(crate) fn check_clean(work_tree: &WorkTree) -> Result<Cleanliness, SyncAbort> {
    // A content filter (git-lfs's clean filter, in particular) can run
    // while git compares a file's content, so this command's own locale is
    // inherited rather than fixed — the same reason `cat-file` is.
    let mut command = work_tree.git(Locale::Inherited);
    command.args([
        "-c",
        "core.fsmonitor=false",
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--ignored=no",
        "--ignore-submodules=none",
        "--no-renames",
    ]);
    let finished = run_local(command, GitQuestion::Status)?;
    classify_status(
        finished.success(),
        finished.stdout(),
        finished.stderr_head(),
        work_tree.prefix(),
    )
}

/// The pure classifier behind [`check_clean`]: `status`'s exit, stdout and
/// stderr in, a verdict (or the abort they amount to) out — split out so a
/// unit test drives a truncated stream through the very code production
/// runs. Truncated output is an abort, never a clean tree: a status cut
/// short may have dropped exactly the record that says dirty.
fn classify_status(
    exit_ok: bool,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
    prefix: &RepositoryPrefix,
) -> Result<Cleanliness, SyncAbort> {
    if !exit_ok {
        return Err(SyncAbort::GitFailed {
            command: "status",
            diagnostic: git::diagnostic(stderr),
        });
    }
    let stdout = stdout.map_err(|_truncated| SyncAbort::GitOutputTooLarge { command: "status" })?;

    let dirty = parse_status(stdout, prefix);
    if dirty.is_empty() {
        Ok(Cleanliness::Clean(CleanWorkTree(())))
    } else {
        Ok(Cleanliness::Dirty(dirty))
    }
}

/// Parses `status --porcelain=v2 -z`'s own NUL-separated output into one
/// [`DirtyPath`] per record, sorted by `shown`. An empty stream parses to an
/// empty list — `check_clean` reads that as [`Cleanliness::Clean`]; **any**
/// record at all reads as dirty, whatever it classifies as.
fn parse_status(bytes: &[u8], prefix: &RepositoryPrefix) -> Vec<DirtyPath> {
    let mut dirty: Vec<DirtyPath> = bytes
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| parse_status_record(record, prefix))
        .collect();
    dirty.sort_by(|left, right| left.shown.cmp(&right.shown));
    dirty
}

/// Classifies one whole record — everything between two NUL separators.
fn parse_status_record(record: &[u8], prefix: &RepositoryPrefix) -> DirtyPath {
    let text = String::from_utf8_lossy(record);
    match record.first() {
        Some(b'1') => parse_ordinary_record(&text, prefix),
        Some(b'u') => parse_unmerged_record(&text, prefix),
        Some(b'?') => parse_untracked_record(&text, prefix),
        // `!` (ignored — unreachable under `--ignored=no`), `2` (a rename,
        // unreachable under `--no-renames`), `#` (a header line, never
        // printed without `--branch`), and anything shorter or stranger
        // than any of the above. Reported dirty regardless, naming the raw
        // record, rather than silently read as clean.
        _ => unreadable_record(&text),
    }
}

fn unreadable_record(text: &str) -> DirtyPath {
    DirtyPath {
        shown: text.to_owned(),
        dirt: Dirt::Unreadable,
    }
}

/// An ordinary changed entry: `1 XY sub mH mI mW hH hI path`.
fn parse_ordinary_record(text: &str, prefix: &RepositoryPrefix) -> DirtyPath {
    let fields: Vec<&str> = text.splitn(9, ' ').collect();
    let (Some(&xy), Some(&sub), Some(&path)) = (fields.get(1), fields.get(2), fields.get(8)) else {
        return unreadable_record(text);
    };
    DirtyPath {
        shown: prefix.shown(path),
        dirt: classify_ordinary(xy, sub),
    }
}

/// Classifies an ordinary record's own `XY` and `sub` fields: `sub` starting
/// `S` marks a submodule (its gitlink entry changed, or something changed
/// inside it); `X` blank (`.`) with `Y` = `A` marks intent-to-add; any other
/// non-blank `X` marks staged, a staged deletion included; `Y` = `D` with `X`
/// blank marks a work-tree deletion; anything else is an unstaged, uncommitted
/// change.
fn classify_ordinary(xy: &str, sub: &str) -> Dirt {
    if sub.starts_with('S') {
        return Dirt::Submodule;
    }
    let mut characters = xy.chars();
    let x = characters.next().unwrap_or('.');
    let y = characters.next().unwrap_or('.');
    if x == '.' && y == 'A' {
        return Dirt::IntentToAdd;
    }
    if x != '.' {
        return Dirt::Staged;
    }
    if y == 'D' {
        return Dirt::Deleted;
    }
    Dirt::Unstaged
}

/// An unmerged (conflicted) entry: `u XY sub m1 m2 m3 mW h1 h2 h3 path`.
fn parse_unmerged_record(text: &str, prefix: &RepositoryPrefix) -> DirtyPath {
    let fields: Vec<&str> = text.splitn(11, ' ').collect();
    let Some(&path) = fields.get(10) else {
        return unreadable_record(text);
    };
    DirtyPath {
        shown: prefix.shown(path),
        dirt: Dirt::Conflicted,
    }
}

/// An untracked entry: `? path`.
fn parse_untracked_record(text: &str, prefix: &RepositoryPrefix) -> DirtyPath {
    let fields: Vec<&str> = text.splitn(2, ' ').collect();
    let Some(&path) = fields.get(1) else {
        return unreadable_record(text);
    };
    DirtyPath {
        shown: prefix.shown(path),
        dirt: Dirt::Untracked,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Cleanliness, Dirt, classify_ordinary, classify_status, parse_status};
    use crate::git::RepositoryPrefix;
    use crate::subprocess::Truncated;
    use crate::sync::abort::SyncAbort;

    fn top_level() -> RepositoryPrefix {
        RepositoryPrefix::parse("").expect("empty prefix")
    }

    #[test]
    fn truncated_status_stdout_reads_as_git_output_too_large_never_clean() {
        // A status cut short may have dropped exactly the record that says
        // dirty, so `classify_status` must refuse it. The control hands it a
        // complete, empty stream, which reads clean — so the refusal is the
        // truncation, never the (absent) bytes.
        let error = classify_status(
            true,
            Err(Truncated::for_test(16 * 1024 * 1024)),
            b"",
            &top_level(),
        )
        .expect_err("a truncated status stream must be refused");
        assert!(matches!(
            error,
            SyncAbort::GitOutputTooLarge { command: "status" }
        ));

        let verdict = classify_status(true, Ok(b""), b"", &top_level())
            .expect("a complete, empty stream must read");
        assert!(matches!(verdict, Cleanliness::Clean(_)));
    }

    #[test]
    fn empty_input_parses_to_no_entries() {
        assert!(parse_status(b"", &top_level()).is_empty());
    }

    #[test]
    fn an_unstaged_modification_is_read() {
        let bytes = b"1 .M N... 100644 100644 100644 7898192 7898192 f.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].shown, "f.txt");
        assert_eq!(dirty[0].dirt, Dirt::Unstaged);
    }

    #[test]
    fn a_deletion_in_the_work_tree_is_read() {
        let bytes = b"1 .D N... 100644 100644 000000 7898192 7898192 f.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Deleted);
    }

    #[test]
    fn an_intent_to_add_entry_is_read() {
        let bytes = b"1 .A N... 000000 000000 100644 0000000 0000000 ita.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::IntentToAdd);
    }

    #[test]
    fn a_staged_modification_is_read() {
        let bytes = b"1 M. N... 100644 100644 100644 7898192 b478501 f.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Staged);
    }

    #[test]
    fn a_type_change_with_a_blank_index_column_is_read_as_unstaged() {
        let bytes = b"1 .T N... 100644 100644 120000 7898192 7898192 a.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Unstaged);
    }

    #[test]
    fn a_mode_only_change_is_read_as_unstaged() {
        let bytes = b"1 .M N... 100644 100644 100755 7898192 7898192 f.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Unstaged);
    }

    #[test]
    fn a_submodule_gitlink_change_is_read() {
        let bytes = b"1 .M S.M. 160000 160000 160000 b10e1234 b10e1234 .github\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Submodule);
    }

    #[test]
    fn a_submodule_untracked_inner_file_is_read_as_submodule() {
        let bytes = b"1 .M S..U 160000 160000 160000 b10e1234 b10e1234 .github\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Submodule);
    }

    #[test]
    fn an_unmerged_entry_is_read_as_conflicted() {
        let bytes = b"u UU N... 100644 100644 100644 100644 df961234 28ce1234 13e71234 f.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].shown, "f.txt");
        assert_eq!(dirty[0].dirt, Dirt::Conflicted);
    }

    #[test]
    fn an_untracked_entry_is_read() {
        let bytes = b"? d/e/f\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].shown, "d/e/f");
        assert_eq!(dirty[0].dirt, Dirt::Untracked);
    }

    #[test]
    fn an_untracked_directory_reported_as_a_nested_repository_is_read() {
        let bytes = b"? .github/\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].shown, ".github/");
        assert_eq!(dirty[0].dirt, Dirt::Untracked);
    }

    #[test]
    fn a_path_containing_spaces_is_read_in_full() {
        let bytes = b"? has spaces.txt\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].shown, "has spaces.txt");
    }

    #[test]
    fn a_header_record_is_unreadable() {
        let bytes = b"2 something\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Unreadable);
    }

    #[test]
    fn a_two_byte_record_is_unreadable() {
        let bytes = b"1M\0";
        let dirty = parse_status(bytes, &top_level());
        assert_eq!(dirty[0].dirt, Dirt::Unreadable);
    }

    #[test]
    fn shown_is_relative_to_the_workspace_root_not_the_repository_top() {
        let prefix = RepositoryPrefix::parse("ws/").expect("ws/ prefix");
        let bytes = b"? ws/inside.txt\0? top-untracked\0";
        let dirty = parse_status(bytes, &prefix);
        let shown: Vec<&str> = dirty.iter().map(|entry| entry.shown.as_str()).collect();
        assert_eq!(shown, vec!["../top-untracked", "inside.txt"]);
    }

    #[test]
    fn classify_ordinary_reads_a_submodule_regardless_of_xy() {
        assert_eq!(classify_ordinary("..", "S.M."), Dirt::Submodule);
    }

    proptest! {
        /// However `git status --porcelain=v2 -z` output is shaped —
        /// truncated, adversarial, or simply not from git at all —
        /// `parse_status` never panics.
        #[test]
        fn parse_status_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _dirty = parse_status(&bytes, &RepositoryPrefix::parse("").expect("empty prefix"));
        }
    }

    /// Real git, in a temporary repository — the shapes a hand-built
    /// porcelain-v2 byte string cannot stand in for: submodule
    /// configuration, nested repositories, and git's own defaults for
    /// `--ignored`/`--untracked-files`.
    mod real_git {
        use super::super::{Cleanliness, check_clean};
        use crate::sync::test_repository::TestRepository;

        fn dirt_kinds(cleanliness: Cleanliness) -> Vec<super::Dirt> {
            let Cleanliness::Dirty(dirty) = cleanliness else {
                panic!("expected Dirty")
            };
            dirty.into_iter().map(|entry| entry.dirt).collect()
        }

        #[test]
        fn a_freshly_committed_repository_is_clean() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository.commit_all("initial");
            let work_tree = repository.work_tree();
            assert!(matches!(
                check_clean(&work_tree).expect("status must run"),
                Cleanliness::Clean(_)
            ));
        }

        #[test]
        fn an_edited_tracked_file_is_dirty() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository.commit_all("initial");
            repository.write("f.txt", b"edited\n");
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Unstaged]
            );
        }

        #[test]
        fn a_staged_file_is_dirty() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository.commit_all("initial");
            repository.write("f.txt", b"staged\n");
            repository.git(&["add", "f.txt"]).run_ok();
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Staged]
            );
        }

        #[test]
        fn a_worktree_deletion_is_dirty() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository.commit_all("initial");
            std::fs::remove_file(repository.path().join("f.txt")).expect("remove tracked file");
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Deleted]
            );
        }

        #[test]
        fn an_untracked_file_is_dirty() {
            let repository = TestRepository::new();
            repository.write(".gitkeep", b"");
            repository.commit_all("initial");
            repository.write("u.txt", b"untracked\n");
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Untracked]
            );
        }

        #[test]
        fn an_empty_intent_to_add_entry_is_dirty() {
            let repository = TestRepository::new();
            repository.write(".gitkeep", b"");
            repository.commit_all("initial");
            repository.write("ita.txt", b"");
            repository.git(&["add", "-N", "ita.txt"]).run_ok();
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::IntentToAdd]
            );
        }

        #[test]
        fn a_conflicted_merge_is_dirty() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"base\n");
            repository.commit_all("base");
            repository
                .git(&["checkout", "--quiet", "-b", "feature"])
                .run_ok();
            repository.write("f.txt", b"feature\n");
            repository.commit_all("feature");
            repository.git(&["checkout", "--quiet", "main"]).run_ok();
            repository.write("f.txt", b"main\n");
            repository.commit_all("main");
            let merge = repository
                .git(&["merge", "--no-edit", "feature"])
                .run_allow_failure();
            assert!(!merge.status.success(), "the merge must genuinely conflict");

            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Conflicted]
            );
        }

        #[test]
        fn a_mode_only_change_is_dirty() {
            use std::os::unix::fs::PermissionsExt as _;

            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository
                .git(&["config", "core.fileMode", "true"])
                .run_ok();
            repository.commit_all("initial");
            let path = repository.path().join("f.txt");
            let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("chmod +x");

            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Unstaged]
            );
        }

        #[test]
        fn a_submodule_edit_is_dirty_even_under_ignore_all_configuration() {
            let source = TestRepository::new();
            source.write("inner.txt", b"inner\n");
            source.commit_all("source: initial");

            let repository = TestRepository::new();
            repository.write(".gitkeep", b"");
            repository.commit_all("initial");
            let source_url = format!("file://{}", source.path().display());
            repository
                .git(&[
                    "-c",
                    "protocol.file.allow=always",
                    "submodule",
                    "add",
                    "--quiet",
                    &source_url,
                    "sub",
                ])
                .run_ok();
            repository
                .git(&["config", "submodule.sub.ignore", "all"])
                .run_ok();
            repository
                .git(&["config", "diff.ignoreSubmodules", "all"])
                .run_ok();
            repository.commit_all("add submodule");

            repository.write("sub/inner.txt", b"edited\n");
            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Submodule]
            );
        }

        #[test]
        fn a_nested_non_submodule_repository_reads_as_an_untracked_directory() {
            let repository = TestRepository::new();
            repository.write(".gitkeep", b"");
            repository.commit_all("initial");
            std::fs::create_dir_all(repository.path().join("nested")).expect("nested directory");
            let nested = TestRepository::new_at(repository.path().join("nested"));
            nested.write("inner.txt", b"inner\n");

            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Untracked]
            );
        }

        #[test]
        fn status_show_untracked_files_no_does_not_hide_an_untracked_file() {
            let repository = TestRepository::new();
            repository.write(".gitkeep", b"");
            repository.commit_all("initial");
            repository
                .git(&["config", "status.showUntrackedFiles", "no"])
                .run_ok();
            repository.write("u.txt", b"hidden by config\n");

            let work_tree = repository.work_tree();
            assert_eq!(
                dirt_kinds(check_clean(&work_tree).expect("status must run")),
                vec![super::Dirt::Untracked]
            );
        }

        #[test]
        fn an_ignored_file_is_clean() {
            let repository = TestRepository::new();
            repository.write(".gitignore", b"ignored.txt\n");
            repository.commit_all("initial");
            repository.write("ignored.txt", b"ignored\n");

            let work_tree = repository.work_tree();
            assert!(matches!(
                check_clean(&work_tree).expect("status must run"),
                Cleanliness::Clean(_)
            ));
        }
    }
}
