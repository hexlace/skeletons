//! What becomes of the files a skeleton wrote: which are deleted, which are
//! kept because the project has changed them, which are left because git could
//! not give them back or because `remove` itself writes them, and when none of
//! that can be said at all.
//!
//! The decision is in two steps, so that each is a pure function of values.
//! [`plan`] reads the flag the wearer typed and the files the skeleton claims,
//! and says what to try: nothing, or to put every file that is in the work tree
//! to git and judge the answers by a [`Rule`]. [`Plan::settle`] then takes
//! git's answer for each of those files, in path order, and says what became of
//! them, which is what the success lines report.
//!
//! The lockfile `remove` brings current in the run is never deleted under any
//! flag. [`plan`] sets a claimed path that is the lockfile's aside by comparing
//! it with the lockfile's path, before git is asked about anything: it is left
//! whatever git would say, so git is not asked, and it is nobody's change to
//! protect, so it counts for nothing in the decision below.
//! It is a decision of the same kind as a file git could not give back, and
//! only a file git was asked about and could give back can ever be handed to
//! the deleter. The manifest `remove` edits needs no such rule: the walk of a
//! skeleton's files refuses any name like a Cargo manifest, so no skeleton
//! claims one.
//!
//! Whether to keep the files because one has changed is decided only after
//! git has answered, and over the files git could give back alone: a file git
//! could not give back is left under every flag and is nobody's change to
//! protect. The plan therefore cannot say that the files are kept, because
//! that is not known until the answers are.

use super::names::{ManifestShown, WearingKey};
use super::non_empty::NonEmpty;
use crate::claim::{ClaimPath, Claimant, Drift, DriftReason};
use crate::survey::{Refusal, RefusedWearing, Survey};

/// Which flag the wearer typed about the skeleton's files, if one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesFlag {
    /// Neither flag: delete every file that still matches what the skeleton
    /// renders, and keep every file if the project has changed one.
    Bare,
    /// `--delete-files`: delete every file, changed ones included.
    DeleteFiles,
    /// `--keep-files`: leave every file where it is.
    KeepFiles,
}

impl FilesFlag {
    /// The flag as it is typed, or `None` for neither.
    pub(crate) const fn typed(self) -> Option<&'static str> {
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
/// only when no refusal names the wearing. Each reason is one such refusal.
/// There is always at least one, since files are unknowable only because a
/// refusal names the wearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unknowable {
    pub(crate) reasons: NonEmpty<Reason>,
}

/// One refusal that names a wearing, as far as `remove` words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A refusal of any kind but an overlap with other wearings, in its own
    /// words, which are already one escaped line.
    Refused(String),
    /// The wearing claims paths that other wearings claim. Running `remove`
    /// again does not clear it, so it is data for `remove` to word, not a
    /// message to quote.
    Overlap(Box<Overlapping>),
}

/// An overlap between the wearing being removed and others: who else claims,
/// and the paths that collide. Boxed in [`Reason`] so that the refusal that
/// carries a reason, which every function returning one pays for on each move,
/// stays small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Overlapping {
    /// The other wearings in the overlap, each once, in the order the survey
    /// lists its claims. Never the wearing being removed.
    pub(crate) others: NonEmpty<Claimant>,
    /// Every path in the overlap, sorted: the paths the wearings share, or,
    /// where they only collide, the paths that do.
    pub(crate) paths: NonEmpty<ClaimPath>,
}

impl Reason {
    /// The reason `refusal`, which names the wearing `key` of the manifest
    /// shown as `manifest`, gives for its files being unknown.
    fn of(refusal: &Refusal, manifest: &ManifestShown, key: &WearingKey) -> Self {
        let Refusal::Overlap(overlap) = refusal else {
            return Self::Refused(refusal.message());
        };
        let mut others: Vec<Claimant> = Vec::new();
        for claimant in &overlap.claimants {
            let this_wearing =
                claimant.manifest == manifest.as_str() && claimant.dependency == key.as_str();
            let already_listed = others.iter().any(|listed| {
                listed.manifest == claimant.manifest && listed.dependency == claimant.dependency
            });
            if !this_wearing && !already_listed {
                others.push(claimant.clone());
            }
        }
        // The paths are ours: an overlap is two or more claims, each with a
        // path.
        let Some(paths) = NonEmpty::from_vec(overlap.paths.clone()) else {
            unreachable!("an overlap is two or more claims, each with a path")
        };
        // With no other wearing to name, the claims are this wearing's alone,
        // and the survey's own words are all there is to quote.
        NonEmpty::from_vec(others).map_or_else(
            || Self::Refused(refusal.message()),
            |others| Self::Overlap(Box::new(Overlapping { others, paths })),
        )
    }
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
    pub(crate) fn of(survey: &Survey<'_>, manifest: &ManifestShown, key: &WearingKey) -> Self {
        let names_the_wearing = |wearing: &RefusedWearing| {
            wearing.manifest == manifest.as_str()
                && wearing.dependency.as_deref() == Some(key.as_str())
        };
        let mut reasons = survey
            .refusals
            .iter()
            .filter(|refusal| refusal.record().wearings.iter().any(names_the_wearing))
            .map(|refusal| Reason::of(refusal, manifest, key));
        if let Some(first) = reasons.next() {
            return Self::Unknown(Unknowable {
                reasons: NonEmpty::new(first, reasons.collect()),
            });
        }
        let files = survey
            .rows
            .iter()
            .filter(|row| {
                row.claimant.manifest == manifest.as_str()
                    && row.claimant.dependency == key.as_str()
            })
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

/// A file that is in the work tree, and whether it holds what the skeleton
/// renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Present {
    pub(crate) path: ClaimPath,
    pub(crate) state: Presence,
}

/// The state of a file that is in the work tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Matches,
    Changed,
}

/// How the answers from git decide what becomes of the files that git could
/// give back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Bare: delete them, unless one of them has changed, and then keep them
    /// all.
    KeepAllIfOneChanged,
    /// `--delete-files`: delete them, changed ones included.
    DeleteAll,
}

/// What `remove` means to do with the files, before git is asked whether it
/// can give back any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// `--keep-files`: nothing is touched, whatever the survey found.
    LeaveAll,
    /// Ask git about every file that is in the work tree and that is not the
    /// lockfile `remove` brings current, in path order, and judge the answers
    /// by `rule`. `present` is empty when no such file is there. The lockfile,
    /// when the skeleton claims it and it is there, is in `withheld` and asked
    /// of no one.
    Prove {
        rule: Rule,
        present: Vec<Present>,
        withheld: Vec<ClaimPath>,
    },
}

/// How git answered for one file in the work tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fate {
    /// Git can give the file back, so nothing is lost by deleting it. The rule
    /// then deletes the file or keeps it; it is called `Deleted` because the
    /// wearer is told of a file deleted, which is the only place a `Fate` is
    /// read after the rule has judged.
    Deleted,
    /// Git could not give the file back, so it stays.
    Left(LeftBecause),
}

impl Fate {
    /// Whether git can give the file back, which is what makes it the rule's to
    /// delete or to keep.
    const fn git_can_give_back(&self) -> bool {
        match self {
            Self::Deleted => true,
            Self::Left(_) => false,
        }
    }
}

/// Why a file is left where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftBecause {
    /// It is the lockfile `remove` brings current in this run, so deleting it
    /// would undo what the run has just written.
    Lockfile,
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

/// A file in the work tree, and what git said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Disposition {
    pub(crate) path: ClaimPath,
    pub(crate) fate: Fate,
}

/// What became of one file when the rule kept the files git could give back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Remaining {
    /// Git could give the file back and the rule kept it.
    Kept(Present),
    /// The file was left for a reason of its own: git could not give it back,
    /// or it is the lockfile `remove` brings current.
    Left {
        path: ClaimPath,
        because: LeftBecause,
    },
}

/// What became of the files, which is what the success lines report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Settlement {
    /// Every file was left where it is, because `--keep-files` was typed.
    LeftAll,
    /// Nothing was deleted, because one of the files git could give back has
    /// changed: those files were kept, and the files left for a reason of
    /// their own were left, in path order.
    Kept(Vec<Remaining>),
    /// The files that are there, each deleted or left, in path order. Empty
    /// when none is there.
    Deleting(Vec<Disposition>),
}

impl Plan {
    /// Turns the plan into what became of the files, asking `fate` about each
    /// file that is in the work tree and that is not the lockfile, in path
    /// order, and only those.
    ///
    /// Every such file is asked about, changed or not, before anything is
    /// judged, because whether the files have changed is a question about the
    /// ones git can give back. A plan to prove settles into one entry for each
    /// path planned and each path withheld, in path order.
    ///
    /// # Errors
    ///
    /// Returns the first error `fate` returns: a question to git that could
    /// not be asked ends the run before anything is written.
    pub(crate) fn settle<Error>(
        self,
        mut fate: impl FnMut(&ClaimPath) -> Result<Fate, Error>,
    ) -> Result<Settlement, Error> {
        match self {
            Self::LeaveAll => Ok(Settlement::LeftAll),
            Self::Prove {
                rule,
                present,
                withheld,
            } => {
                let (asked_count, withheld_count) = (present.len(), withheld.len());
                assert!(
                    present.windows(2).all(|pair| pair[0].path < pair[1].path),
                    "the files are settled in path order, as they were planned"
                );
                let mut answered = Vec::with_capacity(asked_count + withheld_count);
                for file in present {
                    let answer = fate(&file.path)?;
                    answered.push(Answered::Asked { file, fate: answer });
                }
                answered.extend(withheld.into_iter().map(Answered::Withheld));
                answered.sort_by(|left, right| left.path().cmp(right.path()));
                assert_eq!(
                    answered.len(),
                    asked_count + withheld_count,
                    "each path planned is settled once"
                );
                Ok(judge(rule, answered))
            }
        }
    }
}

/// A file in the work tree, and what became of it before the rule judged: git
/// answered for it, or it is the lockfile and so it was never put to git.
enum Answered {
    Asked { file: Present, fate: Fate },
    Withheld(ClaimPath),
}

impl Answered {
    const fn path(&self) -> &ClaimPath {
        match self {
            Self::Asked { file, .. } => &file.path,
            Self::Withheld(path) => path,
        }
    }
}

/// Judges the answers by `rule`: the files left for a reason of their own are
/// left whatever the rule, and the rule applies to the files git could give
/// back alone.
fn judge(rule: Rule, answered: Vec<Answered>) -> Settlement {
    let a_file_git_can_give_back_has_changed = answered.iter().any(|file| match file {
        Answered::Asked { file, fate } => {
            fate.git_can_give_back() && file.state == Presence::Changed
        }
        Answered::Withheld(_) => false,
    });
    match (rule, a_file_git_can_give_back_has_changed) {
        (Rule::KeepAllIfOneChanged, true) => Settlement::Kept(
            answered
                .into_iter()
                .map(|file| match file {
                    Answered::Asked {
                        file,
                        fate: Fate::Deleted,
                    } => Remaining::Kept(file),
                    Answered::Asked {
                        file,
                        fate: Fate::Left(because),
                    } => Remaining::Left {
                        path: file.path,
                        because,
                    },
                    Answered::Withheld(path) => Remaining::Left {
                        path,
                        because: LeftBecause::Lockfile,
                    },
                })
                .collect(),
        ),
        (Rule::KeepAllIfOneChanged, false) | (Rule::DeleteAll, _) => Settlement::Deleting(
            answered
                .into_iter()
                .map(|file| match file {
                    Answered::Asked { file, fate } => Disposition {
                        path: file.path,
                        fate,
                    },
                    Answered::Withheld(path) => Disposition {
                        path,
                        fate: Fate::Left(LeftBecause::Lockfile),
                    },
                })
                .collect(),
        ),
    }
}

impl Settlement {
    /// The files to delete: those git can give back, in path order.
    pub(crate) fn to_delete(&self) -> Vec<&ClaimPath> {
        match self {
            Self::LeftAll | Self::Kept(_) => Vec::new(),
            Self::Deleting(dispositions) => dispositions
                .iter()
                .filter(|disposition| disposition.fate.git_can_give_back())
                .map(|disposition| &disposition.path)
                .collect(),
        }
    }
}

/// Decides what to try with the files of one wearing.
///
/// `--keep-files` touches no file, so it leaves every one whatever `claimed`
/// is, and files that cannot be known stop nothing. Every other flag deletes
/// from the files that are there, and so needs to know them all. Neither is
/// judged here: which files git can give back is asked of git first, and the
/// rule is applied to what it says in [`Plan::settle`]. A claimed file whose
/// path is `lockfile`, spelled as a claim spells a path, is set aside here, and
/// git is never asked about it. The comparison is exact text. A claim spelled
/// differently from the entry on disk, by case or by Unicode normalisation,
/// never reaches it as a file of this wearing: the survey accepts a claimed
/// component only when its directory lists an entry spelled exactly so, and
/// refuses the claim when it lists a variant, which makes the wearing's files
/// unknowable.
///
/// # Errors
///
/// Returns [`Unknowable`] when the flag would delete files and a refusal names
/// the wearing, so the files it claims are not all known.
pub(crate) fn plan(flag: FilesFlag, claimed: Claimed, lockfile: &str) -> Result<Plan, Unknowable> {
    let (rule, files) = match (flag, claimed) {
        (FilesFlag::KeepFiles, _) => return Ok(Plan::LeaveAll),
        (FilesFlag::Bare | FilesFlag::DeleteFiles, Claimed::Unknown(unknowable)) => {
            return Err(unknowable);
        }
        (FilesFlag::Bare, Claimed::Known(files)) => (Rule::KeepAllIfOneChanged, files),
        (FilesFlag::DeleteFiles, Claimed::Known(files)) => (Rule::DeleteAll, files),
    };
    let mut present = Vec::new();
    let mut withheld = Vec::new();
    for file in files {
        let state = match file.state {
            FileState::Matches => Presence::Matches,
            FileState::Changed => Presence::Changed,
            FileState::Missing => continue,
        };
        if file.path.as_str() == lockfile {
            withheld.push(file.path);
        } else {
            present.push(Present {
                path: file.path,
                state,
            });
        }
    }
    Ok(Plan::Prove {
        rule,
        present,
        withheld,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        Claimed, ClaimedFile, Disposition, Fate, FileState, FilesFlag, LeftBecause, Overlapping,
        Plan, Presence, Present, Reason, Remaining, Rule, Settlement, Unknowable, plan,
    };
    use crate::claim::{Claimant, Drift, DriftReason, Overlap};
    use crate::remove::names::{ManifestShown, WearingKey};
    use crate::remove::non_empty::NonEmpty;
    use crate::survey::poison::claim;
    use crate::survey::{Refusal, Row, Survey};
    use crate::workspace::WearingRefusal;

    fn file(path: &str, state: FileState) -> ClaimedFile {
        ClaimedFile {
            path: claim(path),
            state,
        }
    }

    fn present(path: &str, state: Presence) -> Present {
        Present {
            path: claim(path),
            state,
        }
    }

    /// The lockfile's path as a claim spells it, in a workspace whose command
    /// line crate is at its root.
    const LOCKFILE_PATH: &str = "Cargo.lock";

    /// A plan that puts every one of `files` to git and judges by `rule`, and
    /// sets nothing aside.
    fn prove(rule: Rule, files: &[(&str, Presence)]) -> Plan {
        prove_beside(rule, files, &[])
    }

    /// [`prove`], with the lockfile set aside.
    fn prove_beside(rule: Rule, files: &[(&str, Presence)], withheld: &[&str]) -> Plan {
        Plan::Prove {
            rule,
            present: files
                .iter()
                .map(|(path, state)| present(path, *state))
                .collect(),
            withheld: withheld.iter().map(|path| claim(path)).collect(),
        }
    }

    /// An answer for each path: git gives back every file but those named.
    fn all_but<'a>(
        left: &'a [(&'static str, LeftBecause)],
    ) -> impl FnMut(&crate::claim::ClaimPath) -> Result<Fate, ()> + 'a {
        move |path| {
            Ok(left
                .iter()
                .find(|(named, _)| *named == path.as_str())
                .map_or(Fate::Deleted, |(_, because)| Fate::Left(*because)))
        }
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
            reasons: NonEmpty::new(Reason::Refused("one reason".to_owned()), Vec::new()),
        })
    }

    fn manifest(text: &str) -> ManifestShown {
        ManifestShown::new(text)
    }

    fn key(text: &str) -> WearingKey {
        WearingKey::new(text)
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
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci")),
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

        let Claimed::Unknown(unknowable) =
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci"))
        else {
            panic!("a refusal names the wearing, so its files are unknown");
        };

        assert_eq!(
            unknowable.reasons,
            NonEmpty::new(
                Reason::Refused(dangling("a/Cargo.toml", "ci").message()),
                Vec::new()
            )
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
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci")),
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

        for name in ["ci", "lint"] {
            assert!(
                matches!(
                    Claimed::of(&survey, &manifest("a/Cargo.toml"), &key(name)),
                    Claimed::Unknown(_)
                ),
                "{name}"
            );
        }
    }

    fn claimant(manifest: &str, key: &str) -> Claimant {
        Claimant {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        }
    }

    fn overlap(paths: &[&str], claimants: Vec<Claimant>) -> Refusal {
        Refusal::Overlap(Overlap {
            paths: paths.iter().map(|path| claim(path)).collect(),
            claimants,
        })
    }

    #[test]
    fn an_overlap_is_data_naming_the_other_wearings_and_the_paths_not_the_wearing_itself() {
        // `ci` claims two of the three paths and `lint` and the same key in
        // another manifest claim the rest, so the survey lists `ci` twice. The
        // reason for `ci` names the other two wearings, once each, in the order
        // the survey lists them, and every path; the same wearing's key in
        // another manifest is another wearing.
        let survey = survey_of(
            Vec::new(),
            vec![overlap(
                &["A/y", "a/x", "a/z"],
                vec![
                    claimant("a/Cargo.toml", "ci"),
                    claimant("a/Cargo.toml", "lint"),
                    claimant("a/Cargo.toml", "ci"),
                    claimant("b/Cargo.toml", "ci"),
                    claimant("a/Cargo.toml", "lint"),
                ],
            )],
        );

        let Claimed::Unknown(unknowable) =
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci"))
        else {
            panic!("an overlap names the wearing, so its files are unknown");
        };

        assert_eq!(
            unknowable.reasons,
            NonEmpty::new(
                Reason::Overlap(Box::new(Overlapping {
                    others: NonEmpty::new(
                        claimant("a/Cargo.toml", "lint"),
                        vec![claimant("b/Cargo.toml", "ci")]
                    ),
                    paths: NonEmpty::new(claim("A/y"), vec![claim("a/x"), claim("a/z")]),
                })),
                Vec::new()
            )
        );
    }

    #[test]
    fn an_overlap_beside_another_refusal_is_one_more_reason_in_the_surveys_order() {
        let survey = survey_of(
            Vec::new(),
            vec![
                overlap(
                    &["a.yml"],
                    vec![
                        claimant("a/Cargo.toml", "ci"),
                        claimant("a/Cargo.toml", "lint"),
                    ],
                ),
                dangling("a/Cargo.toml", "ci"),
            ],
        );

        let Claimed::Unknown(unknowable) =
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci"))
        else {
            panic!("refusals name the wearing, so its files are unknown");
        };

        assert_eq!(
            unknowable.reasons,
            NonEmpty::new(
                Reason::Overlap(Box::new(Overlapping {
                    others: NonEmpty::new(claimant("a/Cargo.toml", "lint"), Vec::new()),
                    paths: NonEmpty::new(claim("a.yml"), Vec::new()),
                })),
                vec![Reason::Refused(dangling("a/Cargo.toml", "ci").message())]
            )
        );
    }

    #[test]
    fn an_overlap_among_the_claims_of_the_wearing_alone_keeps_the_surveys_words() {
        // There is no other wearing to name, so the survey's own message is
        // the reason.
        let refusal = overlap(
            &["A/y", "a/x"],
            vec![
                claimant("a/Cargo.toml", "ci"),
                claimant("a/Cargo.toml", "ci"),
            ],
        );
        let message = refusal.message();
        let survey = survey_of(Vec::new(), vec![refusal]);

        let Claimed::Unknown(unknowable) =
            Claimed::of(&survey, &manifest("a/Cargo.toml"), &key("ci"))
        else {
            panic!("an overlap names the wearing, so its files are unknown");
        };

        assert_eq!(
            unknowable.reasons,
            NonEmpty::new(Reason::Refused(message), Vec::new())
        );
    }

    #[test]
    fn a_wearing_that_claims_nothing_and_is_refused_by_nothing_has_no_files() {
        assert_eq!(
            Claimed::of(
                &survey_of(Vec::new(), Vec::new()),
                &manifest("a/Cargo.toml"),
                &key("ci")
            ),
            Claimed::Known(Vec::new())
        );
    }

    #[test]
    fn each_flag_is_typed_as_itself_and_neither_as_nothing() {
        assert_eq!(FilesFlag::Bare.typed(), None);
        assert_eq!(FilesFlag::DeleteFiles.typed(), Some("--delete-files"));
        assert_eq!(FilesFlag::KeepFiles.typed(), Some("--keep-files"));
    }

    #[test]
    fn bare_with_every_file_matching_asks_git_about_them_all_in_path_order() {
        assert_eq!(
            plan(FilesFlag::Bare, all_matching(), LOCKFILE_PATH),
            Ok(prove(
                Rule::KeepAllIfOneChanged,
                &[
                    ("a.yml", Presence::Matches),
                    ("b/c.yml", Presence::Matches),
                    ("d.yml", Presence::Matches),
                ]
            ))
        );
    }

    #[test]
    fn bare_with_one_changed_file_still_only_asks_git_and_decides_nothing() {
        // Whether a changed file keeps the others depends on whether git can
        // give it back, which is not known until git has been asked, so the
        // plan holds every file that is there, changed or not, and no keep.
        let claimed = Claimed::known(vec![
            file("a.yml", FileState::Matches),
            file("b/c.yml", FileState::Changed),
            file("d.yml", FileState::Missing),
        ]);

        assert_eq!(
            plan(FilesFlag::Bare, claimed, LOCKFILE_PATH),
            Ok(prove(
                Rule::KeepAllIfOneChanged,
                &[("a.yml", Presence::Matches), ("b/c.yml", Presence::Changed)]
            ))
        );
    }

    #[test]
    fn bare_with_a_file_missing_does_not_ask_about_it() {
        let claimed = Claimed::known(vec![
            file("a.yml", FileState::Matches),
            file("b.yml", FileState::Missing),
        ]);

        assert_eq!(
            plan(FilesFlag::Bare, claimed, LOCKFILE_PATH),
            Ok(prove(
                Rule::KeepAllIfOneChanged,
                &[("a.yml", Presence::Matches)]
            ))
        );
    }

    #[test]
    fn bare_with_none_present_asks_about_nothing() {
        let claimed = Claimed::known(vec![file("a.yml", FileState::Missing)]);

        assert_eq!(
            plan(FilesFlag::Bare, claimed, LOCKFILE_PATH),
            Ok(prove(Rule::KeepAllIfOneChanged, &[]))
        );
    }

    #[test]
    fn bare_over_a_skeleton_that_claims_no_file_asks_about_nothing() {
        assert_eq!(
            plan(FilesFlag::Bare, Claimed::known(Vec::new()), LOCKFILE_PATH),
            Ok(prove(Rule::KeepAllIfOneChanged, &[]))
        );
    }

    #[test]
    fn delete_files_asks_about_changed_files_too_and_skips_missing_ones() {
        let claimed = Claimed::known(vec![
            file("a.yml", FileState::Changed),
            file("b.yml", FileState::Missing),
            file("c.yml", FileState::Matches),
        ]);

        assert_eq!(
            plan(FilesFlag::DeleteFiles, claimed, LOCKFILE_PATH),
            Ok(prove(
                Rule::DeleteAll,
                &[("a.yml", Presence::Changed), ("c.yml", Presence::Matches)]
            ))
        );
    }

    #[test]
    fn delete_files_with_none_present_asks_about_nothing() {
        let claimed = Claimed::known(vec![file("a.yml", FileState::Missing)]);

        assert_eq!(
            plan(FilesFlag::DeleteFiles, claimed, LOCKFILE_PATH),
            Ok(prove(Rule::DeleteAll, &[]))
        );
    }

    #[test]
    fn keep_files_leaves_everything_whether_the_files_are_known_or_not() {
        // `--keep-files` has no use for the files: it leaves them all, however
        // the survey found them. Each case is one way it can have found them,
        // and the last is the one every other flag refuses.
        let cases = [
            all_matching(),
            Claimed::known(vec![
                file("a.yml", FileState::Changed),
                file("b.yml", FileState::Missing),
            ]),
            Claimed::known(Vec::new()),
            unknown(),
        ];

        for claimed in cases {
            assert_eq!(
                plan(FilesFlag::KeepFiles, claimed.clone(), LOCKFILE_PATH),
                Ok(Plan::LeaveAll),
                "{claimed:?}"
            );
        }
    }

    #[test]
    fn unknown_files_refuse_a_bare_run_and_a_delete_run_with_the_reasons() {
        for flag in [FilesFlag::Bare, FilesFlag::DeleteFiles] {
            assert_eq!(
                plan(flag, unknown(), LOCKFILE_PATH),
                Err(Unknowable {
                    reasons: NonEmpty::new(Reason::Refused("one reason".to_owned()), Vec::new())
                }),
                "{flag:?}"
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

    fn left_remaining(path: &str, because: LeftBecause) -> Remaining {
        Remaining::Left {
            path: claim(path),
            because,
        }
    }

    const MATCHES: Presence = Presence::Matches;
    const CHANGED: Presence = Presence::Changed;
    const NOT_TRACKED: LeftBecause = LeftBecause::NotTracked;

    #[test]
    fn settling_asks_about_each_file_in_order_changed_or_not_and_records_the_answers() {
        // The first file has changed and git can give it back, which is already
        // reason to keep the rest; git is still asked about the others, since
        // one of them may be a file it could not give back.
        let mut asked = Vec::new();
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", CHANGED), ("b.yml", MATCHES), ("c.yml", MATCHES)],
        )
        .settle(|path| {
            asked.push(path.as_str().to_owned());
            Ok::<_, ()>(if path.as_str() == "b.yml" {
                Fate::Left(NOT_TRACKED)
            } else {
                Fate::Deleted
            })
        });

        assert_eq!(asked, ["a.yml", "b.yml", "c.yml"]);
        assert_eq!(
            outcome,
            Ok(Settlement::Kept(vec![
                Remaining::Kept(present("a.yml", CHANGED)),
                left_remaining("b.yml", NOT_TRACKED),
                Remaining::Kept(present("c.yml", MATCHES)),
            ]))
        );
    }

    #[test]
    fn bare_deletes_every_file_git_can_give_back_when_none_has_changed() {
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", MATCHES), ("b.yml", MATCHES), ("c.yml", MATCHES)],
        )
        .settle(all_but(&[("b.yml", NOT_TRACKED)]));

        assert_eq!(
            outcome,
            Ok(Settlement::Deleting(vec![
                deleted("a.yml"),
                left("b.yml", NOT_TRACKED),
                deleted("c.yml"),
            ]))
        );
    }

    #[test]
    fn bare_keeps_every_file_git_can_give_back_when_one_of_them_has_changed() {
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", MATCHES), ("b.yml", CHANGED), ("c.yml", MATCHES)],
        )
        .settle(all_but(&[]));

        assert_eq!(
            outcome,
            Ok(Settlement::Kept(vec![
                Remaining::Kept(present("a.yml", MATCHES)),
                Remaining::Kept(present("b.yml", CHANGED)),
                Remaining::Kept(present("c.yml", MATCHES)),
            ]))
        );
    }

    #[test]
    fn a_changed_file_git_could_not_give_back_does_not_make_bare_keep_the_others() {
        // The file git could not give back is left, edited as it is, and is
        // nobody's change to protect: the files that match are deleted.
        for because in [
            NOT_TRACKED,
            LeftBecause::NotWhatGitChecksOut,
            LeftBecause::CannotShowItHolds,
        ] {
            let outcome = prove(
                Rule::KeepAllIfOneChanged,
                &[("a.yml", MATCHES), ("b.yml", CHANGED), ("c.yml", MATCHES)],
            )
            .settle(all_but(&[("b.yml", because)]));

            assert_eq!(
                outcome,
                Ok(Settlement::Deleting(vec![
                    deleted("a.yml"),
                    left("b.yml", because),
                    deleted("c.yml"),
                ])),
                "{because:?}"
            );
        }
    }

    #[test]
    fn a_file_git_could_not_give_back_is_left_and_named_when_the_rest_are_kept() {
        // `b.yml` is left, `c.yml` has changed and git can give it back, so
        // `a.yml` and `c.yml` are kept and `b.yml` is still left, in its place
        // in path order.
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", MATCHES), ("b.yml", MATCHES), ("c.yml", CHANGED)],
        )
        .settle(all_but(&[("b.yml", NOT_TRACKED)]));

        assert_eq!(
            outcome,
            Ok(Settlement::Kept(vec![
                Remaining::Kept(present("a.yml", MATCHES)),
                left_remaining("b.yml", NOT_TRACKED),
                Remaining::Kept(present("c.yml", CHANGED)),
            ]))
        );
    }

    #[test]
    fn when_git_could_not_give_back_any_file_none_is_kept_or_deleted_by_the_rule() {
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", CHANGED), ("b.yml", MATCHES)],
        )
        .settle(|_| Ok::<_, ()>(Fate::Left(NOT_TRACKED)));

        assert_eq!(
            outcome,
            Ok(Settlement::Deleting(vec![
                left("a.yml", NOT_TRACKED),
                left("b.yml", NOT_TRACKED),
            ]))
        );
    }

    #[test]
    fn delete_files_deletes_what_git_can_give_back_changed_or_not_and_leaves_the_rest() {
        let outcome = prove(
            Rule::DeleteAll,
            &[("a.yml", CHANGED), ("b.yml", CHANGED), ("c.yml", MATCHES)],
        )
        .settle(all_but(&[("b.yml", NOT_TRACKED)]));

        assert_eq!(
            outcome,
            Ok(Settlement::Deleting(vec![
                deleted("a.yml"),
                left("b.yml", NOT_TRACKED),
                deleted("c.yml"),
            ]))
        );
    }

    #[test]
    #[should_panic(expected = "the files are settled in path order")]
    fn settling_files_planned_out_of_path_order_is_a_defect_in_the_plan() {
        let _ =
            prove(Rule::DeleteAll, &[("b.yml", MATCHES), ("a.yml", MATCHES)]).settle(all_but(&[]));
    }

    #[test]
    fn settling_stops_at_the_first_question_that_cannot_be_asked() {
        let mut asked = 0_usize;
        let outcome = prove(
            Rule::KeepAllIfOneChanged,
            &[("a.yml", MATCHES), ("b.yml", MATCHES)],
        )
        .settle(|_| {
            asked += 1;
            Err::<Fate, _>("git could not be asked")
        });

        assert_eq!(outcome, Err("git could not be asked"));
        assert_eq!(asked, 1);
    }

    #[test]
    fn settling_a_plan_with_no_file_to_ask_about_asks_nothing() {
        let never = |_: &crate::claim::ClaimPath| -> Result<Fate, ()> {
            panic!("no file is in the work tree, so none is asked about")
        };

        assert_eq!(
            Plan::LeaveAll.settle(never),
            Ok(Settlement::LeftAll),
            "--keep-files asks about nothing"
        );
        for rule in [Rule::KeepAllIfOneChanged, Rule::DeleteAll] {
            assert_eq!(
                prove(rule, &[]).settle(never),
                Ok(Settlement::Deleting(Vec::new())),
                "{rule:?}"
            );
        }
    }

    #[test]
    fn only_the_files_git_can_give_back_are_to_be_deleted() {
        let outcome = Settlement::Deleting(vec![
            deleted("a.yml"),
            left("b.yml", LeftBecause::CannotShowItHolds),
            deleted("c.yml"),
        ]);

        assert_eq!(outcome.to_delete(), [&claim("a.yml"), &claim("c.yml")]);
        assert!(Settlement::LeftAll.to_delete().is_empty());
        assert!(
            Settlement::Kept(vec![
                Remaining::Kept(present("a.yml", CHANGED)),
                left_remaining("b.yml", NOT_TRACKED),
            ])
            .to_delete()
            .is_empty()
        );
    }

    const LOCKFILE: LeftBecause = LeftBecause::Lockfile;

    #[test]
    fn a_claimed_lockfile_is_set_aside_whatever_its_state_and_whatever_the_flag() {
        // The lockfile is set aside whether it matches what the skeleton
        // renders or has changed, under both flags that delete, and is asked of
        // no one: only `a.yml` is put to git. A lockfile that is missing is not
        // there, so it is not set aside either.
        for (flag, rule) in [
            (FilesFlag::Bare, Rule::KeepAllIfOneChanged),
            (FilesFlag::DeleteFiles, Rule::DeleteAll),
        ] {
            for state in [FileState::Changed, FileState::Matches] {
                let claimed = Claimed::known(vec![
                    file("Cargo.lock", state),
                    file("a.yml", FileState::Matches),
                    file("z/Cargo.lock", FileState::Missing),
                ]);

                assert_eq!(
                    plan(flag, claimed, LOCKFILE_PATH),
                    Ok(prove_beside(rule, &[("a.yml", MATCHES)], &["Cargo.lock"])),
                    "{flag:?} {state:?}"
                );
            }
        }
    }

    #[test]
    fn a_claimed_file_is_the_lockfile_only_when_its_text_is_exactly_the_path() {
        // The workspace root is not the command line crate's, so the lockfile
        // is at `workspace/Cargo.lock`; a path that only resembles it is
        // another file.
        let claimed = Claimed::known(vec![
            file("Cargo.lock", FileState::Matches),
            file("workspace/Cargo.lock", FileState::Matches),
            file("workspace/Cargo.lock.bak", FileState::Matches),
            file("workspace/cargo.lock", FileState::Matches),
            file("workspace/sub/Cargo.lock", FileState::Matches),
        ]);

        assert_eq!(
            plan(FilesFlag::DeleteFiles, claimed, "workspace/Cargo.lock"),
            Ok(prove_beside(
                Rule::DeleteAll,
                &[
                    ("Cargo.lock", MATCHES),
                    ("workspace/Cargo.lock.bak", MATCHES),
                    ("workspace/cargo.lock", MATCHES),
                    ("workspace/sub/Cargo.lock", MATCHES),
                ],
                &["workspace/Cargo.lock"]
            ))
        );
    }

    #[test]
    fn keep_files_says_nothing_about_a_claimed_lockfile() {
        // `--keep-files` touches nothing, so a claimed lockfile is no different
        // from any other file under it.
        let claimed = Claimed::known(vec![file("Cargo.lock", FileState::Changed)]);

        assert_eq!(
            plan(FilesFlag::KeepFiles, claimed, LOCKFILE_PATH),
            Ok(Plan::LeaveAll)
        );
    }

    #[test]
    fn a_claimed_lockfile_is_left_in_its_place_in_path_order_and_git_is_not_asked() {
        // The lockfile is settled between the two files by its path, but only
        // the two put to git are asked about.
        let mut asked = Vec::new();
        let outcome = prove_beside(
            Rule::DeleteAll,
            &[("Bravo.yml", MATCHES), ("a.yml", CHANGED)],
            &["Cargo.lock"],
        )
        .settle(|path| {
            asked.push(path.as_str().to_owned());
            Ok::<_, ()>(Fate::Deleted)
        });

        assert_eq!(asked, ["Bravo.yml", "a.yml"]);
        assert_eq!(
            outcome,
            Ok(Settlement::Deleting(vec![
                deleted("Bravo.yml"),
                left("Cargo.lock", LOCKFILE),
                deleted("a.yml"),
            ]))
        );
    }

    #[test]
    fn a_claimed_lockfile_is_no_change_to_the_project_so_bare_deletes_the_rest() {
        // The claimed lockfile differs from the render, as it nearly always
        // does, and the claimed ordinary file matches. Bare run: the lockfile
        // is left, not counted as changed, and the ordinary file is deleted.
        let claimed = Claimed::known(vec![
            file("Cargo.lock", FileState::Changed),
            file("ci.yml", FileState::Matches),
        ]);

        let outcome = plan(FilesFlag::Bare, claimed, LOCKFILE_PATH)
            .expect("the files are known")
            .settle(all_but(&[]));

        assert_eq!(
            outcome,
            Ok(Settlement::Deleting(vec![
                left("Cargo.lock", LOCKFILE),
                deleted("ci.yml"),
            ]))
        );
    }

    #[test]
    fn a_claimed_lockfile_is_left_among_the_files_kept_for_a_change() {
        let claimed = Claimed::known(vec![
            file("Cargo.lock", FileState::Matches),
            file("ci.yml", FileState::Changed),
            file("lint.yml", FileState::Matches),
        ]);

        let outcome = plan(FilesFlag::Bare, claimed, LOCKFILE_PATH)
            .expect("the files are known")
            .settle(all_but(&[]));

        assert_eq!(
            outcome,
            Ok(Settlement::Kept(vec![
                left_remaining("Cargo.lock", LOCKFILE),
                Remaining::Kept(present("ci.yml", CHANGED)),
                Remaining::Kept(present("lint.yml", MATCHES)),
            ]))
        );
    }

    #[test]
    fn a_claimed_lockfile_is_never_among_the_files_to_delete() {
        // Every file git was asked about can be given back, and still the
        // lockfile is not handed to the deleter, under either flag.
        for flag in [FilesFlag::Bare, FilesFlag::DeleteFiles] {
            let claimed = Claimed::known(vec![
                file("Cargo.lock", FileState::Matches),
                file("ci.yml", FileState::Matches),
            ]);

            let outcome = plan(flag, claimed, LOCKFILE_PATH)
                .expect("the files are known")
                .settle(all_but(&[]))
                .expect("git is askable");

            assert_eq!(outcome.to_delete(), [&claim("ci.yml")], "{flag:?}");
        }
    }

    #[test]
    fn a_claimed_lockfile_alone_leaves_nothing_to_delete_and_nothing_kept() {
        let claimed = Claimed::known(vec![file("Cargo.lock", FileState::Changed)]);

        let outcome = plan(FilesFlag::Bare, claimed, LOCKFILE_PATH)
            .expect("the files are known")
            .settle(|_| -> Result<Fate, ()> { panic!("git is asked about no file") })
            .expect("nothing is asked");

        assert_eq!(
            outcome,
            Settlement::Deleting(vec![left("Cargo.lock", LOCKFILE)])
        );
        assert!(outcome.to_delete().is_empty());
    }
}
