//! What a writing command says when it cannot ask git about the work tree,
//! or when git says the work tree is not clean, or that git does not read a
//! file the command writes.
//!
//! `sync` and `wear` refuse on the same questions in the same shapes, so the
//! messages live here once and each takes the [`WritingCommand`] that is
//! asking: it supplies the command's name, what it would write over and the
//! remedy that runs it again. Nothing here names a command itself.

mod hidden;
mod timed_out;

use std::path::Path;

use rituals::report as write_report;

pub(crate) use hidden::hidden_from_work_tree_line;

use super::abort::WorkTreeAbort;
use super::clean::{Dirt, DirtyPath};
use super::writing_command::WritingCommand;
use crate::skeleton::Escaped;
use crate::survey::count;
use crate::survey::join_and;

/// The stderr message for a [`WorkTreeAbort`]: the command could not even
/// ask git the questions it needs answered, as opposed to git answering "no"
/// to one of them, which is a refusal reported through [`report_dirty`] (or,
/// for `sync`, through its own report of unproven paths) instead.
pub(crate) fn abort_message(abort: &WorkTreeAbort, root: &Path, command: WritingCommand) -> String {
    let root = root.display().to_string();
    let root = Escaped(&root);
    let name = command.name();
    match abort {
        WorkTreeAbort::RedirectedGit { variables } => redirected_git_message(variables, command),
        WorkTreeAbort::NotAWorkTree => format!(
            "{root} is not inside a git work tree, so {name} wrote nothing: without git there is \
             no undo for {}; commit the workspace to git first",
            command.undone_by_git()
        ),
        WorkTreeAbort::DubiousOwnership { diagnostic } => {
            let diagnostic = Escaped(diagnostic);
            format!(
                "git refuses to read this repository because another user owns it (git's \
                 safe.directory check), so {name} wrote nothing: {diagnostic}"
            )
        }
        WorkTreeAbort::GitUnavailable { detail } => {
            let detail = Escaped(detail);
            format!("running `git` failed, so {name} wrote nothing: {detail}")
        }
        WorkTreeAbort::GitFailed {
            command: git_command,
            diagnostic,
        } => {
            let diagnostic = Escaped(diagnostic);
            format!("git {git_command} failed, so {name} wrote nothing: {diagnostic}")
        }
        WorkTreeAbort::GitTimedOut { question } => timed_out::timed_out_message(question, command),
        WorkTreeAbort::GitOutputTooLarge {
            command: git_command,
        } => format!(
            "git {git_command} printed more than 16 MiB, the most `skeletons` reads from git, so \
             {name} wrote nothing"
        ),
    }
}

/// `` {GIT_A and GIT_B} {is|are} set, and git would answer from wherever
/// {it points|they point} rather than from this work tree's own repository,
/// so {command} wrote nothing: git sets {it|them} for the hooks it runs, so
/// run the `{command}` task outside a git hook (the `check` task works inside
/// one), or unset {it|them} ``.
///
/// For `sync` this is only reached when something would be written: with
/// nothing to write, `sync` never asks git anything, so it needs no such
/// refusal. Inside a commit the index git is using may be a temporary one
/// (`git commit <paths>`, `git commit -a`), and a file written mid-commit is
/// not part of that commit — which is what refusing there protects.
fn redirected_git_message(variables: &[&'static str], command: WritingCommand) -> String {
    let count = variables.len();
    let names = join_and(
        &variables
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
    );
    let name = command.name();
    format!(
        "{names} {} set, and git would answer from wherever {} rather than from this work \
         tree's own repository, so {name} wrote nothing: git sets {} for the hooks it runs, so \
         run the `{name}` task outside a git hook (the `check` task works inside one), or unset {}",
        count::is_or_are(count),
        count::it_points_or_they_point(count),
        count::it_or_them(count),
        count::it_or_them(count),
    )
}

/// Reports every dirty path on stdout, one line per path, sorted by its own
/// shown spelling.
pub(crate) fn report_dirty(dirty: &[DirtyPath]) {
    let mut sorted: Vec<&DirtyPath> = dirty.iter().collect();
    sorted.sort_by(|left, right| left.shown.cmp(&right.shown));
    let lines: Vec<String> = sorted.iter().map(|entry| dirty_line(entry)).collect();
    write_report(lines.join("\n"));
}

fn dirty_line(dirty: &DirtyPath) -> String {
    let shown = Escaped(&dirty.shown);
    match dirty.dirt {
        Dirt::Unreadable => {
            format!("git status reported a line `skeletons` cannot read: {shown}")
        }
        other => format!("{shown} {}", dirt_phrase(other)),
    }
}

fn dirt_phrase(dirt: Dirt) -> &'static str {
    match dirt {
        Dirt::Unstaged => "has uncommitted changes",
        Dirt::Staged => "has staged changes",
        Dirt::Deleted => "is deleted",
        Dirt::Untracked => "is untracked",
        Dirt::IntentToAdd => "is marked intent-to-add",
        Dirt::Conflicted => "is conflicted",
        Dirt::Submodule => "is a submodule with changes of its own",
        Dirt::Unreadable => unreachable!("dirty_line handles Unreadable before reaching this"),
    }
}

/// `` the working tree has {n} uncommitted {change|changes}, so {command}
/// wrote nothing: it writes only into a clean working tree, where git holds
/// {everything it could replace|the manifest it changes and any Cargo.lock git
/// tracks}; commit, stash or move {it|them}, then run the `{command}` task again ``.
pub(crate) fn dirty_summary(count: usize, command: WritingCommand) -> String {
    format!(
        "the working tree has {count} uncommitted {}, so {} wrote nothing: it \
         writes only into a clean working tree, where git holds {}; \
         commit, stash or move {}, then {}",
        count::change(count),
        command.name(),
        command.held_by_git(),
        count::it_or_them(count),
        command.run_again(),
    )
}

#[cfg(test)]
mod outside_text;

#[cfg(test)]
mod tests {
    use super::{WorkTreeAbort, abort_message, dirty_summary, redirected_git_message};
    use crate::work_tree::abort::GitQuestion;
    use crate::work_tree::writing_command::WritingCommand;

    #[test]
    fn dirty_summary_is_singular_for_one_change() {
        assert_eq!(
            dirty_summary(1, WritingCommand::Sync),
            "the working tree has 1 uncommitted change, so sync wrote nothing: it writes only \
             into a clean working tree, where git holds everything it could replace; commit, \
             stash or move it, then run the `sync` task again"
        );
    }

    #[test]
    fn dirty_summary_is_plural_for_several_changes() {
        assert_eq!(
            dirty_summary(2, WritingCommand::Sync),
            "the working tree has 2 uncommitted changes, so sync wrote nothing: it writes only \
             into a clean working tree, where git holds everything it could replace; commit, \
             stash or move them, then run the `sync` task again"
        );
    }

    #[test]
    fn redirected_git_names_one_variable_singular() {
        let message = redirected_git_message(&["GIT_DIR"], WritingCommand::Sync);
        assert_eq!(
            message,
            "GIT_DIR is set, and git would answer from wherever it points rather than from \
             this work tree's own repository, so sync wrote nothing: git sets it for the hooks \
             it runs, so run the `sync` task outside a git hook (the `check` task works inside \
             one), or unset it"
        );
    }

    #[test]
    fn redirected_git_names_several_variables_plural() {
        let message = redirected_git_message(&["GIT_DIR", "GIT_WORK_TREE"], WritingCommand::Sync);
        assert_eq!(
            message,
            "GIT_DIR and GIT_WORK_TREE are set, and git would answer from wherever they point \
             rather than from this work tree's own repository, so sync wrote nothing: git sets \
             them for the hooks it runs, so run the `sync` task outside a git hook \
             (the `check` task works inside one), or unset them"
        );
    }

    #[test]
    fn not_a_work_tree_names_the_root_and_the_remedy() {
        let message = abort_message(
            &WorkTreeAbort::NotAWorkTree,
            std::path::Path::new("/path/to/root"),
            WritingCommand::Sync,
        );
        assert_eq!(
            message,
            "/path/to/root is not inside a git work tree, so sync wrote nothing: without git \
             there is no undo for what it replaces; commit the workspace to git first"
        );
    }

    #[test]
    fn git_unavailable_carries_its_own_detail() {
        let message = abort_message(
            &WorkTreeAbort::GitUnavailable {
                detail: "no such file or directory".to_owned(),
            },
            std::path::Path::new("/root"),
            WritingCommand::Sync,
        );
        assert_eq!(
            message,
            "running `git` failed, so sync wrote nothing: no such file or directory"
        );
    }

    #[test]
    fn dubious_ownership_names_safe_directory_and_carries_the_diagnostic() {
        let message = abort_message(
            &WorkTreeAbort::DubiousOwnership {
                diagnostic: "detected dubious ownership in repository at '/root'".to_owned(),
            },
            std::path::Path::new("/root"),
            WritingCommand::Sync,
        );
        assert_eq!(
            message,
            "git refuses to read this repository because another user owns it (git's \
             safe.directory check), so sync wrote nothing: detected dubious ownership in \
             repository at '/root'"
        );
    }

    #[test]
    fn git_failed_names_the_command_and_the_diagnostic() {
        let message = abort_message(
            &WorkTreeAbort::GitFailed {
                command: "status",
                diagnostic: "bad object".to_owned(),
            },
            std::path::Path::new("/root"),
            WritingCommand::Sync,
        );
        assert_eq!(
            message,
            "git status failed, so sync wrote nothing: bad object"
        );
    }

    #[test]
    fn git_output_too_large_names_the_command_and_the_cap() {
        let message = abort_message(
            &WorkTreeAbort::GitOutputTooLarge { command: "status" },
            std::path::Path::new("/root"),
            WritingCommand::Sync,
        );
        assert_eq!(
            message,
            "git status printed more than 16 MiB, the most `skeletons` reads from git, so sync wrote \
             nothing"
        );
    }

    /// Every abort this module words, with the question a timeout names
    /// covering each of its own kinds, for the tests that read all of them.
    fn every_abort() -> Vec<WorkTreeAbort> {
        let mut aborts = vec![
            WorkTreeAbort::RedirectedGit {
                variables: vec!["GIT_DIR"],
            },
            WorkTreeAbort::NotAWorkTree,
            WorkTreeAbort::DubiousOwnership {
                diagnostic: "detected dubious ownership".to_owned(),
            },
            WorkTreeAbort::GitUnavailable {
                detail: "no such file or directory".to_owned(),
            },
            WorkTreeAbort::GitFailed {
                command: "status",
                diagnostic: "bad object".to_owned(),
            },
            WorkTreeAbort::GitOutputTooLarge { command: "status" },
        ];
        aborts.extend(
            [
                GitQuestion::WorkTree,
                GitQuestion::Status,
                GitQuestion::IndexListing,
            ]
            .map(|question| WorkTreeAbort::GitTimedOut { question }),
        );
        aborts
    }

    #[test]
    fn wear_words_the_clean_work_tree_refusal_in_its_own_terms_for_one_change() {
        // `wear` changes files in place, so what git holds is what it
        // changes, and the remedy runs the `wear` task.
        assert_eq!(
            dirty_summary(1, WritingCommand::Wear),
            "the working tree has 1 uncommitted change, so wear wrote nothing: it writes only \
             into a clean working tree, where git holds the manifest it changes and any \
             Cargo.lock git tracks; commit, stash or move it, then run the `wear` task again"
        );
    }

    #[test]
    fn wear_words_the_clean_work_tree_refusal_in_its_own_terms_for_several_changes() {
        assert_eq!(
            dirty_summary(2, WritingCommand::Wear),
            "the working tree has 2 uncommitted changes, so wear wrote nothing: it writes only \
             into a clean working tree, where git holds the manifest it changes and any \
             Cargo.lock git tracks; commit, stash or move them, then run the `wear` task again"
        );
    }

    #[test]
    fn wear_names_one_redirecting_variable_and_the_wear_task() {
        assert_eq!(
            redirected_git_message(&["GIT_DIR"], WritingCommand::Wear),
            "GIT_DIR is set, and git would answer from wherever it points rather than from \
             this work tree's own repository, so wear wrote nothing: git sets it for the hooks \
             it runs, so run the `wear` task outside a git hook (the `check` task works inside \
             one), or unset it"
        );
    }

    #[test]
    fn wear_names_several_redirecting_variables_and_the_wear_task() {
        assert_eq!(
            redirected_git_message(&["GIT_DIR", "GIT_WORK_TREE"], WritingCommand::Wear),
            "GIT_DIR and GIT_WORK_TREE are set, and git would answer from wherever they point \
             rather than from this work tree's own repository, so wear wrote nothing: git sets \
             them for the hooks it runs, so run the `wear` task outside a git hook \
             (the `check` task works inside one), or unset them"
        );
    }

    #[test]
    fn wear_reads_a_directory_outside_git_as_having_no_undo_for_what_it_changes() {
        let message = abort_message(
            &WorkTreeAbort::NotAWorkTree,
            std::path::Path::new("/path/to/root"),
            WritingCommand::Wear,
        );
        assert_eq!(
            message,
            "/path/to/root is not inside a git work tree, so wear wrote nothing: without git \
             there is no undo for what it changes; commit the workspace to git first"
        );
    }

    #[test]
    fn wear_words_each_git_failure_with_its_own_name() {
        // The four aborts that name no remedy differ from `sync`'s only in
        // who wrote nothing, so each is pinned whole.
        let root = std::path::Path::new("/root");
        let wear = |abort| abort_message(&abort, root, WritingCommand::Wear);

        assert_eq!(
            wear(WorkTreeAbort::GitUnavailable {
                detail: "no such file or directory".to_owned()
            }),
            "running `git` failed, so wear wrote nothing: no such file or directory"
        );
        assert_eq!(
            wear(WorkTreeAbort::DubiousOwnership {
                diagnostic: "detected dubious ownership in repository at '/root'".to_owned()
            }),
            "git refuses to read this repository because another user owns it (git's \
             safe.directory check), so wear wrote nothing: detected dubious ownership in \
             repository at '/root'"
        );
        assert_eq!(
            wear(WorkTreeAbort::GitFailed {
                command: "status",
                diagnostic: "bad object".to_owned()
            }),
            "git status failed, so wear wrote nothing: bad object"
        );
        assert_eq!(
            wear(WorkTreeAbort::GitOutputTooLarge { command: "status" }),
            "git status printed more than 16 MiB, the most `skeletons` reads from git, so wear \
             wrote nothing"
        );
    }

    #[test]
    fn no_message_worded_for_wear_names_sync() {
        // Every shared message must take its command from the argument and
        // never keep a word of `sync`'s own, or `wear` would send the wearer
        // to a command it did not run. Reads every abort, both dirty
        // summaries and both redirected-variable lines.
        let root = std::path::Path::new("/root");
        for abort in every_abort() {
            let message = abort_message(&abort, root, WritingCommand::Wear);
            assert!(!message.contains("sync"), "{abort:?} read: {message}");
        }
        for count in [1, 2] {
            let message = dirty_summary(count, WritingCommand::Wear);
            assert!(!message.contains("sync"), "{count} read: {message}");
        }
    }
}
