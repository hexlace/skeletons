//! `check`: compare this repository's files against the skeletons it wears.

mod behind_words;
#[cfg(test)]
mod golden;
mod json;
mod pin_words;
mod report;
mod table;

use rituals::{Failure, Outcome, Task, clap, report as write_report};

use report::{Filter, Report};

use crate::behind::{self, Remotes};
use crate::current_directory;
use crate::skeleton::Escaped;
use crate::survey::{count, survey};
use crate::workspace::{self, Network, ReadWorkspaceError};

/// The line printed, and the only line printed, when the workspace wears no
/// skeletons at all — an exact, positive answer, not an error (tested by
/// `ritual/tests/check_worn.rs` →
/// `a_repository_with_no_dependencies_at_all_wears_nothing_and_says_so`,
/// which asserts `check`'s entire stdout against this exact text). `sync`
/// prints this same line, verbatim, when it finds nothing worn either
/// (tested by `ritual/tests/sync.rs` →
/// `sync_wearing_nothing_outside_a_git_working_tree_says_so_and_exits_zero`).
pub(crate) const WEARS_NOTHING_LINE: &str = "this workspace wears no skeletons; a manifest wears \
                                              one with a [package.metadata.skeletons.<dependency>] \
                                              table beside the dependency";

// `check`'s own command-line arguments.
//
// `Copy`: four `bool`s, and `Task::new`'s own contract
// hands the handler its arguments by value, so a plain reference is not an
// option here — `Copy` is what lets `run` read them without clippy asking
// for a reference it cannot take.
//
// No `///` doc comment on this struct. `rituals` sets a task's
// `about` from the string `Task::new` is built with,
// after clap's derived `augment_args`, but leaves `long_about` alone — so a
// struct doc comment of more than one paragraph (clap's rule for when a doc
// comment also becomes a `long_about`) would replace `--help`'s text, and
// `-h` would end in "(see more with '--help')" rather than a bare "Print
// help". Each field below keeps its own `///` line, because that line is
// the flag's help text clap reads.
#[derive(Clone, Copy, clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent clap flags, not a state machine in disguise"
)]
struct CheckArguments {
    /// show only bones that have drifted; the exit status still counts every bone
    #[arg(long)]
    drifted: bool,
    /// show only skeletons that are behind, or where that is undetermined; the exit status
    /// still counts every bone
    #[arg(long)]
    behind: bool,
    /// also fail when a worn skeleton is behind, or where that is undetermined
    #[arg(long)]
    fail_behind: bool,
    /// print the answer as one JSON document (format version 1)
    #[arg(long)]
    json: bool,
}

/// Builds the `check` task.
pub(crate) fn task() -> Task {
    Task::new(
        "compare this repository's files against the skeletons it wears",
        run,
    )
}

fn run(arguments: CheckArguments) -> Outcome {
    let filter = Filter {
        drifted: arguments.drifted,
        behind: arguments.behind,
    };

    let directory = current_directory::read()?;

    let workspace = match workspace::read(&directory, Network::Allowed) {
        Ok(workspace) => workspace,
        Err(error) => {
            if arguments.json {
                write_report(json::aborted(&error));
            }
            let (_kind, message) = abort_message(&error, AbortingCommand::Check);
            return Err(Failure::new(message));
        }
    };

    let wears_nothing = workspace.wearing.is_empty();
    let survey = survey(&workspace);
    let remotes = Remotes::from_environment();
    let behind = behind::determine(&survey.worn, &remotes);
    let report = Report::new(survey, behind);

    if arguments.json {
        write_report(json::document(&report, filter, arguments.fail_behind));
    } else if wears_nothing {
        write_report(WEARS_NOTHING_LINE);
    } else {
        write_report(table::lines(&report, filter, arguments.fail_behind).join("\n"));
    }

    let summary = report.summary(arguments.fail_behind);
    failure_message(&summary, arguments.fail_behind)
        .map_or_else(|| Ok(()), |message| Err(Failure::new(message)))
}

/// The `<bin>: <message>` ritual's own dispatch prints to stderr on
/// failure — built from up to three clauses, each present only when it
/// applies: drift, refusals, and, only with `--fail-behind`, behind.
fn failure_message(summary: &report::Summary, fail_behind: bool) -> Option<String> {
    let mut clauses = Vec::new();
    if summary.drifted > 0 {
        clauses.push(format!(
            "{} {} drifted",
            count::of_total(summary.drifted, summary.bones, count::bone),
            count::has_or_have(summary.drifted)
        ));
    }
    if summary.refusals > 0 {
        clauses.push(count::there_are_refusals(summary.refusals));
    }
    if fail_behind {
        if let Some(behind_clause) = behind_clause(summary.behind, summary.undetermined) {
            clauses.push(format!("{behind_clause} (--fail-behind)"));
        }
    }
    if clauses.is_empty() {
        return None;
    }

    let mut message = clauses.join(", and ");
    if summary.drifted > 0 && summary.refusals == 0 {
        use std::fmt::Write as _;
        let remedy_pronoun = count::it_or_them(summary.drifted);
        let _unused = write!(message, "; the `sync` task puts {remedy_pronoun} back");
    }
    Some(message)
}

/// The behind-related clause of [`failure_message`], before the trailing
/// `(--fail-behind)` is appended: `<n> worn skeleton(s) is/are behind`,
/// `whether <n> worn skeleton(s) is/are behind is undetermined`, or, when
/// both counts are nonzero, the two joined by `, and whether <n> is behind
/// is undetermined` (the second half drops "worn skeleton(s)", already
/// named by the first). `None` when neither count is nonzero.
fn behind_clause(behind: usize, undetermined: usize) -> Option<String> {
    match (behind, undetermined) {
        (0, 0) => None,
        (behind, 0) => Some(format!(
            "{behind} {} {} behind",
            count::worn_skeleton(behind),
            count::is_or_are(behind)
        )),
        (0, undetermined) => Some(format!(
            "whether {undetermined} {} {} behind is undetermined",
            count::worn_skeleton(undetermined),
            count::is_or_are(undetermined)
        )),
        (behind, undetermined) => Some(format!(
            "{behind} {} {} behind, and whether {undetermined} {} behind is undetermined",
            count::worn_skeleton(behind),
            count::is_or_are(behind),
            count::is_or_are(undetermined)
        )),
    }
}

/// Which command aborted — the two places [`abort_message`]'s own text
/// differs by which one is running: the lockfile remedy names the command
/// that failed, and says what that command does with the lockfile, and a
/// `cargo metadata` failure is worded differently for `sync`, which passes
/// `--offline` and so has its own reason to name. `wear` reads the workspace
/// `--locked` with the network allowed, as `check` does, so its
/// `cargo metadata` failure is worded as `check`'s is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbortingCommand {
    Check,
    Sync,
    Wear,
}

impl AbortingCommand {
    const fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Sync => "sync",
            Self::Wear => "wear",
        }
    }
}

/// The lockfile abort's message: why the command could not go on with the
/// lockfile as it is, and the remedy that runs it again.
///
/// `check` and `sync` read the lockfile and never write it; `wear` writes it,
/// but only to add the skeleton, so a lockfile that is already stale would
/// have more rewritten than the skeleton.
fn lockfile_message(command: AbortingCommand) -> String {
    let reason = match command {
        AbortingCommand::Check | AbortingCommand::Sync => {
            "`skeletons` reads it without ever writing it"
        }
        AbortingCommand::Wear => "wear changes it only to add the skeleton",
    };
    format!(
        "Cargo.lock is missing or out of date, and {reason}; run `cargo update --workspace`, \
         then run the `{}` task again",
        command.name()
    )
}

/// The `kind` and message for an abort — shared between the stderr `Failure`
/// this task returns and `--json`'s own `aborted` object, and, for `sync`,
/// its own equivalent stderr line (`sync` prints no JSON of its own).
pub(crate) fn abort_message(
    error: &ReadWorkspaceError,
    command: AbortingCommand,
) -> (&'static str, String) {
    match error {
        ReadWorkspaceError::Lockfile => ("lockfile", lockfile_message(command)),
        ReadWorkspaceError::CargoMetadataFailed { stderr } => {
            // Cargo's stderr runs to several lines. It is escaped, not cut to
            // its first line, so the message keeps all of cargo's words and
            // still holds one.
            let stderr = Escaped(stderr);
            (
                "cargo-metadata-failed",
                match command {
                    AbortingCommand::Check | AbortingCommand::Wear => {
                        format!("cargo metadata --locked failed: {stderr}")
                    }
                    AbortingCommand::Sync => format!(
                        "cargo metadata --locked --offline failed (sync makes no network \
                         request): {stderr}"
                    ),
                },
            )
        }
        ReadWorkspaceError::CargoUnavailable { detail } => (
            "cargo-unavailable",
            format!("running `cargo metadata` failed: {}", Escaped(detail)),
        ),
        ReadWorkspaceError::MetadataUnreadable { detail } => {
            ("metadata-unreadable", Escaped(detail).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AbortingCommand, abort_message, behind_clause, failure_message, report::Summary};
    use crate::survey::poison::{POISON, assert_escaped_once, assert_every_kind, assert_one_line};
    use crate::workspace::ReadWorkspaceError;

    fn summary(
        bones: usize,
        matches: usize,
        drifted: usize,
        skeletons: usize,
        behind: usize,
        undetermined: usize,
        refusals: usize,
    ) -> Summary {
        Summary {
            bones,
            matches,
            drifted,
            skeletons,
            current: skeletons - behind - undetermined,
            behind,
            pinned: 0,
            undetermined,
            refusals,
            failed: drifted > 0 || refusals > 0,
        }
    }

    #[test]
    fn no_drift_and_no_refusals_is_no_failure() {
        assert_eq!(failure_message(&summary(4, 4, 0, 4, 0, 0, 0), false), None);
    }

    #[test]
    fn drift_only_names_the_sync_remedy() {
        assert_eq!(
            failure_message(&summary(4, 2, 2, 1, 0, 0, 0), false).as_deref(),
            Some("2 of 4 bones have drifted; the `sync` task puts them back")
        );
    }

    #[test]
    fn one_drifted_bone_is_singular() {
        assert_eq!(
            failure_message(&summary(4, 3, 1, 1, 0, 0, 0), false).as_deref(),
            Some("1 of 4 bones has drifted; the `sync` task puts it back")
        );
    }

    // The noun after `of` agrees with the total it names, and the verb with
    // the number of bones that drifted, which is its subject.
    #[test]
    fn the_only_bone_drifting_names_a_singular_bone() {
        assert_eq!(
            failure_message(&summary(1, 0, 1, 1, 0, 0, 0), false).as_deref(),
            Some("1 of 1 bone has drifted; the `sync` task puts it back")
        );
    }

    #[test]
    fn every_one_of_several_bones_drifting_names_plural_bones() {
        assert_eq!(
            failure_message(&summary(2, 0, 2, 1, 0, 0, 0), false).as_deref(),
            Some("2 of 2 bones have drifted; the `sync` task puts them back")
        );
    }

    #[test]
    fn refusals_only_names_no_remedy() {
        assert_eq!(
            failure_message(&summary(4, 4, 0, 1, 0, 0, 1), false).as_deref(),
            Some("there is 1 refusal")
        );
    }

    #[test]
    fn drift_and_refusals_together_drop_the_remedy() {
        assert_eq!(
            failure_message(&summary(4, 2, 2, 1, 0, 0, 1), false).as_deref(),
            Some("2 of 4 bones have drifted, and there is 1 refusal")
        );
    }

    #[test]
    fn several_refusals_are_plural() {
        assert_eq!(
            failure_message(&summary(4, 4, 0, 1, 0, 0, 2), false).as_deref(),
            Some("there are 2 refusals")
        );
    }

    #[test]
    fn behind_alone_is_ignored_without_fail_behind() {
        assert_eq!(failure_message(&summary(4, 4, 0, 1, 1, 0, 0), false), None);
    }

    #[test]
    fn fail_behind_names_a_behind_skeleton() {
        assert_eq!(
            failure_message(&summary(4, 4, 0, 1, 1, 0, 0), true).as_deref(),
            Some("1 worn skeleton is behind (--fail-behind)")
        );
    }

    #[test]
    fn fail_behind_names_an_undetermined_skeleton() {
        assert_eq!(
            failure_message(&summary(4, 4, 0, 1, 0, 1, 0), true).as_deref(),
            Some("whether 1 worn skeleton is behind is undetermined (--fail-behind)")
        );
    }

    #[test]
    fn fail_behind_combines_behind_and_undetermined_in_one_clause() {
        assert_eq!(
            failure_message(&summary(4, 4, 0, 2, 1, 1, 0), true).as_deref(),
            Some(
                "1 worn skeleton is behind, and whether 1 is behind is undetermined \
                 (--fail-behind)"
            )
        );
    }

    #[test]
    fn fail_behind_alongside_drift_and_refusals_lists_all_three() {
        assert_eq!(
            failure_message(&summary(4, 3, 1, 1, 1, 0, 1), true).as_deref(),
            Some(
                "1 of 4 bones has drifted, and there is 1 refusal, and 1 worn skeleton is \
                 behind (--fail-behind)"
            )
        );
    }

    #[test]
    fn behind_clause_conjugates_the_undetermined_count_it_names() {
        assert_eq!(
            behind_clause(1, 3).as_deref(),
            Some("1 worn skeleton is behind, and whether 3 are behind is undetermined")
        );
        assert_eq!(
            behind_clause(2, 1).as_deref(),
            Some("2 worn skeletons are behind, and whether 1 is behind is undetermined")
        );
    }

    #[test]
    fn behind_clause_is_none_when_neither_count_is_nonzero() {
        assert_eq!(behind_clause(0, 0), None);
    }

    #[test]
    fn behind_clause_pluralises_several_behind_skeletons() {
        assert_eq!(
            behind_clause(2, 0).as_deref(),
            Some("2 worn skeletons are behind")
        );
    }

    /// How many kinds of [`ReadWorkspaceError`] there are. The match in
    /// [`error_kind`] has no wildcard, so a kind added without an arm does not
    /// compile, and the samples must then cover it.
    const ERROR_KINDS: usize = 4;

    const fn error_kind(error: &ReadWorkspaceError) -> usize {
        match error {
            ReadWorkspaceError::Lockfile => 0,
            ReadWorkspaceError::CargoMetadataFailed { .. } => 1,
            ReadWorkspaceError::CargoUnavailable { .. } => 2,
            ReadWorkspaceError::MetadataUnreadable { .. } => 3,
        }
    }

    #[test]
    fn every_abort_prints_cargos_words_on_one_line_escaped_once() {
        // Cargo's own stderr runs to several lines, and the text of an I/O
        // failure or an unreadable document is whatever the system said. Each
        // is escaped, so the message is one line that keeps every word. A
        // lockfile abort holds no text from outside.
        let text = || POISON.to_owned();
        let errors = [
            ReadWorkspaceError::Lockfile,
            ReadWorkspaceError::CargoMetadataFailed { stderr: text() },
            ReadWorkspaceError::CargoUnavailable { detail: text() },
            ReadWorkspaceError::MetadataUnreadable { detail: text() },
        ];
        assert_every_kind(
            errors.iter().map(error_kind),
            ERROR_KINDS,
            "ReadWorkspaceError",
        );

        for error in &errors {
            for command in [
                AbortingCommand::Check,
                AbortingCommand::Sync,
                AbortingCommand::Wear,
            ] {
                let (_kind, message) = abort_message(error, command);
                let what = format!("{error:?} under {command:?}");
                if matches!(error, ReadWorkspaceError::Lockfile) {
                    assert_one_line(&message, &what);
                } else {
                    assert_escaped_once(&message, &what);
                }
            }
        }
    }

    #[test]
    fn the_lockfile_remedy_names_the_command_that_aborted_without_a_command_line() {
        // The abort message fills the subcommand from the command that
        // aborted, and names nothing before it: the task is told neither its
        // mount key nor how its command line is reached.
        let expected = |command: &str| {
            format!(
                "Cargo.lock is missing or out of date, and `skeletons` reads it without ever \
                 writing it; run `cargo update --workspace`, then run the `{command}` task again"
            )
        };

        let (check_kind, check_message) =
            abort_message(&ReadWorkspaceError::Lockfile, AbortingCommand::Check);
        let (sync_kind, sync_message) =
            abort_message(&ReadWorkspaceError::Lockfile, AbortingCommand::Sync);

        assert_eq!(check_kind, "lockfile");
        assert_eq!(sync_kind, "lockfile");
        assert_eq!(check_message, expected("check"));
        assert_eq!(sync_message, expected("sync"));
    }

    #[test]
    fn the_wear_lockfile_remedy_says_wear_changes_it_only_to_add_the_skeleton() {
        // `wear` writes `Cargo.lock`, so the words every other command uses,
        // that the lockfile is read and never written, would be untrue of it.
        let (kind, message) = abort_message(&ReadWorkspaceError::Lockfile, AbortingCommand::Wear);

        assert_eq!(kind, "lockfile");
        assert_eq!(
            message,
            "Cargo.lock is missing or out of date, and wear changes it only to add the \
             skeleton; run `cargo update --workspace`, then run the `wear` task again"
        );
    }

    #[test]
    fn a_wear_cargo_failure_is_worded_as_checks_is() {
        // `wear` reads `--locked` with the network allowed, which is what
        // `check` does, so the two name the same command.
        let error = ReadWorkspaceError::CargoMetadataFailed {
            stderr: "error: no manifest".to_owned(),
        };

        assert_eq!(
            abort_message(&error, AbortingCommand::Wear),
            abort_message(&error, AbortingCommand::Check)
        );
    }

    #[test]
    fn a_multi_line_cargo_failure_keeps_every_line_as_one() {
        let error = ReadWorkspaceError::CargoMetadataFailed {
            stderr: "error: failed to load manifest\n\nCaused by:\n  no such file".to_owned(),
        };

        let (kind, message) = abort_message(&error, AbortingCommand::Check);

        assert_eq!(kind, "cargo-metadata-failed");
        assert_eq!(
            message,
            "cargo metadata --locked failed: error: failed to load manifest\\n\\nCaused by:\\n  \
             no such file"
        );
    }
}
