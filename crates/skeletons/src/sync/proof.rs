//! Rule (b): positive proof, per path, that what `sync` is about to write
//! either replaces nothing (the path is absent from the work tree *and*
//! from this repository's index) or replaces exactly what git would check
//! out for the path's index entry, so `git checkout` gives every byte of
//! it back. Both halves also prove the other thing a write needs: that git
//! will *see* it. A file git does not read from the work tree (flagged
//! skip-worktree or assume-unchanged), a directory whose name git tracks as a
//! file, submodule or link at any level above the claim, and a spelling of
//! the claim or of a directory above it that the filesystem takes for a
//! hidden entry under another spelling, and a file to create that git's ignore
//! rules ignore, all refuse, since `sync` would report a write git then
//! ignores.
//!
//! "What git would check out" is asked of git itself (`cat-file --filters`,
//! which applies the smudge side of every attribute and filter as a
//! checkout does) and compared byte for byte with the disk. Asking whether
//! git would *record* the file as unchanged is a weaker statement: a clean
//! filter that is not the exact inverse of its smudge maps bytes git never
//! stored onto the committed object, so those bytes would read as held and
//! then be lost.
//!
//! "Rule (a)" and "rule (b)" throughout this module are the two conditions
//! `.docs/design.md` sets out for a write: (a) "The whole work tree is clean",
//! checked in [`crate::work_tree::clean`], and (b) "Positive proof, per path", which this
//! module establishes.

mod above;
mod ignored;
mod listing;

use crate::claim::{ClaimPath, DriftReason, OnDisk, UnsafePathCause, resolve};
use crate::git::{self, Locale, ObjectId};
use crate::subprocess::Truncated;

use super::fold_variant::{FoldVariant, fold_variants};
use super::write::Write;
use crate::work_tree::abort::{GitQuestion, WorkTreeAbort};
use crate::work_tree::clean::CleanWorkTree;
use crate::work_tree::index_entry::{HiddenFlag, IndexRecord, IndexTag};
use crate::work_tree::index_records::{self, UnusableIndexAnswer};
use crate::work_tree::{WorkTree, run_local};

/// A drifted write `sync` may actually perform: nothing is at its path, or
/// git's index holds exactly what is there. The fields are private; only
/// [`prove`] constructs one, so a `ProvenWrite` in hand is itself the proof
/// that rule (b) already held for it.
#[derive(Debug)]
pub(crate) struct ProvenWrite {
    write: Write,
    evidence: Evidence,
    fold_variants: Vec<FoldVariant>,
}

impl ProvenWrite {
    /// Consumes this proof, handing back the write it was for, the evidence
    /// that proved it and the index entries a filesystem that folds names
    /// could take for it — [`super::write::prepare`] keeps the evidence with
    /// the write and re-verifies against it, in place of the drift reason
    /// `resolve` first found, and asks the filesystem about each variant once
    /// the staging file exists.
    pub(crate) fn into_parts(self) -> (Write, Evidence, Vec<FoldVariant>) {
        (self.write, self.evidence, self.fold_variants)
    }

    /// Builds a `ProvenWrite` directly, for `write`'s own unit tests, which
    /// exercise staging, committing and verifying independently of any real
    /// git process — they need a proof to hand `prepare`, not `prove`
    /// itself. It has no fold variants: a test that needs one builds it with
    /// [`Self::for_test_with_variants`].
    #[cfg(test)]
    pub(crate) const fn for_test(write: Write, evidence: Evidence) -> Self {
        Self {
            write,
            evidence,
            fold_variants: Vec::new(),
        }
    }

    /// [`Self::for_test`], carrying `fold_variants`.
    #[cfg(test)]
    pub(crate) const fn for_test_with_variants(
        write: Write,
        evidence: Evidence,
        fold_variants: Vec<FoldVariant>,
    ) -> Self {
        Self {
            write,
            evidence,
            fold_variants,
        }
    }
}

/// What was proven about a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Evidence {
    /// Nothing is at the path, on disk or in git's index.
    Absent,
    /// A regular file whose bytes are exactly what git would check out.
    Held(HeldFile),
}

/// A file [`prove`] found to hold exactly what git would check out for its
/// index entry. The fields are private and only [`prove`] fills them, so a
/// `HeldFile` in hand is itself the proof that rule (b) held for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeldFile {
    object: ObjectId,
    mode: IndexMode,
    checkout: CheckoutBytes,
}

impl HeldFile {
    /// The bytes the file held when it was proven, which are exactly what
    /// git would check out for it: `write::reverify` compares the
    /// disk against them right before each write lands.
    pub(crate) fn checkout(&self) -> &[u8] {
        &self.checkout.0
    }

    /// Builds a `HeldFile` directly, for `write`'s own unit tests, which
    /// exercise staging and committing without a real git process.
    #[cfg(test)]
    pub(crate) const fn for_test(object: ObjectId, mode: IndexMode, checkout: Vec<u8>) -> Self {
        Self {
            object,
            mode,
            checkout: CheckoutBytes(checkout),
        }
    }
}

/// Exactly what `git cat-file --filters` wrote for an index entry, which is
/// exactly what was on disk when the proof read it. The field is private and
/// only [`prove`] constructs one.
///
/// Held from the proof until the write is committed, so its size is bounded
/// by what `skeletons` reads from git: no more than `GIT_OUTPUT_BYTES_MAX`
/// (16 MiB), because a longer checkout is refused as too large before it
/// becomes evidence.
#[derive(Clone, PartialEq, Eq)]
struct CheckoutBytes(Vec<u8>);

impl std::fmt::Debug for CheckoutBytes {
    /// Shows the length only: the bytes are file content, and a file's whole
    /// content has no place in a debug line.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "CheckoutBytes({} bytes)", self.0.len())
    }
}

/// The two index modes rule (b) ever accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexMode {
    /// `100644`.
    Regular,
    /// `100755`.
    Executable,
}

/// Every write `sync` was asked to make, once each has either been proven
/// or refused.
#[derive(Debug)]
pub(crate) enum Proven {
    All(Vec<ProvenWrite>),
    Refused(Vec<Unproven>),
}

/// One write `prove` could not establish rule (b) for, and why.
#[derive(Debug)]
pub(crate) struct Unproven {
    pub(crate) path: ClaimPath,
    pub(crate) why: Why,
    /// What the survey found at the path: missing, so `sync` would create it,
    /// or changed, so `sync` would replace it. The refusal's own summary is
    /// worded by it, because a cause (a directory above tracked as a file, a
    /// change since the survey) can refuse either.
    pub(crate) drift: DriftReason,
}

/// Why one path could not be proven.
#[derive(Debug, Clone)]
pub(crate) enum Why {
    /// The index holds no entry for the path under exactly the spelling the
    /// claim uses.
    NotInIndex,
    /// The path is absent from the work tree but this repository's index still
    /// tracks `git_path` at or under it — skip-worktree or a sparse checkout
    /// hides it, so git would ignore what `sync` wrote there.
    TrackedButAbsent { git_path: String },
    /// The path is a regular file whose index entry is flagged so that git
    /// does not read its bytes from the work tree: git would not see what
    /// `sync` wrote, and `git commit -a` would record the old bytes.
    HiddenFromWorkTree { flag: HiddenFlag },
    /// This repository's index tracks `git_path`, spelled as `git_path`
    /// spells it, at a directory above the path, so a directory `sync`
    /// creates there is one git would put the tracked entry back over.
    TrackedAbove { git_path: String, entry: AboveEntry },
    /// A file `sync` would create is one git's ignore rules ignore, so `git
    /// status` would never show it and `git add` would refuse it. `rule` is
    /// what `git check-ignore -v` reports for it, `<source>:<line>:<pattern>`,
    /// exactly as git prints it.
    IgnoredByGit { rule: String },
    /// Git could not say whether its ignore rules ignore a file `sync` would
    /// create, or answered with something `skeletons` cannot read. `detail` is one
    /// line: git's own diagnostic, or the first line of an answer `skeletons` could
    /// not read, and never more of that answer.
    IgnoreCheckFailed { detail: String },
    /// The index has one entry for the path, but git lists it as
    /// `git_path`: a different spelling of it, or an entry beneath the path
    /// when the path is a directory, so what `sync` wrote would not be the
    /// file that entry names.
    ListedAs { git_path: String },
    /// The index entry is conflicted: it has an entry at a stage other than
    /// zero.
    Conflicted,
    /// The index entry is a symbolic link (mode `120000`).
    SymbolicLinkInIndex,
    /// The index entry is a submodule's gitlink (mode `160000`).
    SubmoduleInIndex,
    /// The index entry has a mode that is neither a regular file's, an
    /// executable's, a symbolic link's nor a gitlink's; `mode` is git's
    /// text for it.
    UnexpectedMode { mode: String },
    /// Git's answer about the index entry could not be used: the listing
    /// failed, could not be parsed, or held more than one entry for the
    /// path. `detail` says which.
    IndexEntryUnreadable { detail: String },
    /// The bytes on disk are not what git would check out for the entry,
    /// whatever the cause: an edit git cannot see, or bytes a filter
    /// would not write back.
    NotWhatGitChecksOut,
    /// Git checks the entry out differently each time it is asked, so there
    /// is no one set of bytes the file could be brought back to: a content
    /// filter whose output is not a function of the file alone. Nothing `skeletons`
    /// can name fixes it.
    CheckoutNotReproducible,
    /// Git could not say what it would check out for the entry.
    CheckoutFailed { diagnostic: String },
    /// The output of the git command `command` (`ls-files` or `cat-file`)
    /// ran past the size `sync` reads, so nothing can be concluded from it.
    OutputTooLarge { command: &'static str },
    /// The path exists but is a symbolic link, a directory or something
    /// else that is not a regular file.
    NotARegularFile,
    /// The path could not be examined or read on disk; `detail` is the
    /// operating system's error.
    Unreadable { detail: String },
    /// The path differs from what the survey found: a file where the survey
    /// saw none, none where it saw one, or something other than a plain file
    /// at the claimed spelling when read back.
    ChangedSinceSurvey,
}

/// What git's index tracks at a directory above a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AboveEntry {
    /// A regular file, mode `100644` or `100755`.
    File,
    /// A symbolic link, mode `120000`.
    SymbolicLink,
    /// A submodule's gitlink, mode `160000`: another repository.
    Submodule,
    /// Any other mode.
    Other { mode: String },
}

impl AboveEntry {
    fn from_mode(mode: &str) -> Self {
        match mode {
            "100644" | "100755" => Self::File,
            "120000" => Self::SymbolicLink,
            "160000" => Self::Submodule,
            other => Self::Other {
                mode: other.to_owned(),
            },
        }
    }
}

/// Proves rule (b) for every one of `writes`. The questions about one path
/// are asked one path per git invocation, so every answer is attributable by
/// construction. Two are asked of the writes together, because their answers
/// belong to no single path: which entries git's index lists at the depth of
/// the writes (from which the entries a filesystem could fold onto a write are
/// picked out, [`fold_variants`]), and, once per distinct directory above any
/// write, whether git tracks an entry there ([`above::tracked_above`]).
///
/// `clean` is never read: its presence is what makes "not intent-to-add"
/// established. An intent-to-add entry of an empty file is exactly the bytes
/// git would check out for its own index entry, so rule (b) alone cannot see
/// it — only rule (a), which `clean`'s existence proves already ran and found
/// the whole tree clean, catches an intent-to-add entry at all.
///
/// Every write is proven or refused before anything is reported, so a
/// caller sees every failing path at once rather than only the first.
pub(crate) fn prove(
    work_tree: &WorkTree,
    _clean: &CleanWorkTree,
    writes: Vec<Write>,
) -> Result<Proven, WorkTreeAbort> {
    let claims: Vec<&ClaimPath> = writes.iter().map(|write| &write.path).collect();
    let index_paths = listing::index_listing(work_tree, &claims)?;
    let variants_per_write = fold_variants(&index_paths, &claims);
    let tracked_above = above::tracked_above(work_tree, &claims)?;
    assert_eq!(
        variants_per_write.len(),
        writes.len(),
        "fold_variants answers every write"
    );
    assert_eq!(
        tracked_above.len(),
        writes.len(),
        "tracked_above answers every write"
    );

    let mut proven = Vec::with_capacity(writes.len());
    let mut unproven = Vec::new();

    for ((write, variants), above) in writes
        .into_iter()
        .zip(variants_per_write)
        .zip(tracked_above)
    {
        // A refused ancestor settles the write: nothing asked of the claim
        // itself could make a directory git tracks as a file one it would
        // keep.
        let outcome = match above {
            Some(why) => Err(why),
            None => prove_one(work_tree, &write)?,
        };
        match outcome {
            Ok(evidence) => proven.push(ProvenWrite {
                write,
                evidence,
                fold_variants: variants,
            }),
            Err(why) => unproven.push(Unproven {
                path: write.path.clone(),
                why,
                drift: write.reason,
            }),
        }
    }

    if unproven.is_empty() {
        Ok(Proven::All(proven))
    } else {
        Ok(Proven::Refused(unproven))
    }
}

/// Proves rule (b) for one write. The outer [`Result`] is a whole-command
/// abort (git itself could not be run); the inner one is this one path's
/// own refusal, when rule (b) does not hold for it.
///
/// A path on disk is proven in three steps, asking git before reading the
/// file: `ls-files -v` names the one index entry (its object, mode and, in the
/// letter it prints first, whether git reads the file from the work tree at
/// all: a flagged file is refused here, before `cat-file` runs), then
/// `cat-file --filters` says what git would check out for that object at
/// this path, and last the file is read through the same walk the survey
/// used, bounded by the checkout's own length plus one. Reading last means
/// anything a content filter did to the file while git ran is what gets
/// compared. When the bytes differ, `cat-file` is asked once more so the
/// refusal can say whether there is one checkout to bring the file back to
/// ([`why_not_the_checkout`]); that second question is asked only on the
/// refusal path.
fn prove_one(work_tree: &WorkTree, write: &Write) -> Result<Result<Evidence, Why>, WorkTreeAbort> {
    let target = write.path.to_path(work_tree.root());
    match std::fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return prove_absent(work_tree, write);
        }
        Err(error) => {
            return Ok(Err(Why::Unreadable {
                detail: error.to_string(),
            }));
        }
        Ok(metadata) => {
            if metadata.is_symlink() || !metadata.is_file() {
                return Ok(Err(Why::NotARegularFile));
            }
            if matches!(write.reason, DriftReason::Missing) {
                return Ok(Err(Why::ChangedSinceSurvey));
            }
        }
    }

    let pathspec = write.path.as_str();

    let records = match index_records::records_for(work_tree, pathspec)? {
        Err(unusable) => return Ok(Err(why_unusable(unusable))),
        Ok(records) => records,
    };
    let (mode, object) = match index_entry_from(&records, pathspec) {
        Err(why) => return Ok(Err(why)),
        Ok(found) => found,
    };
    let checkout = match checkout_bytes_for(work_tree, &write.path, &object)? {
        Err(why) => return Ok(Err(why)),
        Ok(checkout) => checkout,
    };

    match disk_bytes_equal(work_tree, &write.path, &checkout) {
        Err(why) => Ok(Err(why)),
        // The bytes differ, so this path is refused whatever comes next. Git
        // is asked once more, only on this refusal path, so the refusal can
        // say whether there is a checkout to bring the file back to.
        Ok(false) => {
            let again = checkout_bytes_for(work_tree, &write.path, &object)?;
            Ok(Err(why_not_the_checkout(&checkout, again)))
        }
        Ok(true) => Ok(Ok(Evidence::Held(HeldFile {
            object,
            mode,
            checkout: CheckoutBytes(checkout),
        }))),
    }
}

/// Why a file whose bytes are not what git checked out is refused, given what
/// git checked out (`first`) and what it checked out when asked a second
/// time (`second`).
///
/// The same bytes both times mean there is one thing git would check out, and
/// `git checkout` writes it, so the file can be brought to it:
/// [`Why::NotWhatGitChecksOut`]. Different bytes mean the object and the path
/// do not determine the checkout (a content filter whose output changes from
/// one run to the next), so `git checkout` would write yet another variant
/// and the next run would refuse again: [`Why::CheckoutNotReproducible`],
/// with no remedy to offer. A second run that fails is that failure, which is
/// what the path is refused for.
///
/// A filter whose output changes only between whole seconds can return the
/// same bytes twice, and is then refused once more after the remedy; the line
/// the user sees is the same, and nothing worse than that follows.
fn why_not_the_checkout(first: &[u8], second: Result<Vec<u8>, Why>) -> Why {
    match second {
        Err(why) => why,
        Ok(again) if again == first => Why::NotWhatGitChecksOut,
        Ok(_) => Why::CheckoutNotReproducible,
    }
}

/// Proves rule (b) for a path nothing is at on disk: "absent" holds only
/// when this repository's index has no entry at or under it either.
///
/// A path git was told not to materialise (`skip-worktree`, or a sparse
/// checkout that excludes it) is missing from the work tree while its
/// index entry stays: writing there produces a file git ignores, so `sync`
/// would report success and leave the committed bytes in force. Refusing
/// is what keeps that report true; the remedy is one the user already
/// chose to withhold (`git sparse-checkout add`, or `git update-index
/// --no-skip-worktree`). The index answer is the `ls-files` question
/// [`prove_one`] asks of a path that exists, asked with git's case fold
/// ([`absent_index_records_for`]), and read through the same classifier.
///
/// A path the index holds nothing for is then asked whether git's ignore
/// rules ignore it ([`ignored::refusal`]): a file `sync` created there would
/// never show in `git status`, and the next drift would find it untracked.
/// Only a file `sync` would create is asked; a present file git tracks is
/// not ignored in effect.
///
/// Tested by `a_skip_worktree_path_absent_from_the_work_tree_is_tracked_but_absent`,
/// `an_absent_path_git_folds_onto_a_tracked_entry_is_tracked_but_absent`,
/// `a_glob_character_in_an_absent_claim_stays_a_literal_character` and
/// `ritual/tests/sync_worktree.rs` →
/// `a_skip_worktree_path_absent_from_the_work_tree_is_refused_not_created`,
/// and `ritual/tests/sync_free_paths.rs` →
/// `a_path_git_hides_under_another_case_is_refused_not_created` and →
/// `a_path_git_hides_under_a_directory_of_another_case_is_refused_not_created`.
fn prove_absent(
    work_tree: &WorkTree,
    write: &Write,
) -> Result<Result<Evidence, Why>, WorkTreeAbort> {
    if matches!(write.reason, DriftReason::Changed) {
        return Ok(Err(Why::ChangedSinceSurvey));
    }
    let records = match absent_index_records_for(work_tree, &write.path)? {
        Err(unusable) => return Ok(Err(why_unusable(unusable))),
        Ok(records) => records,
    };
    // Any record, at any stage, counts — a conflicted entry is tracked too —
    // and the pathspec matches everything under a directory of that name and
    // every case variant git folds onto it, so a record need not spell the
    // claimed path itself.
    if let Some(record) = records.first() {
        return Ok(Err(Why::TrackedButAbsent {
            git_path: String::from_utf8_lossy(&record.path).into_owned(),
        }));
    }
    // Only a path the index holds nothing for is asked about the ignore
    // rules: an entry git tracks is not ignored in effect, whatever a rule
    // says, and is refused above for its own reason.
    Ok(ignored::refusal(work_tree, &write.path)?.map_or(Ok(Evidence::Absent), Err))
}

/// The `ls-files -v --stage -z` question ([`index_records::records_for`]) for
/// a path nothing is at on disk, asked with git's own case fold: the pathspec is
/// `:(literal,icase)<claim>`, so git lists every entry at or beneath the
/// claim under any case it would take for it.
///
/// The fold is git's and not this crate's `FoldedName` on purpose. What
/// `sync` must not write is a file git would ignore, because under
/// `core.ignorecase` git takes it for a tracked entry that only differs by
/// case (for a skip-worktree `a.yml` and a claim of `A.yml`, writing `A.yml`
/// creates a file git ignores). Git's fold is ASCII only (captured), so it
/// keeps a Unicode variant such as `É.yml` beside `é.yml` visible, and
/// `FoldedName` would refuse what git tracks and shows without trouble.
///
/// It is asked whatever `core.ignorecase` says. Reading the setting would be
/// one more git command, and skipping it costs only a refusal of an
/// index-only case variant on a case-sensitive git, which is rare and names
/// the entry. It also answers the same on every platform.
///
/// The command comes from [`WorkTree::git_for_pathspec_magic`], because the
/// global `--literal-pathspecs` [`WorkTree::git`] passes disables `:(icase)`;
/// `literal` magic in the pathspec keeps `*`, `?` and `[` ordinary instead.
fn absent_index_records_for(
    work_tree: &WorkTree,
    path: &ClaimPath,
) -> Result<Result<Vec<IndexRecord>, UnusableIndexAnswer>, WorkTreeAbort> {
    let mut ls_files = work_tree.git_for_pathspec_magic(Locale::Fixed);
    ls_files.args([
        "ls-files",
        "-v",
        "--stage",
        "-z",
        "--",
        &git::pathspec_ignoring_case(path),
    ]);
    index_records::run_ls_files(ls_files, GitQuestion::IndexEntry(path.as_str().to_owned()))
}

/// What an `ls-files` answer that cannot be used means for the path it was
/// asked about: git's own diagnostic, or an answer past the cap, as `sync`
/// words them.
pub(super) fn why_unusable(unusable: UnusableIndexAnswer) -> Why {
    match unusable {
        UnusableIndexAnswer::Failed { detail } => Why::IndexEntryUnreadable { detail },
        UnusableIndexAnswer::TooLarge => Why::OutputTooLarge {
            command: "ls-files",
        },
    }
}

/// Reads the single, stage-0, regular-file entry rule (b) requires out of
/// `records`, one git reads from the work tree — or says why `pathspec` does
/// not have one.
///
/// The flags come last. An entry flagged skip-worktree or assume-unchanged is
/// refused as [`Why::HiddenFromWorkTree`] before `cat-file` is ever asked,
/// however its bytes compare: whatever `sync` wrote there git would not read.
fn index_entry_from(records: &[IndexRecord], pathspec: &str) -> Result<(IndexMode, ObjectId), Why> {
    if records.is_empty() {
        return Err(Why::NotInIndex);
    }
    if records.iter().any(|record| record.stage != 0) {
        return Err(Why::Conflicted);
    }
    if records.len() > 1 {
        return Err(Why::IndexEntryUnreadable {
            detail: format!("git listed {} index entries for it", records.len()),
        });
    }
    let record = &records[0];
    if record.path != pathspec.as_bytes() {
        return Err(Why::ListedAs {
            git_path: String::from_utf8_lossy(&record.path).into_owned(),
        });
    }
    let mode = match record.mode.as_str() {
        "100644" => IndexMode::Regular,
        "100755" => IndexMode::Executable,
        "120000" => return Err(Why::SymbolicLinkInIndex),
        "160000" => return Err(Why::SubmoduleInIndex),
        other => {
            return Err(Why::UnexpectedMode {
                mode: other.to_owned(),
            });
        }
    };
    if let Some(flag) = record.tag.hiding_flag() {
        return Err(Why::HiddenFromWorkTree { flag });
    }
    match record.tag {
        IndexTag::Tracked => Ok((mode, record.object.clone())),
        // A stage-0 record is never unmerged: the stage check above already
        // refused every unmerged entry. Reaching here means git printed a
        // contradiction, which is a refusal, not a guess.
        IndexTag::Unmerged => Err(Why::Conflicted),
        IndexTag::SkipWorktree
        | IndexTag::AssumeUnchanged
        | IndexTag::SkipWorktreeAndAssumeUnchanged => {
            unreachable!("hiding_flag named a flag for every tag that hides a file: {record:?}")
        }
    }
}

/// Runs `cat-file --filters --path=<prefix><claim> <object>` and reads back
/// the bytes git would write into the work tree for that index entry — or
/// why it could not say.
///
/// `--path` names where the file would be checked out, and git reads its
/// attributes for that path relative to the *repository* root, not the
/// workspace: from a workspace below the root, a workspace-relative path
/// would match no attribute at all and the raw blob would be compared, so the
/// workspace's own prefix leads it (`ritual/tests/sync_checkout_side.rs` →
/// `a_file_a_repository_path_attribute_would_expand_differently_is_refused_in_a_nested_workspace`
/// and →
/// `a_clean_file_under_a_filter_in_a_workspace_below_the_repository_root_is_held_and_updated`).
/// The object is the hex id `ls-files` just printed, so no revision syntax
/// is ever read from a claimed path, and it cannot be read as an option.
///
/// `GIT_NO_LAZY_FETCH=1` (honoured by git 2.44 and later) makes git itself
/// refuse to fetch a missing promisor object, so this process makes no
/// network request of its own. The wearer's smudge filter runs as it does on
/// checkout, and a filter that downloads (git-lfs) is that filter's own
/// doing.
fn checkout_bytes_for(
    work_tree: &WorkTree,
    path: &ClaimPath,
    object: &ObjectId,
) -> Result<Result<Vec<u8>, Why>, WorkTreeAbort> {
    let mut cat_file = work_tree.git(Locale::Inherited);
    cat_file.env("GIT_NO_LAZY_FETCH", "1");
    cat_file.args([
        "cat-file",
        "--filters",
        &format!("--path={}{path}", work_tree.prefix().as_str()),
        object.as_str(),
    ]);
    let finished = run_local(cat_file, GitQuestion::Checkout(path.clone()))?;
    Ok(classify_cat_file(
        finished.success(),
        finished.stdout(),
        finished.stderr_head(),
    ))
}

/// The pure classifier behind [`checkout_bytes_for`], split out for the same
/// reason as [`index_records::classify_ls_files`]. The checkout is stdout as written, never
/// parsed.
fn classify_cat_file(
    exit_ok: bool,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<Vec<u8>, Why> {
    if !exit_ok {
        return Err(Why::CheckoutFailed {
            diagnostic: git::diagnostic(stderr),
        });
    }
    let stdout = stdout.map_err(|_truncated| Why::OutputTooLarge {
        command: "cat-file",
    })?;
    Ok(stdout.to_vec())
}

/// Reads `path` from disk through [`resolve`], the walk the survey uses, no
/// further than one byte past `checkout`, and says whether it holds exactly
/// `checkout`.
///
/// A path that has stopped being a plain file at the claimed spelling since
/// the survey (gone, or a link, or a respelling above it) is
/// [`Why::ChangedSinceSurvey`]: the same statement `prove_one`'s own first
/// look makes.
fn disk_bytes_equal(work_tree: &WorkTree, path: &ClaimPath, checkout: &[u8]) -> Result<bool, Why> {
    match resolve(work_tree.root(), path, checkout.len()) {
        Ok(OnDisk::File(bytes)) => Ok(bytes == checkout),
        Err(UnsafePathCause::Unreadable { detail }) => Err(Why::Unreadable { detail }),
        Ok(OnDisk::Missing)
        | Err(
            UnsafePathCause::SpelledDifferently { .. }
            | UnsafePathCause::SymbolicLinkAbove { .. }
            | UnsafePathCause::NotADirectoryAbove { .. }
            | UnsafePathCause::Symlink
            | UnsafePathCause::NotAFile
            | UnsafePathCause::InsideGitDirectory
            | UnsafePathCause::UntrackableName { .. }
            | UnsafePathCause::InsideAnotherRepository { .. }
            | UnsafePathCause::NameTooLong { .. },
        ) => Err(Why::ChangedSinceSurvey),
    }
}

#[cfg(test)]
mod tests {
    use crate::claim::ClaimPath;
    use crate::sync::test_repository::{TestRepository, describe_difference};
    use crate::work_tree::abort::WorkTreeAbort;
    use crate::work_tree::clean::{Cleanliness, Dirt, check_clean};

    use crate::subprocess::Truncated;

    use super::{
        AboveEntry, Evidence, IndexMode, Proven, Why, classify_cat_file, prove,
        why_not_the_checkout, why_unusable,
    };
    use crate::work_tree::index_entry::HiddenFlag;
    use crate::work_tree::index_records::classify_ls_files;

    fn claim_path(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    fn write(path: &str, reason: crate::claim::DriftReason) -> super::Write {
        super::Write {
            path: claim_path(path),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
            rendered: b"whatever the render is, prove never reads it".to_vec(),
            reason,
        }
    }

    /// Runs `check_clean` (always `Clean` here — every fixture starts from
    /// a fresh commit) and then `prove`, returning the single write's own
    /// result: [`Evidence`] when proven, [`Why`] when refused.
    fn prove_one(
        repository: &TestRepository,
        path: &str,
        reason: crate::claim::DriftReason,
    ) -> Result<Evidence, Why> {
        let work_tree = repository.work_tree();
        let clean = match check_clean(&work_tree).expect("status must run") {
            crate::work_tree::clean::Cleanliness::Clean(clean) => clean,
            crate::work_tree::clean::Cleanliness::Dirty(dirty) => {
                panic!("fixture must be clean going into prove: {dirty:?}")
            }
        };
        let writes = vec![write(path, reason)];
        match prove(&work_tree, &clean, writes).expect("prove must run") {
            Proven::All(mut proven) => {
                let (_write, evidence, _fold_variants) = proven.remove(0).into_parts();
                Ok(evidence)
            }
            Proven::Refused(mut unproven) => Err(unproven.remove(0).why),
        }
    }

    #[test]
    fn an_absent_path_is_proven_absent() {
        let repository = TestRepository::new();
        repository.write(".gitkeep", b"");
        repository.commit_all("initial");
        let evidence = prove_one(
            &repository,
            "missing.txt",
            crate::claim::DriftReason::Missing,
        )
        .expect("an absent path must be proven");
        assert_eq!(evidence, Evidence::Absent);
    }

    // The positive control for the next two: nothing on disk and nothing in
    // the index is `Absent` (`an_absent_path_is_proven_absent`, above), so
    // what refuses below is the index entry alone, never the missing file.

    #[test]
    fn a_skip_worktree_path_absent_from_the_work_tree_is_tracked_but_absent() {
        // The scenario: commit `f.txt`, mark it skip-worktree, remove it.
        // `git status` reports nothing, `symlink_metadata` finds nothing,
        // and only the index still says the path is tracked — writing there
        // would create a file git ignores.
        let repository = TestRepository::new();
        repository.write("f.txt", b"committed\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "f.txt"])
            .run_ok();
        std::fs::remove_file(repository.path().join("f.txt")).expect("remove the file");

        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Missing)
            .expect_err("a skip-worktree path absent from disk must be refused");
        let Why::TrackedButAbsent { git_path } = why else {
            panic!("expected TrackedButAbsent, got {why:?}")
        };
        assert_eq!(git_path, "f.txt");
    }

    #[test]
    fn an_absent_path_with_a_tracked_entry_beneath_it_names_that_entry() {
        // A literal pathspec matches a directory prefix, so a claim whose
        // path is a directory git tracks files under is tracked-but-absent
        // too, and the message must name the entry git actually holds.
        let repository = TestRepository::new();
        repository.write("sub/inner.txt", b"committed\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "sub/inner.txt"])
            .run_ok();
        std::fs::remove_dir_all(repository.path().join("sub")).expect("remove the directory");

        let why = prove_one(&repository, "sub", crate::claim::DriftReason::Missing)
            .expect_err("a tracked entry beneath an absent path must be refused");
        let Why::TrackedButAbsent { git_path } = why else {
            panic!("expected TrackedButAbsent, got {why:?}")
        };
        assert_eq!(git_path, "sub/inner.txt");
    }

    // The next six share one setup: the index holds `hidden`, hidden from
    // the work tree by skip-worktree and removed from disk, so the only
    // witness that git tracks anything is the index itself.

    fn hide_committed(repository: &TestRepository, hidden: &str, ignorecase: &str) {
        repository.write(hidden, b"committed\n");
        repository.commit_all("initial");
        repository
            .git(&["config", "core.ignorecase", ignorecase])
            .run_ok();
        repository
            .git(&["update-index", "--skip-worktree", hidden])
            .run_ok();
        let on_disk = repository.path().join(hidden);
        std::fs::remove_file(&on_disk).expect("remove the file");
        let parent = on_disk.parent().expect("a joined path has a parent");
        if parent != repository.path() {
            std::fs::remove_dir(parent).expect("remove the emptied directory");
        }
    }

    #[test]
    fn an_absent_path_git_folds_onto_a_tracked_entry_is_tracked_but_absent() {
        // `core.ignorecase` makes git take `A.yml` for the hidden `a.yml`
        // and ignore what is written there, so the proof must refuse and
        // name git's own spelling. Set by hand so Linux proves git's fold
        // alone, apart from the filesystem's.
        let repository = TestRepository::new();
        hide_committed(&repository, "a.yml", "true");

        let why = prove_one(&repository, "A.yml", crate::claim::DriftReason::Missing)
            .expect_err("a case variant of a tracked entry must be refused");
        let Why::TrackedButAbsent { git_path } = why else {
            panic!("expected TrackedButAbsent, got {why:?}")
        };
        assert_eq!(git_path, "a.yml");
    }

    #[test]
    fn an_absent_path_under_a_directory_git_folds_onto_a_tracked_one_is_tracked_but_absent() {
        // The fold applies to every component: `d/f.yml` against the hidden
        // `D/f.yml`.
        let repository = TestRepository::new();
        hide_committed(&repository, "D/f.yml", "true");

        let why = prove_one(&repository, "d/f.yml", crate::claim::DriftReason::Missing)
            .expect_err("a directory case variant of a tracked entry must be refused");
        let Why::TrackedButAbsent { git_path } = why else {
            panic!("expected TrackedButAbsent, got {why:?}")
        };
        assert_eq!(git_path, "D/f.yml");
    }

    #[test]
    fn an_entry_beneath_a_directory_of_another_case_is_refused_naming_that_entry() {
        // The claim is the directory itself, one case away from the one git
        // tracks a file under.
        let repository = TestRepository::new();
        hide_committed(&repository, "Sub/in.yml", "true");

        let why = prove_one(&repository, "sub", crate::claim::DriftReason::Missing)
            .expect_err("a tracked entry beneath a case variant must be refused");
        let Why::TrackedButAbsent { git_path } = why else {
            panic!("expected TrackedButAbsent, got {why:?}")
        };
        assert_eq!(git_path, "Sub/in.yml");
    }

    #[test]
    fn the_case_fold_is_asked_of_git_whatever_core_ignorecase_says() {
        // Reading `core.ignorecase` would be one more git command, and the
        // refusal it would spare is of a state too rare to earn it, so the
        // question is unconditional: a case-sensitive git refuses the
        // variant too, and the message names the entry.
        let repository = TestRepository::new();
        hide_committed(&repository, "a.yml", "false");

        let why = prove_one(&repository, "A.yml", crate::claim::DriftReason::Missing)
            .expect_err("the case variant is refused whatever core.ignorecase says");
        assert!(
            matches!(why, Why::TrackedButAbsent { .. }),
            "expected TrackedButAbsent, got {why:?}"
        );
    }

    #[test]
    fn a_glob_character_in_an_absent_claim_stays_a_literal_character() {
        // Without the global `--literal-pathspecs` a pathspec is a glob
        // unless it says otherwise; `literal` must say so. `[ab].yml` would
        // match the hidden `a.yml` as a glob, and `*` would match anything.
        // The positive control below tracks a file really named `[ab].yml`,
        // which the very same pathspec must still find.
        let repository = TestRepository::new();
        hide_committed(&repository, "a.yml", "true");
        for claim in ["[ab].yml", "*", "?.yml"] {
            let evidence = prove_one(&repository, claim, crate::claim::DriftReason::Missing)
                .unwrap_or_else(|why| panic!("{claim} is not a glob, got {why:?}"));
            assert_eq!(evidence, Evidence::Absent, "{claim}");
        }

        let control = TestRepository::new();
        hide_committed(&control, "[ab].yml", "true");
        let why = prove_one(&control, "[ab].yml", crate::claim::DriftReason::Missing)
            .expect_err("a file really named [ab].yml is found by its own spelling");
        assert!(matches!(why, Why::TrackedButAbsent { .. }), "got {why:?}");
    }

    #[test]
    fn git_does_not_fold_beyond_ascii_so_a_unicode_case_variant_is_still_absent() {
        // The hazard is git ignoring what `sync` writes, and git's own fold
        // is ASCII only (captured), so `É.yml` beside a hidden `é.yml` is
        // visible to git and must not be refused. This is why the proof
        // asks git rather than folding by the crate's own rules.
        let repository = TestRepository::new();
        hide_committed(&repository, "é.yml", "true");

        let evidence = prove_one(&repository, "É.yml", crate::claim::DriftReason::Missing)
            .expect("git keeps a non-ASCII case variant visible, so it is absent");
        assert_eq!(evidence, Evidence::Absent);
    }

    /// One well-formed `ls-files --stage -z` record.
    const ONE_INDEX_RECORD: &[u8] = b"H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";

    /// Commits `.gitkeep`, then adds `git_path`, spelled as given, to the
    /// index as a tracked file hidden by skip-worktree and absent from disk.
    /// The name is added under `core.precomposeunicode=false` so a
    /// decomposed spelling stays decomposed whatever platform this runs on.
    fn hide_under_exact_spelling(repository: &TestRepository, git_path: &str) {
        repository.write(".gitkeep", b"");
        repository.commit_all("initial");
        repository.write("blob.tmp", b"tracked\n");
        let object = repository
            .git(&["hash-object", "-w", "blob.tmp"])
            .output_ok();
        std::fs::remove_file(repository.path().join("blob.tmp")).expect("remove the scratch file");
        for arguments in [
            vec![
                "-c",
                "core.precomposeunicode=false",
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{object},{git_path}"),
            ],
            vec![
                "-c",
                "core.precomposeunicode=false",
                "commit",
                "--quiet",
                "--message",
                "the entry",
            ],
            vec![
                "-c",
                "core.precomposeunicode=false",
                "update-index",
                "--skip-worktree",
                git_path,
            ],
        ] {
            repository.git(&arguments).run_ok();
        }
    }

    #[test]
    fn an_index_entry_hidden_under_another_spelling_is_attached_to_the_write_as_a_fold_variant() {
        // The claim is `café.yml` precomposed and git holds `café.yml`
        // decomposed, hidden. Git keeps them apart, so the claim is proven
        // absent; whether the filesystem takes them for one name is asked
        // later, when the staging file exists, so what `prove` owes is the
        // candidate, on the write it belongs to and on no other. The listing
        // is read from git as it is, so this holds on every platform.
        let repository = TestRepository::new();
        hide_under_exact_spelling(&repository, "cafe\u{301}.yml");
        let work_tree = repository.work_tree();
        let Cleanliness::Clean(clean) = check_clean(&work_tree).expect("status must run") else {
            panic!("the hidden entry leaves the tree clean")
        };
        let writes = vec![
            write("caf\u{e9}.yml", crate::claim::DriftReason::Missing),
            write("plain.yml", crate::claim::DriftReason::Missing),
        ];

        let Proven::All(proven) = prove(&work_tree, &clean, writes).expect("prove must run") else {
            panic!("git keeps the two spellings apart, so both writes are proven")
        };

        let variants: Vec<_> = proven
            .into_iter()
            .map(|proven_write| proven_write.into_parts().2)
            .collect();
        assert_eq!(variants.len(), 2);
        assert_eq!(
            variants[0].len(),
            1,
            "the precomposed claim has the variant"
        );
        assert_eq!(variants[0][0].git_path(), "cafe\u{301}.yml");
        assert!(
            variants[1].is_empty(),
            "plain.yml has none: {:?}",
            variants[1]
        );
    }

    #[test]
    fn an_ls_files_answer_that_cannot_be_used_is_worded_as_sync_words_it() {
        // Every ls-files answer, for a path on disk or not, is read through
        // `classify_ls_files`, and a truncated stream is the cap's refusal.
        // The control hands it the same well-formed record uncut, which must
        // parse, so the refusal is the truncation, never the bytes.
        let refused = classify_ls_files(true, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("a truncated ls-files stream must be refused");
        assert!(matches!(
            why_unusable(refused),
            Why::OutputTooLarge {
                command: "ls-files"
            }
        ));

        let records = classify_ls_files(true, Ok(ONE_INDEX_RECORD), b"")
            .expect("the same record, uncut, must parse");
        assert_eq!(records.len(), 1);

        let failed = classify_ls_files(false, Ok(b""), b"fatal: no\n")
            .expect_err("a failed ls-files must be refused");
        let Why::IndexEntryUnreadable { detail } = why_unusable(failed) else {
            panic!("a failed ls-files is an unreadable entry")
        };
        assert_eq!(detail, "no");
    }

    #[test]
    fn truncated_cat_file_stdout_reads_as_output_too_large() {
        // The same pairing for `cat-file`, whose whole stdout is the
        // checkout. The control hands it the same bytes uncut, which must
        // come back byte for byte, so the refusal is the truncation and
        // never the content.
        let refused = classify_cat_file(true, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("a truncated cat-file stream must be refused");
        assert!(matches!(
            refused,
            Why::OutputTooLarge {
                command: "cat-file"
            }
        ));

        let checkout = classify_cat_file(true, Ok(b"hello\n"), b"")
            .expect("the same bytes, uncut, must be the checkout");
        assert_eq!(checkout, b"hello\n");
    }

    #[test]
    fn a_successful_cat_file_is_its_stdout_whatever_the_bytes() {
        // The checkout is compared byte for byte with the disk, so nothing
        // may be parsed, trimmed or decoded on the way: bytes that are not
        // UTF-8, a NUL and an empty answer all come back exactly.
        for bytes in [&b""[..], b"\xff\xfe\x00 not text\n", b"no final newline"] {
            let checkout = classify_cat_file(true, Ok(bytes), b"")
                .expect("a successful cat-file is its own stdout");
            assert_eq!(checkout, bytes);
        }
    }

    #[test]
    fn a_failed_cat_file_is_a_checkout_failure_naming_gits_own_detail() {
        // A missing object, a required smudge failing and an LFS download
        // failing all exit non-zero, and all must refuse with git's own
        // words. Bytes on stdout beside the failure are never the checkout.
        let refused = classify_cat_file(
            false,
            Ok(b"partial bytes"),
            b"error: cannot read object 0000\n",
        )
        .expect_err("a non-zero exit must be refused");
        let Why::CheckoutFailed { diagnostic } = refused else {
            panic!("expected CheckoutFailed, got {refused:?}")
        };
        assert_eq!(diagnostic, "error: cannot read object 0000");
    }

    #[test]
    fn a_clean_tracked_file_is_held() {
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        let evidence = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("a clean tracked file must be held");
        let Evidence::Held(held) = evidence else {
            panic!("expected Held, got {evidence:?}")
        };
        assert_eq!(held.mode, IndexMode::Regular);
        assert_eq!(
            held.checkout.0, b"hello\n",
            "the evidence carries exactly what git would check out"
        );
    }

    #[test]
    fn an_absent_claim_a_rule_ignores_is_refused_naming_the_rule() {
        // Nothing is at the path and the index holds nothing for it, so the
        // only thing that can refuse it is git's ignore rule; a sibling no
        // rule matches, asked the same way, is the control.
        let repository = TestRepository::new();
        repository.write(".gitignore", b"*.local.yml\n");
        repository.commit_all("initial");

        let why = prove_one(
            &repository,
            "cfg/editor.local.yml",
            crate::claim::DriftReason::Missing,
        )
        .expect_err("a claim a rule ignores is refused");
        let Why::IgnoredByGit { rule } = why else {
            panic!("expected IgnoredByGit, got {why:?}")
        };
        assert_eq!(rule, ".gitignore:1:*.local.yml");

        let control = prove_one(
            &repository,
            "cfg/plain.yml",
            crate::claim::DriftReason::Missing,
        )
        .expect("a claim no rule ignores is absent");
        assert_eq!(control, Evidence::Absent);
    }

    #[test]
    fn a_tracked_file_a_rule_matches_is_held_and_never_asked_about_the_rule() {
        // Git does not ignore what it tracks: a file added with `-f` is
        // updated by `sync` whatever the rule says. A present file is never
        // asked the ignore question at all.
        let repository = TestRepository::new();
        repository.write(".gitignore", b"*.local.yml\n");
        repository.write("editor.local.yml", b"committed\n");
        repository
            .git(&["add", "-f", "--", "editor.local.yml", ".gitignore"])
            .run_ok();
        repository.commit_all("initial");

        let evidence = prove_one(
            &repository,
            "editor.local.yml",
            crate::claim::DriftReason::Changed,
        )
        .expect("a tracked file is held whatever a rule says");

        assert!(matches!(evidence, Evidence::Held(_)), "got {evidence:?}");
    }

    #[test]
    fn an_absent_claim_the_index_tracks_is_tracked_but_absent_not_ignored() {
        // A hidden entry that a rule also matches is refused for the entry
        // git holds, which is the fact a user acts on, and not for the rule.
        let repository = TestRepository::new();
        repository.write(".gitignore", b"*.local.yml\n");
        repository.write("hidden.local.yml", b"tracked\n");
        repository
            .git(&["add", "-f", "--", "hidden.local.yml", ".gitignore"])
            .run_ok();
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "hidden.local.yml"])
            .run_ok();
        std::fs::remove_file(repository.path().join("hidden.local.yml")).expect("remove it");

        let why = prove_one(
            &repository,
            "hidden.local.yml",
            crate::claim::DriftReason::Missing,
        )
        .expect_err("a hidden entry is refused");

        assert!(matches!(why, Why::TrackedButAbsent { .. }), "got {why:?}");
    }

    #[test]
    fn an_executable_clean_tracked_file_is_held_as_executable() {
        use std::os::unix::fs::PermissionsExt as _;

        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository
            .git(&["config", "core.fileMode", "true"])
            .run_ok();
        let path = repository.path().join("f.txt");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod +x");
        repository.commit_all("initial");
        let evidence = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("a clean executable file must be held");
        let Evidence::Held(held) = evidence else {
            panic!("expected Held, got {evidence:?}")
        };
        assert_eq!(held.mode, IndexMode::Executable);
    }

    #[test]
    fn an_assume_unchanged_edit_is_refused_as_hidden_from_the_work_tree() {
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--assume-unchanged", "f.txt"])
            .run_ok();
        repository.write("f.txt", b"edited\n");
        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("an assume-unchanged edit must be refused");
        assert!(matches!(
            why,
            Why::HiddenFromWorkTree {
                flag: HiddenFlag::AssumeUnchanged
            }
        ));
    }

    #[test]
    fn a_skip_worktree_edit_is_refused_as_hidden_from_the_work_tree() {
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "f.txt"])
            .run_ok();
        repository.write("f.txt", b"edited\n");
        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("a skip-worktree edit must be refused");
        assert!(matches!(
            why,
            Why::HiddenFromWorkTree {
                flag: HiddenFlag::SkipWorktree
            }
        ));
    }

    #[test]
    fn a_flagged_file_whose_bytes_equal_git_s_is_refused_for_each_flag() {
        // The sharp edge: nothing marks the file as edited, the disk holds
        // exactly what git holds, and only the flag says git will not read
        // it. The control is the same file with no flag, held
        // (`a_clean_tracked_file_is_held`).
        for (flags, expected) in [
            (vec!["--skip-worktree"], HiddenFlag::SkipWorktree),
            (vec!["--assume-unchanged"], HiddenFlag::AssumeUnchanged),
            (
                vec!["--skip-worktree", "--assume-unchanged"],
                HiddenFlag::Both,
            ),
        ] {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello\n");
            repository.commit_all("initial");
            // One flag per command: given both in one, `update-index` sets
            // only the last (captured: git 2.53.0).
            for flag in &flags {
                repository.git(&["update-index", flag, "f.txt"]).run_ok();
            }

            let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
                .expect_err("a flagged file must be refused");

            let Why::HiddenFromWorkTree { flag } = why else {
                panic!("{flags:?}: expected HiddenFromWorkTree, got {why:?}")
            };
            assert_eq!(flag, expected, "{flags:?}");
        }
    }

    #[test]
    fn a_flagged_file_is_refused_before_git_is_asked_what_it_would_check_out() {
        // A missing object makes `cat-file` fail with `CheckoutFailed`, so a
        // refusal that is `HiddenFromWorkTree` instead can only have come
        // before `cat-file` ran.
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        repository
            .git(&["update-index", "--skip-worktree", "f.txt"])
            .run_ok();
        let object = repository.git(&["rev-parse", ":f.txt"]).output_ok();
        let loose = repository
            .path()
            .join(".git/objects")
            .join(&object[..2])
            .join(&object[2..]);
        let mut permissions = std::fs::metadata(&loose)
            .expect("loose object")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o644);
        std::fs::set_permissions(&loose, permissions).expect("make it removable");
        std::fs::remove_file(&loose).expect("remove the loose object");

        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("a flagged file must be refused");

        assert!(matches!(why, Why::HiddenFromWorkTree { .. }), "got {why:?}");
    }

    #[test]
    fn an_ignored_file_is_not_in_the_index() {
        let repository = TestRepository::new();
        repository.write(".gitignore", b"ignored.txt\n");
        repository.commit_all("initial");
        repository.write("ignored.txt", b"ignored\n");
        let why = prove_one(
            &repository,
            "ignored.txt",
            crate::claim::DriftReason::Changed,
        )
        .expect_err("an ignored file must be refused");
        assert!(matches!(why, Why::NotInIndex));
    }

    #[test]
    fn a_file_inside_a_submodule_is_tracked_above_by_the_gitlink() {
        let source = TestRepository::new();
        source.write("inner.txt", b"inner\n");
        source.commit_all("source: initial");

        let repository = TestRepository::new();
        repository.write(".gitkeep", b"");
        repository.commit_all("initial");
        let source_url = format!("file://{}", source.path().display());
        repository
            .git(&[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "--quiet",
                &source_url,
                "sub",
            ])
            .run_ok();
        repository.commit_all("add submodule");

        let why = prove_one(
            &repository,
            "sub/inner.txt",
            crate::claim::DriftReason::Changed,
        )
        .expect_err("a file inside a submodule must be refused");
        // The directory above the path is asked about first, and it is a
        // gitlink: another repository, which is a better reason than that
        // this repository's index holds no entry for the path.
        let Why::TrackedAbove { git_path, entry } = why else {
            panic!("expected TrackedAbove, got {why:?}")
        };
        assert_eq!(git_path, "sub");
        assert_eq!(entry, AboveEntry::Submodule);
    }

    /// `SymbolicLinkInIndex` and `SubmoduleInIndex` are both refusals rule
    /// (b)'s own ls-files mode check produces for a *regular file on disk*
    /// whose index entry names mode 120000/160000. No real, clean git
    /// state ever reaches them: replacing a committed symlink or gitlink
    /// with a regular file — even one whose bytes happen to hash equal —
    /// is a type change (`git status --porcelain=v2` reports it `.T`), which
    /// `work_tree::clean`'s own parser reads as `Dirt::Unstaged` (tested by
    /// `crates/skeletons/src/work_tree/clean.rs` →
    /// `a_type_change_with_a_blank_index_column_is_read_as_unstaged`), so
    /// rule (a) always refuses first in a real run. These two are
    /// exercised by a hand-crafted index entry (`update-index --cacheinfo`)
    /// naming a mode disagreeing with what is really on disk, and
    /// [`CleanWorkTree::assume_clean_for_test`] standing in for rule (a)
    /// having already run.
    mod mode_checks_unreachable_from_a_clean_tree {
        use crate::sync::test_repository::TestRepository;
        use crate::work_tree::clean::CleanWorkTree;

        use super::super::{Proven, Why, prove};
        use super::write;

        fn hand_craft_mode(repository: &TestRepository, path: &str, mode: &str) -> String {
            let object = repository.git(&["hash-object", path]).output_ok();
            repository
                .git(&["rm", "--cached", "-q", "--", path])
                .run_ok();
            repository
                .git(&[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("{mode},{object},{path}"),
                ])
                .run_ok();
            object
        }

        #[test]
        fn a_regular_file_recorded_as_mode_120000_is_a_symbolic_link_in_the_index() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello");
            repository.commit_all("initial");
            hand_craft_mode(&repository, "f.txt", "120000");

            let work_tree = repository.work_tree();
            let clean = CleanWorkTree::assume_clean_for_test();
            let writes = vec![write("f.txt", crate::claim::DriftReason::Changed)];
            let Proven::Refused(mut unproven) =
                prove(&work_tree, &clean, writes).expect("prove must run")
            else {
                panic!("expected Refused")
            };
            assert!(matches!(unproven.remove(0).why, Why::SymbolicLinkInIndex));
        }

        #[test]
        fn a_regular_file_recorded_as_mode_160000_is_a_submodule_in_the_index() {
            let repository = TestRepository::new();
            repository.write("f.txt", b"hello");
            repository.commit_all("initial");
            hand_craft_mode(&repository, "f.txt", "160000");

            let work_tree = repository.work_tree();
            let clean = CleanWorkTree::assume_clean_for_test();
            let writes = vec![write("f.txt", crate::claim::DriftReason::Changed)];
            let Proven::Refused(mut unproven) =
                prove(&work_tree, &clean, writes).expect("prove must run")
            else {
                panic!("expected Refused")
            };
            assert!(matches!(unproven.remove(0).why, Why::SubmoduleInIndex));
        }
    }

    #[test]
    fn a_clean_file_under_a_clean_smudge_filter_is_held() {
        let repository = TestRepository::new();
        repository
            .git(&["config", "filter.upper.clean", "tr a-z A-Z"])
            .run_ok();
        repository
            .git(&["config", "filter.upper.smudge", "tr A-Z a-z"])
            .run_ok();
        repository.write(".gitattributes", b"f.txt filter=upper\n");
        repository.write("f.txt", b"lowercase\n");
        repository.commit_all("initial");
        repository.git(&["checkout", "--", "f.txt"]).run_ok();

        let evidence = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("a clean file under a filter must be held");
        assert!(matches!(evidence, Evidence::Held { .. }));
    }

    #[test]
    fn a_line_a_lossy_clean_filter_strips_is_not_what_git_checks_out() {
        // The scenario: a clean filter that deletes `local:` lines and a
        // smudge that is `cat`. The index holds `old`; the disk holds `old`
        // plus a line git never stored, staged so `status` reads clean. What
        // git would record for the disk (the clean side) is the committed
        // object, which is why proving that is not enough: the checkout is
        // `old`, and the extra line has no copy anywhere. Control: the
        // committed bytes themselves are held under the same filter.
        let repository = TestRepository::new();
        repository
            .git(&["config", "filter.strip.clean", "sed '/^local:/d'"])
            .run_ok();
        repository
            .git(&["config", "filter.strip.smudge", "cat"])
            .run_ok();
        repository.write(".gitattributes", b"f.txt filter=strip\n");
        repository.write("f.txt", b"old\n");
        repository.commit_all("initial");
        prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("the control: the committed bytes are what git checks out");

        repository.write("f.txt", b"old\nlocal: only copy\n");
        repository.git(&["add", "f.txt"]).run_ok();

        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("bytes git would not write back must be refused");
        assert!(matches!(why, Why::NotWhatGitChecksOut));
    }

    #[test]
    fn a_second_checkout_that_differs_from_the_first_is_not_reproducible() {
        // The two-run comparison as a pure function: what git checked out
        // the first time, and what it checked out when asked again.
        let first = b"one\n";

        assert!(matches!(
            why_not_the_checkout(first, Ok(b"one\n".to_vec())),
            Why::NotWhatGitChecksOut
        ));
        assert!(matches!(
            why_not_the_checkout(first, Ok(b"two\n".to_vec())),
            Why::CheckoutNotReproducible
        ));
        assert!(matches!(
            why_not_the_checkout(first, Ok(Vec::new())),
            Why::CheckoutNotReproducible
        ));
    }

    #[test]
    fn a_second_checkout_that_fails_is_that_failure() {
        let why = why_not_the_checkout(
            b"one\n",
            Err(Why::CheckoutFailed {
                diagnostic: "the filter failed the second time".to_owned(),
            }),
        );

        let Why::CheckoutFailed { diagnostic } = why else {
            panic!("expected CheckoutFailed, got {why:?}")
        };
        assert_eq!(diagnostic, "the filter failed the second time");
    }

    #[test]
    fn a_file_whose_checkout_differs_every_time_git_is_asked_is_not_reproducible() {
        // The scenario: a smudge filter that appends the process id, so
        // no two checkouts are alike. The clean side keeps the first line, so
        // `status` reads the tree clean whatever is on disk. The disk never
        // equals a checkout, and `git checkout` would write yet another
        // variant, which is why the refusal must not offer it as a remedy.
        // The control is the deterministic case in
        // `a_line_a_lossy_clean_filter_strips_is_not_what_git_checks_out`:
        // bytes git would write back are `NotWhatGitChecksOut`.
        let repository = TestRepository::new();
        repository
            .git(&["config", "filter.varying.clean", "sed 1q"])
            .run_ok();
        repository
            .git(&["config", "filter.varying.smudge", "sh -c 'cat; echo $$'"])
            .run_ok();
        repository.write(".gitattributes", b"f.txt filter=varying\n");
        repository.write("f.txt", b"committed: first line\n");
        repository.commit_all("initial");
        repository.git(&["checkout", "--", "f.txt"]).run_ok();

        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("a checkout that differs every time is never held");

        assert!(matches!(why, Why::CheckoutNotReproducible), "got {why:?}");
    }

    #[test]
    fn a_checked_out_ident_file_is_held_with_its_expanded_bytes() {
        // `ident` expands `$Id$` to `$Id: <blob> $` on checkout, a pure
        // function of the blob, so a file git wrote is what a second checkout
        // writes. The evidence carries the expanded bytes, not the blob's,
        // since those are what is on disk and what `commit` will compare.
        let repository = TestRepository::new();
        repository.write(".gitattributes", b"f.txt ident\n");
        repository.write("f.txt", b"x $Id$ y\n");
        repository.commit_all("initial");
        std::fs::remove_file(repository.path().join("f.txt")).expect("remove the file");
        repository.git(&["checkout", "--", "f.txt"]).run_ok();
        let on_disk = std::fs::read(repository.path().join("f.txt")).expect("read the file");
        assert!(
            on_disk.windows(5).any(|window| window == b"$Id: "),
            "premise: the checkout expanded `$Id$`"
        );

        let evidence = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("a checked-out ident file must be held");
        let Evidence::Held(held) = evidence else {
            panic!("expected Held, got {evidence:?}")
        };
        assert_eq!(held.checkout.0, on_disk);
    }

    #[test]
    fn a_missing_object_is_a_checkout_failure_not_a_held_file() {
        // `cat-file` exits non-zero for an object the store does not hold,
        // which must refuse with git's own words rather than compare
        // anything. The object is one `ls-files` reports but the store lost:
        // deleting the loose object after committing makes exactly that.
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        let object = repository.git(&["rev-parse", ":f.txt"]).output_ok();
        let loose = repository
            .path()
            .join(".git/objects")
            .join(&object[..2])
            .join(&object[2..]);
        let mut permissions = std::fs::metadata(&loose)
            .expect("loose object")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o644);
        std::fs::set_permissions(&loose, permissions).expect("make it removable");
        std::fs::remove_file(&loose).expect("remove the loose object");

        let why = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect_err("a missing object must be refused");
        assert!(
            matches!(why, Why::CheckoutFailed { .. }),
            "expected CheckoutFailed, got {why:?}"
        );
    }

    #[test]
    fn an_edited_file_under_a_clean_smudge_filter_is_dirty_by_rule_a() {
        // An ordinary tracked-file edit — unlike `--assume-unchanged` or
        // `--skip-worktree` — is exactly what `git status` itself already
        // calls modified, filter or no filter, so a real `sync` run refuses
        // at rule (a) and `prove` is never reached for this path. This
        // tests that refusal directly, at the `check_clean` seam.
        let repository = TestRepository::new();
        repository
            .git(&["config", "filter.upper.clean", "tr a-z A-Z"])
            .run_ok();
        repository
            .git(&["config", "filter.upper.smudge", "tr A-Z a-z"])
            .run_ok();
        repository.write(".gitattributes", b"f.txt filter=upper\n");
        repository.write("f.txt", b"lowercase\n");
        repository.commit_all("initial");
        repository.git(&["checkout", "--", "f.txt"]).run_ok();

        repository.write("f.txt", b"edited\n");
        let work_tree = repository.work_tree();
        let Cleanliness::Dirty(dirty) = check_clean(&work_tree).expect("status must run") else {
            panic!("an edited file under a filter must be dirty")
        };
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].dirt, Dirt::Unstaged);
    }

    #[test]
    fn a_required_filter_with_clean_unset_fails_the_whole_tree_status() {
        // A required clean filter with no `clean` command configured makes
        // `git status` itself fail (exit 128), for exactly the path that needs
        // it — not only the per-path proof. A real `sync` run therefore aborts
        // at rule (a), naming `status`, and never reaches `prove` for this path
        // at all.
        let repository = TestRepository::new();
        repository
            .git(&["config", "filter.broken.smudge", "cat"])
            .run_ok();
        repository
            .git(&["config", "filter.broken.required", "true"])
            .run_ok();
        repository.write("f.txt", b"content\n");
        repository.commit_all("initial");
        // `clean` for `broken` is deliberately never configured, and is
        // marked required — added *after* the commit, so the commit itself
        // (which would also run the missing filter) can succeed. Never
        // committed either: `.gitattributes` applies by its presence on
        // disk, not by being tracked, and `status` fails outright before
        // ever reporting the uncommitted attributes file itself.
        repository.write(".gitattributes", b"f.txt filter=broken\n");
        // Git runs a path's filter only when it has to compare content, and
        // it skips that whenever the file's stat data still matches the
        // index. Rewriting the same bytes changes the mtime, so the
        // comparison, and with it the failing filter, always happens:
        // without this, the outcome depends on whether the commit and the
        // check land in the same timestamp tick, and a loaded machine
        // reads the tree as merely dirty (an untracked `.gitattributes`).
        repository.write("f.txt", b"content\n");

        let work_tree = repository.work_tree();
        let error = check_clean(&work_tree).expect_err("a failing required filter must abort");
        assert!(matches!(
            error,
            WorkTreeAbort::GitFailed {
                command: "status",
                ..
            }
        ));
    }

    /// A snapshot of `.git/` (outside `lfs/objects/`) before and after
    /// `check_clean` and `prove` must be identical — the mechanism behind
    /// design.md's guarantee that none of the git commands `sync` runs
    /// writes git's own index, refs, config or object database, for the two
    /// read-only questions this module and its sibling `clean` ask.
    #[test]
    fn dot_git_is_byte_identical_before_and_after_check_clean_and_prove() {
        let repository = TestRepository::new();
        repository.write("f.txt", b"hello\n");
        repository.commit_all("initial");
        // Racily clean: touched after commit, so status has to actually
        // compare content rather than trust a cached stat match.
        repository.write("f.txt", b"hello\n");

        let before = repository.snapshot_dot_git();
        let _evidence = prove_one(&repository, "f.txt", crate::claim::DriftReason::Changed)
            .expect("a clean tracked file must be held");
        let after = repository.snapshot_dot_git();
        // Names the differing paths and their sizes rather than letting
        // `assert_eq!` print every file's bytes, which is enough output to
        // cut a CI log off before it reaches the path that differs.
        assert!(
            before == after,
            ".git/ must be unchanged by check_clean and prove:\n{}",
            describe_difference(&before, &after)
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn an_nfd_claim_whose_committed_entry_was_normalised_to_nfc_is_listed_as() {
        // The premise: this filesystem (and git's own `core.precomposeunicode`)
        // normalises an NFD name read from disk to NFC when recording it:
        // committing a file named with a decomposed accent records its
        // *index* entry as the precomposed spelling, even though the
        // working-tree name (and, in this test, the claim itself) stayed
        // decomposed. Checked at run time rather than assumed via
        // `cfg!(target_os = ...)`, as `contributing.md`'s "Platform-dependent
        // tests" section requires.
        let repository = TestRepository::new();
        let nfd_name = "cafe\u{0301}.txt"; // "café.txt", decomposed: the claim.
        let precomposed = "café.txt"; // The same name, precomposed.
        repository.write(nfd_name, b"content\n");
        repository.commit_all("initial");

        let recorded = repository
            .git(&["-c", "core.quotepath=false", "ls-files", "--", nfd_name])
            .output_ok();
        if recorded != precomposed {
            eprintln!(
                "skipped: this filesystem/git does not normalise an NFD name to NFC on commit \
                 (recorded as {recorded:?})"
            );
            return;
        }

        let why = prove_one(&repository, nfd_name, crate::claim::DriftReason::Changed)
            .expect_err("a claim listed under another spelling must be refused");
        let Why::ListedAs { git_path } = why else {
            panic!("expected ListedAs, got {why:?}")
        };
        assert_eq!(git_path, precomposed);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_case_variant_in_the_index_is_not_in_this_index_under_the_claimed_spelling() {
        // The premise: this filesystem folds case, so writing
        // `dependabot.yml` on disk and looking it up as `DEPENDABOT.YML`
        // (the claim's own spelling) both name the same on-disk entry —
        // Linux CI, which is case-sensitive, has no such entry to fold and
        // this test then has nothing to exercise.
        let repository = TestRepository::new();
        repository.write("dependabot.yml", b"content\n");
        repository.commit_all("initial");

        // Only "not found" means the filesystem keeps the spellings apart;
        // any other failure of the lookup fails the test.
        let folds_case = repository
            .path()
            .join("DEPENDABOT.YML")
            .try_exists()
            .expect("look up the committed file under the claim's spelling");
        if !folds_case {
            eprintln!("skipped: this filesystem does not fold case");
            return;
        }

        let why = prove_one(
            &repository,
            "DEPENDABOT.YML",
            crate::claim::DriftReason::Changed,
        )
        .expect_err("a case-variant claim must be refused");
        assert!(matches!(why, Why::NotInIndex));
    }
}
