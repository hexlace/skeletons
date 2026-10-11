//! The command line's own crate, which is the only package a command that
//! changes a manifest writes into, and the questions asked about its two files
//! before anything is written.
//!
//! What such a command writes is the crate the running command line was built
//! from, which is the one place a task is told about itself, so there is no
//! manifest to point it at. Each question takes the [`WritingCommand`] that
//! asks it, so that a refusal names the command that wrote nothing.

pub(crate) mod hand_back;
pub(crate) mod writable;

use std::path::{Path, PathBuf};

use crate::skeleton::Escaped;
use crate::work_tree::writing_command::WritingCommand;
use crate::workspace::{self, Member};

/// The command line's own crate, which is the only package a command writes
/// into, and the two files it changes.
pub(crate) struct CommandLineCrate {
    pub(crate) package: String,
    /// Absolute, as `rollback` has to be handed it; it spells the path from
    /// the workspace root itself when it reports.
    pub(crate) manifest_path: PathBuf,
    /// `manifest_path` as a message shows it: relative to the workspace root.
    pub(crate) manifest_shown: String,
    /// Absolute, as `manifest_path` is.
    pub(crate) lockfile_path: PathBuf,
    /// `lockfile_path` as a message shows it: relative to the workspace root.
    pub(crate) lockfile_shown: String,
}

impl CommandLineCrate {
    /// The crate of `member`, the package `package` names in the workspace
    /// at `workspace_root`, with `Cargo.lock` beside the workspace's manifest.
    pub(crate) fn new(package: &str, member: &Member, workspace_root: &Path) -> Self {
        // `cargo metadata` reports every path as absolute, and `rollback`
        // spells the paths it reports from the absolute ones it is handed (it
        // panics on a relative one), so the two it is given are asserted here,
        // where they come in, and the lockfile path joined onto the root is
        // absolute with it.
        assert!(
            workspace_root.is_absolute(),
            "the workspace root is always absolute"
        );
        assert!(
            member.manifest_path.is_absolute(),
            "a member's manifest path is always absolute"
        );
        let lockfile_path = workspace_root.join("Cargo.lock");
        Self {
            package: package.to_owned(),
            manifest_path: member.manifest_path.clone(),
            manifest_shown: member.manifest.clone(),
            lockfile_shown: workspace::relative_to_root(workspace_root, &lockfile_path),
            lockfile_path,
        }
    }
}

/// The refusal for a command line whose package is no member of the workspace
/// at `root`: `command` wrote nothing, and has to be run from inside the project
/// the command line belongs to.
pub(crate) fn outside_its_project_message(
    package: &str,
    root: &Path,
    command: WritingCommand,
) -> String {
    let root = root.display().to_string();
    format!(
        "this command line is built from the package `{}`, which is not a member of the \
         workspace at {}, so {} wrote nothing: run the `{}` task from inside the project \
         this command line belongs to",
        Escaped(package),
        Escaped(&root),
        command.name(),
        command.name(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::outside_its_project_message;
    use crate::work_tree::writing_command::WritingCommand;

    #[test]
    fn the_refusal_names_the_package_the_root_and_the_command_that_wrote_nothing() {
        assert_eq!(
            outside_its_project_message("tool", Path::new("/work/other"), WritingCommand::Wear),
            "this command line is built from the package `tool`, which is not a member of the \
             workspace at /work/other, so wear wrote nothing: run the `wear` task from inside \
             the project this command line belongs to"
        );
        assert_eq!(
            outside_its_project_message("tool", Path::new("/work/other"), WritingCommand::Sync),
            "this command line is built from the package `tool`, which is not a member of the \
             workspace at /work/other, so sync wrote nothing: run the `sync` task from inside \
             the project this command line belongs to"
        );
    }

    #[test]
    fn text_from_outside_is_escaped_once_and_the_line_stays_one_line() {
        let message = outside_its_project_message("a\nb", Path::new("/c\nd"), WritingCommand::Wear);

        assert!(!message.contains('\n'), "{message:?}");
    }
}
