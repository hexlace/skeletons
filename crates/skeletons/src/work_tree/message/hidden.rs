//! The line for a file git is told not to read from the work tree.

use crate::skeleton::Escaped;
use crate::work_tree::index_entry::HiddenFlag;
use crate::work_tree::writing_command::WritingCommand;

/// `` {path} is marked {flag} in git's index, so git does not read its bytes
/// from the work tree and would ignore what {command} wrote there: run `git
/// update-index --no-{flag} -- {path}`, then run the `{command}` task again ``.
///
/// `path` is the path as the command shows it, and is escaped here.
pub(crate) fn hidden_from_work_tree_line(
    path: &str,
    flag: HiddenFlag,
    command: WritingCommand,
) -> String {
    let path = Escaped(path);
    let name = command.name();
    let run_again = command.run_again();
    let (marked, options) = match flag {
        HiddenFlag::SkipWorktree => ("skip-worktree", "--no-skip-worktree"),
        HiddenFlag::AssumeUnchanged => ("assume-unchanged", "--no-assume-unchanged"),
        HiddenFlag::Both => (
            "skip-worktree and assume-unchanged",
            "--no-skip-worktree --no-assume-unchanged",
        ),
    };
    format!(
        "{path} is marked {marked} in git's index, so git does not read its bytes from the work \
         tree and would ignore what {name} wrote there: run `git update-index {options} -- \
         {path}`, then {run_again}"
    )
}

#[cfg(test)]
mod tests {
    use super::hidden_from_work_tree_line;
    use crate::work_tree::index_entry::HiddenFlag;
    use crate::work_tree::writing_command::WritingCommand;

    #[test]
    fn a_hidden_file_names_its_flag_and_the_command_that_clears_exactly_that_flag() {
        for (flag, marked, options) in [
            (
                HiddenFlag::SkipWorktree,
                "marked skip-worktree in",
                "--no-skip-worktree --",
            ),
            (
                HiddenFlag::AssumeUnchanged,
                "marked assume-unchanged in",
                "--no-assume-unchanged --",
            ),
            (
                HiddenFlag::Both,
                "marked skip-worktree and assume-unchanged in",
                "--no-skip-worktree --no-assume-unchanged --",
            ),
        ] {
            let line = hidden_from_work_tree_line("d/plain.yml", flag, WritingCommand::Sync);
            assert_eq!(
                line,
                format!(
                    "d/plain.yml is {marked} git's index, so git does not read its bytes from \
                     the work tree and would ignore what sync wrote there: run `git \
                     update-index {options} d/plain.yml`, then run the `sync` task \
                     again"
                )
            );
        }
    }

    #[test]
    fn wear_words_the_skip_worktree_manifest_line_in_its_own_terms() {
        assert_eq!(
            hidden_from_work_tree_line(
                "Cargo.toml",
                HiddenFlag::SkipWorktree,
                WritingCommand::Wear
            ),
            "Cargo.toml is marked skip-worktree in git's index, so git does not read its bytes \
             from the work tree and would ignore what wear wrote there: run `git update-index \
             --no-skip-worktree -- Cargo.toml`, then run the `wear` task again"
        );
    }
}
