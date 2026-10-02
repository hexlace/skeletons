//! The line for a git command that ran past the time a writing command
//! allows a local question, and was stopped.
//!
//! A bare "timed out" names nothing to look at. The line says which git
//! command was stopped, what it was answering (a path, where there is one),
//! what most likely held it up, and how to find out.

use crate::git::local_timeout;
use crate::skeleton::Escaped;
use crate::work_tree::abort::GitQuestion;
use crate::work_tree::writing_command::WritingCommand;

/// `` `git {command}` timed out after {n} s {doing what}, and was stopped, so
/// {sync|wear} wrote nothing: {what held it up}; {what to do} ``.
pub(super) fn timed_out_message(question: &GitQuestion, command: WritingCommand) -> String {
    let seconds = local_timeout().as_secs();
    let name = command.name();
    let run_again = command.run_again();
    match question {
        GitQuestion::Checkout(path) => {
            let path = Escaped(path.as_str());
            format!(
                "`git cat-file` timed out after {seconds} s writing what git would check out \
                 for {path}, and was stopped, so {name} wrote nothing: a content filter this \
                 repository configures for {path} did not finish (`git check-attr filter -- \
                 {path}` names it); make it finish, then {run_again}"
            )
        }
        GitQuestion::Status => format!(
            "`git status` timed out after {seconds} s and was stopped, so {name} wrote nothing: a \
             content filter this repository configures, or a slow filesystem, kept it from \
             finishing; run `git status` to see it finish, then {run_again}"
        ),
        GitQuestion::IndexEntry(path) => index_message(
            seconds,
            &format!("reading git's index entry for {}", Escaped(path)),
            command,
        ),
        GitQuestion::IndexAbove(directory) => index_message(
            seconds,
            &format!(
                "reading what git's index tracks at the directory {}",
                Escaped(directory.as_str())
            ),
            command,
        ),
        GitQuestion::Ignored(path) => {
            let path = Escaped(path.as_str());
            format!(
                "`git check-ignore` timed out after {seconds} s asking whether git ignores \
                 {path}, and was stopped, so {name} wrote nothing: git could not read its ignore \
                 rules in time (a slow filesystem); {run_again} once `git status` answers \
                 promptly"
            )
        }
        GitQuestion::IndexListing => {
            index_message(seconds, "reading the paths in git's index", command)
        }
        GitQuestion::WorkTree => format!(
            "`git rev-parse` timed out after {seconds} s and was stopped, so {name} wrote \
             nothing: git could not answer in time (another git process holding the repository, \
             or a slow filesystem); {run_again} once `git status` answers promptly"
        ),
    }
}

/// The line for a `git ls-files` that `doing` was stopped in the middle of.
fn index_message(seconds: u64, doing: &str, command: WritingCommand) -> String {
    let name = command.name();
    let run_again = command.run_again();
    format!(
        "`git ls-files` timed out after {seconds} s {doing}, and was stopped, so {name} wrote \
         nothing: git could not read its own index in time (another git process holding it, or \
         a slow filesystem); {run_again} once `git status` answers promptly"
    )
}

#[cfg(test)]
mod tests {
    use super::timed_out_message;
    use crate::survey::poison::claim;
    use crate::work_tree::abort::GitQuestion;
    use crate::work_tree::writing_command::WritingCommand;

    #[test]
    fn a_checkout_that_timed_out_names_the_path_the_command_and_the_filter() {
        let message = timed_out_message(
            &GitQuestion::Checkout(claim("a/plain.yml")),
            WritingCommand::Sync,
        );

        assert_eq!(
            message,
            "`git cat-file` timed out after 30 s writing what git would check out for \
             a/plain.yml, and was stopped, so sync wrote nothing: a content filter this \
             repository configures for a/plain.yml did not finish (`git check-attr filter -- \
             a/plain.yml` names it); make it finish, then run the `sync` task again"
        );
    }

    #[test]
    fn a_status_that_timed_out_says_to_run_it_by_hand() {
        let message = timed_out_message(&GitQuestion::Status, WritingCommand::Sync);

        assert_eq!(
            message,
            "`git status` timed out after 30 s and was stopped, so sync wrote nothing: a \
             content filter this repository configures, or a slow filesystem, kept it from \
             finishing; run `git status` to see it finish, then run the `sync` task \
             again"
        );
    }

    #[test]
    fn an_index_question_that_timed_out_names_the_path_or_directory_it_was_reading() {
        let entry = timed_out_message(
            &GitQuestion::IndexEntry("a/b.yml".to_owned()),
            WritingCommand::Sync,
        );
        assert!(
            entry.starts_with(
                "`git ls-files` timed out after 30 s reading git's index entry for a/b.yml, and \
                 was stopped, so sync wrote nothing: git could not read its own index in time"
            ),
            "{entry}"
        );

        let above = timed_out_message(&GitQuestion::IndexAbove(claim("a/b")), WritingCommand::Sync);
        assert!(
            above.contains("reading what git's index tracks at the directory a/b,"),
            "{above}"
        );

        let ignored = timed_out_message(
            &GitQuestion::Ignored(claim("a/b.yml")),
            WritingCommand::Sync,
        );
        assert!(
            ignored.starts_with(
                "`git check-ignore` timed out after 30 s asking whether git ignores a/b.yml, and \
                 was stopped, so sync wrote nothing"
            ),
            "{ignored}"
        );

        let listing = timed_out_message(&GitQuestion::IndexListing, WritingCommand::Sync);
        assert!(
            listing.contains("reading the paths in git's index,"),
            "{listing}"
        );
        assert!(
            listing.ends_with("run the `sync` task again once `git status` answers promptly"),
            "{listing}"
        );
    }

    #[test]
    fn a_work_tree_question_that_timed_out_names_rev_parse() {
        let message = timed_out_message(&GitQuestion::WorkTree, WritingCommand::Sync);

        assert!(
            message.starts_with("`git rev-parse` timed out after 30 s and was stopped"),
            "{message}"
        );
    }

    #[test]
    fn wear_words_a_timed_out_checkout_status_and_work_tree_question_for_itself() {
        // The three questions whose text does not come from a shared helper,
        // pinned whole: who wrote nothing and which task to run again.
        assert_eq!(
            timed_out_message(
                &GitQuestion::Checkout(claim("a/plain.yml")),
                WritingCommand::Wear
            ),
            "`git cat-file` timed out after 30 s writing what git would check out for \
             a/plain.yml, and was stopped, so wear wrote nothing: a content filter this \
             repository configures for a/plain.yml did not finish (`git check-attr filter -- \
             a/plain.yml` names it); make it finish, then run the `wear` task again"
        );
        assert_eq!(
            timed_out_message(&GitQuestion::Status, WritingCommand::Wear),
            "`git status` timed out after 30 s and was stopped, so wear wrote nothing: a \
             content filter this repository configures, or a slow filesystem, kept it from \
             finishing; run `git status` to see it finish, then run the `wear` task again"
        );
        assert_eq!(
            timed_out_message(&GitQuestion::WorkTree, WritingCommand::Wear),
            "`git rev-parse` timed out after 30 s and was stopped, so wear wrote nothing: git \
             could not answer in time (another git process holding the repository, or a slow \
             filesystem); run the `wear` task again once `git status` answers promptly"
        );
    }

    #[test]
    fn wear_words_a_timed_out_index_and_ignore_question_for_itself() {
        // The four questions about git's index and ignore rules, which share
        // the index helper or the ignore sentence.
        assert_eq!(
            timed_out_message(
                &GitQuestion::IndexEntry("a/b.yml".to_owned()),
                WritingCommand::Wear
            ),
            "`git ls-files` timed out after 30 s reading git's index entry for a/b.yml, and was \
             stopped, so wear wrote nothing: git could not read its own index in time (another \
             git process holding it, or a slow filesystem); run the `wear` task again once \
             `git status` answers promptly"
        );
        assert_eq!(
            timed_out_message(&GitQuestion::IndexAbove(claim("a/b")), WritingCommand::Wear),
            "`git ls-files` timed out after 30 s reading what git's index tracks at the \
             directory a/b, and was stopped, so wear wrote nothing: git could not read its own \
             index in time (another git process holding it, or a slow filesystem); run the \
             `wear` task again once `git status` answers promptly"
        );
        assert_eq!(
            timed_out_message(&GitQuestion::IndexListing, WritingCommand::Wear),
            "`git ls-files` timed out after 30 s reading the paths in git's index, and was \
             stopped, so wear wrote nothing: git could not read its own index in time (another \
             git process holding it, or a slow filesystem); run the `wear` task again once \
             `git status` answers promptly"
        );
        assert_eq!(
            timed_out_message(
                &GitQuestion::Ignored(claim("a/b.yml")),
                WritingCommand::Wear
            ),
            "`git check-ignore` timed out after 30 s asking whether git ignores a/b.yml, and was \
             stopped, so wear wrote nothing: git could not read its ignore rules in time (a slow \
             filesystem); run the `wear` task again once `git status` answers promptly"
        );
    }
}
