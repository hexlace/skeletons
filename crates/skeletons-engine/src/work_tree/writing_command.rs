//! Which command is asking git about the work tree, so that the questions
//! and the refusals they lead to are worded for the command the wearer ran.
//!
//! `sync` and `wear` both write into the work tree and both refuse unless git
//! can say what a write would lose. The refusals are the same refusals, and
//! each names the command that wrote nothing and the remedy that runs it
//! again, so each message takes a [`WritingCommand`] for those words.

/// A command that writes into the work tree and so asks git whether it may.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritingCommand {
    /// `sync`: replaces files with what the worn skeletons ship.
    Sync,
    /// `wear`: changes the command line's own manifest and `Cargo.lock`.
    Wear,
}

impl WritingCommand {
    /// The command's own name, as a message says what "wrote nothing".
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Wear => "wear",
        }
    }

    /// The remedy every message that ends by asking for another run gives.
    ///
    /// It names the command as a task and nothing before it. The task is told
    /// neither the key it is mounted under nor how its command line is reached,
    /// so the remedy names only the subcommand, which the wearer recognises on
    /// their own command line. It says "task" because a bare `sync` is also a
    /// shell command that flushes buffers, prints nothing and exits 0.
    pub(crate) const fn run_again(self) -> &'static str {
        match self {
            Self::Sync => "run the `sync` task again",
            Self::Wear => "run the `wear` task again",
        }
    }

    /// What the command writes over, for the sentence that says without git
    /// there is no undo for it.
    pub(crate) const fn undone_by_git(self) -> &'static str {
        match self {
            Self::Sync => "what it replaces",
            Self::Wear => "what it changes",
        }
    }

    /// What git holds once the work tree is clean, for the sentence that says
    /// the command writes only into a clean one.
    pub(crate) const fn held_by_git(self) -> &'static str {
        match self {
            Self::Sync => "everything it could replace",
            Self::Wear => "the manifest it changes and any Cargo.lock git tracks",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WritingCommand;

    #[test]
    fn each_command_names_itself() {
        // Pins the one word the refusals share, once per command.
        assert_eq!(WritingCommand::Sync.name(), "sync");
        assert_eq!(WritingCommand::Wear.name(), "wear");
    }

    #[test]
    fn each_command_asks_for_a_run_of_itself_as_a_task() {
        // The remedy is the only place a message names how to run the command
        // again, so it must name the task and the right one.
        assert_eq!(
            WritingCommand::Sync.run_again(),
            "run the `sync` task again"
        );
        assert_eq!(
            WritingCommand::Wear.run_again(),
            "run the `wear` task again"
        );
    }

    #[test]
    fn each_command_says_what_git_would_have_to_undo_and_hold() {
        // The two phrases differ between the commands because `sync` replaces
        // files and `wear` changes them in place.
        assert_eq!(WritingCommand::Sync.undone_by_git(), "what it replaces");
        assert_eq!(WritingCommand::Wear.undone_by_git(), "what it changes");
        assert_eq!(
            WritingCommand::Sync.held_by_git(),
            "everything it could replace"
        );
        assert_eq!(
            WritingCommand::Wear.held_by_git(),
            "the manifest it changes and any Cargo.lock git tracks"
        );
    }
}
