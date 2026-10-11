//! What `remove` tells the wearer when it has stopped wearing a skeleton.
//!
//! The lines come in the order the wearer reads them: what was taken out of the
//! manifest, what became of each file, and what to commit. A line names a file
//! or a key from outside only through [`Escaped`], so each is one line however
//! its words came.

use super::files::{Fate, Kept, LeftBecause, Outcome, Presence};
use super::resolve::{Candidate, Standing};
use crate::command_line_crate::hand_back::Tracking;
use crate::skeleton::Escaped;
use crate::survey::count;
use crate::survey::join_and;

/// Whether this run changed `Cargo.lock`, and if it did, whether git tracks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lockfile {
    /// Bringing the lockfile current changed nothing.
    Unchanged,
    /// Bringing the lockfile current rewrote it.
    Changed(Tracking),
}

/// What a successful run did, as far as its lines say.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Removed<'run> {
    /// The wearing that was taken out.
    pub(crate) wearing: &'run Candidate,
    /// The manifest it was taken out of, as a message shows it.
    pub(crate) manifest: &'run str,
    /// What became of the files.
    pub(crate) files: &'run Outcome,
    /// What happened to `Cargo.lock`.
    pub(crate) lockfile: Lockfile,
}

/// The lines `remove` prints when it succeeds: what was taken out, then a line
/// for each file or one for all of them, then what to commit.
pub(crate) fn removed_lines(removed: &Removed<'_>) -> Vec<String> {
    let mut lines = vec![taken_out_line(removed.wearing, removed.manifest)];
    lines.extend(file_lines(removed.files));
    lines.push(commit_line(
        removed.manifest,
        removed.lockfile,
        deleted_count(removed.files),
    ));
    assert!(
        lines.len() >= 2,
        "a run says what it took out and what to commit"
    );
    lines
}

/// What was taken out of the manifest. A worn skeleton is named with its
/// version. A wearing `check` refuses is not a skeleton to name: it says what
/// was taken out, and says when there was no dependency to take.
fn taken_out_line(wearing: &Candidate, manifest: &str) -> String {
    let (key, manifest) = (Escaped(wearing.key()), Escaped(manifest));
    match wearing.standing() {
        Standing::Worn {
            crate_name,
            version,
        } => format!(
            "stopped wearing {} {version}: took the dependency `{key}` and \
             [package.metadata.skeletons.{key}] out of {manifest}",
            Escaped(crate_name)
        ),
        Standing::RefusedWithDependency { .. } => format!(
            "took the dependency `{key}` and [package.metadata.skeletons.{key}] out of {manifest}"
        ),
        Standing::RefusedWithoutDependency => format!(
            "took [package.metadata.skeletons.{key}] out of {manifest}; no dependency was \
             declared under `{key}`"
        ),
    }
}

/// The lines about the files: what became of each, in path order, and for kept
/// files why they were kept.
fn file_lines(files: &Outcome) -> Vec<String> {
    match files {
        Outcome::LeftAll => vec!["left every file it claims in place".to_owned()],
        Outcome::Kept(kept) => {
            let mut lines: Vec<String> = kept.iter().map(kept_line).collect();
            lines.push(kept_summary(kept));
            lines
        }
        Outcome::Deleting(dispositions) if dispositions.is_empty() => {
            vec!["none of its files is in the work tree, so there was nothing to delete".to_owned()]
        }
        Outcome::Deleting(dispositions) => dispositions
            .iter()
            .map(|disposition| match disposition.fate {
                Fate::Deleted => {
                    format!("deleted {}", Escaped(disposition.path.as_str()))
                }
                Fate::Left(because) => left_line(disposition.path.as_str(), because),
            })
            .collect(),
    }
}

/// `` kept {path}, which {matches the skeleton|has changed} ``.
fn kept_line(kept: &Kept) -> String {
    let which = match kept.state {
        Presence::Matches => "matches the skeleton",
        Presence::Changed => "has changed",
    };
    format!("kept {}, which {which}", Escaped(kept.path.as_str()))
}

/// Why every file that is there was kept, when the project changed some.
///
/// With one file the sentence is about it; with several it counts the ones that
/// changed. A file that is missing is not counted: the sentence is about the
/// files that are there, which are the ones kept.
fn kept_summary(kept: &[Kept]) -> String {
    let total = kept.len();
    let changed = kept
        .iter()
        .filter(|file| file.state == Presence::Changed)
        .count();
    assert!(changed > 0, "files are kept only because one has changed");
    assert!(changed <= total, "no more files changed than there are");
    if total == 1 {
        return "kept its file, because it has changed from what the skeleton renders: it is the \
                project's own now, to keep or to delete by hand"
            .to_owned();
    }
    format!(
        "kept all {total} of its files, because {changed} {} changed from what the skeleton \
         renders: they are the project's own now, to keep or to delete by hand",
        count::has_or_have(changed)
    )
}

/// `` left {path} in place: {why git could not give it back}, so git could not
/// give it back; delete it by hand if it should go ``.
fn left_line(path: &str, because: LeftBecause) -> String {
    let because = match because {
        LeftBecause::NotTracked => "git does not track it".to_owned(),
        LeftBecause::Hidden(flag) => format!("it is marked {} in git's index", flag.marked()),
        LeftBecause::NotWhatGitChecksOut => "it is not what git would check out for it".to_owned(),
        LeftBecause::CannotShowItHolds => "git could not show that it holds it".to_owned(),
    };
    format!(
        "left {} in place: {because}, so git could not give it back; delete it by hand if it \
         should go",
        Escaped(path)
    )
}

/// How many files the run deleted.
fn deleted_count(files: &Outcome) -> usize {
    files.to_delete().len()
}

/// `` commit {the manifest, Cargo.lock if git tracks it and this run changed
/// it, and the deleted files if there are any} to record it ``.
///
/// `Cargo.lock` is named only when `git add` will take it: a lockfile git
/// ignores is refused by `git add`, and one this run left alone has nothing to
/// commit. It is always at the workspace root, which is where the wearer sees it
/// named.
fn commit_line(manifest: &str, lockfile: Lockfile, deleted: usize) -> String {
    let mut things = vec![Escaped(manifest).to_string()];
    match lockfile {
        Lockfile::Changed(Tracking::Tracked) => things.push("Cargo.lock".to_owned()),
        Lockfile::Changed(Tracking::NotTracked) | Lockfile::Unchanged => {}
    }
    if deleted > 0 {
        things.push(format!("the {deleted} deleted {}", count::file(deleted)));
    }
    format!("commit {} to record it", join_and(&things))
}

#[cfg(test)]
mod tests {
    use super::{Lockfile, Removed, removed_lines};
    use crate::command_line_crate::hand_back::Tracking;
    use crate::remove::files::{Disposition, Fate, Kept, LeftBecause, Outcome, Presence};
    use crate::remove::resolve::{Candidate, Standing};
    use crate::survey::poison::{assert_escaped_once, assert_one_line, claim, poison};
    use crate::work_tree::index_entry::HiddenFlag;

    fn worn(key: &str, crate_name: &str) -> Candidate {
        Candidate::new(
            key,
            Standing::Worn {
                crate_name: crate_name.to_owned(),
                version: semver::Version::new(0, 1, 0),
            },
        )
    }

    fn deleted(path: &str) -> Disposition {
        Disposition {
            path: claim(path),
            fate: Fate::Deleted,
        }
    }

    fn left(path: &str, because: LeftBecause) -> Disposition {
        Disposition {
            path: claim(path),
            fate: Fate::Left(because),
        }
    }

    fn kept(path: &str, state: Presence) -> Kept {
        Kept {
            path: claim(path),
            state,
        }
    }

    fn lines(wearing: &Candidate, files: &Outcome, lockfile: Lockfile) -> Vec<String> {
        removed_lines(&Removed {
            wearing,
            manifest: "ritual/Cargo.toml",
            files,
            lockfile,
        })
    }

    const TRACKED: Lockfile = Lockfile::Changed(Tracking::Tracked);

    #[test]
    fn a_worn_skeleton_is_named_with_its_version_and_every_file_is_listed() {
        let files = Outcome::Deleting(vec![
            deleted(".github/workflows/ci.yml"),
            deleted("root.yml"),
        ]);

        assert_eq!(
            lines(&worn("ci", "a-skeleton"), &files, TRACKED),
            [
                "stopped wearing a-skeleton 0.1.0: took the dependency `ci` and \
                 [package.metadata.skeletons.ci] out of ritual/Cargo.toml",
                "deleted .github/workflows/ci.yml",
                "deleted root.yml",
                "commit ritual/Cargo.toml, Cargo.lock and the 2 deleted files to record it",
            ]
        );
    }

    #[test]
    fn a_wearing_check_refuses_that_has_a_dependency_says_what_was_taken_out() {
        let wearing = Candidate::new(
            "ci",
            Standing::RefusedWithDependency {
                crate_name: "not-a-skeleton".to_owned(),
            },
        );

        assert_eq!(
            lines(&wearing, &Outcome::LeftAll, Lockfile::Unchanged),
            [
                "took the dependency `ci` and [package.metadata.skeletons.ci] out of \
                 ritual/Cargo.toml",
                "left every file it claims in place",
                "commit ritual/Cargo.toml to record it",
            ]
        );
    }

    #[test]
    fn a_wearing_check_refuses_that_has_no_dependency_says_there_was_none() {
        let wearing = Candidate::new("ci", Standing::RefusedWithoutDependency);

        assert_eq!(
            lines(&wearing, &Outcome::LeftAll, TRACKED),
            [
                "took [package.metadata.skeletons.ci] out of ritual/Cargo.toml; no dependency \
                 was declared under `ci`",
                "left every file it claims in place",
                "commit ritual/Cargo.toml and Cargo.lock to record it",
            ]
        );
    }

    #[test]
    fn no_file_in_the_work_tree_says_there_was_nothing_to_delete() {
        assert_eq!(
            lines(
                &worn("ci", "a-skeleton"),
                &Outcome::Deleting(Vec::new()),
                TRACKED
            )[1],
            "none of its files is in the work tree, so there was nothing to delete"
        );
    }

    #[test]
    fn a_file_git_could_not_give_back_is_left_and_says_why_for_each_reason() {
        let tail = ", so git could not give it back; delete it by hand if it should go";
        for (because, why) in [
            (LeftBecause::NotTracked, "git does not track it"),
            (
                LeftBecause::Hidden(HiddenFlag::SkipWorktree),
                "it is marked skip-worktree in git's index",
            ),
            (
                LeftBecause::Hidden(HiddenFlag::AssumeUnchanged),
                "it is marked assume-unchanged in git's index",
            ),
            (
                LeftBecause::Hidden(HiddenFlag::Both),
                "it is marked skip-worktree and assume-unchanged in git's index",
            ),
            (
                LeftBecause::NotWhatGitChecksOut,
                "it is not what git would check out for it",
            ),
            (
                LeftBecause::CannotShowItHolds,
                "git could not show that it holds it",
            ),
        ] {
            let files = Outcome::Deleting(vec![left("local/notes.yml", because)]);

            assert_eq!(
                lines(&worn("ci", "a-skeleton"), &files, TRACKED)[1],
                format!("left local/notes.yml in place: {why}{tail}"),
                "{because:?}"
            );
        }
    }

    #[test]
    fn deleted_and_left_files_are_listed_in_the_order_given_and_only_deleted_ones_are_counted() {
        let files = Outcome::Deleting(vec![
            deleted(".github/dependabot.yml"),
            left("a/deep.yml", LeftBecause::NotTracked),
            deleted("root.yml"),
        ]);

        let printed = lines(&worn("ci", "a-skeleton"), &files, TRACKED);

        assert_eq!(
            printed[1..],
            [
                "deleted .github/dependabot.yml",
                "left a/deep.yml in place: git does not track it, so git could not give it \
                 back; delete it by hand if it should go",
                "deleted root.yml",
                "commit ritual/Cargo.toml, Cargo.lock and the 2 deleted files to record it",
            ]
        );
    }

    #[test]
    fn the_commit_line_counts_deleted_files_in_the_singular_and_the_plural() {
        let one = Outcome::Deleting(vec![deleted("a.yml")]);
        let three = Outcome::Deleting(vec![deleted("a.yml"), deleted("b.yml"), deleted("c.yml")]);

        let last = |files: &Outcome, lockfile| {
            lines(&worn("ci", "a-skeleton"), files, lockfile)
                .pop()
                .expect("a run ends with its commit line")
        };

        assert_eq!(
            last(&one, Lockfile::Unchanged),
            "commit ritual/Cargo.toml and the 1 deleted file to record it"
        );
        assert_eq!(
            last(&three, Lockfile::Changed(Tracking::NotTracked)),
            "commit ritual/Cargo.toml and the 3 deleted files to record it"
        );
        assert_eq!(
            last(&three, TRACKED),
            "commit ritual/Cargo.toml, Cargo.lock and the 3 deleted files to record it"
        );
    }

    #[test]
    fn the_lockfile_is_named_only_when_git_tracks_it_and_this_run_changed_it() {
        let files = Outcome::Deleting(Vec::new());
        let last = |lockfile| {
            lines(&worn("ci", "a-skeleton"), &files, lockfile)
                .pop()
                .expect("a run ends with its commit line")
        };

        assert_eq!(
            last(Lockfile::Changed(Tracking::Tracked)),
            "commit ritual/Cargo.toml and Cargo.lock to record it"
        );
        assert_eq!(
            last(Lockfile::Changed(Tracking::NotTracked)),
            "commit ritual/Cargo.toml to record it"
        );
        assert_eq!(
            last(Lockfile::Unchanged),
            "commit ritual/Cargo.toml to record it"
        );
    }

    #[test]
    fn files_that_were_kept_are_listed_with_why_and_summed_up_by_how_many_changed() {
        let files = Outcome::Kept(vec![
            kept(".github/dependabot.yml", Presence::Matches),
            kept(".github/workflows/ci.yml", Presence::Changed),
        ]);

        assert_eq!(
            lines(&worn("ci", "a-skeleton"), &files, TRACKED)[1..],
            [
                "kept .github/dependabot.yml, which matches the skeleton",
                "kept .github/workflows/ci.yml, which has changed",
                "kept all 2 of its files, because 1 has changed from what the skeleton renders: \
                 they are the project's own now, to keep or to delete by hand",
                "commit ritual/Cargo.toml and Cargo.lock to record it",
            ]
        );
    }

    #[test]
    fn the_summary_agrees_its_verb_with_how_many_changed_and_its_noun_with_how_many_there_are() {
        let summary = |states: &[Presence]| {
            let kept: Vec<Kept> = states
                .iter()
                .enumerate()
                .map(|(index, state)| kept(&format!("{index}.yml"), *state))
                .collect();
            let printed = lines(&worn("ci", "a"), &Outcome::Kept(kept), TRACKED);
            printed[printed.len() - 2].clone()
        };

        assert_eq!(
            summary(&[Presence::Changed, Presence::Changed]),
            "kept all 2 of its files, because 2 have changed from what the skeleton renders: \
             they are the project's own now, to keep or to delete by hand"
        );
        assert_eq!(
            summary(&[Presence::Matches, Presence::Changed, Presence::Changed]),
            "kept all 3 of its files, because 2 have changed from what the skeleton renders: \
             they are the project's own now, to keep or to delete by hand"
        );
        assert_eq!(
            summary(&[Presence::Changed]),
            "kept its file, because it has changed from what the skeleton renders: it is the \
             project's own now, to keep or to delete by hand"
        );
    }

    #[test]
    #[should_panic(expected = "files are kept only because one has changed")]
    fn files_none_of_which_changed_are_never_a_reason_to_keep_them() {
        let _ = lines(
            &worn("ci", "a"),
            &Outcome::Kept(vec![kept("a.yml", Presence::Matches)]),
            Lockfile::Unchanged,
        );
    }

    #[test]
    fn every_line_prints_text_from_outside_on_one_line_escaped_once() {
        // The crate, the key, the manifest and every file's path are text from
        // outside: a path can hold a newline, and the key and the manifest are
        // named on several lines. Every shape of outcome and of wearing is
        // read, and each line stays one line with the name shown as `\n`.
        let outcomes = [
            Outcome::LeftAll,
            Outcome::Kept(vec![
                kept("a\nb.yml", Presence::Matches),
                kept("c\nd.yml", Presence::Changed),
            ]),
            Outcome::Deleting(vec![
                deleted("a\nb.yml"),
                left("c\nd.yml", LeftBecause::NotTracked),
            ]),
            Outcome::Deleting(Vec::new()),
        ];
        let wearings = [
            Candidate::new(
                &poison(),
                Standing::Worn {
                    crate_name: poison(),
                    version: semver::Version::new(1, 2, 3),
                },
            ),
            Candidate::new(
                &poison(),
                Standing::RefusedWithDependency {
                    crate_name: poison(),
                },
            ),
            Candidate::new(&poison(), Standing::RefusedWithoutDependency),
        ];

        for wearing in &wearings {
            for outcome in &outcomes {
                for line in removed_lines(&Removed {
                    wearing,
                    manifest: &poison(),
                    files: outcome,
                    lockfile: TRACKED,
                }) {
                    assert_one_line(&line, &format!("{wearing:?} with {outcome:?}"));
                    assert!(!line.contains("\\\\"), "escaped twice: {line:?}");
                }
            }
        }
        let first = &removed_lines(&Removed {
            wearing: &wearings[0],
            manifest: &poison(),
            files: &outcomes[0],
            lockfile: TRACKED,
        })[0];
        assert_escaped_once(first, "the line that says what was taken out");
    }

    #[test]
    fn a_file_path_with_a_line_break_in_it_is_shown_escaped_once() {
        let files = Outcome::Deleting(vec![
            deleted(&poison()),
            left(&poison(), LeftBecause::NotTracked),
        ]);

        let printed = lines(&worn("ci", "a-skeleton"), &files, TRACKED);

        assert_escaped_once(&printed[1], "the deleted line");
        assert_escaped_once(&printed[2], "the left line");
    }

    #[test]
    fn no_line_names_sync_or_wear() {
        // The lines are about `remove` alone, so they send the wearer to no
        // other command. `stopped wearing` is the wearer's word for the state
        // the skeleton was in, so it is not read as naming `wear`.
        let files = Outcome::Deleting(vec![
            deleted("a.yml"),
            left("b.yml", LeftBecause::CannotShowItHolds),
        ]);
        let all = lines(&worn("ci", "a-skeleton"), &files, TRACKED)
            .into_iter()
            .chain(lines(&worn("ci", "a-skeleton"), &Outcome::LeftAll, TRACKED))
            .chain(lines(
                &worn("ci", "a-skeleton"),
                &Outcome::Kept(vec![kept("a.yml", Presence::Changed)]),
                TRACKED,
            ));

        for line in all {
            for other in ["sync", "`wear`", "wear task", "wear wrote"] {
                assert!(!line.contains(other), "{line:?} names {other}");
            }
        }
    }
}
