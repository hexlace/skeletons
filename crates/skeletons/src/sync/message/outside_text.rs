//! Every message `sync` builds prints the text it names from outside (a
//! path, a name git or the operating system gave, a rule, a diagnostic)
//! escaped exactly once, and stays on one line.
//!
//! Each test puts the poison, a name with a line break in it, into every
//! outside field of every kind of one enum and asserts the message holds no
//! line break and shows the name as the two characters `\n`, once. Every enum
//! is counted through a match that has no wildcard, so a variant added without
//! an arm does not compile, and the sample list must then cover it.

use std::path::Path;

use super::poisoned::poisoned_leftover;
use super::{
    dirty_line, leftover_text, leftovers_after_success_message, sync_abort_message, unproven_line,
    write_failure_message, written_line,
};
use crate::claim::{DriftReason, UnsafePathCause};
use crate::survey::poison::{
    POISON, POISON_FOLDED, assert_escaped_once, assert_every_kind, assert_one_line, claim, poison,
};
use crate::sync::abort::{GitQuestion, SyncAbort};
use crate::sync::clean::{Dirt, DirtyPath};
use crate::sync::fold_variant::{FoldVariant, fold_variants};
use crate::sync::proof::{AboveEntry, HiddenFlag, Unproven, Why};
use crate::sync::write::{
    CollisionAt, CommitCause, CommitFailure, Leftover, LeftoverReason, StagingRelation,
    TargetChange, WriteFailure,
};

/// Whether a message of some kind names text from outside, and so must show
/// it escaped; a message that names none is only asked to stay on one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutsideText {
    Named,
    NotNamed,
}

/// One kind of an enum a message reads: its place among the kinds, and
/// whether its message names text from outside.
#[derive(Clone, Copy, Debug)]
struct Kind {
    index: usize,
    outside_text: OutsideText,
}

const fn named(index: usize) -> Kind {
    Kind {
        index,
        outside_text: OutsideText::Named,
    }
}

const fn not_named(index: usize) -> Kind {
    Kind {
        index,
        outside_text: OutsideText::NotNamed,
    }
}

/// Asserts `text` is one line, and shows the poison escaped once if `kind`'s
/// message names any.
fn assert_message(text: &str, kind: Kind, what: &str) {
    match kind.outside_text {
        OutsideText::Named => assert_escaped_once(text, what),
        OutsideText::NotNamed => assert_one_line(text, what),
    }
}

// ---------------------------------------------------------------------------
// SyncAbort and GitQuestion
// ---------------------------------------------------------------------------

/// How many kinds of [`SyncAbort`] there are.
const ABORT_KINDS: usize = 7;

/// A timed-out abort names what the question it timed out on names, so its
/// kind reads that question's.
const fn abort_kind(abort: &SyncAbort) -> Kind {
    match abort {
        SyncAbort::RedirectedGit { .. } => not_named(0),
        SyncAbort::NotAWorkTree => named(1),
        SyncAbort::DubiousOwnership { .. } => named(2),
        SyncAbort::GitUnavailable { .. } => named(3),
        SyncAbort::GitTimedOut { question } => Kind {
            index: 4,
            outside_text: question_kind(question).outside_text,
        },
        SyncAbort::GitFailed { .. } => named(5),
        SyncAbort::GitOutputTooLarge { .. } => not_named(6),
    }
}

/// How many kinds of [`GitQuestion`] there are.
const QUESTION_KINDS: usize = 7;

const fn question_kind(question: &GitQuestion) -> Kind {
    match question {
        GitQuestion::WorkTree => not_named(0),
        GitQuestion::Status => not_named(1),
        GitQuestion::IndexEntry(_) => named(2),
        GitQuestion::IndexAbove(_) => named(3),
        GitQuestion::IndexListing => not_named(4),
        GitQuestion::Checkout(_) => named(5),
        GitQuestion::Ignored(_) => named(6),
    }
}

fn question_samples() -> Vec<GitQuestion> {
    vec![
        GitQuestion::WorkTree,
        GitQuestion::Status,
        GitQuestion::IndexEntry(claim(POISON)),
        GitQuestion::IndexAbove(claim(POISON)),
        GitQuestion::IndexListing,
        GitQuestion::Checkout(claim(POISON)),
        GitQuestion::Ignored(claim(POISON)),
    ]
}

fn abort_samples() -> Vec<SyncAbort> {
    let mut samples = vec![
        SyncAbort::RedirectedGit {
            variables: vec!["GIT_DIR"],
        },
        SyncAbort::NotAWorkTree,
        SyncAbort::DubiousOwnership {
            diagnostic: poison(),
        },
        SyncAbort::GitUnavailable { detail: poison() },
        SyncAbort::GitFailed {
            command: "status",
            diagnostic: poison(),
        },
        SyncAbort::GitOutputTooLarge { command: "status" },
    ];
    samples.extend(
        question_samples()
            .into_iter()
            .map(|question| SyncAbort::GitTimedOut { question }),
    );
    samples
}

#[test]
fn every_sync_abort_prints_its_outside_text_escaped_once() {
    // The root the message names when the directory is no work tree is
    // poisoned too. Kinds that name nothing from outside are still one line.
    let samples = abort_samples();
    assert_every_kind(
        samples.iter().map(|abort| abort_kind(abort).index),
        ABORT_KINDS,
        "SyncAbort",
    );

    for abort in &samples {
        let text = sync_abort_message(abort, Path::new(POISON));
        assert_message(&text, abort_kind(abort), &format!("{abort:?}"));
    }
}

#[test]
fn every_git_question_names_its_path_escaped_once() {
    let samples = question_samples();
    assert_every_kind(
        samples.iter().map(|question| question_kind(question).index),
        QUESTION_KINDS,
        "GitQuestion",
    );

    for question in &samples {
        let text = super::timed_out::timed_out_message(question);
        assert_message(&text, question_kind(question), &format!("{question:?}"));
    }
}

// ---------------------------------------------------------------------------
// Dirty paths
// ---------------------------------------------------------------------------

/// How many kinds of [`Dirt`] there are.
const DIRT_KINDS: usize = 8;

const fn dirt_kind(dirt: Dirt) -> usize {
    match dirt {
        Dirt::Unstaged => 0,
        Dirt::Staged => 1,
        Dirt::Deleted => 2,
        Dirt::Untracked => 3,
        Dirt::IntentToAdd => 4,
        Dirt::Conflicted => 5,
        Dirt::Submodule => 6,
        Dirt::Unreadable => 7,
    }
}

#[test]
fn every_dirty_path_prints_its_name_escaped_once() {
    // `shown` is a path, or for an unreadable record the record itself.
    let samples = [
        Dirt::Unstaged,
        Dirt::Staged,
        Dirt::Deleted,
        Dirt::Untracked,
        Dirt::IntentToAdd,
        Dirt::Conflicted,
        Dirt::Submodule,
        Dirt::Unreadable,
    ];
    assert_every_kind(
        samples.iter().map(|dirt| dirt_kind(*dirt)),
        DIRT_KINDS,
        "Dirt",
    );

    for dirt in samples {
        let dirty = DirtyPath {
            shown: poison(),
            dirt,
        };
        assert_escaped_once(&dirty_line(&dirty), &format!("{dirt:?}"));
    }
}

// ---------------------------------------------------------------------------
// Unproven paths
// ---------------------------------------------------------------------------

/// How many kinds of [`Why`] there are.
const WHY_KINDS: usize = 19;

const fn why_kind(why: &Why) -> usize {
    match why {
        Why::NotInIndex => 0,
        Why::TrackedButAbsent { .. } => 1,
        Why::HiddenFromWorkTree { .. } => 2,
        Why::TrackedAbove { .. } => 3,
        Why::IgnoredByGit { .. } => 4,
        Why::IgnoreCheckFailed { .. } => 5,
        Why::ListedAs { .. } => 6,
        Why::Conflicted => 7,
        Why::SymbolicLinkInIndex => 8,
        Why::SubmoduleInIndex => 9,
        Why::UnexpectedMode { .. } => 10,
        Why::IndexEntryUnreadable { .. } => 11,
        Why::NotWhatGitChecksOut => 12,
        Why::CheckoutNotReproducible => 13,
        Why::CheckoutFailed { .. } => 14,
        Why::OutputTooLarge { .. } => 15,
        Why::NotARegularFile => 16,
        Why::Unreadable { .. } => 17,
        Why::ChangedSinceSurvey => 18,
    }
}

/// One unproven path, at `path`, for `why`.
fn unproven(path: &str, why: Why) -> Unproven {
    Unproven {
        path: claim(path),
        why,
        drift: DriftReason::Changed,
    }
}

/// A claimed file whose name holds the poison.
fn poisoned_file() -> String {
    format!("{POISON}.yml")
}

/// A claimed file beneath a directory whose name holds the poison.
fn poisoned_file_under_directory() -> String {
    format!("{POISON}/x.yml")
}

/// Every [`Why`] that carries no flag or entry kind of its own, once, at a
/// poisoned path.
fn plain_why_samples() -> Vec<Unproven> {
    let file = poisoned_file();
    vec![
        unproven(&file, Why::NotInIndex),
        unproven(
            &poisoned_file_under_directory(),
            Why::IgnoredByGit { rule: poison() },
        ),
        unproven(&file, Why::IgnoreCheckFailed { detail: poison() }),
        unproven(&file, Why::ListedAs { git_path: poison() }),
        unproven(&file, Why::Conflicted),
        unproven(&file, Why::SymbolicLinkInIndex),
        unproven(&file, Why::SubmoduleInIndex),
        unproven(&file, Why::UnexpectedMode { mode: poison() }),
        unproven(&file, Why::IndexEntryUnreadable { detail: poison() }),
        unproven(&file, Why::NotWhatGitChecksOut),
        unproven(&file, Why::CheckoutNotReproducible),
        unproven(
            &file,
            Why::CheckoutFailed {
                diagnostic: poison(),
            },
        ),
        unproven(&file, Why::NotARegularFile),
        unproven(&file, Why::Unreadable { detail: poison() }),
        unproven(&file, Why::ChangedSinceSurvey),
    ]
}

/// The three ways git's entry can relate to the claim: at it, beneath it, and
/// another spelling of it.
fn tracked_but_absent_samples() -> Vec<Unproven> {
    let file = poisoned_file();
    [
        file.clone(),
        format!("{file}/x"),
        POISON_FOLDED.to_owned() + ".yml",
    ]
    .into_iter()
    .map(|git_path| unproven(&file, Why::TrackedButAbsent { git_path }))
    .collect()
}

/// A hidden file under each flag that hides it.
fn hidden_samples() -> Vec<Unproven> {
    [
        HiddenFlag::SkipWorktree,
        HiddenFlag::AssumeUnchanged,
        HiddenFlag::Both,
    ]
    .into_iter()
    .map(|flag| unproven(&poisoned_file(), Why::HiddenFromWorkTree { flag }))
    .collect()
}

/// A path beneath a tracked entry of each kind.
fn tracked_above_samples() -> Vec<Unproven> {
    [
        AboveEntry::File,
        AboveEntry::SymbolicLink,
        AboveEntry::Submodule,
        AboveEntry::Other { mode: poison() },
    ]
    .into_iter()
    .map(|entry| {
        unproven(
            &poisoned_file_under_directory(),
            Why::TrackedAbove {
                git_path: poison(),
                entry,
            },
        )
    })
    .collect()
}

/// A cause a line words the same for a file that is changed and one that is
/// missing, once for each.
fn output_too_large_samples() -> Vec<Unproven> {
    [DriftReason::Changed, DriftReason::Missing]
        .into_iter()
        .map(|drift| Unproven {
            drift,
            ..unproven(
                &poisoned_file(),
                Why::OutputTooLarge {
                    command: "cat-file",
                },
            )
        })
        .collect()
}

fn why_samples() -> Vec<Unproven> {
    let mut samples = plain_why_samples();
    samples.extend(tracked_but_absent_samples());
    samples.extend(hidden_samples());
    samples.extend(tracked_above_samples());
    samples.extend(output_too_large_samples());
    samples
}

#[test]
fn every_unproven_path_prints_its_outside_text_escaped_once() {
    // A path is named by every line, so each line shows the poisoned path;
    // the cause's own text (a git path, a mode, a rule, a diagnostic) is
    // poisoned too, and the remedies that repeat a path show it as the line
    // does.
    let samples = why_samples();
    assert_every_kind(
        samples.iter().map(|entry| why_kind(&entry.why)),
        WHY_KINDS,
        "Why",
    );

    for entry in &samples {
        assert_escaped_once(&unproven_line(entry), &format!("{:?}", entry.why));
    }
}

// ---------------------------------------------------------------------------
// Leftovers
// ---------------------------------------------------------------------------

/// How many kinds of [`LeftoverReason`] there are.
const LEFTOVER_KINDS: usize = 4;

const fn leftover_kind(reason: &LeftoverReason) -> usize {
    match reason {
        LeftoverReason::CouldNotRemove { .. } => 0,
        LeftoverReason::PathUnsafe(_) => 1,
        LeftoverReason::SomethingElseThere => 2,
        LeftoverReason::Gone => 3,
    }
}

/// One leftover of every kind, each at a poisoned path.
fn leftover_samples() -> Vec<Leftover> {
    let mut samples = vec![poisoned_leftover()];
    samples.extend(
        [
            LeftoverReason::PathUnsafe(UnsafePathCause::SymbolicLinkAbove { at: poison() }),
            LeftoverReason::PathUnsafe(UnsafePathCause::Unreadable { detail: poison() }),
            LeftoverReason::SomethingElseThere,
            LeftoverReason::Gone,
        ]
        .into_iter()
        .map(|reason| Leftover {
            path: poison(),
            reason,
        }),
    );
    samples
}

#[test]
fn every_leftover_prints_its_path_and_detail_escaped_once() {
    let samples = leftover_samples();
    assert_every_kind(
        samples
            .iter()
            .map(|leftover| leftover_kind(&leftover.reason)),
        LEFTOVER_KINDS,
        "LeftoverReason",
    );

    for leftover in &samples {
        assert_escaped_once(&leftover_text(leftover), &format!("{leftover:?}"));
    }
    assert_escaped_once(
        &leftovers_after_success_message(&samples),
        "the leftovers after success",
    );
}

// ---------------------------------------------------------------------------
// Write failures
// ---------------------------------------------------------------------------

/// How many kinds of [`WriteFailure`] there are.
const FAILURE_KINDS: usize = 8;

const fn failure_kind(failure: &WriteFailure) -> usize {
    match failure {
        WriteFailure::StagingClaimed { .. } => 0,
        WriteFailure::Collision { .. } => 1,
        WriteFailure::TargetChanged { .. } => 2,
        WriteFailure::FoldsOntoTracked { .. } => 3,
        WriteFailure::DirectoryNotWritable { .. } => 4,
        WriteFailure::Prepare { .. } => 5,
        WriteFailure::Commit(_) => 6,
        WriteFailure::ChangedAfterWrite { .. } => 7,
    }
}

const RELATION_KINDS: usize = 4;

const fn relation_kind(relation: StagingRelation) -> usize {
    match relation {
        StagingRelation::Exact => 0,
        StagingRelation::Folded => 1,
        StagingRelation::DirectoryAbove => 2,
        StagingRelation::Beneath => 3,
    }
}

const COLLISION_KINDS: usize = 2;

const fn collision_kind(what: CollisionAt) -> usize {
    match what {
        CollisionAt::StagingFile => 0,
        CollisionAt::Directory => 1,
    }
}

const CHANGE_KINDS: usize = 5;

const fn change_kind(what: &TargetChange) -> usize {
    match what {
        TargetChange::Appeared => 0,
        TargetChange::Disappeared => 1,
        TargetChange::NoLongerAFile => 2,
        TargetChange::ContentChanged => 3,
        TargetChange::PathUnsafe(_) => 4,
    }
}

fn change_samples() -> Vec<TargetChange> {
    vec![
        TargetChange::Appeared,
        TargetChange::Disappeared,
        TargetChange::NoLongerAFile,
        TargetChange::ContentChanged,
        TargetChange::PathUnsafe(UnsafePathCause::SymbolicLinkAbove { at: poison() }),
    ]
}

const CAUSE_KINDS: usize = 3;

const fn cause_kind(cause: &CommitCause) -> usize {
    match cause {
        CommitCause::Replace { .. } => 0,
        CommitCause::Create { .. } => 1,
        CommitCause::Changed(_) => 2,
    }
}

fn cause_samples() -> Vec<CommitCause> {
    let mut causes = vec![
        CommitCause::Replace { detail: poison() },
        CommitCause::Create { detail: poison() },
    ];
    causes.extend(change_samples().into_iter().map(CommitCause::Changed));
    causes
}

/// A fold variant of `claim_text` found in an index listing holding
/// `entry_text`.
fn fold_variant(claim_text: &str, entry_text: &str) -> FoldVariant {
    let claimed = claim(claim_text);
    let listing = format!("{entry_text}\0");
    let mut found = fold_variants(listing.as_bytes(), &[&claimed]);
    found
        .remove(0)
        .into_iter()
        .next()
        .expect("the entry is a fold variant of the claim")
}

/// A commit failure at a poisoned path, before any write (`written` false) or
/// after one.
fn commit_failure(cause: CommitCause, written: bool, leftovers: Vec<Leftover>) -> WriteFailure {
    let (already_written, not_yet_written) = if written {
        (vec![claim(POISON)], vec![claim(POISON)])
    } else {
        (Vec::new(), vec![claim(POISON)])
    };
    WriteFailure::Commit(Box::new(CommitFailure {
        failed_path: claim(&poisoned_file()),
        already_written,
        not_yet_written,
        total: 3,
        cause,
        leftovers,
    }))
}

/// A staging name that relates to the claim in each of the four ways.
fn staging_claimed_samples() -> Vec<WriteFailure> {
    [
        StagingRelation::Exact,
        StagingRelation::Folded,
        StagingRelation::DirectoryAbove,
        StagingRelation::Beneath,
    ]
    .into_iter()
    .map(|relation| WriteFailure::StagingClaimed {
        claim: claim(&poisoned_file()),
        staging: poison(),
        claimed: claim(POISON),
        relation,
    })
    .collect()
}

/// A collision of each kind, with and without leftovers.
fn collision_samples() -> Vec<WriteFailure> {
    let mut samples = Vec::new();
    for what in [CollisionAt::StagingFile, CollisionAt::Directory] {
        for leftovers in [Vec::new(), leftover_samples()] {
            samples.push(WriteFailure::Collision {
                claim: claim(&poisoned_file()),
                shown: poison(),
                what,
                leftovers,
            });
        }
    }
    samples
}

/// A target that changed in each way, with and without leftovers.
fn target_changed_samples() -> Vec<WriteFailure> {
    let mut samples = Vec::new();
    for what in change_samples() {
        for leftovers in [Vec::new(), leftover_samples()] {
            samples.push(WriteFailure::TargetChanged {
                claim: claim(&poisoned_file()),
                what: what.clone(),
                leftovers,
            });
        }
    }
    samples
}

/// The failures that carry leftovers and are not counted by a kind of their
/// own above: a fold onto a tracked entry (of a file and of a directory), a
/// directory that cannot be written, a failed preparation, and a failed
/// commit before and after a write, each for every cause.
fn samples_carrying(leftovers: &[Leftover]) -> Vec<WriteFailure> {
    let file = poisoned_file();
    let under = poisoned_file_under_directory();
    let mut samples = vec![
        WriteFailure::FoldsOntoTracked {
            claim: claim(&file),
            variant: fold_variant(&file, &format!("{POISON_FOLDED}.yml")),
            leftovers: leftovers.to_vec(),
        },
        WriteFailure::FoldsOntoTracked {
            claim: claim(&under),
            variant: fold_variant(&under, POISON_FOLDED),
            leftovers: leftovers.to_vec(),
        },
    ];
    for directory in [Some(claim(POISON)), None] {
        samples.push(WriteFailure::DirectoryNotWritable {
            path: claim(&file),
            directory,
            detail: poison(),
            leftovers: leftovers.to_vec(),
        });
    }
    samples.push(WriteFailure::Prepare {
        path: claim(&file),
        detail: poison(),
        leftovers: leftovers.to_vec(),
    });
    for written in [false, true] {
        for cause in cause_samples() {
            samples.push(commit_failure(cause, written, leftovers.to_vec()));
        }
    }
    samples
}

/// The files changed after a write: one, and several.
fn changed_after_write_samples() -> Vec<WriteFailure> {
    vec![
        WriteFailure::ChangedAfterWrite {
            paths: vec![claim(POISON)],
        },
        WriteFailure::ChangedAfterWrite {
            paths: vec![claim(POISON), claim(&poisoned_file())],
        },
    ]
}

fn failure_samples() -> Vec<WriteFailure> {
    let mut samples = staging_claimed_samples();
    samples.extend(collision_samples());
    samples.extend(target_changed_samples());
    for leftovers in [Vec::new(), leftover_samples()] {
        samples.extend(samples_carrying(&leftovers));
    }
    samples.extend(changed_after_write_samples());
    samples
}

#[test]
fn every_write_failure_prints_its_outside_text_escaped_once() {
    // Every kind, with its own sub-kinds (how a staging name relates to a
    // claim, what a collision found, what changed, why a commit failed), each
    // with and without leftovers, and a commit before and after a write.
    let samples = failure_samples();
    assert_every_kind(
        samples.iter().map(failure_kind),
        FAILURE_KINDS,
        "WriteFailure",
    );
    let relations = samples.iter().filter_map(|failure| match failure {
        WriteFailure::StagingClaimed { relation, .. } => Some(relation_kind(*relation)),
        _ => None,
    });
    assert_every_kind(relations, RELATION_KINDS, "StagingRelation");
    let collisions = samples.iter().filter_map(|failure| match failure {
        WriteFailure::Collision { what, .. } => Some(collision_kind(*what)),
        _ => None,
    });
    assert_every_kind(collisions, COLLISION_KINDS, "CollisionAt");
    let changes = samples.iter().filter_map(|failure| match failure {
        WriteFailure::TargetChanged { what, .. } => Some(change_kind(what)),
        _ => None,
    });
    assert_every_kind(changes, CHANGE_KINDS, "TargetChange");
    let causes = samples.iter().filter_map(|failure| match failure {
        WriteFailure::Commit(commit) => Some(cause_kind(&commit.cause)),
        _ => None,
    });
    assert_every_kind(causes, CAUSE_KINDS, "CommitCause");

    for failure in &samples {
        assert_escaped_once(&write_failure_message(failure), &format!("{failure:?}"));
    }
}

#[test]
fn a_written_line_names_its_path_and_skeleton_escaped_once() {
    // `PreparedWrite` cannot be built from here (its staging fields are
    // private to the write module), so the line's own words are tested.
    for reason in [DriftReason::Missing, DriftReason::Changed] {
        let line = written_line(
            &claim(POISON),
            POISON,
            &semver::Version::new(0, 1, 0),
            reason,
        );
        assert_escaped_once(&line, &format!("{reason:?}"));
    }
}
