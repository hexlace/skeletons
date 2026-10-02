//! Every message about the work tree prints the text it names from outside
//! (a path, a name git or the operating system gave, a diagnostic) escaped
//! exactly once, and stays on one line, whichever command is asking.
//!
//! Each test puts the poison, a name with a line break in it, into every
//! outside field of every kind of one enum and asserts the message holds no
//! line break and shows the name as the two characters `\n`, once. Every enum
//! is counted through a match that has no wildcard, so a variant added without
//! an arm does not compile, and the sample list must then cover it.

use std::path::Path;

use super::{abort_message, dirty_line};
use crate::survey::poison::{
    POISON, assert_escaped_once, assert_every_kind, assert_one_line, claim, poison,
};
use crate::work_tree::abort::{GitQuestion, WorkTreeAbort};
use crate::work_tree::clean::{Dirt, DirtyPath};
use crate::work_tree::writing_command::WritingCommand;

/// Both commands that word these messages, so each is read for every kind.
const COMMANDS: [WritingCommand; 2] = [WritingCommand::Sync, WritingCommand::Wear];

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
// WorkTreeAbort and GitQuestion
// ---------------------------------------------------------------------------

/// How many kinds of [`WorkTreeAbort`] there are.
const ABORT_KINDS: usize = 7;

/// A timed-out abort names what the question it timed out on names, so its
/// kind reads that question's.
const fn abort_kind(abort: &WorkTreeAbort) -> Kind {
    match abort {
        WorkTreeAbort::RedirectedGit { .. } => not_named(0),
        WorkTreeAbort::NotAWorkTree => named(1),
        WorkTreeAbort::DubiousOwnership { .. } => named(2),
        WorkTreeAbort::GitUnavailable { .. } => named(3),
        WorkTreeAbort::GitTimedOut { question } => Kind {
            index: 4,
            outside_text: question_kind(question).outside_text,
        },
        WorkTreeAbort::GitFailed { .. } => named(5),
        WorkTreeAbort::GitOutputTooLarge { .. } => not_named(6),
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

fn abort_samples() -> Vec<WorkTreeAbort> {
    let mut samples = vec![
        WorkTreeAbort::RedirectedGit {
            variables: vec!["GIT_DIR"],
        },
        WorkTreeAbort::NotAWorkTree,
        WorkTreeAbort::DubiousOwnership {
            diagnostic: poison(),
        },
        WorkTreeAbort::GitUnavailable { detail: poison() },
        WorkTreeAbort::GitFailed {
            command: "status",
            diagnostic: poison(),
        },
        WorkTreeAbort::GitOutputTooLarge { command: "status" },
    ];
    samples.extend(
        question_samples()
            .into_iter()
            .map(|question| WorkTreeAbort::GitTimedOut { question }),
    );
    samples
}

#[test]
fn every_work_tree_abort_prints_its_outside_text_escaped_once() {
    // The root the message names when the directory is no work tree is
    // poisoned too. Kinds that name nothing from outside are still one line.
    let samples = abort_samples();
    assert_every_kind(
        samples.iter().map(|abort| abort_kind(abort).index),
        ABORT_KINDS,
        "WorkTreeAbort",
    );

    for command in COMMANDS {
        for abort in &samples {
            let text = abort_message(abort, Path::new(POISON), command);
            assert_message(&text, abort_kind(abort), &format!("{command:?} {abort:?}"));
        }
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

    for command in COMMANDS {
        for question in &samples {
            let text = super::timed_out::timed_out_message(question, command);
            assert_message(
                &text,
                question_kind(question),
                &format!("{command:?} {question:?}"),
            );
        }
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
