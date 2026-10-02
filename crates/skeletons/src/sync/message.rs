//! Every message `sync` shows — on refusal, on abort, and on success — kept
//! apart from the orchestration in `sync.rs` itself.

mod hidden;
mod links;

use rituals::report as write_report;

use crate::claim::{ClaimPath, DriftReason};
use crate::skeleton::Escaped;
use crate::survey::count;
use crate::survey::{Refusal, join_and, told_apart, unsafe_path_change_clause};

use super::proof::{Unproven, Why};
use super::write::{
    CollisionAt, CommitCause, CommitFailure, Committed, Leftover, LeftoverReason, StagingRelation,
    TargetChange, WriteFailure,
};
use crate::work_tree::writing_command::WritingCommand;

/// Reports every refusal on stdout, each as `refused: <message>` — the same
/// self-contained text `check` shows for a refusal about no single
/// skeleton, and the same text prefixed with a skeleton's own identity for
/// one about a single skeleton (`Refusal::message`'s own contract). Sorted
/// the same way `--json`'s own `refusals[]` is, so the order does not
/// depend on anything but the refusals themselves.
pub(crate) fn report_refusals(refusals: &[Refusal]) {
    let mut sorted: Vec<&Refusal> = refusals.iter().collect();
    sorted.sort_by_key(|refusal| (refusal.kind(), refusal.message()));
    let lines: Vec<String> = sorted
        .iter()
        .map(|refusal| format!("refused: {}", refusal.message()))
        .collect();
    write_report(lines.join("\n"));
}

/// `sync writes all or nothing, and there is/are <n> refusal(s), so it
/// wrote nothing`.
pub(crate) fn refused_summary(count: usize) -> String {
    format!(
        "sync writes all or nothing, and {}, so it wrote nothing",
        count::there_are_refusals(count)
    )
}

/// The line for a file git could not give the bytes of back, with the remedy
/// that hands git a clean file to check out.
fn not_what_git_checks_out_line(path: &ClaimPath) -> String {
    let path = Escaped(path.as_str());
    format!(
        "{path} is not what git would check out for it, so git could not give these bytes \
         back: keep a copy of it outside the repository, remove it, run `git checkout -- \
         {path}`, then {RUN_SYNC_AGAIN}"
    )
}

/// The line for a git command whose output passed the cap `skeletons` reads.
/// Only a file `sync` would replace has anything to replace; for one it would
/// create, what cannot be shown is that git would see it.
fn output_too_large_line(path: &ClaimPath, command: &str, drift: DriftReason) -> String {
    let path = Escaped(path.as_str());
    let unshown = match drift {
        DriftReason::Changed => "what it would replace",
        DriftReason::Missing => "that git would see the file it would create",
    };
    format!(
        "{path}: git {command} printed more than 16 MiB, the most `skeletons` reads, so sync \
         cannot show {unshown}"
    )
}

/// Reports every unproven path on stdout, one line per path, sorted by its
/// own claim path.
pub(crate) fn report_unproven(unproven: &[Unproven]) {
    let mut sorted: Vec<&Unproven> = unproven.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let lines: Vec<String> = sorted.iter().copied().map(unproven_line).collect();
    write_report(lines.join("\n"));
}

/// One line per unproven path: what is wrong with it, and a remedy that fits
/// that cause and no other, or none where no single command is right for
/// every case. The summary after the lines gives no remedy of its own, since
/// what fixes one path (committing a file git does not hold) breaks another
/// (a file git holds and cannot give back).
fn unproven_line(unproven: &Unproven) -> String {
    let path = Escaped(unproven.path.as_str());
    match &unproven.why {
        Why::NotInIndex => format!(
            "{path} is not in git's index under exactly this spelling: commit it, or move it \
             away, then {RUN_SYNC_AGAIN}"
        ),
        Why::TrackedButAbsent { git_path } => tracked_but_absent_line(&unproven.path, git_path),
        Why::HiddenFromWorkTree { flag } => {
            hidden::hidden_from_work_tree_line(&unproven.path, *flag)
        }
        Why::TrackedAbove { git_path, entry } => {
            hidden::tracked_above_line(&unproven.path, git_path, entry)
        }
        Why::IgnoredByGit { rule } => hidden::ignored_line(&unproven.path, rule),
        Why::IgnoreCheckFailed { detail } => {
            let detail = Escaped(detail);
            format!("{path} could not be checked against git's ignore rules: {detail}")
        }
        Why::ListedAs { git_path } => {
            let git_path = Escaped(git_path);
            format!("{path} is listed in git's index as {git_path}")
        }
        Why::Conflicted => format!(
            "{path} is conflicted in git's index: resolve the conflict and commit it, then \
             {RUN_SYNC_AGAIN}"
        ),
        Why::SymbolicLinkInIndex => format!("{path} is a symbolic link in git's index"),
        Why::SubmoduleInIndex => format!("{path} is a submodule in git's index"),
        Why::UnexpectedMode { mode } => {
            let mode = Escaped(mode);
            format!("{path} has mode {mode} in git's index, not a regular file's")
        }
        Why::IndexEntryUnreadable { detail } => {
            let detail = Escaped(detail);
            format!("{path}'s entry in git's index could not be read: {detail}")
        }
        Why::NotWhatGitChecksOut => not_what_git_checks_out_line(&unproven.path),
        Why::CheckoutNotReproducible => format!(
            "{path} is checked out differently each time git is asked (a content filter whose \
             output is not a function of the file alone), so sync can never show that git \
             would give its bytes back, and does not write it"
        ),
        Why::CheckoutFailed { diagnostic } => {
            let diagnostic = Escaped(diagnostic);
            format!(
                "{path} could not be compared with what git would check out for it: {diagnostic}"
            )
        }
        Why::OutputTooLarge { command } => {
            output_too_large_line(&unproven.path, command, unproven.drift)
        }
        Why::NotARegularFile => {
            format!("{path} is not a regular file: move it away, then {RUN_SYNC_AGAIN}")
        }
        Why::Unreadable { detail } => {
            let detail = Escaped(detail);
            format!("{path} could not be read: {detail}")
        }
        Why::ChangedSinceSurvey => {
            format!("{path} changed while sync was checking it: {RUN_SYNC_AGAIN}")
        }
    }
}

/// `` {path} is tracked in git's index [as {git_path}] but absent from the
/// work tree (skip-worktree or a sparse checkout hides it), so git would
/// ignore what sync wrote there ``. The `as {git_path}` clause appears only
/// when git's entry is not at the claimed path itself but under it.
///
/// When git's entry is neither the claimed path nor beneath it byte for
/// byte, git listed it because it folds case, and the line says that
/// instead: `` {path} is tracked in git's index under another case, as
/// {git_path}, but absent from the work tree (…), so where git ignores case
/// (core.ignorecase) it would take what sync wrote there for that entry and
/// ignore it ``.
fn tracked_but_absent_line(path: &ClaimPath, git_path: &str) -> String {
    let claimed = path.as_str();
    let at_or_beneath_claim = git_path == claimed
        || git_path
            .strip_prefix(claimed)
            .is_some_and(|rest| rest.starts_with('/'));
    let remedy = format!(
        ": bring {} back into the work tree ({}), then {}",
        Escaped(git_path),
        hidden::bring_back_remedy(git_path),
        RUN_SYNC_AGAIN
    );
    if !at_or_beneath_claim {
        // `told_apart` takes the two names as they are and escapes them.
        let shown = told_apart(&[claimed, git_path]);
        return format!(
            "{} is tracked in git's index under another case, as {}, but absent from the work \
             tree{} (skip-worktree or a sparse checkout hides it), so where git ignores case \
             (core.ignorecase) it would take what sync wrote there for that entry and ignore \
             it{remedy}",
            &shown[0],
            &shown[1],
            shown.note_in_parentheses(),
        );
    }
    // Both comparisons above read the names as they are; from here they are
    // only shown.
    let path = Escaped(claimed);
    let tracked_as = if git_path == claimed {
        String::new()
    } else {
        format!(" as {}", Escaped(git_path))
    };
    format!(
        "{path} is tracked in git's index{tracked_as} but absent from the work tree \
         (skip-worktree or a sparse checkout hides it), so git would ignore what sync wrote \
         there{remedy}"
    )
}

/// The closing summary of a refusal, worded for what the refused paths are:
/// `` sync cannot show that git holds what {n} {file|files} would replace ``
/// for paths `sync` would replace, `` sync cannot show that git would see the
/// {n} {file|files} it would create `` for paths it would create (nothing is
/// replaced, so no claim about replacing is made), and both joined by `` , or
/// that it would see the `` … when the refusals are of both kinds. Each ends
/// `` , so it wrote nothing: the {line above says|lines above say} why ``.
///
/// A path is a creation or a replacement by what the survey found at it
/// ([`Unproven::drift`]) and not by why it was refused: several causes (a
/// tracked directory above it, a change since the survey, an unreadable
/// entry) arise for either.
///
/// It offers no remedy of its own. Each line above carries the one that fits
/// its cause, or none, and a summary that names one for all of them names a
/// wrong one for most.
pub(crate) fn unproven_summary(unproven: &[Unproven]) -> String {
    let (replacements, creations) = unproven.iter().fold(
        (0_usize, 0_usize),
        |(replacements, creations), entry| match entry.drift {
            DriftReason::Changed => (replacements + 1, creations),
            DriftReason::Missing => (replacements, creations + 1),
        },
    );
    let cannot_show = match (replacements, creations) {
        (0, 0) => unreachable!("a refusal summary is only written for refused paths"),
        (replaced, 0) => format!(
            "git holds what {replaced} {} would replace",
            count::file(replaced)
        ),
        (0, created) => format!(
            "git would see the {created} {} it would create",
            count::file(created)
        ),
        (replaced, created) => format!(
            "git holds what {replaced} {} would replace, or that it would see the {created} {} \
             it would create",
            count::file(replaced),
            count::file(created)
        ),
    };
    format!(
        "sync cannot show that {cannot_show}, so it wrote nothing: the {} why",
        count::line_above_says_or_lines_above_say(unproven.len())
    )
}

/// The message for a staging name that could be taken for a claimed path,
/// worded for how the two relate. Every path it names is shown through
/// [`told_apart`], so a claimed path that differs from the staging name only
/// in Unicode normalization can be told from it.
fn staging_claimed_message(
    claim: &ClaimPath,
    staging: &str,
    claimed: &ClaimPath,
    relation: StagingRelation,
) -> String {
    let shown = told_apart(&[claim.as_str(), staging, claimed.as_str()]);
    let (claim, staging, claimed) = (&shown[0], &shown[1], &shown[2]);
    let note = shown.note_in_parentheses();
    let how = match relation {
        StagingRelation::Exact => "which is itself a claimed path".to_owned(),
        StagingRelation::Folded => format!(
            "which is one name with the claimed path {claimed} to a filesystem that ignores \
             case or Unicode normalization{note}"
        ),
        StagingRelation::DirectoryAbove => {
            format!("which is a directory above the claimed path {claimed}{note}")
        }
        StagingRelation::Beneath => {
            format!("which is beneath the claimed path {claimed}{note}")
        }
    };
    format!(
        "sync stages {claim} at {staging}, {how}, so it wrote nothing: the two cannot both be \
         written; this is a defect in how the worn skeletons name their files, not in this \
         repository"
    )
}

/// The stderr message for a [`WriteFailure`] — every way `sync` can fail
/// once it has already decided to write.
pub(crate) fn write_failure_message(failure: &WriteFailure) -> String {
    match failure {
        WriteFailure::StagingClaimed {
            claim,
            staging,
            claimed,
            relation,
        } => staging_claimed_message(claim, staging, claimed, *relation),
        WriteFailure::Collision {
            claim,
            shown,
            what,
            leftovers,
        } => collision_message(claim, shown, *what, leftovers),
        WriteFailure::TargetChanged {
            claim,
            what,
            leftovers,
        } => target_changed_message(claim, what, leftovers),
        WriteFailure::FoldsOntoTracked {
            claim,
            variant,
            leftovers,
        } => hidden::folds_onto_tracked_message(claim, variant, leftovers),
        WriteFailure::Prepare {
            path,
            detail,
            leftovers,
        } => {
            let (path, detail) = (Escaped(path.as_str()), Escaped(detail).to_string());
            format!(
                "writing {path} failed, {}",
                nothing_written_with_detail(leftovers, &detail)
            )
        }
        WriteFailure::DirectoryNotWritable {
            path,
            directory,
            detail,
            leftovers,
        } => directory_not_writable_message(path, directory.as_ref(), detail, leftovers),
        WriteFailure::Commit(commit) => commit_failure_message(commit),
        WriteFailure::ChangedAfterWrite { paths } => changed_after_write_message(paths),
    }
}

/// The `DirectoryNotWritable` half of [`write_failure_message`]: `sync` could
/// not create what it stages `path` beside, because this user has no
/// permission to create anything in `directory` (`None` for the workspace
/// root, which reads `the workspace root` and `chmod u+w .`).
fn directory_not_writable_message(
    path: &ClaimPath,
    directory: Option<&ClaimPath>,
    detail: &str,
    leftovers: &[Leftover],
) -> String {
    let (path, detail) = (Escaped(path.as_str()), Escaped(detail));
    let (named, chmod_target) = directory.map_or_else(
        || ("the workspace root".to_owned(), ".".to_owned()),
        |directory| {
            let directory = Escaped(directory.as_str());
            (directory.to_string(), directory.to_string())
        },
    );
    format!(
        "writing {path} failed, because sync cannot create anything in {named} ({detail}), {}",
        nothing_written_with_remedy(
            leftovers,
            &format!(
                "give yourself write permission there (`chmod u+w {chmod_target}`) and \
                 {RUN_SYNC_AGAIN}",
            )
        )
    )
}

/// The `Collision` half of [`write_failure_message`]: something already sat
/// at a path `sync` needed to create exclusively, so it never overwrote or
/// removed it — `shown` names what was found there, `claim` the bone
/// whose write it blocked.
fn collision_message(
    claim: &ClaimPath,
    shown: &str,
    what: CollisionAt,
    leftovers: &[Leftover],
) -> String {
    let (claim, shown) = (Escaped(claim.as_str()), Escaped(shown));
    let base = match what {
        CollisionAt::StagingFile => format!(
            "{shown} is already there, and sync stages {claim} at exactly that path; it never \
             overwrites or removes anything it did not create, so it wrote nothing: move \
             {shown} away, then {RUN_SYNC_AGAIN}",
        ),
        CollisionAt::Directory => format!(
            "{shown} appeared while sync was creating it for {claim}; it never writes into \
             anything it did not create, so it wrote nothing: {RUN_SYNC_AGAIN}",
        ),
    };
    format!("{base}{}", leftovers_clause(leftovers))
}

/// The `TargetChanged` half of [`write_failure_message`]: the claimed
/// target itself, or the path to it, no longer matches what `sync` proved,
/// found while staging it.
fn target_changed_message(
    claim: &ClaimPath,
    what: &TargetChange,
    leftovers: &[Leftover],
) -> String {
    let claim = Escaped(claim.as_str());
    format!(
        "{claim} changed while sync was preparing to write it ({}), {}",
        target_change_clause(what),
        nothing_written_with_remedy(leftovers, RUN_SYNC_AGAIN)
    )
}

/// What changed, as a clause about a path the message has already named.
fn target_change_clause(what: &TargetChange) -> String {
    match what {
        TargetChange::Appeared => "a file appeared there".to_owned(),
        TargetChange::Disappeared => "it is no longer there".to_owned(),
        TargetChange::NoLongerAFile => "it is no longer a regular file".to_owned(),
        TargetChange::ContentChanged => {
            "its content is no longer what sync proved git can give back".to_owned()
        }
        TargetChange::PathUnsafe(cause) => unsafe_path_change_clause(cause),
    }
}

/// The remedy every message that ends by asking for another run gives:
/// [`WritingCommand::run_again`] for `sync`, which says why it names a task.
pub(super) const RUN_SYNC_AGAIN: &str = WritingCommand::Sync.run_again();

/// One leftover as the messages name it: `<path> (<reason>)`.
fn leftover_text(leftover: &Leftover) -> String {
    let path = Escaped(&leftover.path);
    let reason = match &leftover.reason {
        LeftoverReason::CouldNotRemove { detail } => {
            let detail = Escaped(detail);
            format!("it could not be removed: {detail}")
        }
        LeftoverReason::PathUnsafe(cause) => {
            format!("left in place because {}", unsafe_path_change_clause(cause))
        }
        LeftoverReason::SomethingElseThere => {
            "left in place because what is there now is not what sync created".to_owned()
        }
        LeftoverReason::Gone => "no longer where sync created it".to_owned(),
    };
    format!("{path} ({reason})")
}

/// Every leftover, as [`leftover_text`] names it, in the list grammar the
/// crate's messages share.
fn leftover_list(leftovers: &[Leftover]) -> String {
    join_and(&leftovers.iter().map(leftover_text).collect::<Vec<_>>())
}

/// The `` so sync ... wrote nothing `` clause a message carries when the
/// failure came before any file was written and what follows is a remedy to
/// run: `` so sync removed what it had prepared and wrote
/// nothing: <remedy> `` when everything was removed, and otherwise
/// `` so sync wrote nothing, and removed what it had prepared except <list>:
/// find and remove it/them by hand, then <remedy> ``.
fn nothing_written_with_remedy(leftovers: &[Leftover], remedy: &str) -> String {
    if leftovers.is_empty() {
        return format!("so sync removed what it had prepared and wrote nothing: {remedy}");
    }
    format!(
        "so sync wrote nothing, and removed what it had prepared except {}: find and remove {} \
         by hand, then {remedy}",
        leftover_list(leftovers),
        count::it_or_them(leftovers.len()),
    )
}

/// As [`nothing_written_with_remedy`], for a message whose tail is the
/// underlying error rather than a remedy: `` so sync removed what it had
/// prepared and wrote nothing: <detail> ``, and with leftovers `` so sync
/// wrote nothing, and removed what it had prepared except <list>: <detail>;
/// find and remove it/them by hand ``.
///
/// `detail` is message text already: the caller escapes what it took from
/// outside, and this shows it as it comes.
fn nothing_written_with_detail(leftovers: &[Leftover], detail: &str) -> String {
    if leftovers.is_empty() {
        return format!("so sync removed what it had prepared and wrote nothing: {detail}");
    }
    format!(
        "so sync wrote nothing, and removed what it had prepared except {}: {detail}; find and \
         remove {} by hand",
        leftover_list(leftovers),
        count::it_or_them(leftovers.len()),
    )
}

/// Appended when `leftovers` names anything sync left in place or could not
/// remove while cleaning up after a `Collision` or a `Commit` that had
/// already written some files: `` ; sync did not remove everything it had
/// prepared: <list>; find and remove it/them by hand ``. Empty when
/// `leftovers` is empty.
fn leftovers_clause(leftovers: &[Leftover]) -> String {
    if leftovers.is_empty() {
        return String::new();
    }
    format!(
        "; sync did not remove everything it had prepared: {}; find and remove {} by hand",
        leftover_list(leftovers),
        count::it_or_them(leftovers.len()),
    )
}

/// Every path in `paths`, escaped, in the form [`join_and`] takes.
fn escaped_paths<'path>(paths: impl IntoIterator<Item = &'path ClaimPath>) -> Vec<String> {
    paths
        .into_iter()
        .map(|path| Escaped(path.as_str()).to_string())
        .collect()
}

/// The `Commit` half of [`write_failure_message`]: a refusal or failure
/// while landing the writes, before the first one or after some.
fn commit_failure_message(commit: &CommitFailure) -> String {
    if commit.already_written.is_empty() {
        commit_failure_before_any_write(commit)
    } else {
        commit_failure_after_some_writes(commit)
    }
}

/// A commit failure with nothing written yet: the remedy is another run.
fn commit_failure_before_any_write(commit: &CommitFailure) -> String {
    let CommitFailure {
        failed_path,
        cause,
        leftovers,
        ..
    } = commit;
    match cause {
        CommitCause::Changed(what) => {
            let failed_path = Escaped(failed_path.as_str());
            format!(
                "{failed_path} changed while sync was writing it ({}), {}",
                target_change_clause(what),
                nothing_written_with_remedy(leftovers, RUN_SYNC_AGAIN)
            )
        }
        CommitCause::Replace { detail } => {
            let (failed_path, detail) =
                (Escaped(failed_path.as_str()), Escaped(detail).to_string());
            format!(
                "replacing {failed_path} failed before any file was written, {}",
                nothing_written_with_detail(leftovers, &detail)
            )
        }
        // `links` takes the path and the detail as they are and escapes them.
        CommitCause::Create { detail } => {
            links::link_failed_before_any_write(failed_path, detail, leftovers)
        }
    }
}

/// A commit failure after some files landed: says which were and were not
/// written.
fn commit_failure_after_some_writes(commit: &CommitFailure) -> String {
    let CommitFailure {
        failed_path,
        already_written,
        not_yet_written,
        total,
        cause,
        leftovers,
    } = commit;

    let written = already_written.len();
    let written_list = join_and(&escaped_paths(already_written));
    let unwritten = escaped_paths(std::iter::once(failed_path).chain(not_yet_written));
    let unwritten_count = unwritten.len();
    let unwritten_list = join_and(&unwritten);
    let written_file_word = count::file(written);
    let written_was_were = count::was_or_were(written);
    let unwritten_was_were = count::was_or_were(unwritten_count);
    let written_of_total = count::of_total(written, *total, count::file);
    let outcome = format!(
        "after {written_of_total} {written_was_were} written ({written_list}); \
         {unwritten_list} {unwritten_was_were} not written, and git holds what the written \
         {written_file_word} replaced"
    );
    let leftovers_text = leftovers_clause(leftovers);

    match cause {
        CommitCause::Changed(what) => {
            let failed_path = Escaped(failed_path.as_str());
            format!(
                "{failed_path} changed while sync was writing it ({}) {outcome}: \
                 {}{leftovers_text}",
                target_change_clause(what),
                RUN_SYNC_AGAIN
            )
        }
        CommitCause::Replace { detail } => {
            let (failed_path, detail) = (Escaped(failed_path.as_str()), Escaped(detail));
            format!("replacing {failed_path} failed {outcome}: {detail}{leftovers_text}")
        }
        CommitCause::Create { detail } => {
            links::link_failed_after_some_writes(failed_path, detail, &outcome, leftovers)
        }
    }
}

/// The message for a [`WriteFailure::ChangedAfterWrite`]: every write
/// landed, but `paths` no longer read back as sync wrote them.
fn changed_after_write_message(paths: &[ClaimPath]) -> String {
    let list = join_and(&escaped_paths(paths));
    let written_object = count::it_or_them(paths.len());
    let holder = count::it_or_each(paths.len());
    format!(
        "{list} changed after sync wrote {written_object}, so sync cannot confirm what {holder} \
         holds now: run the `check` task to see"
    )
}

/// The failure `sync` ends with when every write landed but `leftovers`, the
/// staging names it prepared, are still there: reported after the
/// `created`/`updated` lines, since every one of those writes is finished.
pub(crate) fn leftovers_after_success_message(leftovers: &[Leftover]) -> String {
    format!(
        "every drifted file was written, but sync did not remove everything it had prepared: \
         {}; find and remove {} by hand",
        leftover_list(leftovers),
        count::it_or_them(leftovers.len()),
    )
}

/// Reports every write `sync` made, in path order, then the closing count
/// line: `created <path> (<skeleton> <version>)` for a `Missing` write and
/// `updated <path> (<skeleton> <version>)` for a `Changed` one.
pub(crate) fn report_written(committed: &Committed) {
    let mut lines: Vec<String> = committed
        .writes()
        .iter()
        .map(|write| written_line(&write.path, &write.skeleton, &write.version, write.reason))
        .collect();

    let created = committed
        .writes()
        .iter()
        .filter(|write| matches!(write.reason, DriftReason::Missing))
        .count();
    let updated = committed
        .writes()
        .iter()
        .filter(|write| matches!(write.reason, DriftReason::Changed))
        .count();
    lines.push(format!(
        "every bone now matches ({updated} updated, {created} created)"
    ));
    write_report(lines.join("\n"));
}

/// The line for one write: `created <path> (<skeleton> <version>)` when the
/// file was missing and `updated <path> (<skeleton> <version>)` when it
/// drifted.
fn written_line(
    path: &ClaimPath,
    skeleton: &str,
    version: &semver::Version,
    reason: DriftReason,
) -> String {
    let (path, skeleton) = (Escaped(path.as_str()), Escaped(skeleton));
    let verb = match reason {
        DriftReason::Missing => "created",
        DriftReason::Changed => "updated",
    };
    format!("{verb} {path} ({skeleton} {version})")
}

#[cfg(test)]
mod outside_text;
#[cfg(test)]
mod poisoned;

#[cfg(test)]
mod tests {
    use super::{
        CollisionAt, CommitCause, CommitFailure, Leftover, LeftoverReason, StagingRelation,
        TargetChange, WriteFailure, directory_not_writable_message,
        leftovers_after_success_message, refused_summary, unproven_summary, write_failure_message,
    };
    use crate::claim::{DriftReason, UnsafePathCause};
    use crate::survey::poison::claim;
    use crate::sync::proof::{Unproven, Why};

    /// A staging file `sync` could not remove: the operating system refused.
    fn stuck(path: &str) -> Leftover {
        Leftover {
            path: path.to_owned(),
            reason: LeftoverReason::CouldNotRemove {
                detail: "permission denied (os error 13)".to_owned(),
            },
        }
    }

    /// A staging file `sync` left in place because a directory above it is
    /// now a link.
    fn under_a_link(path: &str, link: &str) -> Leftover {
        Leftover {
            path: path.to_owned(),
            reason: LeftoverReason::PathUnsafe(UnsafePathCause::SymbolicLinkAbove {
                at: link.to_owned(),
            }),
        }
    }

    #[test]
    fn refused_summary_is_singular_for_one_refusal() {
        assert_eq!(
            refused_summary(1),
            "sync writes all or nothing, and there is 1 refusal, so it wrote nothing"
        );
    }

    #[test]
    fn refused_summary_is_plural_for_several_refusals() {
        assert_eq!(
            refused_summary(2),
            "sync writes all or nothing, and there are 2 refusals, so it wrote nothing"
        );
    }

    /// `count` refusals of one drift, each for a different claimed path.
    fn refused(drift: DriftReason, count: usize) -> Vec<Unproven> {
        (0..count)
            .map(|index| Unproven {
                path: claim(&format!("file{index}.yml")),
                why: Why::NotInIndex,
                drift,
            })
            .collect()
    }

    #[test]
    fn unproven_summary_is_singular_for_one_replacement() {
        assert_eq!(
            unproven_summary(&refused(DriftReason::Changed, 1)),
            "sync cannot show that git holds what 1 file would replace, so it wrote nothing: \
             the line above says why"
        );
    }

    #[test]
    fn unproven_summary_is_plural_for_several_replacements() {
        assert_eq!(
            unproven_summary(&refused(DriftReason::Changed, 2)),
            "sync cannot show that git holds what 2 files would replace, so it wrote nothing: \
             the lines above say why"
        );
    }

    #[test]
    fn unproven_summary_claims_no_replacement_when_every_refusal_is_a_creation() {
        // A creation replaces nothing, so the replacement wording would say
        // something false: the summary speaks of what sync would create.
        assert_eq!(
            unproven_summary(&refused(DriftReason::Missing, 1)),
            "sync cannot show that git would see the 1 file it would create, so it wrote \
             nothing: the line above says why"
        );
        let several = unproven_summary(&refused(DriftReason::Missing, 3));
        assert_eq!(
            several,
            "sync cannot show that git would see the 3 files it would create, so it wrote \
             nothing: the lines above say why"
        );
        assert!(!several.contains("replace"), "{several}");
    }

    #[test]
    fn unproven_summary_says_both_when_replacements_and_creations_are_refused() {
        let mut mixed = refused(DriftReason::Changed, 2);
        mixed.extend(refused(DriftReason::Missing, 1));
        assert_eq!(
            unproven_summary(&mixed),
            "sync cannot show that git holds what 2 files would replace, or that it would see \
             the 1 file it would create, so it wrote nothing: the lines above say why"
        );
    }

    #[test]
    fn the_unproven_summary_offers_no_remedy_of_its_own() {
        // What fixes one path breaks another: committing a file git does not
        // hold, or moving away a file git holds and cannot give back. The
        // per-path lines carry the remedies.
        for (replacements, creations) in [(1, 0), (2, 0), (0, 1), (0, 3), (1, 1), (4, 2)] {
            let mut refusals = refused(DriftReason::Changed, replacements);
            refusals.extend(refused(DriftReason::Missing, creations));
            let summary = unproven_summary(&refusals);
            for remedy in ["commit", "move", "run `"] {
                assert!(!summary.contains(remedy), "{remedy:?} in {summary}");
            }
        }
    }

    #[test]
    fn tracked_but_absent_at_the_claimed_path_itself_omits_the_as_clause() {
        let line = super::tracked_but_absent_line(&claim("plain.yml"), "plain.yml");
        assert_eq!(
            line,
            "plain.yml is tracked in git's index but absent from the work tree (skip-worktree \
             or a sparse checkout hides it), so git would ignore what sync wrote there: bring \
             plain.yml back into the work tree (`git sparse-checkout add /plain.yml` if a \
             sparse checkout leaves it out, or `git update-index --no-skip-worktree -- \
             plain.yml` and then `git checkout -- plain.yml`), then run the `sync` task again"
        );
    }

    #[test]
    fn tracked_but_absent_under_the_claimed_path_names_git_s_own_entry() {
        let line = super::tracked_but_absent_line(&claim("sub"), "sub/inner.txt");
        assert_eq!(
            line,
            "sub is tracked in git's index as sub/inner.txt but absent from the work tree \
             (skip-worktree or a sparse checkout hides it), so git would ignore what sync wrote \
             there: bring sub/inner.txt back into the work tree (`git sparse-checkout add sub` \
             if a sparse checkout leaves it out, or `git update-index --no-skip-worktree -- \
             sub/inner.txt` and then `git checkout -- sub/inner.txt`), then run the `sync` \
             task again"
        );
    }

    #[test]
    fn an_entry_under_another_case_says_so_and_names_what_git_would_take_it_for() {
        // The three shapes: git's entry is another case of the claim, of
        // its directory, or beneath another case of the claim itself. The
        // wording for an entry at or beneath the claim byte for byte is
        // pinned by the two tests above, and each ends by bringing back the
        // entry git holds, spelled as git holds it.
        for (claimed, git_path, sparse) in [
            ("A.yml", "a.yml", "/a.yml"),
            ("d/f.yml", "D/f.yml", "D"),
            ("d", "D/f.yml", "D"),
        ] {
            let unproven = Unproven {
                path: claim(claimed),
                drift: DriftReason::Changed,
                why: Why::TrackedButAbsent {
                    git_path: git_path.to_owned(),
                },
            };
            assert_eq!(
                super::unproven_line(&unproven),
                format!(
                    "{claimed} is tracked in git's index under another case, as {git_path}, but \
                     absent from the work tree (skip-worktree or a sparse checkout hides it), \
                     so where git ignores case (core.ignorecase) it would take what sync wrote \
                     there for that entry and ignore it: bring {git_path} back into the work \
                     tree (`git sparse-checkout add {sparse}` if a sparse checkout leaves it \
                     out, or `git update-index --no-skip-worktree -- {git_path}` and then `git \
                     checkout -- {git_path}`), then run the `sync` task again"
                )
            );
        }
    }

    #[test]
    fn two_spellings_of_one_name_git_folds_are_told_apart_by_code_point() {
        // A claim `é.yml` and git's entry `É.yml` are visibly different (a
        // case variant), so they stay readable. A claim and an entry that
        // differ only in normalization would not be, and are shown as code
        // points: the case form calls `told_apart` on both names.
        let unproven = Unproven {
            path: claim("caf\u{e9}.yml"),
            drift: DriftReason::Changed,
            why: Why::TrackedButAbsent {
                git_path: "cafe\u{301}.yml".to_owned(),
            },
        };
        let line = super::unproven_line(&unproven);
        assert!(
            line.starts_with(
                "caf\u{e9}.yml (caf\\u00E9.yml) is tracked in git's index under another case, as \
                 cafe\u{301}.yml (cafe\\u0301.yml), but absent from the work tree (the two \
                 spellings differ only in Unicode normalization"
            ),
            "{line}"
        );
    }

    #[test]
    fn an_entry_that_only_shares_a_prefix_with_the_claim_is_under_another_case() {
        // `sub` is a prefix of `subtle.yml` byte for byte but not a directory
        // above it, so this entry is not beneath the claim; git only listed
        // it because it folds.
        let unproven = Unproven {
            path: claim("sub"),
            drift: DriftReason::Changed,
            why: Why::TrackedButAbsent {
                git_path: "Subtle.yml".to_owned(),
            },
        };
        assert!(
            super::unproven_line(&unproven).contains("under another case, as Subtle.yml"),
            "got {}",
            super::unproven_line(&unproven)
        );
    }

    #[test]
    fn a_file_that_is_not_what_git_checks_out_names_the_path_and_a_remedy_that_keeps_a_copy() {
        let unproven = Unproven {
            path: claim("sub/plain.yml"),
            drift: DriftReason::Changed,
            why: Why::NotWhatGitChecksOut,
        };
        assert_eq!(
            super::unproven_line(&unproven),
            "sub/plain.yml is not what git would check out for it, so git could not give these \
             bytes back: keep a copy of it outside the repository, remove it, run `git checkout \
             -- sub/plain.yml`, then run the `sync` task again"
        );
    }

    #[test]
    fn a_checkout_that_differs_each_time_names_the_cause_and_offers_no_remedy() {
        let unproven = Unproven {
            path: claim("plain.yml"),
            drift: DriftReason::Changed,
            why: Why::CheckoutNotReproducible,
        };
        let line = super::unproven_line(&unproven);
        assert_eq!(
            line,
            "plain.yml is checked out differently each time git is asked (a content filter \
             whose output is not a function of the file alone), so sync can never show that git \
             would give its bytes back, and does not write it"
        );
        assert!(!line.contains("git checkout"), "{line}");
    }

    /// The line for an unproven `plain.yml` refused for `why`, which the
    /// survey found in `drift`.
    fn unproven_text(why: Why, drift: DriftReason) -> String {
        super::unproven_line(&Unproven {
            path: claim("plain.yml"),
            drift,
            why,
        })
    }

    #[test]
    fn each_remedy_a_line_names_fits_its_cause() {
        let line = |why| unproven_text(why, DriftReason::Changed);
        assert_eq!(
            line(Why::NotInIndex),
            "plain.yml is not in git's index under exactly this spelling: commit it, or move it \
             away, then run the `sync` task again"
        );
        assert_eq!(
            line(Why::Conflicted),
            "plain.yml is conflicted in git's index: resolve the conflict and commit it, then \
             run the `sync` task again"
        );
        assert_eq!(
            line(Why::NotARegularFile),
            "plain.yml is not a regular file: move it away, then run the `sync` task \
             again"
        );
        assert_eq!(
            line(Why::ChangedSinceSurvey),
            "plain.yml changed while sync was checking it: run the `sync` task again"
        );
    }

    #[test]
    fn an_output_too_large_line_says_what_it_cannot_show_for_a_replacement_and_for_a_creation() {
        assert_eq!(
            unproven_text(
                Why::OutputTooLarge {
                    command: "cat-file"
                },
                DriftReason::Changed
            ),
            "plain.yml: git cat-file printed more than 16 MiB, the most `skeletons` reads, so sync \
             cannot show what it would replace"
        );
        assert_eq!(
            unproven_text(
                Why::OutputTooLarge {
                    command: "ls-files"
                },
                DriftReason::Missing
            ),
            "plain.yml: git ls-files printed more than 16 MiB, the most `skeletons` reads, so sync \
             cannot show that git would see the file it would create",
            "a file sync would create has nothing to replace"
        );
    }

    #[test]
    fn the_lines_that_deliberately_name_no_remedy_name_none() {
        // For these the line names the fact, and no single command is right
        // for every case that produces it.
        for why in [
            Why::SymbolicLinkInIndex,
            Why::SubmoduleInIndex,
            Why::UnexpectedMode {
                mode: "100664".to_owned(),
            },
            Why::IndexEntryUnreadable {
                detail: "detail".to_owned(),
            },
            Why::ListedAs {
                git_path: "PLAIN.yml".to_owned(),
            },
            Why::Unreadable {
                detail: "detail".to_owned(),
            },
            Why::CheckoutFailed {
                diagnostic: "detail".to_owned(),
            },
        ] {
            let text = unproven_text(why, DriftReason::Changed);
            for remedy in [
                "run the `sync` task",
                "run the `check` task",
                "commit",
                "move it",
                "run `",
            ] {
                assert!(!text.contains(remedy), "{remedy:?} in {text}");
            }
        }
    }

    #[test]
    fn a_checkout_git_could_not_produce_names_git_s_own_detail() {
        let unproven = Unproven {
            path: claim("plain.yml"),
            drift: DriftReason::Changed,
            why: Why::CheckoutFailed {
                diagnostic: "cannot read object 0000".to_owned(),
            },
        };
        assert_eq!(
            super::unproven_line(&unproven),
            "plain.yml could not be compared with what git would check out for it: cannot read \
             object 0000"
        );
    }

    #[test]
    fn a_prepare_failure_names_the_path_and_the_io_error() {
        let message = write_failure_message(&WriteFailure::Prepare {
            path: claim(".github/dependabot.yml"),
            detail: "permission denied (os error 13)".to_owned(),
            leftovers: Vec::new(),
        });
        assert_eq!(
            message,
            "writing .github/dependabot.yml failed, so sync removed what it had prepared and \
             wrote nothing: permission denied (os error 13)"
        );
    }

    #[test]
    fn a_prepare_failure_names_what_it_could_not_remove() {
        let message = write_failure_message(&WriteFailure::Prepare {
            path: claim(".github/dependabot.yml"),
            detail: "permission denied (os error 13)".to_owned(),
            leftovers: vec![stuck(".github/.dependabot.yml.skeletons-sync")],
        });
        assert_eq!(
            message,
            "writing .github/dependabot.yml failed, so sync wrote nothing, and removed what it \
             had prepared except .github/.dependabot.yml.skeletons-sync (it could not be removed: \
             permission denied (os error 13)): permission denied (os error 13); find and \
             remove it by hand"
        );
    }

    fn staging_claimed_message_for(claimed: &str, relation: StagingRelation) -> String {
        write_failure_message(&WriteFailure::StagingClaimed {
            claim: claim("a/b"),
            staging: "a/.b.skeletons-sync".to_owned(),
            claimed: claim(claimed),
            relation,
        })
    }

    const STAGING_CLAIMED_TAIL: &str = "so it wrote nothing: the two cannot both be written; \
         this is a defect in how the worn skeletons name their files, not in this repository";

    #[test]
    fn a_staging_claimed_failure_by_exact_equality_names_the_defect() {
        assert_eq!(
            staging_claimed_message_for("a/.b.skeletons-sync", StagingRelation::Exact),
            format!(
                "sync stages a/b at a/.b.skeletons-sync, which is itself a claimed path, \
                 {STAGING_CLAIMED_TAIL}"
            )
        );
    }

    #[test]
    fn a_staging_claimed_failure_by_the_fold_names_the_other_spelling() {
        assert_eq!(
            staging_claimed_message_for("a/.B.skeletons-sync", StagingRelation::Folded),
            format!(
                "sync stages a/b at a/.b.skeletons-sync, which is one name with the claimed path \
                 a/.B.skeletons-sync to a filesystem that ignores case or Unicode normalization, \
                 {STAGING_CLAIMED_TAIL}"
            )
        );
    }

    #[test]
    fn a_staging_claimed_failure_by_an_ancestor_names_the_claimed_path() {
        assert_eq!(
            staging_claimed_message_for("a/.b.skeletons-sync/c", StagingRelation::DirectoryAbove),
            format!(
                "sync stages a/b at a/.b.skeletons-sync, which is a directory above the claimed \
                 path a/.b.skeletons-sync/c, {STAGING_CLAIMED_TAIL}"
            )
        );
    }

    #[test]
    fn a_staging_claimed_failure_beneath_a_claim_names_the_claimed_path() {
        assert_eq!(
            staging_claimed_message_for("a", StagingRelation::Beneath),
            format!(
                "sync stages a/b at a/.b.skeletons-sync, which is beneath the claimed path a, \
                 {STAGING_CLAIMED_TAIL}"
            )
        );
    }

    #[test]
    fn a_staging_file_collision_names_what_was_found_and_the_remedy() {
        let message = write_failure_message(&WriteFailure::Collision {
            claim: claim("plain.yml"),
            shown: ".plain.yml.skeletons-sync".to_owned(),
            what: CollisionAt::StagingFile,
            leftovers: Vec::new(),
        });
        assert_eq!(
            message,
            ".plain.yml.skeletons-sync is already there, and sync stages plain.yml at exactly that \
             path; it never overwrites or removes anything it did not create, so it wrote \
             nothing: move .plain.yml.skeletons-sync away, then run the `sync` task again"
        );
    }

    #[test]
    fn a_directory_collision_names_the_remedy() {
        let message = write_failure_message(&WriteFailure::Collision {
            claim: claim("a/plain.yml"),
            shown: "a".to_owned(),
            what: CollisionAt::Directory,
            leftovers: Vec::new(),
        });
        assert_eq!(
            message,
            "a appeared while sync was creating it for a/plain.yml; it never writes into \
             anything it did not create, so it wrote nothing: run the \
             `sync` task again"
        );
    }

    #[test]
    fn a_collision_names_what_it_could_not_remove_while_cleaning_up() {
        let message = write_failure_message(&WriteFailure::Collision {
            claim: claim("b.yml"),
            shown: ".b.yml.skeletons-sync".to_owned(),
            what: CollisionAt::StagingFile,
            leftovers: vec![stuck("a.yml.skeletons-sync"), stuck("c.yml.skeletons-sync")],
        });
        assert_eq!(
            message,
            ".b.yml.skeletons-sync is already there, and sync stages b.yml at exactly that path; it \
             never overwrites or removes anything it did not create, so it wrote nothing: move \
             .b.yml.skeletons-sync away, then run the `sync` task again; sync did not \
             remove everything it had prepared: a.yml.skeletons-sync (it could not be removed: \
             permission denied (os error 13)) and c.yml.skeletons-sync (it could not be removed: \
             permission denied (os error 13)); find and remove them by hand"
        );
    }

    #[test]
    fn a_target_that_appeared_names_the_remedy() {
        let message = write_failure_message(&WriteFailure::TargetChanged {
            claim: claim("a.yml"),
            what: TargetChange::Appeared,
            leftovers: Vec::new(),
        });
        assert_eq!(
            message,
            "a.yml changed while sync was preparing to write it (a file appeared there), so \
             sync removed what it had prepared and wrote nothing: run the `sync` task \
             again"
        );
    }

    #[test]
    fn a_target_that_is_no_longer_a_regular_file_names_the_remedy() {
        let message = write_failure_message(&WriteFailure::TargetChanged {
            claim: claim("a.yml"),
            what: TargetChange::NoLongerAFile,
            leftovers: vec![stuck("a.yml.skeletons-sync")],
        });
        assert_eq!(
            message,
            "a.yml changed while sync was preparing to write it (it is no longer a regular \
             file), so sync wrote nothing, and removed what it had prepared except \
             a.yml.skeletons-sync (it could not be removed: permission denied (os error 13)): find \
             and remove it by hand, then run the `sync` task again"
        );
    }

    #[test]
    fn a_staging_file_left_under_a_swapped_directory_is_named_with_why() {
        let message = write_failure_message(&WriteFailure::TargetChanged {
            claim: claim("x/f10.yml"),
            what: TargetChange::PathUnsafe(UnsafePathCause::SymbolicLinkAbove {
                at: "x".to_owned(),
            }),
            leftovers: vec![under_a_link("x/.f1.yml.skeletons-sync", "x")],
        });
        assert_eq!(
            message,
            "x/f10.yml changed while sync was preparing to write it (x above it is now a \
             symbolic link), so sync wrote nothing, and removed what it had prepared except \
             x/.f1.yml.skeletons-sync (left in place because x above it is now a symbolic link): \
             find and remove it by hand, then run the `sync` task again"
        );
    }

    #[test]
    fn each_reason_a_leftover_can_have_reads_as_its_own_clause() {
        let leftover = |reason| Leftover {
            path: "a/.b.yml.skeletons-sync".to_owned(),
            reason,
        };
        let message = |reason| {
            write_failure_message(&WriteFailure::Collision {
                claim: claim("c.yml"),
                shown: ".c.yml.skeletons-sync".to_owned(),
                what: CollisionAt::StagingFile,
                leftovers: vec![leftover(reason)],
            })
        };
        assert!(message(LeftoverReason::SomethingElseThere).ends_with(
            "; sync did not remove everything it had prepared: a/.b.yml.skeletons-sync (left in \
                 place because what is there now is not what sync created); find and remove it \
                 by hand"
        ));
        assert!(message(LeftoverReason::Gone).ends_with(
            "; sync did not remove everything it had prepared: a/.b.yml.skeletons-sync (no longer \
             where sync created it); find and remove it by hand"
        ));
    }

    fn commit_failure(
        failed: &str,
        already_written: &[&str],
        not_yet_written: &[&str],
        cause: CommitCause,
        leftovers: &[&str],
    ) -> WriteFailure {
        WriteFailure::Commit(Box::new(CommitFailure {
            failed_path: claim(failed),
            already_written: already_written.iter().map(|path| claim(path)).collect(),
            not_yet_written: not_yet_written.iter().map(|path| claim(path)).collect(),
            total: 1 + already_written.len() + not_yet_written.len(),
            cause,
            leftovers: leftovers.iter().map(|path| stuck(path)).collect(),
        }))
    }

    fn replace(detail: &str) -> CommitCause {
        CommitCause::Replace {
            detail: detail.to_owned(),
        }
    }

    #[test]
    fn a_rename_failure_before_anything_was_written_says_so_and_names_nothing() {
        let message = write_failure_message(&commit_failure(
            "a.yml",
            &[],
            &[],
            replace("disk full"),
            &[],
        ));
        assert_eq!(
            message,
            "replacing a.yml failed before any file was written, so sync removed what it had \
             prepared and wrote nothing: disk full"
        );
    }

    #[test]
    fn a_link_failure_before_anything_was_written_says_creating() {
        let cause = CommitCause::Create {
            detail: "no space left on device".to_owned(),
        };
        let message = write_failure_message(&commit_failure("a.yml", &[], &[], cause, &[]));
        assert_eq!(
            message,
            "creating a.yml failed before any file was written, so sync removed what it had \
             prepared and wrote nothing: sync creates new files with hard links, and the link \
             failed: no space left on device"
        );
    }

    #[test]
    fn a_rename_failure_after_one_of_two_names_the_one_file_already_written() {
        let message = write_failure_message(&commit_failure(
            ".github/workflows/ci.yml",
            &[".github/dependabot.yml"],
            &[],
            replace("permission denied (os error 13)"),
            &[],
        ));
        assert_eq!(
            message,
            "replacing .github/workflows/ci.yml failed after 1 of 2 files was written \
             (.github/dependabot.yml); .github/workflows/ci.yml was not written, and git holds \
             what the written file replaced: permission denied (os error 13)"
        );
    }

    #[test]
    fn a_rename_failure_after_two_of_three_pluralises_was_and_file() {
        let message = write_failure_message(&commit_failure(
            "c.yml",
            &["a.yml", "b.yml"],
            &[],
            replace("disk full"),
            &[],
        ));
        assert_eq!(
            message,
            "replacing c.yml failed after 2 of 3 files were written (a.yml and b.yml); c.yml \
             was not written, and git holds what the written files replaced: disk full"
        );
    }

    #[test]
    fn a_rename_failure_names_every_path_still_queued_after_it() {
        let message = write_failure_message(&commit_failure(
            "b.yml",
            &["a.yml"],
            &["c.yml"],
            replace("disk full"),
            &[],
        ));
        assert_eq!(
            message,
            "replacing b.yml failed after 1 of 3 files was written (a.yml); b.yml and c.yml \
             were not written, and git holds what the written file replaced: disk full"
        );
    }

    #[test]
    fn a_rename_failure_after_some_writes_names_what_it_could_not_remove() {
        let message = write_failure_message(&commit_failure(
            "b.yml",
            &["a.yml"],
            &[],
            replace("disk full"),
            &["b.yml.skeletons-sync"],
        ));
        assert_eq!(
            message,
            "replacing b.yml failed after 1 of 2 files was written (a.yml); b.yml was not \
             written, and git holds what the written file replaced: disk full; sync did not \
             remove everything it had prepared: b.yml.skeletons-sync (it could not be removed: \
             permission denied (os error 13)); find and remove it by hand"
        );
    }

    #[test]
    fn a_link_failure_after_some_writes_says_creating() {
        let cause = CommitCause::Create {
            detail: "no space left on device".to_owned(),
        };
        let message = write_failure_message(&commit_failure("b.yml", &["a.yml"], &[], cause, &[]));
        assert_eq!(
            message,
            "creating b.yml failed after 1 of 2 files was written (a.yml); b.yml was not \
             written, and git holds what the written file replaced: sync creates new files with \
             hard links, and the link failed: no space left on device"
        );
    }

    #[test]
    fn a_change_before_any_write_says_sync_wrote_nothing_and_names_the_remedy() {
        let cause = CommitCause::Changed(TargetChange::ContentChanged);
        let message = write_failure_message(&commit_failure("a.yml", &[], &["b.yml"], cause, &[]));
        assert_eq!(
            message,
            "a.yml changed while sync was writing it (its content is no longer what sync proved \
             git can give back), so sync removed what it had prepared and wrote nothing: run the \
             `sync` task again"
        );
    }

    #[test]
    fn a_change_after_some_writes_names_what_was_and_was_not_written() {
        let cause = CommitCause::Changed(TargetChange::Appeared);
        let message =
            write_failure_message(&commit_failure("b.yml", &["a.yml"], &["c.yml"], cause, &[]));
        assert_eq!(
            message,
            "b.yml changed while sync was writing it (a file appeared there) after 1 of 3 files \
             was written (a.yml); b.yml and c.yml were not written, and git holds what the \
             written file replaced: run the `sync` task again"
        );
    }

    #[test]
    fn a_change_after_some_writes_names_what_it_could_not_remove() {
        let cause = CommitCause::Changed(TargetChange::Disappeared);
        let message = write_failure_message(&commit_failure(
            "b.yml",
            &["a.yml"],
            &[],
            cause,
            &[".b.yml.skeletons-sync"],
        ));
        assert_eq!(
            message,
            "b.yml changed while sync was writing it (it is no longer there) after 1 of 2 files \
             was written (a.yml); b.yml was not written, and git holds what the written file \
             replaced: run the `sync` task again; sync did not remove everything it \
             had prepared: .b.yml.skeletons-sync (it could not be removed: permission denied (os \
             error 13)); find and remove it by hand"
        );
    }

    #[test]
    fn a_path_that_became_unsafe_is_named_by_the_same_cause_check_reports() {
        let message = write_failure_message(&WriteFailure::TargetChanged {
            claim: claim("x/one.yml"),
            what: TargetChange::PathUnsafe(UnsafePathCause::SymbolicLinkAbove {
                at: "x".to_owned(),
            }),
            leftovers: Vec::new(),
        });
        assert_eq!(
            message,
            "x/one.yml changed while sync was preparing to write it (x above it is now a \
             symbolic link), so sync removed what it had prepared and wrote nothing: run the \
             `sync` task again"
        );
    }

    #[test]
    fn a_stage_time_change_names_a_vanished_file_and_changed_content() {
        for (what, clause) in [
            (TargetChange::Disappeared, "it is no longer there"),
            (
                TargetChange::ContentChanged,
                "its content is no longer what sync proved git can give back",
            ),
        ] {
            let message = write_failure_message(&WriteFailure::TargetChanged {
                claim: claim("a.yml"),
                what,
                leftovers: Vec::new(),
            });
            assert!(message.contains(&format!("({clause})")), "{message}");
        }
    }

    #[test]
    fn a_file_changed_after_sync_wrote_it_names_it_and_the_check_that_shows_what_it_holds() {
        let message = write_failure_message(&WriteFailure::ChangedAfterWrite {
            paths: vec![claim("a.yml")],
        });
        assert_eq!(
            message,
            "a.yml changed after sync wrote it, so sync cannot confirm what it holds now: run the \
             `check` task to see"
        );
    }

    #[test]
    fn several_files_changed_after_sync_wrote_them_are_all_named() {
        let message = write_failure_message(&WriteFailure::ChangedAfterWrite {
            paths: vec![claim("a.yml"), claim("b.yml")],
        });
        assert_eq!(
            message,
            "a.yml and b.yml changed after sync wrote them, so sync cannot confirm what each \
             holds now: run the `check` task to see"
        );
    }

    #[test]
    fn a_staging_name_that_survived_its_link_is_named_after_every_file_was_written() {
        assert_eq!(
            leftovers_after_success_message(&[stuck("a/.b.yml.skeletons-sync")]),
            "every drifted file was written, but sync did not remove everything it had \
             prepared: a/.b.yml.skeletons-sync (it could not be removed: permission denied (os \
             error 13)); find and remove it by hand"
        );
        assert_eq!(
            leftovers_after_success_message(&[stuck("a"), stuck("b")]),
            "every drifted file was written, but sync did not remove everything it had \
             prepared: a (it could not be removed: permission denied (os error 13)) and b (it \
             could not be removed: permission denied (os error 13)); find and remove them by \
             hand"
        );
    }

    #[test]
    fn an_unwritable_directory_names_it_and_the_permission_to_give() {
        let message = directory_not_writable_message(
            &claim("d/x.yml"),
            Some(&claim("d")),
            "Permission denied (os error 13)",
            &[],
        );

        assert_eq!(
            message,
            "writing d/x.yml failed, because sync cannot create anything in d (Permission \
             denied (os error 13)), so sync removed what it had prepared and wrote nothing: \
             give yourself write permission there (`chmod u+w d`) and run the `sync` task again"
        );
    }

    #[test]
    fn an_unwritable_workspace_root_reads_as_the_workspace_root_and_chmods_dot() {
        let message = directory_not_writable_message(
            &claim("x.yml"),
            None,
            "Permission denied (os error 13)",
            &[stuck(".x.yml.skeletons-sync")],
        );

        assert!(
            message.starts_with(
                "writing x.yml failed, because sync cannot create anything in the workspace \
                 root (Permission denied (os error 13)), so sync wrote nothing, and removed \
                 what it had prepared except .x.yml.skeletons-sync"
            ),
            "{message}"
        );
        assert!(
            message.ends_with(
                "give yourself write permission there (`chmod u+w .`) and run the `sync` task again"
            ),
            "{message}"
        );
    }
}
