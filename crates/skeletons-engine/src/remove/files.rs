//! What becomes of the files a skeleton wrote: which are deleted, which are
//! kept because the project has changed them, which are left because git could
//! not give them back, and when none of that can be said at all.
//!
//! The decision is in two steps, so that each is a pure function of values.
//! [`plan`] reads the mode the wearer typed and the files the skeleton claims,
//! and says what to try: nothing, a keep, or a deletion of the files that are
//! there. [`Plan::settle`] then takes git's answer for each file about to be
//! deleted and says what became of it, which is what the success lines report.

use crate::claim::{ClaimPath, Drift, DriftReason};
use crate::survey::{Refusal, RefusedWearing, Survey};

/// What the wearer typed about the skeleton's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Neither flag: delete every file that still matches what the skeleton
    /// renders, and keep every file if the project has changed one.
    Bare,
    /// `--delete-files`: delete every file, changed ones included.
    DeleteFiles,
    /// `--keep-files`: leave every file where it is.
    KeepFiles,
}

impl Mode {
    /// The flag that was typed, or `None` for neither, as it is typed.
    pub(crate) const fn flag(self) -> Option<&'static str> {
        match self {
            Self::Bare => None,
            Self::DeleteFiles => Some("--delete-files"),
            Self::KeepFiles => Some("--keep-files"),
        }
    }
}

/// Whether a claimed file is in the work tree, and if it is, whether it holds
/// what the skeleton renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileState {
    /// The file holds the bytes the skeleton renders.
    Matches,
    /// The file is there and holds other bytes.
    Changed,
    /// There is no file at the path.
    Missing,
}

impl FileState {
    const fn of(drift: Drift) -> Self {
        match drift {
            Drift::Matches => Self::Matches,
            Drift::Drifted(DriftReason::Changed) => Self::Changed,
            Drift::Drifted(DriftReason::Missing) => Self::Missing,
        }
    }
}

/// One file a wearing claims, and the state it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaimedFile {
    pub(crate) path: ClaimPath,
    pub(crate) state: FileState,
}

/// Why the files a wearing claims cannot be known.
///
/// The survey drops a skeleton's rows whenever it refuses the skeleton, and
/// drops the claims that overlap, so a set of rows is the whole set of files
/// only when no refusal names the wearing. Each reason is a refusal's own
/// message, which is already one escaped line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unknowable {
    pub(crate) reasons: Vec<String>,
}

/// The files one wearing claims: all of them, or not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claimed {
    /// No refusal names the wearing, so these are every file it claims, in
    /// path order.
    Known(Vec<ClaimedFile>),
    /// A refusal names the wearing, so its files are not all in the survey.
    Unknown(Unknowable),
}

impl Claimed {
    /// The files of the wearing `key` in the manifest shown as `manifest`, as
    /// `survey` found them.
    pub(crate) fn of(survey: &Survey<'_>, manifest: &str, key: &str) -> Self {
        let names_the_wearing = |wearing: &RefusedWearing| {
            wearing.manifest == manifest && wearing.dependency.as_deref() == Some(key)
        };
        let reasons: Vec<String> = survey
            .refusals
            .iter()
            .filter(|refusal| refusal.record().wearings.iter().any(names_the_wearing))
            .map(Refusal::message)
            .collect();
        if !reasons.is_empty() {
            return Self::Unknown(Unknowable { reasons });
        }
        let files = survey
            .rows
            .iter()
            .filter(|row| row.claimant.manifest == manifest && row.claimant.dependency == key)
            .map(|row| ClaimedFile {
                path: row.path.clone(),
                state: FileState::of(row.drift),
            })
            .collect();
        Self::known(files)
    }

    /// `files` as the whole set of files a wearing claims, put in path order.
    pub(crate) fn known(mut files: Vec<ClaimedFile>) -> Self {
        files.sort_by(|left, right| left.path.cmp(&right.path));
        assert!(
            files.windows(2).all(|pair| pair[0].path != pair[1].path),
            "a wearing claims each path once"
        );
        Self::Known(files)
    }
}

/// A file that was in the work tree and is kept, and why it matters that it
/// was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) path: ClaimPath,
    pub(crate) state: Presence,
}

/// The state of a file that is in the work tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Matches,
    Changed,
}

/// What `remove` means to do with the files, before git is asked whether it
/// can give back any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// `--keep-files`: nothing is touched, and which files there are makes no
    /// difference.
    LeaveAll,
    /// Bare, with a file the project has changed: nothing is deleted, and
    /// every file that is there is kept, in path order.
    KeepAll(Vec<Kept>),
    /// Delete these files, which are the ones in the work tree, in path
    /// order. Empty when none is.
    Delete(Vec<ClaimPath>),
}

/// How git answered for one file about to be deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fate {
    /// Git can give the file back, so it is deleted.
    Deleted,
    /// Git could not give the file back, so it stays.
    Left(LeftBecause),
}

/// Why git could not give a file back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftBecause {
    /// Git's index has no entry for the file.
    NotTracked,
    /// The index flags the file so that git does not read it from the work
    /// tree.
    Hidden(crate::work_tree::index_entry::HiddenFlag),
    /// The file holds bytes git would not check out for its entry.
    NotWhatGitChecksOut,
    /// Git could not show that it holds the file, for a reason none of the
    /// others names.
    CannotShowItHolds,
}

/// A file about to be deleted, and what git said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Disposition {
    pub(crate) path: ClaimPath,
    pub(crate) fate: Fate,
}

/// What became of the files, which is what the success lines report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Every file was left where it is, because `--keep-files` was typed.
    LeftAll,
    /// Every file that is there was kept because the project changed one.
    Kept(Vec<Kept>),
    /// The files that are there, each deleted or left, in path order. Empty
    /// when none is there.
    Deleting(Vec<Disposition>),
}

impl Plan {
    /// Turns the plan into what became of the files, asking `fate` about each
    /// file the plan deletes, in path order, and only those.
    ///
    /// # Errors
    ///
    /// Returns the first error `fate` returns: a question to git that could
    /// not be asked ends the run before anything is written.
    pub(crate) fn settle<Error>(
        self,
        mut fate: impl FnMut(&ClaimPath) -> Result<Fate, Error>,
    ) -> Result<Outcome, Error> {
        match self {
            Self::LeaveAll => Ok(Outcome::LeftAll),
            Self::KeepAll(kept) => Ok(Outcome::Kept(kept)),
            Self::Delete(paths) => {
                let mut dispositions = Vec::with_capacity(paths.len());
                for path in paths {
                    let answer = fate(&path)?;
                    dispositions.push(Disposition { path, fate: answer });
                }
                Ok(Outcome::Deleting(dispositions))
            }
        }
    }
}

impl Outcome {
    /// The files to delete: those git can give back, in path order.
    pub(crate) fn to_delete(&self) -> Vec<&ClaimPath> {
        match self {
            Self::LeftAll | Self::Kept(_) => Vec::new(),
            Self::Deleting(dispositions) => dispositions
                .iter()
                .filter(|disposition| disposition.fate == Fate::Deleted)
                .map(|disposition| &disposition.path)
                .collect(),
        }
    }
}

/// Decides what to try with the files of one wearing.
///
/// `claimed` is called only when the mode needs to know the files:
/// `--keep-files` touches none, so the closure is never called and files that
/// cannot be known stop nothing. A caller that has surveyed for another reason
/// hands the answer in through the closure all the same.
///
/// # Errors
///
/// Returns [`Unknowable`] when the mode would delete files and a refusal
/// names the wearing, so the files it claims are not all known.
pub(crate) fn plan(mode: Mode, claimed: impl FnOnce() -> Claimed) -> Result<Plan, Unknowable> {
    let claimed = match mode {
        Mode::KeepFiles => return Ok(Plan::LeaveAll),
        Mode::Bare | Mode::DeleteFiles => claimed(),
    };
    let files = match claimed {
        Claimed::Unknown(unknowable) => return Err(unknowable),
        Claimed::Known(files) => files,
    };
    let present: Vec<Kept> = files
        .iter()
        .filter_map(|file| match file.state {
            FileState::Matches => Some(Kept {
                path: file.path.clone(),
                state: Presence::Matches,
            }),
            FileState::Changed => Some(Kept {
                path: file.path.clone(),
                state: Presence::Changed,
            }),
            FileState::Missing => None,
        })
        .collect();
    let any_changed = present.iter().any(|kept| kept.state == Presence::Changed);
    Ok(match (mode, any_changed) {
        (Mode::Bare, true) => Plan::KeepAll(present),
        (Mode::Bare, false) | (Mode::DeleteFiles, _) => {
            Plan::Delete(present.into_iter().map(|kept| kept.path).collect())
        }
        (Mode::KeepFiles, _) => unreachable!("--keep-files returned before the files were read"),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        Claimed, ClaimedFile, Disposition, Fate, FileState, Kept, LeftBecause, Mode, Outcome, Plan,
        Presence, Unknowable, plan,
    };
    use crate::claim::{Claimant, Drift, DriftReason, Overlap};
    use crate::survey::poison::claim;
    use crate::survey::{Refusal, Row, Survey};
    use crate::workspace::WearingRefusal;

    fn file(path: &str, state: FileState) -> ClaimedFile {
        ClaimedFile {
            path: claim(path),
            state,
        }
    }

    fn kept(path: &str, state: Presence) -> Kept {
        Kept {
            path: claim(path),
            state,
        }
    }

    fn paths(paths: &[&str]) -> Vec<crate::claim::ClaimPath> {
        paths.iter().map(|path| claim(path)).collect()
    }

    /// Three files in path order, every one matching.
    fn all_matching() -> Claimed {
        Claimed::known(vec![
            file("a.yml", FileState::Matches),
            file("b/c.yml", FileState::Matches),
            file("d.yml", FileState::Matches),
        ])
    }

    fn unknown() -> Claimed {
        Claimed::Unknown(Unknowable {
            reasons: vec!["one reason".to_owned()],
        })
    }

    /// A survey of a workspace whose `a/Cargo.toml` wears `ci` and `lint`, and
    /// whose `b/Cargo.toml` wears `ci`, holding `rows` and `refusals`.
    fn survey_of(rows: Vec<Row>, refusals: Vec<Refusal>) -> Survey<'static> {
        Survey {
            root: PathBuf::from("/workspace"),
            worn: Vec::new(),
            rows,
            refusals,
        }
    }

    fn row(manifest: &str, key: &str, path: &str, drift: Drift) -> Row {
        Row {
            path: claim(path),
            claimant: Claimant {
                manifest: manifest.to_owned(),
                dependency: key.to_owned(),
                skeleton: "a-skeleton".to_owned(),
                version: semver::Version::new(0, 1, 0),
            },
            drift,
            rendered: Vec::new(),
        }
    }

    fn dangling(manifest: &str, key: &str) -> Refusal {
        Refusal::Wearing(WearingRefusal::NamesNoDependency {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
        })
    }

    #[test]
    fn the_files_of_a_wearing_are_the_rows_of_that_manifest_and_key_with_their_states() {
        let survey = survey_of(
            vec![
                row("a/Cargo.toml", "ci", "z.yml", Drift::Matches),
                row("a/Cargo.toml", "lint", "other.yml", Drift::Matches),
                row("b/Cargo.toml", "ci", "elsewhere.yml", Drift::Matches),
                row(
                    "a/Cargo.toml",
                    "ci",
                    "a.yml",
                    Drift::Drifted(DriftReason::Changed),
                ),
                row(
                    "a/Cargo.toml",
                    "ci",
                    "m.yml",
                    Drift::Drifted(DriftReason::Missing),
                ),
            ],
            Vec::new(),
        );

        assert_eq!(
            Claimed::of(&survey, "a/Cargo.toml", "ci"),
            Claimed::Known(vec![
                file("a.yml", FileState::Changed),
                file("m.yml", FileState::Missing),
                file("z.yml", FileState::Matches),
            ])
        );
    }

    #[test]
    fn a_refusal_that_names_the_wearing_makes_its_files_unknown_with_that_refusals_words() {
        let survey = survey_of(
            vec![row("a/Cargo.toml", "ci", "a.yml", Drift::Matches)],
            vec![
                dangling("a/Cargo.toml", "ci"),
                dangling("a/Cargo.toml", "lint"),
            ],
        );

        let Claimed::Unknown(unknowable) = Claimed::of(&survey, "a/Cargo.toml", "ci") else {
            panic!("a refusal names the wearing, so its files are unknown");
        };

        assert_eq!(
            unknowable.reasons,
            [dangling("a/Cargo.toml", "ci").message()]
        );
    }

    #[test]
    fn a_refusal_that_names_other_wearings_leaves_the_files_known() {
        // The same key in another manifest, and another key in this one, are
        // other wearings.
        let survey = survey_of(
            vec![row("a/Cargo.toml", "ci", "a.yml", Drift::Matches)],
            vec![
                dangling("b/Cargo.toml", "ci"),
                dangling("a/Cargo.toml", "lint"),
            ],
        );

        assert_eq!(
            Claimed::of(&survey, "a/Cargo.toml", "ci"),
            Claimed::Known(vec![file("a.yml", FileState::Matches)])
        );
    }

    #[test]
    fn an_overlap_makes_the_files_of_every_claimant_unknown() {
        let claimant = |manifest: &str, key: &str| Claimant {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        };
        let survey = survey_of(
            Vec::new(),
            vec![Refusal::Overlap(Overlap {
                paths: vec![claim("a.yml")],
                claimants: vec![
                    claimant("a/Cargo.toml", "ci"),
                    claimant("a/Cargo.toml", "lint"),
                ],
            })],
        );

        for key in ["ci", "lint"] {
            assert!(
                matches!(
                    Claimed::of(&survey, "a/Cargo.toml", key),
                    Claimed::Unknown(_)
                ),
                "{key}"
            );
        }
    }

    #[test]
    fn a_wearing_that_claims_nothing_and_is_refused_by_nothing_has_no_files() {
        assert_eq!(
            Claimed::of(&survey_of(Vec::new(), Vec::new()), "a/Cargo.toml", "ci"),
            Claimed::Known(Vec::new())
        );
    }

    #[test]
    fn each_mode_names_the_flag_that_was_typed_or_none() {
        assert_eq!(Mode::Bare.flag(), None);
        assert_eq!(Mode::DeleteFiles.flag(), Some("--delete-files"));
        assert_eq!(Mode::KeepFiles.flag(), Some("--keep-files"));
    }

    #[test]
    fn bare_with_every_file_matching_deletes_them_all_in_path_order() {
        assert_eq!(
            plan(Mode::Bare, all_matching),
            Ok(Plan::Delete(paths(&["a.yml", "b/c.yml", "d.yml"])))
        );
    }

    #[test]
    fn bare_with_one_changed_file_keeps_every_file_that_is_there() {
        // The changed file is the project's own now, and the matching ones
        // are kept with it rather than leaving a half-deleted skeleton.
        let claimed = || {
            Claimed::known(vec![
                file("a.yml", FileState::Matches),
                file("b/c.yml", FileState::Changed),
                file("d.yml", FileState::Missing),
            ])
        };

        assert_eq!(
            plan(Mode::Bare, claimed),
            Ok(Plan::KeepAll(vec![
                kept("a.yml", Presence::Matches),
                kept("b/c.yml", Presence::Changed),
            ]))
        );
    }

    #[test]
    fn bare_with_a_file_missing_deletes_the_rest_without_naming_it() {
        let claimed = || {
            Claimed::known(vec![
                file("a.yml", FileState::Matches),
                file("b.yml", FileState::Missing),
            ])
        };

        assert_eq!(
            plan(Mode::Bare, claimed),
            Ok(Plan::Delete(paths(&["a.yml"])))
        );
    }

    #[test]
    fn bare_with_none_present_deletes_nothing() {
        let claimed = || Claimed::known(vec![file("a.yml", FileState::Missing)]);

        assert_eq!(plan(Mode::Bare, claimed), Ok(Plan::Delete(Vec::new())));
    }

    #[test]
    fn bare_over_a_skeleton_that_claims_no_file_deletes_nothing() {
        assert_eq!(
            plan(Mode::Bare, || Claimed::known(Vec::new())),
            Ok(Plan::Delete(Vec::new()))
        );
    }

    #[test]
    fn delete_files_deletes_changed_files_too_and_skips_missing_ones() {
        let claimed = || {
            Claimed::known(vec![
                file("a.yml", FileState::Changed),
                file("b.yml", FileState::Missing),
                file("c.yml", FileState::Matches),
            ])
        };

        assert_eq!(
            plan(Mode::DeleteFiles, claimed),
            Ok(Plan::Delete(paths(&["a.yml", "c.yml"])))
        );
    }

    #[test]
    fn delete_files_with_none_present_deletes_nothing() {
        let claimed = || Claimed::known(vec![file("a.yml", FileState::Missing)]);

        assert_eq!(
            plan(Mode::DeleteFiles, claimed),
            Ok(Plan::Delete(Vec::new()))
        );
    }

    #[test]
    fn keep_files_leaves_everything_and_never_asks_for_the_files() {
        // `--keep-files` has no use for the files, so the closure that would
        // hand them over must not be called, whether they could have been
        // known or not.
        let asked = std::cell::Cell::new(0_usize);
        let claimed = || {
            asked.set(asked.get() + 1);
            unknown()
        };

        assert_eq!(plan(Mode::KeepFiles, claimed), Ok(Plan::LeaveAll));
        assert_eq!(asked.get(), 0, "the files were asked for");
    }

    #[test]
    fn unknown_files_refuse_a_bare_run_and_a_delete_run_with_the_reasons() {
        for mode in [Mode::Bare, Mode::DeleteFiles] {
            assert_eq!(
                plan(mode, unknown),
                Err(Unknowable {
                    reasons: vec!["one reason".to_owned()]
                }),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn known_puts_the_files_in_path_order() {
        let Claimed::Known(files) = Claimed::known(vec![
            file("z.yml", FileState::Matches),
            file("a/b.yml", FileState::Matches),
            file("a.yml", FileState::Matches),
        ]) else {
            panic!("known files are known");
        };

        let order: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(order, ["a.yml", "a/b.yml", "z.yml"]);
    }

    #[test]
    #[should_panic(expected = "a wearing claims each path once")]
    fn a_path_claimed_twice_is_a_defect_in_the_survey() {
        let _ = Claimed::known(vec![
            file("a.yml", FileState::Matches),
            file("a.yml", FileState::Changed),
        ]);
    }

    #[test]
    fn settling_a_delete_asks_about_each_file_in_order_and_records_the_answers() {
        let mut asked = Vec::new();
        let outcome = Plan::Delete(paths(&["a.yml", "b.yml", "c.yml"])).settle(|path| {
            asked.push(path.as_str().to_owned());
            Ok::<_, ()>(if path.as_str() == "b.yml" {
                Fate::Left(LeftBecause::NotTracked)
            } else {
                Fate::Deleted
            })
        });

        assert_eq!(asked, ["a.yml", "b.yml", "c.yml"]);
        assert_eq!(
            outcome,
            Ok(Outcome::Deleting(vec![
                Disposition {
                    path: claim("a.yml"),
                    fate: Fate::Deleted
                },
                Disposition {
                    path: claim("b.yml"),
                    fate: Fate::Left(LeftBecause::NotTracked)
                },
                Disposition {
                    path: claim("c.yml"),
                    fate: Fate::Deleted
                },
            ]))
        );
    }

    #[test]
    fn settling_a_delete_stops_at_the_first_question_that_cannot_be_asked() {
        let mut asked = 0_usize;
        let outcome = Plan::Delete(paths(&["a.yml", "b.yml"])).settle(|_| {
            asked += 1;
            Err::<Fate, _>("git could not be asked")
        });

        assert_eq!(outcome, Err("git could not be asked"));
        assert_eq!(asked, 1);
    }

    #[test]
    fn settling_a_plan_that_deletes_nothing_asks_nothing() {
        let kept_all = vec![kept("a.yml", Presence::Changed)];
        let never = |_: &crate::claim::ClaimPath| -> Result<Fate, ()> {
            panic!("no file is deleted, so none is asked about")
        };

        assert_eq!(Plan::LeaveAll.settle(never), Ok(Outcome::LeftAll));
        assert_eq!(
            Plan::KeepAll(kept_all.clone()).settle(never),
            Ok(Outcome::Kept(kept_all))
        );
        assert_eq!(
            Plan::Delete(Vec::new()).settle(never),
            Ok(Outcome::Deleting(Vec::new()))
        );
    }

    #[test]
    fn only_the_files_git_can_give_back_are_to_be_deleted() {
        let outcome = Outcome::Deleting(vec![
            Disposition {
                path: claim("a.yml"),
                fate: Fate::Deleted,
            },
            Disposition {
                path: claim("b.yml"),
                fate: Fate::Left(LeftBecause::CannotShowItHolds),
            },
            Disposition {
                path: claim("c.yml"),
                fate: Fate::Deleted,
            },
        ]);

        assert_eq!(outcome.to_delete(), [&claim("a.yml"), &claim("c.yml")]);
        assert!(Outcome::LeftAll.to_delete().is_empty());
        assert!(
            Outcome::Kept(vec![kept("a.yml", Presence::Changed)])
                .to_delete()
                .is_empty()
        );
    }
}
