//! Writing drifted files back to match their render: every write is staged
//! beside its own target, under an exclusively-created staging file, before
//! any of them lands. Each write is re-verified against what `sync` proved
//! about it ([`reverify`]) at three sites, each catching a different thing:
//!
//! 1. while it is staged, before anything is created for it, so a directory
//!    above the target that became a link never has a staging file written
//!    through it;
//! 2. at the start of the commit, across every write, before the first one
//!    lands, so a change made while later files were being staged still
//!    refuses with nothing written;
//! 3. immediately before each write's own rename or link, which is the
//!    guarantee: the first two only make more of the refusals all or nothing.
//!
//! Staging a write also asks the filesystem one question, right after the
//! staging file exists: whether it takes any index entry git tracks, and
//! hides, under another spelling of the claim (or of a directory above it),
//! for the file just created ([`fold_probe`]). If it does, git would take
//! what `sync` writes for that entry, and the write is refused before a byte
//! is written.
//!
//! A file `sync` replaces is renamed over its target. A file it creates is
//! hard-linked to its target, then the staging name is removed: a link fails
//! with `AlreadyExists` where a rename would silently replace whatever
//! appeared, so a file that shows up after the last look is refused, never
//! overwritten.
//!
//! Every file `sync` creates lands before any file it replaces, each group in
//! path order ([`landing`]). A filesystem that has no hard links refuses the
//! link for every file, so with the links first the first one that fails does
//! so with nothing written, where a rename that had landed before it could
//! not be taken back.
//!
//! `sync` writes every drifted bone's file or none of them until the first
//! write lands. A refusal or failure after that leaves the writes already
//! landed exactly where they are, since they are correct, finished and
//! proven, removes every staging file still outstanding, and says which
//! files were written and which were not. Removal is a step with its own
//! last look: each removal walks the path again and removes only the file
//! or directory `sync` created, and whatever it finds otherwise is left
//! where it is and named with the reason ([`Leftover`]). What no
//! re-verification can close is the instant between each last look and the
//! step it precedes — a create, a rename, a link or a removal — across the
//! whole time from the first staging file to the last removal (the design's
//! Known limits name it).

mod failure;
mod fold_probe;
mod identity;
mod landing;
mod leftover;
mod removal;
mod reverification;
mod staging;
mod staging_claim;
mod verification;

use std::io::Write as _;
use std::path::{Path, PathBuf};

pub(crate) use failure::{CollisionAt, CommitCause, CommitFailure, TargetChange, WriteFailure};
use fold_probe::takes_for_one_name;
use identity::FileIdentity;
use landing::{landing_order, link_failure};
pub(crate) use leftover::{Leftover, LeftoverReason};
use reverification::reverify;
use staging::{StageError, StagedFile, Staging};
use staging_claim::ClaimedNames;
pub(crate) use staging_claim::StagingRelation;
pub(crate) use verification::verify;

use crate::claim::{ClaimPath, Drift, DriftReason};
use crate::survey::Survey;

use super::fold_variant::FoldVariant;
use super::proof::{Evidence, ProvenWrite};

/// One claimed file `sync` is about to write: its target path, the bytes it
/// should hold, and whether writing it creates a missing file or replaces a
/// changed one — which decides both the verb `sync` reports it with
/// (`created`/`updated`) and whether an existing file's own permissions are
/// carried onto the replacement.
#[derive(Debug)]
pub(crate) struct Write {
    pub(crate) path: ClaimPath,
    pub(crate) skeleton: String,
    pub(crate) version: semver::Version,
    pub(crate) rendered: Vec<u8>,
    pub(crate) reason: DriftReason,
}

/// Every row `survey` found drifted, turned into the write `sync` would make
/// for it — nothing here touches disk. A `Drift::Matches` row is never
/// turned into a write at all: rewriting identical bytes would still change
/// a file's own mtime and permissions for no reason.
///
/// # Errors
///
/// [`WriteFailure::StagingClaimed`] when any drifted write's own staging
/// path ([`ClaimPath::staging`]) is, to a filesystem that ignores case or Unicode
/// normalization, equal to, a directory above, or beneath a path some row
/// (drifted or not) claims ([`ClaimedNames`]): staging one bone's file would
/// otherwise put its bytes on another bone's path before anything is
/// committed.
pub(crate) fn plan(survey: &Survey<'_>) -> Result<Vec<Write>, WriteFailure> {
    let writes: Vec<Write> = survey
        .rows
        .iter()
        .filter_map(|row| {
            let Drift::Drifted(reason) = row.drift else {
                return None;
            };
            Some(Write {
                path: row.path.clone(),
                skeleton: row.claimant.skeleton.clone(),
                version: row.claimant.version.clone(),
                rendered: row.rendered.clone(),
                reason,
            })
        })
        .collect();

    // Re-verified here, against the same survey this plan was built from,
    // rather than trusted from the `filter_map` above alone.
    let drifted_rows = survey
        .rows
        .iter()
        .filter(|row| matches!(row.drift, Drift::Drifted(_)))
        .count();
    assert_eq!(
        writes.len(),
        drifted_rows,
        "a sync plan writes exactly the drifted rows, no more and no fewer"
    );

    let claimed_names = ClaimedNames::new(survey.rows.iter().map(|row| &row.path));
    for write in &writes {
        if let Some((claimed, relation)) = claimed_names.taking_the_name_of(&write.path) {
            return Err(WriteFailure::StagingClaimed {
                claim: write.path.clone(),
                staging: write.path.staging().to_string(),
                claimed: claimed.clone(),
                relation,
            });
        }
    }

    Ok(writes)
}

/// One write, already staged beside its own target and ready to be landed.
#[derive(Debug)]
pub(crate) struct PreparedWrite {
    pub(crate) path: ClaimPath,
    pub(crate) skeleton: String,
    pub(crate) version: semver::Version,
    pub(crate) reason: DriftReason,
    rendered: Vec<u8>,
    target: PathBuf,
    staging: PathBuf,
    /// What [`super::proof::prove`] proved about the target, moved here from
    /// the [`ProvenWrite`] so [`reverify`] can ask again right before the
    /// write lands. Whether the write renames over a file or links a new one
    /// is decided by this too: [`Evidence::Held`] renames, [`Evidence::Absent`]
    /// links.
    evidence: Evidence,
    /// The target's own existing permissions, read through
    /// [`std::fs::symlink_metadata`] and carried onto the staging file
    /// through its open handle — `Some` for a `Changed` write, `None` for a
    /// `Missing` one, since there is nothing existing to carry.
    carried_permissions: Option<std::fs::Permissions>,
}

/// Every write [`prepare`] staged, ready to be committed and then verified,
/// together with the ledger that staged them — so a [`commit`](Prepared::commit)
/// failure can roll back exactly what is still outstanding, the same ledger
/// a failed [`prepare`] already rolls back for the writes it never finished
/// staging.
#[derive(Debug)]
pub(crate) struct Prepared {
    root: PathBuf,
    writes: Vec<PreparedWrite>,
    staging: Staging,
}

impl Prepared {
    /// Lands every staged write, every file it creates before any file it
    /// replaces and each group in path order ([`landing_order`]), and becomes
    /// a [`Committed`] — the one thing [`verify`] accepts, so it can never be
    /// called on a run that never finished committing.
    ///
    /// Links land first because a filesystem that refuses `link(2)` refuses
    /// it for every file: the first link fails with nothing written, where a
    /// rename that landed before it could not be taken back.
    ///
    /// Every write is first re-verified against its proof ([`reverify`],
    /// site 2 of the module doc), before any of them lands: a refusal there
    /// writes nothing. Then each write is re-verified again immediately
    /// before it lands (site 3): a file `sync` replaces is renamed over its
    /// target, and a file it creates is hard-linked to its target and the
    /// staging name removed, which fails rather than replace anything that
    /// appeared in the instant since that last look. A file whose staging
    /// name was left in place or could not be removed after its link still
    /// counts as written; the
    /// name is kept in [`Committed::leftovers`], with the reason it is still
    /// there.
    ///
    /// A failure after earlier writes landed names exactly which paths were
    /// written and which were not — the ones written are not rolled back:
    /// they are correct, finished writes, and rolling one back would itself
    /// be an unasked-for write. Every staging file still outstanding — the
    /// failed write's own, and every later write's, since `commit` never even
    /// attempts them — is removed through the same ledger [`prepare`] built,
    /// and whatever was left in place or could not be removed is named in the
    /// returned [`CommitFailure::leftovers`], with the reason.
    ///
    /// # Errors
    ///
    /// [`WriteFailure::Commit`], with a [`CommitCause`] naming what changed
    /// or what the filesystem refused.
    pub(crate) fn commit(self) -> Result<Committed, WriteFailure> {
        let first_change = landing_order(&self.writes).into_iter().find_map(|index| {
            let write = &self.writes[index];
            reverify(&self.root, &write.path, &write.evidence)
                .err()
                .map(|what| (index, what))
        });
        match first_change {
            Some((index, what)) => {
                Err(self.fail(index, &[], CommitCause::Changed(what), Vec::new()))
            }
            None => self.land(),
        }
    }

    /// Lands every write, one at a time in [`landing_order`], re-verifying
    /// each right before it lands. Split from [`Self::commit`] so a test can
    /// drive the last look on its own, past the up-front pass a change made
    /// in the instant between the two would otherwise never reach.
    fn land(mut self) -> Result<Committed, WriteFailure> {
        // Staging names that survived their own link: those writes are
        // finished, and only the name remains.
        let mut lingering = Vec::new();
        let order = landing_order(&self.writes);
        for (position, &index) in order.iter().enumerate() {
            match self.land_one(index) {
                Ok(None) => {}
                Ok(Some(leftover)) => lingering.push(leftover),
                Err(cause) => {
                    return Err(self.fail(index, &order[..position], cause, lingering));
                }
            }
        }
        self.staging.keep();
        lingering.sort_by(|first, second| first.path.cmp(&second.path));
        Ok(Committed {
            writes: self.writes,
            leftovers: lingering,
        })
    }

    /// Re-verifies write `index` and lands it. Returns the staging name that
    /// was left in place or could not be removed after a link, if that
    /// happened.
    fn land_one(&mut self, index: usize) -> Result<Option<Leftover>, CommitCause> {
        let write = &self.writes[index];
        reverify(&self.root, &write.path, &write.evidence).map_err(CommitCause::Changed)?;
        match write.evidence {
            Evidence::Held(_) => {
                std::fs::rename(&write.staging, &write.target).map_err(|error| {
                    CommitCause::Replace {
                        detail: error.to_string(),
                    }
                })?;
                self.staging.handed_over(&write.path.staging());
                Ok(None)
            }
            Evidence::Absent => {
                link_into_place(&write.staging, &write.target)?;
                Ok(self.staging.linked(&write.path.staging()))
            }
        }
    }

    /// Turns a failure at write `failed` into the [`WriteFailure`] `sync`
    /// reports, once every staging file still outstanding is removed. Both
    /// `failed` and every entry of `landed` are positions in `self.writes`:
    /// `landed` is the writes that had landed, in the order they did, empty
    /// for a refusal before the first one. `lingering` is every staging name
    /// that survived its own link earlier in this run.
    ///
    /// The paths are reported in path order whatever order they landed in, so
    /// the message does not depend on the order links and renames were made.
    fn fail(
        self,
        failed: usize,
        landed: &[usize],
        cause: CommitCause,
        lingering: Vec<Leftover>,
    ) -> WriteFailure {
        let total = self.writes.len();
        assert!(
            failed < total,
            "the failed write {failed} is one of the {total} writes"
        );
        assert!(
            landed.iter().all(|&index| index < total),
            "the writes that landed are among the {total} writes: {landed:?}"
        );
        assert!(
            landed.iter().all(|&index| index != failed),
            "the failed write {failed} never landed: {landed:?}"
        );
        let failed_path = self.writes[failed].path.clone();
        let mut already_written: Vec<ClaimPath> = landed
            .iter()
            .map(|&index| self.writes[index].path.clone())
            .collect();
        already_written.sort();
        let not_yet_written: Vec<ClaimPath> = self
            .writes
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != failed && !landed.contains(index))
            .map(|(_, written)| written.path.clone())
            .collect();
        assert_eq!(
            already_written.len() + not_yet_written.len() + 1,
            total,
            "every write is landed, failed or not yet written, exactly once"
        );
        let mut leftovers = self.staging.roll_back();
        leftovers.extend(lingering);
        leftovers.sort_by(|first, second| first.path.cmp(&second.path));
        WriteFailure::Commit(Box::new(CommitFailure {
            failed_path,
            already_written,
            not_yet_written,
            total,
            cause,
            leftovers,
        }))
    }
}

/// Gives `target` the staged file's own contents by hard-linking `staging`
/// to it. `link` fails with `AlreadyExists` on an existing file, a dangling
/// link and, on a filesystem that folds case, a case variant, and replaces
/// none of them, so a file that appeared since the last look is refused
/// rather than overwritten. Any other failure of the link is one cause carrying
/// the operating system's error, which [`link_failure`] says.
fn link_into_place(staging: &Path, target: &Path) -> Result<(), CommitCause> {
    std::fs::hard_link(staging, target).map_err(|error| link_failure(&error))
}

/// Every write `sync` has actually landed — the one state [`verify`]
/// accepts, so it can never be asked to confirm a write that was only ever
/// staged.
#[derive(Debug)]
pub(crate) struct Committed {
    writes: Vec<PreparedWrite>,
    leftovers: Vec<Leftover>,
}

impl Committed {
    /// Every write this run committed, in the order [`prepare`] staged them
    /// (path order, since [`plan`] built them from the survey's own sorted
    /// rows) — what `sync` reads to report `created`/`updated` lines, and
    /// what [`verify`] reads back.
    pub(crate) fn writes(&self) -> &[PreparedWrite] {
        &self.writes
    }

    /// The staging files that survived their own link, sorted by path, each
    /// with the reason it is still there: every write is finished, but each
    /// of these names is a second link to a file that is now the wearer's,
    /// and an untracked file that would stop the next `sync`, so `sync`
    /// fails naming them after reporting the writes.
    pub(crate) fn leftovers(&self) -> &[Leftover] {
        &self.leftovers
    }
}

/// Stages every one of `proven` beside its own target under `root`, through
/// one [`Staging`] ledger shared across every one of them: creates whatever
/// parent directories are missing, an exclusive staging file next to the
/// target, carries an existing file's own permissions onto it for a write
/// whose [`Evidence`] is [`Evidence::Held`], writes the render and
/// `sync_all`s it. Nothing under `root` is landed yet.
///
/// Every write handed to this function already carries
/// [`super::proof::prove`]'s own positive proof (rule (b)), and the proof
/// is kept with the write. `stage_one` re-verifies against it before it
/// creates anything for that write ([`reverify`], site 1 of the module doc),
/// which catches what happened between `prove` looking at this path and this
/// call: every later path's proof runs in that time, and a repository's
/// content filter runs with it. What it cannot catch is anything after it:
/// the writes staged later in this loop, and the commit itself, are covered
/// by [`Prepared::commit`]'s own two re-verifications, and the last instant
/// by nothing (`prove`'s own check being the first).
///
/// On any failure, every directory and staging file this ledger recorded is
/// removed before the error is returned, and whatever was left in place or
/// could not be removed is named in the failure's own `leftovers`.
pub(crate) fn prepare(root: &Path, proven: Vec<ProvenWrite>) -> Result<Prepared, WriteFailure> {
    let mut staging = Staging::new(root);
    let mut prepared = Vec::with_capacity(proven.len());

    for proven_write in proven {
        let (write, evidence, fold_variants) = proven_write.into_parts();
        if let Err(failure) = stage_one(
            root,
            write,
            evidence,
            &fold_variants,
            &mut staging,
            &mut prepared,
        ) {
            let leftovers = staging.roll_back();
            return Err(finish_stage_failure(failure, leftovers));
        }
    }

    Ok(Prepared {
        root: root.to_path_buf(),
        writes: prepared,
        staging,
    })
}

/// The cause `stage_one` found, without `leftovers` — [`prepare`] fills that
/// in once it has actually rolled the ledger back, since only it knows what
/// survived.
enum StageOneFailure {
    Collision {
        claim: ClaimPath,
        shown: String,
        what: CollisionAt,
    },
    TargetChanged {
        claim: ClaimPath,
        what: TargetChange,
    },
    /// The filesystem takes an index entry git tracks, and hides, for the
    /// staging file just created: git would take what `sync` writes for that
    /// entry.
    FoldsOntoTracked {
        claim: ClaimPath,
        variant: FoldVariant,
    },
    /// The operating system refused permission to create something in
    /// `directory`, `None` for the workspace root.
    NotWritable {
        path: ClaimPath,
        directory: Option<ClaimPath>,
        detail: String,
    },
    Io {
        path: ClaimPath,
        detail: String,
    },
}

/// Turns a [`StageOneFailure`] into the [`WriteFailure`] `sync` reports,
/// once `leftovers` — everything the ledger's own rollback left in place or
/// could not remove — is known.
fn finish_stage_failure(failure: StageOneFailure, leftovers: Vec<Leftover>) -> WriteFailure {
    match failure {
        StageOneFailure::Collision { claim, shown, what } => WriteFailure::Collision {
            claim,
            shown,
            what,
            leftovers,
        },
        StageOneFailure::TargetChanged { claim, what } => WriteFailure::TargetChanged {
            claim,
            what,
            leftovers,
        },
        StageOneFailure::FoldsOntoTracked { claim, variant } => WriteFailure::FoldsOntoTracked {
            claim,
            variant,
            leftovers,
        },
        StageOneFailure::NotWritable {
            path,
            directory,
            detail,
        } => WriteFailure::DirectoryNotWritable {
            path,
            directory,
            detail,
            leftovers,
        },
        StageOneFailure::Io { path, detail } => WriteFailure::Prepare {
            path,
            detail,
            leftovers,
        },
    }
}

/// Turns a [`StageError`] the ledger reported for `claim` into the matching
/// [`StageOneFailure`].
fn to_stage_one_failure(
    error: StageError,
    claim: &ClaimPath,
    what: CollisionAt,
) -> StageOneFailure {
    match error {
        StageError::Collision { shown } => StageOneFailure::Collision {
            claim: claim.clone(),
            shown,
            what,
        },
        StageError::NotWritable { directory, detail } => StageOneFailure::NotWritable {
            path: claim.clone(),
            directory,
            detail,
        },
        StageError::Io { detail } => StageOneFailure::Io {
            path: claim.clone(),
            detail,
        },
    }
}

/// Stages one write, in the order that keeps a refused write from touching
/// anything it should not: a re-verification of the target and every
/// directory above it against the evidence [`proof::prove`] found for it
/// (before anything at all is created for this write), missing ancestor
/// directories, the target's carried permissions read (for a write whose
/// evidence is [`Evidence::Held`]), the exclusive staging file, the
/// filesystem's answer about every index entry git hides under another
/// spelling of the claim ([`refuse_folded_entries`]), those permissions set
/// on the staging file, then its bytes.
///
/// [`proof::prove`]: super::proof::prove
fn stage_one(
    root: &Path,
    write: Write,
    evidence: Evidence,
    fold_variants: &[FoldVariant],
    staging: &mut Staging,
    prepared: &mut Vec<PreparedWrite>,
) -> Result<(), StageOneFailure> {
    let target = write.path.to_path(root);
    assert_confined_to_root(root, &write.path, &target);

    reverify(root, &write.path, &evidence).map_err(|what| StageOneFailure::TargetChanged {
        claim: write.path.clone(),
        what,
    })?;

    for directory in missing_ancestors(&write.path, root) {
        staging
            .create_directory(&directory)
            .map_err(|error| to_stage_one_failure(error, &write.path, CollisionAt::Directory))?;
    }

    let carried_permissions = carried_permissions(&target, &evidence, &write.path)?;

    let staging_claim = write.path.staging();
    let StagedFile { mut file, identity } = staging
        .create_staging_file(&staging_claim)
        .map_err(|error| to_stage_one_failure(error, &write.path, CollisionAt::StagingFile))?;
    refuse_folded_entries(root, &write.path, fold_variants, identity)?;

    if let Some(permissions) = carried_permissions.clone() {
        file.set_permissions(permissions)
            .map_err(|error| io_failure(&write.path, &error))?;
    }
    file.write_all(&write.rendered)
        .map_err(|error| io_failure(&write.path, &error))?;
    file.sync_all()
        .map_err(|error| io_failure(&write.path, &error))?;
    drop(file);

    prepared.push(PreparedWrite {
        path: write.path,
        skeleton: write.skeleton,
        version: write.version,
        reason: write.reason,
        rendered: write.rendered,
        target,
        staging: staging_claim.to_path(root),
        evidence,
        carried_permissions,
    });
    Ok(())
}

/// Refuses the write when the filesystem takes any of `fold_variants`, index
/// entries git tracks under another spelling, for the staging file whose
/// identity is `staging_identity`, just created. It runs right after the
/// staging file exists, when every directory the write needs exists too, and
/// before a byte is written, so a refusal writes nothing; the ledger removes
/// the empty staging file.
///
/// A lookup that fails for any reason but there being nothing there refuses
/// too, naming both paths: it cannot say the two are different names.
fn refuse_folded_entries(
    root: &Path,
    claim: &ClaimPath,
    fold_variants: &[FoldVariant],
    staging_identity: FileIdentity,
) -> Result<(), StageOneFailure> {
    for variant in fold_variants {
        match takes_for_one_name(root, claim, variant, staging_identity) {
            Ok(false) => {}
            Ok(true) => {
                return Err(StageOneFailure::FoldsOntoTracked {
                    claim: claim.clone(),
                    variant: variant.clone(),
                });
            }
            Err(error) => {
                return Err(StageOneFailure::Io {
                    path: claim.clone(),
                    detail: format!(
                        "could not tell whether {claim} and {}, which git's index tracks, are \
                         one name on this filesystem: {error}",
                        variant.git_path()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn io_failure(path: &ClaimPath, error: &std::io::Error) -> StageOneFailure {
    StageOneFailure::Io {
        path: path.clone(),
        detail: error.to_string(),
    }
}

/// The target's own existing permissions, to carry onto its replacement: read
/// without ever following a link, for [`Evidence::Held`]; `None` for
/// [`Evidence::Absent`], since there is nothing existing to carry. Runs right
/// after [`reverify`] found the target a regular file, so a target that is
/// not one now is a change made in between, reported as such.
fn carried_permissions(
    target: &Path,
    evidence: &Evidence,
    claim: &ClaimPath,
) -> Result<Option<std::fs::Permissions>, StageOneFailure> {
    let changed = |what| StageOneFailure::TargetChanged {
        claim: claim.clone(),
        what,
    };
    match evidence {
        Evidence::Absent => Ok(None),
        Evidence::Held(_) => match std::fs::symlink_metadata(target) {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.permissions())),
            Ok(_) => Err(changed(TargetChange::NoLongerAFile)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(changed(TargetChange::Disappeared))
            }
            Err(error) => Err(io_failure(claim, &error)),
        },
    }
}

/// Re-asserts, right before staging, the root-containment property
/// [`crate::claim::resolve`] already enforced while walking `path` from the
/// workspace root, and [`ClaimPath::from_rendering_path`] already enforced
/// while building it in the first place (a second check; [`verify`] likewise
/// re-checks the spelling rule after the write): `target` sits lexically under
/// `root`, and `path` holds no `..`, no empty, and no absolute component —
/// never trusting that the path handed to `sync::write` was actually the one
/// those two callers vetted.
///
/// # Panics
///
/// If any of those properties do not hold. Nothing but a defect in this
/// crate's own claim-building or path-joining code could ever make one
/// false here, so it is a panic and not an error.
fn assert_confined_to_root(root: &Path, path: &ClaimPath, target: &Path) {
    assert!(
        target.starts_with(root),
        "{} must sit under the workspace root {}",
        target.display(),
        root.display()
    );
    for component in path.as_str().split('/') {
        assert!(!component.is_empty(), "{path} must hold no empty component");
        assert_ne!(component, "..", "{path} must hold no `..` component");
        assert!(
            !Path::new(component).is_absolute(),
            "{path} must hold no absolute component"
        );
    }
}

/// Every directory above `path` that does not exist yet under `root`,
/// root-to-leaf — the order a caller must create them in, so a leaf is never
/// attempted before the parent it needs.
///
/// "Missing" is decided by [`std::fs::symlink_metadata`] returning
/// `NotFound`, never [`Path::exists`] (which follows a link): a dangling
/// symbolic link counts as present, not missing, so this never tries to
/// `create_dir` a path a link already occupies.
fn missing_ancestors(path: &ClaimPath, root: &Path) -> Vec<ClaimPath> {
    path.ancestors()
        .into_iter()
        .filter(|ancestor| {
            matches!(
                std::fs::symlink_metadata(ancestor.to_path(root)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        CollisionAt, CommitCause, DriftReason, Leftover, LeftoverReason, StageError,
        StagingRelation, TargetChange, Write, WriteFailure, link_into_place, missing_ancestors,
        plan, prepare, staging,
    };
    use crate::claim::{ClaimPath, UnsafePathCause};
    use crate::git::ObjectId;
    use crate::sync::fold_variant::fold_variants;
    use crate::sync::proof::{Evidence, HeldFile, IndexMode, ProvenWrite};

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    fn write(path: &str, rendered: &[u8], reason: DriftReason) -> Write {
        Write {
            path: claim_path(path),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
            rendered: rendered.to_vec(),
            reason,
        }
    }

    /// A test-only oid: 40 zeros, which [`ObjectId::parse`] accepts (every
    /// character is valid lowercase hex) without needing a real git object
    /// behind it — `write`'s own tests exercise staging, committing and
    /// verifying, never the proof itself, so the oid's own value is never
    /// read for anything but being a well-formed [`ObjectId`].
    const TEST_OBJECT_ID: &str = "0000000000000000000000000000000000000000";

    /// The same as [`write`], wrapped in the [`Evidence`] `prove` would have
    /// found for it: [`Evidence::Absent`] for a `Missing` write,
    /// [`Evidence::Held`] (as a regular file) for a `Changed` one — exactly the
    /// pairing `proof::prove` always produces. A `Changed` write built here
    /// holds bytes no file on disk has, so it suits a test that never commits
    /// it; use [`proven_holding`] to commit.
    fn proven(path: &str, rendered: &[u8], reason: DriftReason) -> ProvenWrite {
        let evidence = match reason {
            DriftReason::Missing => Evidence::Absent,
            DriftReason::Changed => Evidence::Held(HeldFile::for_test(
                ObjectId::parse(TEST_OBJECT_ID).expect("well-formed test object id"),
                IndexMode::Regular,
                b"bytes no file on disk holds; a test that commits uses `proven_holding`".to_vec(),
            )),
        };
        ProvenWrite::for_test(write(path, rendered, reason), evidence)
    }

    /// A `Changed` write whose proof holds exactly `checkout`: what
    /// `commit`'s re-verification compares the file on disk against, so a
    /// test that commits must make it what it wrote to disk.
    fn proven_holding(path: &str, rendered: &[u8], checkout: &[u8]) -> ProvenWrite {
        let evidence = Evidence::Held(HeldFile::for_test(
            ObjectId::parse(TEST_OBJECT_ID).expect("well-formed test object id"),
            IndexMode::Regular,
            checkout.to_vec(),
        ));
        ProvenWrite::for_test(write(path, rendered, DriftReason::Changed), evidence)
    }

    #[test]
    fn a_missing_file_is_staged_and_committed_creating_missing_directories() {
        let root = TempDir::new().expect("scratch directory");
        let writes = vec![proven("a/b/thing.txt", b"hello", DriftReason::Missing)];

        let prepared = prepare(root.path(), writes).expect("prepare must succeed");
        assert!(
            !root.path().join("a/b/thing.txt").exists(),
            "prepare must not create the target itself yet"
        );

        let committed = prepared.commit().expect("commit must succeed");
        assert_eq!(committed.writes().len(), 1);
        assert_eq!(
            std::fs::read(root.path().join("a/b/thing.txt")).expect("file must exist"),
            b"hello"
        );
    }

    #[test]
    fn permissions_are_carried_through_the_handle_from_the_files_own_lstat() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::write(&target, b"old").expect("write existing file");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .expect("set executable permissions");

        let writes = vec![proven_holding("thing.txt", b"new", b"old")];
        let prepared = prepare(root.path(), writes).expect("prepare must succeed");
        assert_eq!(
            prepared.writes[0]
                .carried_permissions
                .as_ref()
                .expect("a Changed write must carry permissions")
                .mode()
                & 0o777,
            0o755,
        );
        let committed = prepared.commit().expect("commit must succeed");
        super::verify(&committed).expect("nothing changed since commit");

        let mode = std::fs::metadata(&target)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "the replacement must keep the executable bit");
    }

    #[test]
    fn a_prepare_failure_removes_every_directory_it_created() {
        // The second write's own target sits under `a/blocked`, a plain
        // file rather than a directory. Staging re-verifies the path before
        // it creates anything for that write: the walk finds a file where a
        // directory belongs and refuses, `TargetChanged` with
        // `PathUnsafe(NotADirectoryAbove)`. Rollback must still remove the
        // directories the first write created and leave the one it did not
        // itself create alone.
        let root = TempDir::new().expect("scratch directory");
        let writes = vec![
            proven("a/b/thing.txt", b"one", DriftReason::Missing),
            proven("a/blocked/thing.txt", b"two", DriftReason::Missing),
        ];
        std::fs::create_dir_all(root.path().join("a")).expect("pre-existing directory");
        std::fs::write(root.path().join("a/blocked"), b"not a directory")
            .expect("a plain file where a directory is needed");

        let error = prepare(root.path(), writes).expect_err("prepare must fail");
        let WriteFailure::TargetChanged { claim, what, .. } = error else {
            panic!("expected TargetChanged, got {error:?}")
        };
        assert_eq!(claim, claim_path("a/blocked/thing.txt"));
        assert_eq!(
            what,
            TargetChange::PathUnsafe(UnsafePathCause::NotADirectoryAbove {
                at: "a/blocked".to_owned()
            })
        );
        assert!(
            !root.path().join("a/b").exists(),
            "the directory the first write created must be rolled back"
        );
        assert!(
            root.path().join("a").exists(),
            "a directory prepare did not itself create must survive rollback"
        );
    }

    /// A `Missing` write of `path` carrying the one fold variant `entry` is
    /// of it, as `prove` would attach it.
    fn proven_with_variant(path: &str, entry: &str) -> ProvenWrite {
        let claimed = claim_path(path);
        let listing = format!("{entry}\0");
        let variants = fold_variants(listing.as_bytes(), &[&claimed]).remove(0);
        assert_eq!(variants.len(), 1, "{entry} must be a variant of {path}");
        ProvenWrite::for_test_with_variants(
            write(path, b"created", DriftReason::Missing),
            Evidence::Absent,
            variants,
        )
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_fold_variant_the_filesystem_takes_for_the_claim_is_refused_with_nothing_written() {
        // Git holds `a/b` hidden, and the claim is `A/b/x.yml`: where the
        // filesystem takes `a` and `A` for one directory, creating the claim
        // would put a directory over the tracked file. The refusal is made
        // after the staging file exists and before a byte is written, and the
        // ledger removes what was created. Only a filesystem that folds case
        // (APFS) can be made to fold here: a stand-in link `a` beside `A`
        // would itself be refused by the claim walk as a spelling of `A`. So
        // the premise is asked of the filesystem, and the test says
        // `skipped:` where it does not hold; the control below covers the
        // other kind.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("Probe"), b"").expect("the probe");
        let folds_case = root
            .path()
            .join("probe")
            .try_exists()
            .expect("look up the probe under its other spelling");
        std::fs::remove_file(root.path().join("Probe")).expect("remove the probe");
        if !folds_case {
            eprintln!("skipped: this filesystem keeps `a` and `A` apart");
            return;
        }

        let error = prepare(root.path(), vec![proven_with_variant("A/b/x.yml", "a/b")])
            .expect_err("the variant is one name with the claim's directory");

        let WriteFailure::FoldsOntoTracked {
            claim,
            variant,
            leftovers,
        } = error
        else {
            panic!("expected FoldsOntoTracked, got {error:?}")
        };
        assert_eq!(claim, claim_path("A/b/x.yml"));
        assert_eq!(variant.git_path(), "a/b");
        assert!(leftovers.is_empty(), "everything is removed: {leftovers:?}");
        assert!(
            std::fs::symlink_metadata(root.path().join("A")).is_err(),
            "the directories created for the write are removed"
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_fold_variant_the_filesystem_keeps_apart_is_staged_and_committed() {
        // The control for the test above, on a filesystem that keeps `a` and
        // `A` apart (ext4): the same write, the same variant and no link, so
        // the lookup finds nothing and the write goes through. Where the
        // filesystem folds case there is no such variant to test with, and
        // the test says so.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("Probe"), b"").expect("the probe");
        let folds_case = root
            .path()
            .join("probe")
            .try_exists()
            .expect("look up the probe under its other spelling");
        std::fs::remove_file(root.path().join("Probe")).expect("remove the probe");
        if folds_case {
            eprintln!("skipped: this filesystem folds case, so no variant is kept apart");
            return;
        }

        let prepared = prepare(root.path(), vec![proven_with_variant("A/b/x.yml", "a/b")])
            .expect("nothing takes `a/b` for the claim here");
        prepared.commit().expect("commit must succeed");

        assert_eq!(
            std::fs::read(root.path().join("A/b/x.yml")).expect("the file exists"),
            b"created"
        );
    }

    #[test]
    fn a_directory_collision_is_refused_and_named() {
        // Drives `Staging` directly, since a real `prepare()` call can only
        // ever attempt `create_directory` once `missing_ancestors` has
        // already decided a path is absent — this test's own second call is
        // the "something else created it in between" case, which no test
        // can time against a real `prepare()`, exercised here as a plain
        // repeat rather than a race.
        let root = TempDir::new().expect("scratch directory");
        let mut staging = staging::Staging::new(root.path());
        let directory = claim_path("a");

        staging
            .create_directory(&directory)
            .expect("the first create_directory call must succeed");
        let error = staging
            .create_directory(&directory)
            .expect_err("a second create_directory call at the same path must collide");
        assert!(matches!(error, StageError::Collision { .. }));
    }

    #[test]
    fn a_failed_commit_removes_every_pending_staging_file_and_now_empty_directory() {
        // Index 1 of 3 (`c/two.txt`) fails to link because its staging
        // file has been removed after staging, so `hard_link` fails for a
        // reason other than the path being taken: a `Create` failure at the
        // filesystem, which no re-verification could see coming.
        // `commit`'s own cleanup must then remove: the third write's
        // staging file, since it was queued after the failure and never
        // attempted; and `c` and `d`, the directories `prepare` created
        // solely for those two writes, since nothing is left in them. `a`
        // (holding the first write's own successfully landed file) must
        // survive: it is not empty.
        let root = TempDir::new().expect("scratch directory");
        let writes = vec![
            proven("a/one.txt", b"one", DriftReason::Missing),
            proven("c/two.txt", b"two", DriftReason::Missing),
            proven("d/three.txt", b"three", DriftReason::Missing),
        ];
        let prepared = prepare(root.path(), writes).expect("prepare must succeed");

        std::fs::remove_file(root.path().join("c/.two.txt.skeletons-sync"))
            .expect("the staging file goes missing after prepare");

        let error = prepared.commit().expect_err("commit must fail");
        let WriteFailure::Commit(commit) = error else {
            panic!("a link failure must be reported as WriteFailure::Commit")
        };
        assert_eq!(commit.failed_path, claim_path("c/two.txt"));
        assert_eq!(commit.already_written, vec![claim_path("a/one.txt")]);
        assert_eq!(commit.not_yet_written, vec![claim_path("d/three.txt")]);
        assert_eq!(commit.total, 3);
        assert!(
            matches!(commit.cause, CommitCause::Create { .. }),
            "expected a Create cause, got {:?}",
            commit.cause
        );
        // The test itself removed `c/.two.txt.skeletons-sync`, so the ledger finds
        // nothing where it staged that file, and says so rather than claim it
        // removed it: it cannot tell a file removed from a file moved away.
        assert_eq!(
            commit.leftovers,
            vec![Leftover {
                path: "c/.two.txt.skeletons-sync".to_owned(),
                reason: LeftoverReason::Gone,
            }],
            "only the staging file the test removed itself is unaccounted for"
        );

        assert!(
            no_skeletons_sync_staging_files_remain(root.path()),
            "every pending staging file must be removed after a failed commit"
        );
        assert!(
            !root.path().join("d").exists(),
            "a created directory left empty by cleanup must be removed"
        );
        assert!(
            !root.path().join("c").exists(),
            "a created directory left empty by cleanup must be removed"
        );
        assert_eq!(
            std::fs::read(root.path().join("a/one.txt")).expect("the first write must have landed"),
            b"one"
        );
    }

    #[test]
    fn a_change_found_after_the_first_write_landed_leaves_it_and_names_both_lists() {
        // Drives `land` directly, past the up-front pass of `commit`: the
        // instant between that pass and a write's own last look is the one
        // a test cannot reach from outside. The second write's target
        // becomes a directory after the first write has landed, so the
        // last look before its link refuses it. The first write must stay
        // exactly where it landed, the third must never be attempted, and
        // every staging file still outstanding must be removed.
        let root = TempDir::new().expect("scratch directory");
        let writes = vec![
            proven("a/one.txt", b"one", DriftReason::Missing),
            proven("c/two.txt", b"two", DriftReason::Missing),
            proven("d/three.txt", b"three", DriftReason::Missing),
        ];
        let prepared = prepare(root.path(), writes).expect("prepare must succeed");
        std::fs::create_dir(root.path().join("c/two.txt")).expect("blocking directory");
        std::fs::write(root.path().join("c/two.txt/inner"), b"occupied")
            .expect("make the blocking directory non-empty");

        let error = prepared.land().expect_err("the last look must refuse");

        let WriteFailure::Commit(commit) = error else {
            panic!("expected WriteFailure::Commit, got {error:?}")
        };
        assert_eq!(commit.failed_path, claim_path("c/two.txt"));
        assert_eq!(commit.already_written, vec![claim_path("a/one.txt")]);
        assert_eq!(commit.not_yet_written, vec![claim_path("d/three.txt")]);
        assert_eq!(commit.cause, CommitCause::Changed(TargetChange::Appeared));
        assert!(commit.leftovers.is_empty(), "{:?}", commit.leftovers);
        assert!(no_skeletons_sync_staging_files_remain(root.path()));
        assert!(!root.path().join("d").exists());
        assert_eq!(
            std::fs::read(root.path().join("a/one.txt")).expect("the first write must stay"),
            b"one"
        );
        assert!(
            root.path().join("c/two.txt/inner").exists(),
            "what stands where the second write was to land is left untouched"
        );
    }

    #[test]
    fn a_change_found_before_any_write_lands_writes_nothing() {
        // The up-front pass: every write is re-verified before the first
        // lands, so the first write's own target is untouched even though
        // it was the *second* write's target that changed.
        let root = TempDir::new().expect("scratch directory");
        let writes = vec![
            proven("a.txt", b"one", DriftReason::Missing),
            proven("b.txt", b"two", DriftReason::Missing),
        ];
        let prepared = prepare(root.path(), writes).expect("prepare must succeed");
        std::fs::write(root.path().join("b.txt"), b"appeared").expect("appeared after prepare");

        let error = prepared.commit().expect_err("commit must refuse");

        let WriteFailure::Commit(commit) = error else {
            panic!("expected WriteFailure::Commit, got {error:?}")
        };
        assert_eq!(commit.failed_path, claim_path("b.txt"));
        assert!(commit.already_written.is_empty());
        assert_eq!(commit.cause, CommitCause::Changed(TargetChange::Appeared));
        assert!(
            !root.path().join("a.txt").exists(),
            "nothing may land when any write was refused before the first"
        );
        assert!(no_skeletons_sync_staging_files_remain(root.path()));
    }

    #[test]
    fn a_held_file_edited_after_prepare_is_not_replaced_by_commit() {
        // The replacing counterpart of the appeared-file test: a file
        // `sync` proved it held is edited between staging and the commit,
        // and replacing it would destroy what was written.
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::write(&target, b"proven").expect("the proven file");
        let prepared = prepare(
            root.path(),
            vec![proven_holding("thing.txt", b"new", b"proven")],
        )
        .expect("prepare must succeed");
        std::fs::write(&target, b"precious").expect("an edit after prepare");

        let error = prepared.commit().expect_err("commit must refuse");

        let WriteFailure::Commit(commit) = error else {
            panic!("expected WriteFailure::Commit, got {error:?}")
        };
        assert_eq!(
            commit.cause,
            CommitCause::Changed(TargetChange::ContentChanged)
        );
        assert_eq!(
            std::fs::read(&target).expect("the edited file"),
            b"precious"
        );
        assert!(no_skeletons_sync_staging_files_remain(root.path()));
    }

    #[test]
    fn a_link_into_a_path_that_is_taken_is_refused_and_replaces_nothing() {
        // The link itself, past every re-verification: whatever occupies
        // the target when `link` runs is refused by the filesystem, the
        // regular file and the dangling link alike.
        let root = TempDir::new().expect("scratch directory");
        let staged = root.path().join(".staged");
        std::fs::write(&staged, b"new").expect("staging file");
        let taken = root.path().join("taken");
        std::fs::write(&taken, b"someone else's work").expect("taken");
        let dangling = root.path().join("dangling");
        std::os::unix::fs::symlink(root.path().join("nowhere"), &dangling).expect("dangling");

        for target in [&taken, &dangling] {
            assert_eq!(
                link_into_place(&staged, target),
                Err(CommitCause::Changed(TargetChange::Appeared)),
                "{}",
                target.display()
            );
        }
        assert_eq!(
            std::fs::read(&taken).expect("taken"),
            b"someone else's work"
        );
        assert!(
            std::fs::symlink_metadata(&dangling)
                .expect("dangling")
                .is_symlink()
        );
    }

    #[test]
    fn a_created_file_is_linked_and_its_staging_name_removed() {
        let root = TempDir::new().expect("scratch directory");
        let prepared = prepare(
            root.path(),
            vec![proven("thing.txt", b"hello", DriftReason::Missing)],
        )
        .expect("prepare must succeed");

        let committed = prepared.commit().expect("commit must succeed");

        assert!(committed.leftovers().is_empty());
        assert_eq!(
            std::fs::read(root.path().join("thing.txt")).expect("the created file"),
            b"hello"
        );
        assert!(no_skeletons_sync_staging_files_remain(root.path()));
    }

    /// Walks `root` looking for any entry ending in `.skeletons-sync` — the
    /// suffix every staging file this module stages carries — iteratively,
    /// with an explicit stack rather than recursion.
    fn no_skeletons_sync_staging_files_remain(root: &Path) -> bool {
        let mut directories = vec![root.to_path_buf()];
        while let Some(directory) = directories.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".skeletons-sync")
                {
                    return false;
                }
                let path = entry.path();
                if path.is_dir() {
                    directories.push(path);
                }
            }
        }
        true
    }

    #[test]
    fn missing_ancestors_lists_every_missing_directory_root_to_leaf() {
        let root = TempDir::new().expect("scratch directory");
        let missing = missing_ancestors(&claim_path("a/b/thing.yml"), root.path());
        assert_eq!(missing, vec![claim_path("a"), claim_path("a/b")]);
    }

    #[test]
    fn missing_ancestors_counts_a_dangling_symlink_as_present() {
        let root = TempDir::new().expect("scratch directory");
        let dangling = root.path().join("dangling");
        std::os::unix::fs::symlink(root.path().join("does-not-exist"), &dangling)
            .expect("create dangling symlink");

        let missing = missing_ancestors(&claim_path("dangling/thing.yml"), root.path());
        assert!(
            missing.is_empty(),
            "a dangling symlink must count as present, not missing: {missing:?}"
        );
    }

    #[test]
    fn a_symlink_at_the_staging_path_is_refused_and_nothing_is_written_through_it() {
        const SENTINEL: &[u8] = b"precious content that exists nowhere else\n";

        let root = TempDir::new().expect("scratch directory");
        let outside = TempDir::new().expect("outside directory");
        let outside_file = outside.path().join("precious.txt");
        std::fs::write(&outside_file, SENTINEL).expect("write outside sentinel file");

        let staging_target = root.path().join(".deny.toml.skeletons-sync");
        std::os::unix::fs::symlink(&outside_file, &staging_target).expect("plant the symlink");

        let writes = vec![proven("deny.toml", b"new", DriftReason::Missing)];
        let error = prepare(root.path(), writes)
            .expect_err("prepare must refuse a symlink at the staging path");
        let WriteFailure::Collision { claim, what, .. } = error else {
            panic!("expected Collision, got {error:?}")
        };
        assert_eq!(claim, claim_path("deny.toml"));
        assert_eq!(what, CollisionAt::StagingFile);

        assert_eq!(
            std::fs::read(&outside_file).expect("the outside file must still be readable"),
            SENTINEL,
            "sync must never write through a symlink at its staging path to a file outside the \
             workspace"
        );
        assert!(
            std::fs::symlink_metadata(&staging_target)
                .expect("the planted symlink must still be there")
                .is_symlink(),
            "the ledger never recorded the symlink, so it must be left exactly as planted"
        );
        assert!(
            std::fs::symlink_metadata(root.path().join("deny.toml")).is_err(),
            "the claimed path must never come into existence at all from a refused sync"
        );
    }

    #[test]
    fn a_dangling_symlink_at_the_staging_path_is_refused() {
        let root = TempDir::new().expect("scratch directory");
        let staging_target = root.path().join(".plain.yml.skeletons-sync");
        std::os::unix::fs::symlink(root.path().join("does-not-exist"), &staging_target)
            .expect("plant a dangling symlink");

        let writes = vec![proven("plain.yml", b"new", DriftReason::Missing)];
        let error = prepare(root.path(), writes)
            .expect_err("prepare must refuse a dangling symlink at the staging path");
        assert!(matches!(error, WriteFailure::Collision { .. }));
        assert!(
            std::fs::symlink_metadata(&staging_target)
                .expect("the planted symlink must still be there")
                .is_symlink()
        );
    }

    #[test]
    fn a_regular_file_at_the_staging_path_is_refused_and_not_truncated() {
        const LEFTOVER: &[u8] = b"content that must never be truncated\n";

        let root = TempDir::new().expect("scratch directory");
        let staging_target = root.path().join(".plain.yml.skeletons-sync");
        std::fs::write(&staging_target, LEFTOVER).expect("plant a regular file");

        let writes = vec![proven("plain.yml", b"new", DriftReason::Missing)];
        let error = prepare(root.path(), writes)
            .expect_err("prepare must refuse a regular file at the staging path");
        assert!(matches!(error, WriteFailure::Collision { .. }));
        assert_eq!(
            std::fs::read(&staging_target).expect("the leftover file must still be readable"),
            LEFTOVER
        );
        assert!(
            std::fs::symlink_metadata(root.path().join("plain.yml")).is_err(),
            "the claimed path must never come into existence from a refused sync"
        );
    }

    #[test]
    fn a_leftover_staging_file_from_an_earlier_run_is_refused_by_name() {
        // Not a symlink and not planted by this test's own scenario setup —
        // exactly what a `sync` interrupted partway through an earlier run
        // would leave behind: its own render, already written, never
        // handed over.
        let root = TempDir::new().expect("scratch directory");
        let staging_target = root.path().join(".thing.yml.skeletons-sync");
        std::fs::write(&staging_target, b"an earlier run's own unfinished render")
            .expect("plant a leftover staging file");

        let writes = vec![proven("thing.yml", b"new", DriftReason::Missing)];
        let error = prepare(root.path(), writes)
            .expect_err("prepare must refuse a leftover staging file by name");
        let WriteFailure::Collision { claim, what, .. } = error else {
            panic!("expected Collision, got {error:?}")
        };
        assert_eq!(claim, claim_path("thing.yml"));
        assert_eq!(what, CollisionAt::StagingFile);
    }

    #[test]
    fn a_missing_target_that_appeared_is_refused_as_changed() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("thing.txt"), b"already here").expect("target appeared");

        let writes = vec![proven("thing.txt", b"new", DriftReason::Missing)];
        let error =
            prepare(root.path(), writes).expect_err("prepare must refuse a target that appeared");
        let WriteFailure::TargetChanged { claim, what, .. } = error else {
            panic!("expected TargetChanged, got {error:?}")
        };
        assert_eq!(claim, claim_path("thing.txt"));
        assert_eq!(what, TargetChange::Appeared);
    }

    #[test]
    fn a_file_that_appears_after_prepare_is_not_replaced_by_commit() {
        // The property, not the API: a `Missing` write proved that nothing
        // was at its path, and a file that shows up between that proof and
        // the link is someone's work, which `commit` must refuse to
        // replace. `rename` over an existing file replaces it silently, so a
        // `Missing` write is linked into place instead, and the property has
        // to be enforced by how the write is committed, not by the check
        // `prepare` already makes before staging (which runs before this file
        // exists).
        //
        // Nothing driven from outside the process can land between
        // `prepare`'s own check and the link, so this exercises the
        // window directly: prepare the write, create the file at the target,
        // then commit. What must hold however `prepare` and `commit` are
        // shaped is that the appeared file is left byte for byte, the commit
        // fails naming the path, and no staging file is left behind.
        //
        // Platform: Unix only; the premise is POSIX file semantics.
        const APPEARED: &[u8] = b"someone else's work, written after the proof\n";

        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        let writes = vec![proven("thing.txt", b"the render", DriftReason::Missing)];
        let prepared = prepare(root.path(), writes).expect("prepare must succeed");
        std::fs::write(&target, APPEARED).expect("the file appears after prepare returned");

        let result = prepared.commit();

        assert_eq!(
            String::from_utf8_lossy(&std::fs::read(&target).expect("the appeared file")),
            String::from_utf8_lossy(APPEARED),
            "commit replaced a file that appeared after the write was proven absent"
        );
        let failure = result.expect_err("commit must refuse to replace a file that appeared");
        let message = crate::sync::message::write_failure_message(&failure);
        assert!(
            message.contains("thing.txt"),
            "the failure must name the path it refused; message: {message}"
        );
        assert!(
            no_skeletons_sync_staging_files_remain(root.path()),
            "a refused commit must remove its staging file"
        );
    }

    #[test]
    fn a_changed_target_that_became_a_symlink_is_refused_as_changed() {
        let root = TempDir::new().expect("scratch directory");
        let real = root.path().join("elsewhere.txt");
        std::fs::write(&real, b"elsewhere").expect("link target");
        std::os::unix::fs::symlink(&real, root.path().join("thing.txt"))
            .expect("symlink where the target used to be");

        let writes = vec![proven("thing.txt", b"new", DriftReason::Changed)];
        let error = prepare(root.path(), writes)
            .expect_err("prepare must refuse a target that became a symlink");
        let WriteFailure::TargetChanged { claim, what, .. } = error else {
            panic!("expected TargetChanged, got {error:?}")
        };
        assert_eq!(claim, claim_path("thing.txt"));
        assert_eq!(what, TargetChange::NoLongerAFile);
    }

    #[test]
    fn a_staging_path_that_is_itself_claimed_is_refused_before_anything_is_written() {
        use crate::claim::Claimant;
        use crate::survey::{Row, Survey};

        let claimant = Claimant {
            manifest: "Cargo.toml".to_owned(),
            dependency: "a-skeleton".to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        };
        let rows = vec![
            Row {
                path: claim_path("a/b"),
                claimant: claimant.clone(),
                drift: crate::claim::Drift::Drifted(DriftReason::Missing),
                rendered: b"hello".to_vec(),
            },
            Row {
                path: claim_path("a/.b.skeletons-sync"),
                claimant,
                drift: crate::claim::Drift::Matches,
                rendered: Vec::new(),
            },
        ];
        let survey = Survey {
            root: std::path::PathBuf::from("/workspace"),
            worn: Vec::new(),
            rows,
            refusals: Vec::new(),
        };

        let error =
            plan(&survey).expect_err("plan must refuse a staging path that is itself claimed");
        let WriteFailure::StagingClaimed {
            claim,
            staging,
            claimed,
            relation,
        } = error
        else {
            panic!("expected StagingClaimed, got {error:?}")
        };
        assert_eq!(claim, claim_path("a/b"));
        assert_eq!(staging, "a/.b.skeletons-sync");
        assert_eq!(claimed, claim_path("a/.b.skeletons-sync"));
        assert_eq!(relation, StagingRelation::Exact);
    }

    #[test]
    fn a_staging_path_that_folds_onto_another_claim_is_refused_naming_that_claim() {
        // Two bones claim `a/b` and `a/.B.skeletons-sync`, both missing: the
        // staging name of the first is the second by the render's own fold.
        // Also plans a survey where nothing folds onto a staging name, so
        // the refusal is the fold's and not any survey's.
        let claimant = crate::claim::Claimant {
            manifest: "Cargo.toml".to_owned(),
            dependency: "a-skeleton".to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        };
        let survey_of = |paths: &[&str]| crate::survey::Survey {
            root: std::path::PathBuf::from("/workspace"),
            worn: Vec::new(),
            rows: paths
                .iter()
                .map(|path| crate::survey::Row {
                    path: claim_path(path),
                    claimant: claimant.clone(),
                    drift: crate::claim::Drift::Drifted(DriftReason::Missing),
                    rendered: b"hello".to_vec(),
                })
                .collect(),
            refusals: Vec::new(),
        };

        let error = plan(&survey_of(&["a/b", "a/.B.skeletons-sync"]))
            .expect_err("plan must refuse a staging name that folds onto a claim");
        let WriteFailure::StagingClaimed {
            claim,
            claimed,
            relation,
            ..
        } = error
        else {
            panic!("expected StagingClaimed, got {error:?}")
        };
        assert_eq!(claim, claim_path("a/b"));
        assert_eq!(claimed, claim_path("a/.B.skeletons-sync"));
        assert_eq!(relation, StagingRelation::Folded);

        let writes = plan(&survey_of(&["a/b", "a/c"]))
            .expect("two unrelated names in one directory are both staged");
        assert_eq!(writes.len(), 2);
    }

    /// A [`Committed`] for one write already on disk at `target`, built by
    /// hand: a real `sync` run never builds one whose target is not what it
    /// just wrote, and these tests need exactly that.
    fn committed_at(path: &str, target: std::path::PathBuf, rendered: &[u8]) -> super::Committed {
        super::Committed {
            writes: vec![super::PreparedWrite {
                path: claim_path(path),
                skeleton: "a-skeleton".to_owned(),
                version: semver::Version::new(0, 1, 0),
                reason: DriftReason::Missing,
                rendered: rendered.to_vec(),
                staging: target.with_extension("staged"),
                target,
                evidence: Evidence::Absent,
                carried_permissions: None,
            }],
            leftovers: Vec::new(),
        }
    }

    fn changed_after_write(result: Result<(), WriteFailure>) -> Vec<ClaimPath> {
        match result {
            Err(WriteFailure::ChangedAfterWrite { paths }) => paths,
            other => panic!("expected ChangedAfterWrite, got {other:?}"),
        }
    }

    #[test]
    fn a_file_that_still_reads_back_as_written_verifies() {
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::write(&target, b"hello").expect("the written file");

        let result = super::verify(&committed_at("thing.txt", target, b"hello"));

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn a_file_changed_after_commit_is_reported_and_not_a_panic() {
        // The environment, not a bug: another process edits a file in the
        // instant between `commit` returning and `verify` reading it back.
        // The read-back differs, and `verify` must report the path as a
        // failure the caller can print, never panic.
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        let prepared = prepare(
            root.path(),
            vec![proven("thing.txt", b"the render", DriftReason::Missing)],
        )
        .expect("prepare must succeed");
        let committed = prepared.commit().expect("commit must succeed");
        std::fs::write(&target, b"an edit after the commit").expect("a concurrent edit");

        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::verify(&committed)));

        let paths = changed_after_write(result.expect("verify must not panic on an edit"));
        assert_eq!(paths, vec![claim_path("thing.txt")]);
        let message =
            crate::sync::message::write_failure_message(&WriteFailure::ChangedAfterWrite { paths });
        assert!(
            message.contains("thing.txt changed after sync wrote it"),
            "{message}"
        );
    }

    #[test]
    fn a_file_that_grew_after_commit_is_reported() {
        // The rendered bytes are a prefix of what is there now: the bounded
        // read-back reads one byte past the render, which is what makes a
        // longer file differ.
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::write(&target, b"hello and then some").expect("a longer file");

        let result = super::verify(&committed_at("thing.txt", target, b"hello"));

        assert_eq!(changed_after_write(result), vec![claim_path("thing.txt")]);
    }

    #[test]
    fn a_file_removed_after_commit_is_reported() {
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");

        let result = super::verify(&committed_at("thing.txt", target, b"hello"));

        assert_eq!(changed_after_write(result), vec![claim_path("thing.txt")]);
    }

    #[test]
    fn a_file_replaced_by_a_directory_after_commit_is_reported() {
        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::create_dir(&target).expect("a directory where the file was");

        let result = super::verify(&committed_at("thing.txt", target, b"hello"));

        assert_eq!(changed_after_write(result), vec![claim_path("thing.txt")]);
    }

    #[test]
    fn a_file_replaced_by_a_link_holding_the_rendered_bytes_is_reported() {
        // The link leads to a file holding exactly the rendered bytes, so
        // only refusing to read through a link catches it.
        let root = TempDir::new().expect("scratch directory");
        let real = root.path().join("elsewhere.txt");
        std::fs::write(&real, b"hello").expect("link target");
        let target = root.path().join("thing.txt");
        std::os::unix::fs::symlink(&real, &target).expect("symlink at the target");

        let result = super::verify(&committed_at("thing.txt", target, b"hello"));

        assert_eq!(changed_after_write(result), vec![claim_path("thing.txt")]);
    }

    #[test]
    fn a_file_whose_permissions_changed_after_commit_is_reported() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = TempDir::new().expect("scratch directory");
        let target = root.path().join("thing.txt");
        std::fs::write(&target, b"hello").expect("the written file");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
            .expect("carried permissions");
        let mut committed = committed_at("thing.txt", target.clone(), b"hello");
        committed.writes[0].carried_permissions = Some(std::fs::Permissions::from_mode(0o644));
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            .expect("changed after the commit");

        let result = super::verify(&committed);

        assert_eq!(changed_after_write(result), vec![claim_path("thing.txt")]);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_file_listed_under_another_spelling_is_reported() {
        // verify's spelling check, tested directly: on a case-insensitive
        // filesystem, reading `dependabot.yml` back finds `DEPENDABOT.YML`'s
        // own bytes, which happen to match the render, so the byte check
        // alone would pass silently. A real `sync` run can never build a
        // `Committed` like this one — `resolve` already refuses any claim
        // whose target is not listed under exactly its own spelling before
        // anything is staged — so this hand-builds one to prove the second
        // check actually fires when the first one would not have. The
        // premise (this filesystem folds case) is checked at run time
        // rather than assumed, as `contributing.md`'s "Platform-dependent
        // tests" section requires.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("DEPENDABOT.YML"), b"hello").expect("on-disk file");

        let folds_case = root
            .path()
            .join("dependabot.yml")
            .try_exists()
            .expect("look up the on-disk file under its other spelling");
        if !folds_case {
            eprintln!("skipped: this filesystem does not fold case");
            return;
        }

        let result = super::verify(&committed_at(
            "dependabot.yml",
            root.path().join("dependabot.yml"),
            b"hello",
        ));

        assert_eq!(
            changed_after_write(result),
            vec![claim_path("dependabot.yml")]
        );
    }

    #[test]
    fn every_path_that_changed_is_named_and_the_ones_that_did_not_are_not() {
        let root = TempDir::new().expect("scratch directory");
        let prepared = prepare(
            root.path(),
            vec![
                proven("a.txt", b"one", DriftReason::Missing),
                proven("b.txt", b"two", DriftReason::Missing),
                proven("c.txt", b"three", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");
        let committed = prepared.commit().expect("commit must succeed");
        std::fs::write(root.path().join("a.txt"), b"edited").expect("edit a");
        std::fs::remove_file(root.path().join("c.txt")).expect("remove c");

        let paths = changed_after_write(super::verify(&committed));

        assert_eq!(paths, vec![claim_path("a.txt"), claim_path("c.txt")]);
    }

    /// Replaces the staging file of `claim` (a name under `root`) with a
    /// directory. `link(2)` refuses a directory with `EPERM` on every
    /// platform and for every user, which is the error a filesystem without
    /// hard links (FAT, some network and FUSE mounts) gives for any file:
    /// the one deterministic way to make a link fail here, without a
    /// filesystem that lacks them.
    fn make_the_link_fail(root: &Path, claim_directory: &str, staged_name: &str) {
        let staged = root.join(claim_directory).join(staged_name);
        std::fs::remove_file(&staged).expect("the staging file");
        std::fs::create_dir(&staged).expect("a directory at its name");
    }

    #[test]
    fn a_link_that_fails_leaves_every_replacement_unwritten() {
        // One file is replaced (`a.yml`, sorted first) and one is created
        // (`d/b.yml`). The link that creates `d/b.yml` is refused. A refusal
        // like that is known before the first write, so nothing may have
        // landed: `a.yml` holds its old bytes and `d/b.yml` does not exist.
        // Landing in path order would rename `a.yml` first and then fail,
        // leaving the tree half written.
        //
        // What this proves: links land before any rename. What it cannot:
        // that a real filesystem without hard links behaves the same, since
        // the failure here is a directory in the staging file's place.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("a.yml"), b"old").expect("the file to replace");
        let prepared = prepare(
            root.path(),
            vec![
                proven_holding("a.yml", b"new", b"old"),
                proven("d/b.yml", b"created", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");
        make_the_link_fail(root.path(), "d", ".b.yml.skeletons-sync");

        let outcome = prepared.commit();

        assert!(outcome.is_err(), "the link cannot land: {outcome:?}");
        assert_eq!(
            std::fs::read(root.path().join("a.yml")).expect("a.yml exists"),
            b"old",
            "a replacement must not land before every link has"
        );
        assert!(
            !root.path().join("d/b.yml").exists(),
            "the file whose link failed must not exist"
        );
        assert!(
            !root.path().join(".a.yml.skeletons-sync").exists(),
            "the replacement's staging file must be removed"
        );
    }

    /// The operating system's own words for the failure `commit` meets when
    /// it links `staging` into place: the same call, made here at a scratch
    /// name, so the expected text is whatever this platform says and is not
    /// written down twice.
    fn os_words_for_linking(staging: &Path, scratch_target: &Path) -> String {
        std::fs::hard_link(staging, scratch_target)
            .expect_err("the link this scenario built cannot succeed")
            .to_string()
    }

    #[test]
    fn a_link_that_fails_says_sync_creates_new_files_with_hard_links_and_carries_the_os_error() {
        // The only write is a created file, and its link is refused with
        // `EPERM` (see `make_the_link_fail`). `sync` does not read the
        // error number: every link failure other than "already exists" is
        // one failure, and the message must give the user both halves of
        // what they need: that `sync` creates new files with hard links (so
        // the words "Operation not permitted" mean something) and the
        // operating system's own error, unchanged.
        //
        // The message does not say the filesystem does not support hard
        // links: that is a claim about the filesystem, which `sync` would
        // only make by classifying the error number, and it classifies none,
        // so the message may not assert it.
        let root = TempDir::new().expect("scratch directory");
        let prepared = prepare(
            root.path(),
            vec![proven("d/b.yml", b"created", DriftReason::Missing)],
        )
        .expect("prepare must succeed");
        make_the_link_fail(root.path(), "d", ".b.yml.skeletons-sync");
        let os_words = os_words_for_linking(
            &root.path().join("d/.b.yml.skeletons-sync"),
            &root.path().join("scratch-target"),
        );

        let error = prepared.commit().expect_err("the link cannot land");

        let message = crate::sync::message::write_failure_message(&error);
        assert!(
            message.contains("d/b.yml"),
            "the message must name the file: {message}"
        );
        assert!(
            message
                .to_lowercase()
                .contains("creates new files with hard links"),
            "the message must say sync creates new files with hard links: {message}"
        );
        assert!(
            message.contains(&os_words),
            "the message must carry the operating system's own error, {os_words:?}: {message}"
        );
        assert!(
            !message
                .to_lowercase()
                .contains("does not support hard links"),
            "no error number is read, so the message must not say the filesystem lacks hard \
             links: {message}"
        );
    }

    #[test]
    fn a_link_that_fails_for_any_other_reason_says_the_same() {
        // The same message for a failure that is not `EPERM`. The staging
        // file is gone (`ENOENT`), which no filesystem's support for hard
        // links explains. Every link failure but "already exists" is one
        // cause, so the message carries `sync`'s own account (it creates new
        // files with hard links) and the operating system's error, and says
        // nothing more about why.
        let root = TempDir::new().expect("scratch directory");
        let prepared = prepare(
            root.path(),
            vec![proven("d/b.yml", b"created", DriftReason::Missing)],
        )
        .expect("prepare must succeed");
        std::fs::remove_file(root.path().join("d/.b.yml.skeletons-sync"))
            .expect("the staging file goes missing after prepare");
        let os_words = os_words_for_linking(
            &root.path().join("d/.b.yml.skeletons-sync"),
            &root.path().join("scratch-target"),
        );

        let error = prepared.commit().expect_err("the link cannot land");

        let message = crate::sync::message::write_failure_message(&error);
        assert!(
            message.contains("d/b.yml"),
            "the message must name the file: {message}"
        );
        assert!(
            message
                .to_lowercase()
                .contains("creates new files with hard links"),
            "the message must say sync creates new files with hard links: {message}"
        );
        assert!(
            message.contains(&os_words),
            "the message must carry the operating system's own error, {os_words:?}: {message}"
        );
    }

    #[test]
    fn a_replacement_and_a_creation_both_land_when_nothing_fails() {
        // Guard: landing links before renames must not drop or reorder a
        // write: with a replacement and a creation and nothing wrong, both
        // are written with their own bytes.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("a.yml"), b"old").expect("the file to replace");
        let prepared = prepare(
            root.path(),
            vec![
                proven_holding("a.yml", b"new", b"old"),
                proven("d/b.yml", b"created", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");

        let committed = prepared.commit().expect("commit must succeed");

        assert_eq!(committed.writes().len(), 2);
        assert_eq!(
            std::fs::read(root.path().join("a.yml")).expect("a.yml exists"),
            b"new"
        );
        assert_eq!(
            std::fs::read(root.path().join("d/b.yml")).expect("d/b.yml exists"),
            b"created"
        );
        assert!(
            committed.leftovers().is_empty(),
            "{:?}",
            committed.leftovers()
        );
    }

    #[test]
    fn a_failure_after_absent_writes_landed_names_them_in_path_order() {
        // Four writes in path order, `a/one.txt` (created), `b.yml`
        // (replaced), `c/two.txt` and `d/three.txt` (created). Links land
        // first, so the order is a/one, c/two, d/three, then b.yml. The link
        // for `d/three.txt` fails (its staging file is gone), after two
        // links landed and before the rename. Both lists must read in path
        // order, not landing order: `b.yml` sits between `a/one.txt` and
        // `c/two.txt` in one and after the failed path in the other.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("b.yml"), b"old").expect("the file to replace");
        let prepared = prepare(
            root.path(),
            vec![
                proven("a/one.txt", b"one", DriftReason::Missing),
                proven_holding("b.yml", b"new", b"old"),
                proven("c/two.txt", b"two", DriftReason::Missing),
                proven("d/three.txt", b"three", DriftReason::Missing),
            ],
        )
        .expect("prepare must succeed");
        std::fs::remove_file(root.path().join("d/.three.txt.skeletons-sync"))
            .expect("the staging file goes missing after prepare");

        let error = prepared.commit().expect_err("the last link must fail");

        let WriteFailure::Commit(commit) = error else {
            panic!("expected WriteFailure::Commit, got {error:?}")
        };
        assert_eq!(commit.failed_path, claim_path("d/three.txt"));
        assert_eq!(
            commit.already_written,
            vec![claim_path("a/one.txt"), claim_path("c/two.txt")]
        );
        assert_eq!(commit.not_yet_written, vec![claim_path("b.yml")]);
        assert_eq!(commit.total, 4);
        assert_eq!(
            std::fs::read(root.path().join("b.yml")).expect("b.yml exists"),
            b"old",
            "the replacement lands after every link, so it never landed"
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_directory_that_cannot_be_written_is_reported_by_name_with_nothing_written() {
        use std::os::unix::fs::PermissionsExt as _;

        // `d` exists and is read-only for this user, so the staging file for
        // `d/x.yml` cannot be created. Nothing else was created first
        // (`a.yml`'s staging file, made before it, is removed), and the
        // failure names the directory rather than repeating the operating
        // system's "Permission denied". A superuser can write anywhere, so
        // there the test says `skipped:`.
        let root = TempDir::new().expect("scratch directory");
        let locked = root.path().join("d");
        std::fs::create_dir(&locked).expect("the directory to lock");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))
            .expect("chmod the directory");
        let restore = || std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        match std::fs::write(locked.join("probe"), b"x") {
            Ok(()) => {
                restore().expect("restore the directory");
                eprintln!("skipped: this process can write into a 0o555 directory");
                return;
            }
            Err(error) => assert_eq!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied,
                "the write into the 0o555 directory failed, but not on permission: {error}"
            ),
        }

        let error = prepare(
            root.path(),
            vec![
                proven("a.yml", b"one", DriftReason::Missing),
                proven("d/x.yml", b"two", DriftReason::Missing),
            ],
        )
        .expect_err("the directory cannot be written");
        restore().expect("restore the directory");

        let WriteFailure::DirectoryNotWritable {
            path,
            directory,
            leftovers,
            ..
        } = error
        else {
            panic!("expected DirectoryNotWritable, got {error:?}")
        };
        assert_eq!(path, claim_path("d/x.yml"));
        assert_eq!(directory, Some(claim_path("d")));
        assert!(leftovers.is_empty(), "{leftovers:?}");
        assert!(no_skeletons_sync_staging_files_remain(root.path()));
    }
}
