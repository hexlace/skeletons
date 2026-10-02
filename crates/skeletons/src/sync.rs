//! `sync`: rewrite drifted files to match the skeletons this repository
//! wears.
//!
//! It refuses outright, all or nothing, unless every worn skeleton
//! resolves cleanly, every claim is unambiguous, and — for every file it
//! would actually write — the whole git work tree is clean (rule (a)) and
//! git positively proves that what is there is what it would check out
//! (rule (b)). Neither rule is checked at all when there is nothing to
//! write, and neither is anything about git itself (that it is reachable,
//! that no repository-redirecting variable is set): with nothing to
//! overwrite, nothing can be lost, so `sync` then needs no git at all. That
//! refusal is itself all or nothing — one worn skeleton failing any of it
//! stops the whole run before anything is written. The write phase after
//! it is all or nothing only until the first file lands: each write is
//! re-verified against its proof right before it does, and a change found
//! after an earlier write landed, or a rename or link that fails, leaves the
//! earlier writes in place and says which files were written and which were
//! not. `sync::write`'s own module doc names exactly what a partial failure
//! leaves behind and removes (tested by `crates/skeletons/src/sync/write.rs` →
//! `a_failed_commit_removes_every_pending_staging_file_and_now_empty_directory`
//! and → `a_change_found_after_the_first_write_landed_leaves_it_and_names_both_lists`).
//! A file read back after the commit that no longer holds what `sync` wrote
//! is reported as changed, with its path, and never a panic.

mod fold_variant;
mod index_entry;
mod message;
mod proof;
#[cfg(test)]
pub(crate) mod test_repository;
mod write;

use std::path::Path;

use rituals::{Failure, Outcome, Task, clap, report as write_report};

use crate::check::{AbortingCommand, WEARS_NOTHING_LINE, abort_message};
use crate::survey::survey;
use crate::work_tree;
use crate::work_tree::abort::WorkTreeAbort;
use crate::work_tree::clean::{self, Cleanliness};
use crate::work_tree::message::{dirty_summary, report_dirty};
use crate::work_tree::writing_command::WritingCommand;
use crate::workspace::{self, Network};

// `sync` takes no arguments.
//
// No `///` doc comment on this struct, for the same reason
// `check::CheckArguments` carries none: `rituals`' own
// `declare_leaf` sets a task's `about` from the string
// `Task::new` is built with, applied *after* clap's
// derived `augment_args`, but it never clears `long_about` — so a struct
// doc comment would leak through as `--help`'s text instead of the
// intended one. A one-line doc comment would be turned into `about` alone and
// show the intended text, but only by the accident of its length; a plain
// comment keeps `sync` from depending on that.
#[derive(clap::Args)]
struct NoArguments {}

/// Builds the `sync` task.
pub(crate) fn task() -> Task {
    Task::new(
        "rewrite drifted files to match the skeletons this repository wears",
        run,
    )
}

fn run(_arguments: NoArguments) -> Outcome {
    let directory = std::env::current_dir().map_err(|error| {
        Failure::new("could not read the current working directory").caused_by(error)
    })?;

    let workspace = match workspace::read(&directory, Network::Refused) {
        Ok(workspace) => workspace,
        Err(error) => {
            let (_kind, failure_message) = abort_message(&error, AbortingCommand::Sync);
            return Err(Failure::new(failure_message));
        }
    };
    let root = workspace.root.clone();

    // A workspace wearing nothing needs no git at all — sync would write
    // nothing regardless of what it wears — so this answer comes before
    // anything else.
    if workspace.wearing.is_empty() {
        write_report(WEARS_NOTHING_LINE);
        return Ok(());
    }

    let survey = survey(&workspace);
    if !survey.refusals.is_empty() {
        message::report_refusals(&survey.refusals);
        return Err(Failure::new(message::refused_summary(
            survey.refusals.len(),
        )));
    }

    let writes = write::plan(&survey)
        .map_err(|failure| Failure::new(message::write_failure_message(&failure)))?;
    if writes.is_empty() {
        write_report("every bone already matches; nothing was written");
        return Ok(());
    }

    let proven_writes = prove_writes(&root, writes)?;

    let prepared = write::prepare(&root, proven_writes)
        .map_err(|failure| Failure::new(message::write_failure_message(&failure)))?;
    let committed = prepared
        .commit()
        .map_err(|failure| Failure::new(message::write_failure_message(&failure)))?;
    write::verify(&committed)
        .map_err(|failure| Failure::new(message::write_failure_message(&failure)))?;

    message::report_written(&committed);
    // Every write is finished; a staging name that outlived its own link is
    // still a file the wearer must remove, so the run fails after reporting.
    if committed.leftovers().is_empty() {
        Ok(())
    } else {
        Err(Failure::new(message::leftovers_after_success_message(
            committed.leftovers(),
        )))
    }
}

/// Opens the work tree `sync` is about to write into, confirms it is clean,
/// and has git prove what every one of `writes` would overwrite, reporting
/// each refusal's own lines before it returns the `Failure` that stops the
/// run. `writes` is never empty here.
fn prove_writes(
    root: &Path,
    writes: Vec<write::Write>,
) -> Result<Vec<proof::ProvenWrite>, Failure> {
    // Only now, with something to write, does `sync` need git at all: the
    // redirecting-variable refusal and the work-tree question are both
    // about what a write could overwrite, so with nothing to write they have
    // nothing to protect — and answering here rather than earlier is what
    // lets `sync` report "every bone already matches" inside a git hook
    // (which exports `GIT_INDEX_FILE`), outside a git work tree, or with a
    // redirecting variable set. Tested by
    // `ritual/tests/sync_worktree.rs` →
    // `sync_in_a_pre_commit_hook_with_every_bone_matching_needs_no_git_and_exits_zero`.
    let opened_work_tree = work_tree::open(root, |name| std::env::var_os(name).is_some())
        .map_err(|abort| aborted(&abort, root))?;

    let clean =
        match clean::check_clean(&opened_work_tree).map_err(|abort| aborted(&abort, root))? {
            Cleanliness::Clean(clean) => clean,
            Cleanliness::Dirty(dirty) => {
                report_dirty(&dirty);
                return Err(Failure::new(dirty_summary(
                    dirty.len(),
                    WritingCommand::Sync,
                )));
            }
        };

    let proven_writes = match proof::prove(&opened_work_tree, &clean, writes)
        .map_err(|abort| aborted(&abort, root))?
    {
        proof::Proven::All(proven_writes) => proven_writes,
        proof::Proven::Refused(unproven) => {
            message::report_unproven(&unproven);
            return Err(Failure::new(message::unproven_summary(&unproven)));
        }
    };

    Ok(proven_writes)
}

/// The failure for a git question `sync` could not even ask, in `sync`'s own
/// words.
fn aborted(abort: &WorkTreeAbort, root: &Path) -> Failure {
    Failure::new(work_tree::message::abort_message(
        abort,
        root,
        WritingCommand::Sync,
    ))
}
