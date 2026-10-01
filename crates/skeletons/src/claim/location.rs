//! Resolving a claim path against the real filesystem, safely.
//!
//! # Threat model
//!
//! The walk below defends against a symbolic link committed into, or left
//! sitting in, the wearer's own working tree — a link at or above a claimed
//! path that would otherwise let a write land outside the workspace root.
//! Every component is classified from its own [`std::fs::symlink_metadata`],
//! never a followed link's, and a link anywhere in the path refuses the
//! whole claim rather than being resolved and continued past.
//!
//! It is **not** a defence against a concurrent process swapping a directory
//! component for a symlink in the instant between a walk and the step that
//! follows it, the classic time-of-check/time-of-use gap: nothing here can
//! close that, and neither can `sync`. What `sync` does is narrow it. It
//! re-walks each claim with this same function immediately before it writes
//! that claim (`sync::write::reverify`, tested by
//! `ritual/tests/sync_ancestor_swap.rs` →
//! `a_directory_swapped_for_a_link_over_a_different_outside_file_never_writes_through_it`),
//! so a swap made at any earlier moment is refused, and it walks again
//! before it removes anything it created, so a swap made before a removal
//! never has that removal follow the link
//! (`sync::write::staging`, tested by
//! `rolling_back_never_removes_a_file_through_a_directory_swapped_for_a_link`).
//! Content filters are the reason that matters: a repository's own filter
//! runs while `sync` proves later paths, and can swap a directory for a link
//! exactly as an editor or a formatter would. Every git process `sync` runs
//! has exited before its first re-walk, but a content filter can start a
//! process of its own that outlives git, and such a process, like any other
//! on the machine, can act at any moment: each walk catches what it did
//! before that walk, not after it. What is left is the instant between each
//! last walk and the step that follows it — creating, renaming, linking or
//! removing — across the whole time from the first staging file to the last
//! removal.
//!
//! This walk is only half of what keeps `sync`'s own writes inside the
//! workspace root and off a symbolic link: it is what a *claimed* path is
//! checked against, first when it is surveyed and again right before it is
//! written (tested by
//! `a_symlinked_intermediate_directory_is_refused_naming_it`, below), but
//! `sync` also creates paths of its own — a staging file beside its target,
//! and any missing ancestor directory. Those are created exclusively, not
//! walked: `sync::write::staging::Staging` uses `create_dir` and
//! `OpenOptions::create_new`, which fail on anything already there, a
//! dangling symbolic link included, so creating one can never follow one
//! either (tested by `crates/skeletons/src/sync/write.rs` →
//! `a_symlink_at_the_staging_path_is_refused_and_nothing_is_written_through_it`
//! and → `a_dangling_symlink_at_the_staging_path_is_refused`). Removing one
//! is walked: the ledger looks the path up with [`look_up`], and removes an
//! entry only when it is still the very file or directory it created. The
//! walk below and that exclusive creation are the two mechanisms together;
//! neither alone covers every path `sync` writes to.
//!
//! Nor may the walk lead into another repository. A directory below the
//! workspace root that holds a `.git` entry, a directory or a file, in any
//! ASCII case, is refused as [`UnsafePathCause::InsideAnotherRepository`]
//! whether that repository is a checked-out submodule, one this repository
//! ignores or a linked worktree: a file written there is that repository's
//! to track, and this repository's `git status` shows it as one changed or
//! untracked directory at most. The workspace root's own `.git` is this
//! repository's and is not looked at. An uninitialised submodule has no
//! `.git` on disk; the index question in `sync::proof` finds its gitlink.
//! Tested by `a_directory_holding_a_dotgit_directory_below_the_root_is_another_repository`.
//!
//! A second property this walk enforces is unrelated to symbolic links: a
//! claim component is accepted only when the directory it sits in lists an
//! entry spelled exactly as claimed, byte for byte, and lists no other entry
//! that is one name with it under [`FoldedName`], the fold the render and
//! overlap detection use. A lookup by name (`symlink_metadata`) can succeed
//! for a name that is only a case, or a Unicode normalisation, apart from
//! what is really on disk — the lookup itself folds the difference away —
//! on one filesystem and not on another. Two spellings of one name are one
//! file on a case-insensitive filesystem (macOS) and two files on a
//! case-sensitive one (Linux); accepting a folded lookup would therefore
//! give one answer on one machine and a different, data-destroying one on
//! the other, for the identical repository state. So the directory's own
//! listing is compared by that one fold, which names the same variants on
//! every platform, and the lookup stays as the backstop for anything a
//! filesystem folds beyond it. Refusing the mismatch, rather than guessing
//! which of the two files a wearer meant, is the only answer that reads the
//! same on both.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::{ClaimPath, is_git_directory_name};
use crate::skeleton::{FoldedName, ascii_names_collide};

/// What is really at a claim's path on disk.
#[derive(Debug)]
pub(crate) enum OnDisk {
    /// Nothing exists there yet (an intermediate directory absent, or the
    /// final component itself).
    Missing,
    /// A regular file, its bytes read up to the bound `resolve` was given.
    File(Vec<u8>),
}

/// Why a claim's path could not be safely resolved at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsafePathCause {
    /// A component is listed on disk under a different spelling than the
    /// claim gives it — a case difference, a Unicode normalisation
    /// difference or a fuller case fold difference, all named by the
    /// directory listing on every platform.
    SpelledDifferently {
        /// The claimed path up to and including the differing component,
        /// workspace-relative and `/`-separated. Equal to the claim's own
        /// full path when the differing component is the file itself.
        at: String,
        /// The on-disk spellings a directory listing actually found for the
        /// differing component, sorted, each a workspace-relative path (the
        /// claimed spelling for every verified ancestor, then the on-disk
        /// spelling of the differing component itself). Empty when no
        /// listed entry is one name with the claim under the fold the
        /// listing compares by — the filesystem's own lookup folded the
        /// difference away on its own, further than that fold reaches, and
        /// there is no on-disk spelling a listing can name.
        on_disk: Vec<String>,
        /// Whether the claimed spelling is itself also listed, alongside
        /// the differing one(s) in `on_disk` — only possible on a
        /// case-sensitive filesystem, where both spellings are two
        /// different files at once.
        claimed_spelling_present: bool,
    },
    /// An intermediate component is a symbolic link.
    SymbolicLinkAbove { at: String },
    /// An intermediate component exists but is not a directory.
    NotADirectoryAbove { at: String },
    /// The final component is a symbolic link.
    Symlink,
    /// The final component exists but is not a regular file.
    NotAFile,
    /// A `.git` component — refused at claim construction
    /// ([`super::ClaimPath::from_rendering_path`]), before any filesystem
    /// access; carried here too because it shares this crate's one
    /// `unsafe-path` refusal kind.
    InsideGitDirectory,
    /// A component git refuses to track: a spelling git reads as `.git`
    /// (`.git.`, `git~1`, `.g` with an ignorable code point inside) that is
    /// not `.git` itself. Refused at claim construction
    /// ([`super::ClaimPath::from_rendering_path`]), before any filesystem
    /// access, so `check` and `sync` agree on it; carried here too because it
    /// shares this crate's one `unsafe-path` refusal kind. `at` is the
    /// claimed path up to and including that component, workspace-relative.
    UntrackableName { at: String },
    /// A directory between the workspace root and the claim holds a `.git`
    /// entry of its own, a directory or a file (a submodule's, a linked
    /// worktree's), in any ASCII case. It is another git repository, so a
    /// file written there is never seen by this one's `git status`, and
    /// `git checkout` in either repository does not treat it as the same
    /// work. `at` is that directory, workspace-relative.
    InsideAnotherRepository { at: String },
    /// A component of the claimed path is too long for a file name, or the
    /// name `sync` stages the file under would be. Refused when the claim is
    /// built ([`super::ClaimPath::from_rendering_path`]), before any
    /// filesystem access, so `check` and `sync` agree on it; carried here too
    /// because it shares this crate's one `unsafe-path` refusal kind. `at` is
    /// the claimed path up to and including the component, workspace-relative,
    /// and `bytes` is the length of the name that does not fit: the
    /// component's own, or the staging name's.
    NameTooLong {
        at: String,
        bytes: usize,
        name: TooLongName,
    },
    /// Any other I/O failure walking to, or reading, the path.
    Unreadable { detail: String },
}

/// Which name is too long: the one the skeleton gives the file, or the one
/// `sync` writes it under first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TooLongName {
    /// A component of the claimed path itself, which no file could carry.
    Claimed,
    /// The claim's last component fits, but `.<name>.skeletons-sync`, the name
    /// `sync` stages it under, does not.
    Staging,
}

/// What a directory's own listing says about one component: whether the
/// claimed spelling is there byte for byte, and which other entries are one
/// name with it under [`FoldedName`], the fold the render and overlap
/// detection already use.
struct Listing {
    /// An entry spelled exactly like the claimed component exists.
    exact: bool,
    /// Every other entry whose [`FoldedName`] equals the claimed
    /// component's, but whose bytes differ from it — a case variant, a
    /// Unicode normalisation variant or a fuller case fold variant, on
    /// every platform alike — sorted.
    variants: Vec<String>,
    /// Some entry is a `.git` ([`is_git_directory_name`]), of any type: git
    /// finds a repository from a `.git` directory, a `.git` file and a link
    /// alike.
    holds_git_entry: bool,
}

/// Scans `directory`'s own listing for `component`. Streamed: each entry is
/// compared and dropped immediately, so memory is bounded by the number of
/// variants found — realistically zero or one — never by the size of the
/// directory.
///
/// An entry whose name is not valid UTF-8 is ignored: a [`ClaimPath`]
/// component is always valid UTF-8 (it comes only from a
/// [`crate::skeleton::Rendering`]'s own path), so such an entry can never be the
/// one a claim names, exactly or otherwise, and comparing it would need a
/// lossy conversion that could manufacture a false match. Linux is the only
/// platform this can happen on; APFS refuses to create such a name at all.
fn list_directory(directory: &Path, component: &str) -> std::io::Result<Listing> {
    scan(
        component,
        std::fs::read_dir(directory)?.map(|entry| entry.map(|entry| entry.file_name())),
    )
}

/// [`list_directory`]'s comparison, over any stream of entry names: the one
/// place an entry is classified against a component, kept apart from the
/// directory it usually comes from so the classification can be tested on
/// names alone.
///
/// Each entry is compared as cheaply as its own answer allows. The exact
/// spelling is one byte comparison. Two ASCII names are one name exactly when
/// they are equal apart from ASCII case, which needs no folding. Only an
/// entry that is not ASCII, or a component that is not, is folded, and the
/// component's own fold is computed once, not once per entry. A large
/// directory is mostly ASCII names beside an ASCII component, so it is mostly
/// byte comparisons.
fn scan(
    component: &str,
    names: impl Iterator<Item = std::io::Result<std::ffi::OsString>>,
) -> std::io::Result<Listing> {
    let component_folded = FoldedName::of(component);
    let mut exact = false;
    let mut holds_git_entry = false;
    let mut variants = Vec::new();
    for name in names {
        let name = name?;
        let Some(name) = name.to_str() else {
            continue;
        };
        holds_git_entry |= is_git_directory_name(name);
        if name == component {
            exact = true;
            continue;
        }
        let one_name = ascii_names_collide(name, component)
            .unwrap_or_else(|| FoldedName::of(name) == component_folded);
        if one_name {
            variants.push(name.to_owned());
        }
    }
    variants.sort();
    Ok(Listing {
        exact,
        variants,
        holds_git_entry,
    })
}

/// Whether `directory` lists an entry spelled exactly like `component` —
/// `sync::write::verify`'s own re-check of the property [`resolve`] already
/// enforced when it staged this same write: a write never lands under a
/// spelling the claim does not itself name. `resolve` is the check's producer,
/// run once per claim before anything is staged; this is its consumer, run once
/// per write after it has actually landed on disk (tested by
/// `crates/skeletons/src/sync/write.rs` →
/// `a_file_listed_under_another_spelling_is_reported`).
///
/// It never folds a name: the exact spelling is the only question, so it
/// compares bytes and returns at the first entry that matches, where
/// [`list_directory`] reads the whole listing to find every variant too.
pub(crate) fn lists_exact_spelling(directory: &Path, component: &str) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(directory)? {
        if entry?.file_name() == component {
            return Ok(true);
        }
    }
    Ok(false)
}

/// What [`std::fs::symlink_metadata`] answers for a candidate path,
/// classified so [`resolve`]'s own match can be written as one exhaustive
/// tuple instead of nested `if`/`else`.
enum Candidate {
    Found(std::fs::Metadata),
    Missing,
    Errored(std::io::Error),
}

fn stat_candidate(candidate: &Path) -> Candidate {
    match std::fs::symlink_metadata(candidate) {
        Ok(metadata) => Candidate::Found(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Candidate::Missing,
        Err(error) => Candidate::Errored(error),
    }
}

/// Joins `component` onto `parent`, the claimed-relative prefix already
/// walked — empty at the workspace root, so the first component stands
/// alone with no leading `/`.
fn join_claimed(parent: &str, component: &str) -> String {
    if parent.is_empty() {
        component.to_owned()
    } else {
        format!("{parent}/{component}")
    }
}

/// Builds [`UnsafePathCause::SpelledDifferently`] for `component`, sitting
/// under the already-verified claimed prefix `parent` — the one place `at`
/// and `on_disk` are assembled, so [`resolve`]'s two arms that construct it
/// (the listing witness and the lookup witness) cannot disagree about
/// either.
fn spelled_differently(parent: &str, component: &str, listing: &Listing) -> UnsafePathCause {
    let mut on_disk: Vec<String> = listing
        .variants
        .iter()
        .map(|variant| join_claimed(parent, variant))
        .collect();
    on_disk.sort();
    UnsafePathCause::SpelledDifferently {
        at: join_claimed(parent, component),
        on_disk,
        claimed_spelling_present: listing.exact,
    }
}

/// Classifies one component from both witnesses — `listing`, `directory`'s
/// own scan, and a lookup of the claimed spelling — and returns its real
/// metadata once the claimed spelling is confirmed exact and singular.
/// `Ok(None)` means the component does not exist at all, which `resolve`
/// reads as the whole claim being missing rather than a real classification
/// to continue past.
///
/// The listing is checked before the lookup's own classification (one
/// exhaustive match on a tuple, not nested `if`/`else`): on a case-insensitive
/// filesystem the lookup of the claimed spelling still reaches a
/// differently-spelled entry, and classifying that entry (as a symlink, a
/// directory, a file) would describe something the claim does not actually
/// name.
fn classify(
    candidate: &Path,
    claimed_prefix: &str,
    component: &str,
    listing: &Listing,
) -> Result<Option<std::fs::Metadata>, UnsafePathCause> {
    match (
        stat_candidate(candidate),
        listing.exact,
        listing.variants.is_empty(),
    ) {
        (_, _, false) => Err(spelled_differently(claimed_prefix, component, listing)),
        // The entry was found by the listing witness alone (`exact`) but
        // the lookup itself races and reports `NotFound`, or the component
        // is genuinely absent: either way, that is what is true now, so
        // this is the one race arm the module doc's threat model already
        // covers.
        (Candidate::Missing, _, true) => Ok(None),
        (Candidate::Errored(error), _, true) => Err(UnsafePathCause::Unreadable {
            detail: error.to_string(),
        }),
        // The lookup witness alone: the filesystem's own folding found
        // something, but nothing in the listing is one name with the claim
        // under `FoldedName`, so there is no on-disk spelling a listing
        // can name for it.
        (Candidate::Found(_), false, true) => {
            Err(spelled_differently(claimed_prefix, component, listing))
        }
        (Candidate::Found(metadata), true, true) => Ok(Some(metadata)),
    }
}

/// What [`look_up`] found at the end of a claim's path.
#[derive(Debug)]
pub(crate) enum Located {
    /// Nothing exists there (the final component, or a directory above it,
    /// is absent).
    Missing,
    /// Something exists there, described by its own
    /// [`std::fs::symlink_metadata`]: the walk has not judged what kind of
    /// entry it is, so it may be a file, a directory or a link.
    Entry(std::fs::Metadata),
}

/// Walks from `root` through every component of `path`, never following a
/// symbolic link and never accepting a component under any spelling but the
/// one claimed, and reports what is at the end of it, unjudged. The
/// symbolic-link refusal is tested by
/// `a_symlinked_intermediate_directory_is_refused_naming_it` and
/// `looking_up_a_symlinked_final_component_returns_it_unjudged`, below; the
/// spelling refusal by
/// `a_differently_spelled_final_component_with_the_claim_absent_is_refused_by_the_listing`
/// and `a_differently_spelled_intermediate_directory_is_refused_naming_the_ancestor`.
///
/// Each component is classified from two witnesses: `directory`'s own
/// listing (read once, streamed) and a lookup of the claimed spelling
/// (`symlink_metadata`). An entry in the listing that folds to the claimed
/// spelling but is not byte-equal to it — the listing witness — refuses the
/// claim as [`UnsafePathCause::SpelledDifferently`], whatever the lookup
/// says. Failing that, a lookup that finds something the listing does not
/// name under that exact spelling — the lookup witness — refuses it too:
/// the filesystem folded the difference away on its own, further than
/// [`FoldedName`] reaches (an HFS+ ignorable code point, say), and there
/// is no on-disk spelling a listing can name. Only when the listing
/// has the exact entry and no variant does the walk classify what the
/// lookup found and continue.
///
/// Every directory above the last component must be a real directory: a
/// link there is [`UnsafePathCause::SymbolicLinkAbove`], and anything else
/// that is not a directory is [`UnsafePathCause::NotADirectoryAbove`]. A
/// directory below `root` that lists a `.git` entry is another repository,
/// [`UnsafePathCause::InsideAnotherRepository`]; that is read from the
/// listing of every directory the walk goes through, the claim's own
/// directory included, and never from the workspace root's. The
/// last component is returned as it is, so [`resolve`] can refuse it for
/// what it reads, and `sync`'s removals can compare it with what `sync`
/// created.
pub(crate) fn look_up(root: &Path, path: &ClaimPath) -> Result<Located, UnsafePathCause> {
    let components: Vec<&str> = path.as_str().split('/').collect();
    // Postcondition of `ClaimPath::from_rendering_path`: at least one
    // component, none of them empty.
    assert!(
        !components.is_empty(),
        "a claim path always has at least one component"
    );

    let mut directory: PathBuf = root.to_path_buf();
    let mut claimed_prefix = String::new();

    for (index, component) in components.iter().enumerate() {
        let is_last = index + 1 == components.len();

        let listing =
            list_directory(&directory, component).map_err(|error| UnsafePathCause::Unreadable {
                detail: error.to_string(),
            })?;
        // The workspace root's own `.git` is this repository's. Every
        // directory below it that holds one is another repository, and
        // that is the finding itself, so it is decided before the component
        // is classified.
        if index > 0 && listing.holds_git_entry {
            return Err(UnsafePathCause::InsideAnotherRepository { at: claimed_prefix });
        }
        let candidate = directory.join(component);

        let Some(metadata) = classify(&candidate, &claimed_prefix, component, &listing)? else {
            return Ok(Located::Missing);
        };

        if is_last {
            return Ok(Located::Entry(metadata));
        }

        let component_path = join_claimed(&claimed_prefix, component);

        if metadata.is_symlink() {
            return Err(UnsafePathCause::SymbolicLinkAbove { at: component_path });
        }
        if !metadata.is_dir() {
            return Err(UnsafePathCause::NotADirectoryAbove { at: component_path });
        }

        claimed_prefix = component_path;
        directory = candidate;
    }

    unreachable!("the loop above always returns on its last iteration")
}

/// Resolves `path` with [`look_up`] and reads what is there: a regular final
/// component is read up to `compared_len + 1` bytes — enough to decide
/// equality against bytes of that length without ever reading more of a
/// mismatched, larger file than needed to prove it differs. `compared_len`
/// is the length the on-disk bytes are compared against. A symbolic link at
/// the end is refused as [`UnsafePathCause::Symlink`], and anything that is
/// not a regular file as [`UnsafePathCause::NotAFile`]. The symbolic-link
/// refusal is tested by `a_symlinked_final_component_is_refused`.
pub(crate) fn resolve(
    root: &Path,
    path: &ClaimPath,
    compared_len: usize,
) -> Result<OnDisk, UnsafePathCause> {
    match look_up(root, path)? {
        Located::Missing => Ok(OnDisk::Missing),
        Located::Entry(metadata) => {
            if metadata.is_symlink() {
                return Err(UnsafePathCause::Symlink);
            }
            if !metadata.is_file() {
                return Err(UnsafePathCause::NotAFile);
            }
            let bytes = read_at_most(&path.to_path(root), compared_len + 1).map_err(|error| {
                UnsafePathCause::Unreadable {
                    detail: error.to_string(),
                }
            })?;
            Ok(OnDisk::File(bytes))
        }
    }
}

/// Reads at most `cap` bytes of the file at `path`: a caller that wants to
/// see whether a file holds more than it should passes one byte past the
/// length it expects, without reading all of a file that grew far past it.
///
/// # Errors
///
/// Returns an error if `path` cannot be opened or read, or if `cap` itself
/// does not fit in a `u64` — infallible in practice, since every caller's
/// own `cap` is bounded by a render's own byte budget, far below `u64`'s
/// range, but `Read::take` takes a `u64` and a silent `as` cast could
/// truncate it, so the conversion is checked.
pub(crate) fn read_at_most(path: &Path, cap: usize) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let cap = u64::try_from(cap)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let mut buffer = Vec::new();
    file.take(cap).read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        ClaimPath, Located, OnDisk, UnsafePathCause, lists_exact_spelling, look_up, resolve,
    };
    use crate::skeleton::{slow_fold, tricky_name};

    fn claim(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    #[test]
    fn a_missing_file_at_the_root_resolves_to_missing() {
        let root = TempDir::new().expect("scratch directory");
        let result = resolve(root.path(), &claim("thing.txt"), 10).expect("no error");
        assert!(matches!(result, OnDisk::Missing));
    }

    #[test]
    fn a_missing_intermediate_directory_resolves_to_missing() {
        let root = TempDir::new().expect("scratch directory");
        let result = resolve(root.path(), &claim("a/b/thing.txt"), 10).expect("no error");
        assert!(matches!(result, OnDisk::Missing));
    }

    #[test]
    fn a_present_regular_file_is_read_back() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("thing.txt"), b"hello").expect("write fixture file");
        let result = resolve(root.path(), &claim("thing.txt"), 100).expect("no error");
        let OnDisk::File(bytes) = result else {
            panic!("expected a file")
        };
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn a_nested_present_file_is_read_back() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("a/b")).expect("nested directories");
        std::fs::write(root.path().join("a/b/thing.txt"), b"deep").expect("write fixture file");
        let result = resolve(root.path(), &claim("a/b/thing.txt"), 100).expect("no error");
        let OnDisk::File(bytes) = result else {
            panic!("expected a file")
        };
        assert_eq!(bytes, b"deep");
    }

    #[test]
    fn reading_stops_at_the_bound_for_a_longer_file() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("thing.txt"), b"0123456789").expect("write fixture file");
        let result = resolve(root.path(), &claim("thing.txt"), 3).expect("no error");
        let OnDisk::File(bytes) = result else {
            panic!("expected a file")
        };
        // Bound is `compared_len + 1`; asked for 3, so up to 4 bytes.
        assert_eq!(bytes, b"0123");
    }

    #[test]
    fn looking_up_a_file_returns_its_own_metadata() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir(root.path().join("a")).expect("directory");
        std::fs::write(root.path().join("a/thing.txt"), b"hello").expect("file");
        let Located::Entry(metadata) =
            look_up(root.path(), &claim("a/thing.txt")).expect("no error")
        else {
            panic!("expected an entry")
        };
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), 5);
    }

    #[test]
    fn looking_up_a_directory_returns_its_own_metadata() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir(root.path().join("a")).expect("directory");
        let Located::Entry(metadata) = look_up(root.path(), &claim("a")).expect("no error") else {
            panic!("expected an entry")
        };
        assert!(metadata.is_dir());
    }

    #[test]
    fn looking_up_a_symlinked_final_component_returns_it_unjudged() {
        let root = TempDir::new().expect("scratch directory");
        std::os::unix::fs::symlink(root.path().join("nowhere"), root.path().join("link"))
            .expect("create symlink");
        let Located::Entry(metadata) = look_up(root.path(), &claim("link")).expect("no error")
        else {
            panic!("expected an entry")
        };
        assert!(metadata.is_symlink());
    }

    #[test]
    fn a_directory_holding_a_dotgit_directory_below_the_root_is_another_repository() {
        // A nested repository: `nested/.git` is a directory. The claim is
        // in `nested`, which is the repository's own work tree.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("nested/.git")).expect("a nested repository");

        let error = look_up(root.path(), &claim("nested/x.yml"))
            .expect_err("a claim inside another repository is refused");

        assert_eq!(
            error,
            UnsafePathCause::InsideAnotherRepository {
                at: "nested".to_owned()
            }
        );
    }

    #[test]
    fn a_dotgit_file_a_dotgit_link_and_any_ascii_case_all_count() {
        // A submodule and a linked worktree hold a `.git` file, a link is
        // followed by git too, and `.GIT` is the spelling the claim rule
        // already treats as `.git`. Each is its own directory so one
        // refusal cannot stand in for another.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir(root.path().join("as-file")).expect("directory");
        std::fs::write(root.path().join("as-file/.git"), b"gitdir: elsewhere\n").expect("file");
        std::fs::create_dir(root.path().join("as-link")).expect("directory");
        std::os::unix::fs::symlink(
            root.path().join("nowhere"),
            root.path().join("as-link/.git"),
        )
        .expect("link");
        std::fs::create_dir_all(root.path().join("as-upper/.GIT")).expect("directory");

        for directory in ["as-file", "as-link", "as-upper"] {
            let error = look_up(root.path(), &claim(&format!("{directory}/x.yml")))
                .expect_err("a `.git` entry of any type makes it another repository");
            assert_eq!(
                error,
                UnsafePathCause::InsideAnotherRepository {
                    at: directory.to_owned()
                },
                "{directory}"
            );
        }
    }

    #[test]
    fn the_workspace_roots_own_dotgit_is_this_repository_and_is_not_refused() {
        // The positive control for the two above: the same `.git`, at the
        // root, where it is this repository's own.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir(root.path().join(".git")).expect("this repository's own");
        std::fs::create_dir(root.path().join("a")).expect("directory");

        assert!(matches!(
            look_up(root.path(), &claim("a/x.yml")).expect("no error"),
            Located::Missing
        ));
        assert!(matches!(
            look_up(root.path(), &claim("x.yml")).expect("no error"),
            Located::Missing
        ));
    }

    #[test]
    fn a_directory_beside_a_repository_and_a_name_like_dotgit_are_not_another_repository() {
        // `.github` and `.gitignore` only start like it, and `beside` sits
        // next to the nested repository rather than in it.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("nested/.git")).expect("a nested repository");
        std::fs::create_dir_all(root.path().join("beside/.github")).expect("directory");
        std::fs::write(root.path().join("beside/.gitignore"), b"x\n").expect("file");

        assert!(matches!(
            look_up(root.path(), &claim("beside/x.yml")).expect("no error"),
            Located::Missing
        ));
    }

    #[test]
    fn a_repository_two_directories_above_the_claim_is_named_at_its_own_directory() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("a/b/c")).expect("directories");
        std::fs::create_dir(root.path().join("a/b/.git")).expect("a nested repository");

        let error = look_up(root.path(), &claim("a/b/c/x.yml")).expect_err("refused");

        assert_eq!(
            error,
            UnsafePathCause::InsideAnotherRepository {
                at: "a/b".to_owned()
            }
        );
    }

    #[test]
    fn looking_up_a_missing_path_is_missing() {
        let root = TempDir::new().expect("scratch directory");
        assert!(matches!(
            look_up(root.path(), &claim("a/b")).expect("no error"),
            Located::Missing
        ));
    }

    #[test]
    fn looking_up_through_a_symlinked_directory_is_refused_naming_it() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir(root.path().join("real")).expect("directory");
        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("x"))
            .expect("create symlink");
        let error = look_up(root.path(), &claim("x/y")).expect_err("a link above is refused");
        assert_eq!(
            error,
            UnsafePathCause::SymbolicLinkAbove { at: "x".to_owned() }
        );
    }

    #[test]
    fn a_symlinked_final_component_is_refused() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("target.txt"), b"x").expect("link target");
        std::os::unix::fs::symlink(root.path().join("target.txt"), root.path().join("link.txt"))
            .expect("create symlink");
        let error =
            resolve(root.path(), &claim("link.txt"), 10).expect_err("a symlink must be refused");
        assert!(matches!(error, UnsafePathCause::Symlink));
    }

    #[test]
    fn a_symlinked_intermediate_directory_is_refused_naming_it() {
        let root = TempDir::new().expect("scratch directory");
        let real = root.path().join("real");
        std::fs::create_dir_all(&real).expect("real directory");
        std::os::unix::fs::symlink(&real, root.path().join("linked")).expect("create symlink");
        let error = resolve(root.path(), &claim("linked/thing.txt"), 10)
            .expect_err("a symlinked intermediate directory must be refused");
        assert_eq!(
            error,
            UnsafePathCause::SymbolicLinkAbove {
                at: "linked".to_owned()
            }
        );
    }

    #[test]
    fn a_plain_file_where_a_directory_was_expected_is_refused_naming_it() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("notadir"), b"x").expect("plain file");
        let error = resolve(root.path(), &claim("notadir/thing.txt"), 10)
            .expect_err("a non-directory intermediate component must be refused");
        assert_eq!(
            error,
            UnsafePathCause::NotADirectoryAbove {
                at: "notadir".to_owned()
            }
        );
    }

    #[test]
    fn a_directory_where_a_file_was_expected_is_refused_as_not_a_file() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("a-directory")).expect("directory fixture");
        let error = resolve(root.path(), &claim("a-directory"), 10)
            .expect_err("a directory at the final component must be refused");
        assert!(matches!(error, UnsafePathCause::NotAFile));
    }

    #[test]
    fn a_differently_spelled_final_component_with_the_claim_absent_is_refused_by_the_listing() {
        // The listing witness: the directory lists only `DEPENDABOT.YML`,
        // an entry whose `FoldedName` equals the claim's but whose bytes do
        // not. This must refuse on every filesystem, since a directory
        // listing is never folded — it is what makes Linux refuse rather
        // than reading the claim as missing and creating a second file.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("DEPENDABOT.YML"), b"x").expect("on-disk file");
        let error = resolve(root.path(), &claim("dependabot.yml"), 10)
            .expect_err("a case-only spelling must be refused");
        assert_eq!(
            error,
            UnsafePathCause::SpelledDifferently {
                at: "dependabot.yml".to_owned(),
                on_disk: vec!["DEPENDABOT.YML".to_owned()],
                claimed_spelling_present: false,
            }
        );
    }

    #[test]
    fn a_differently_spelled_intermediate_directory_is_refused_naming_the_ancestor() {
        // The same listing witness, one level up: the byte-exact file sits
        // under `.GitHub`, not `.github`. Refused even though the file's
        // own bytes, once found, would match the render exactly — `resolve`
        // never reaches that far, because the ancestor itself already
        // fails.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join(".GitHub")).expect("on-disk directory");
        std::fs::write(root.path().join(".GitHub/dependabot.yml"), b"x").expect("file inside it");
        let error = resolve(root.path(), &claim(".github/dependabot.yml"), 10)
            .expect_err("a case-only ancestor must be refused");
        assert_eq!(
            error,
            UnsafePathCause::SpelledDifferently {
                at: ".github".to_owned(),
                on_disk: vec![".GitHub".to_owned()],
                claimed_spelling_present: false,
            }
        );
    }

    #[test]
    fn both_spellings_present_is_refused_naming_the_claimed_one_too() {
        // Probes whether this scratch directory folds case at all before
        // asserting anything: a case-insensitive filesystem (a default macOS
        // volume, for one) cannot hold both
        // spellings of one name at once, so the case this test is really
        // about — a claim's own spelling coexisting on disk with a
        // differently-spelled entry — is observed only on a case-sensitive
        // filesystem, which Linux CI runs on by default.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("probe"), b"x").expect("case-sensitivity probe file");
        let case_sensitive = !root
            .path()
            .join("PROBE")
            .try_exists()
            .expect("look up the probe under its other spelling");
        std::fs::remove_file(root.path().join("probe")).expect("remove probe file");

        std::fs::write(root.path().join("dependabot.yml"), b"claimed").expect("claimed spelling");

        if case_sensitive {
            std::fs::write(root.path().join("DEPENDABOT.YML"), b"other spelling")
                .expect("the other spelling, representable only on a case-sensitive filesystem");
            let error = resolve(root.path(), &claim("dependabot.yml"), 10)
                .expect_err("a second spelling alongside the claimed one must be refused");
            assert_eq!(
                error,
                UnsafePathCause::SpelledDifferently {
                    at: "dependabot.yml".to_owned(),
                    on_disk: vec!["DEPENDABOT.YML".to_owned()],
                    claimed_spelling_present: true,
                }
            );
        } else {
            // This scratch directory folds case, so the second spelling
            // cannot be created as a distinct entry at all — proving that,
            // rather than asserting nothing, is this branch's own positive
            // control: it is why the case above is Linux-only.
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.path().join("DEPENDABOT.YML"));
            assert!(
                result.is_err(),
                "on a case-insensitive filesystem, creating a second spelling of an existing \
                 name must fail, since the two names are one entry"
            );
        }
    }

    #[test]
    fn an_nfd_entry_beside_an_nfc_claim_is_named_on_every_platform() {
        // "café.txt" decomposed (NFD): the combining acute accent as its
        // own code point, distinct in bytes from the precomposed (NFC)
        // "café.txt" the claim asks for, though a person reads them
        // identically. The listing compares by `FoldedName`, so it names
        // the NFD entry whether or not the filesystem's own lookup would
        // also have found it (APFS does, ext4 does not), and both
        // platforms give the one answer.
        let root = TempDir::new().expect("scratch directory");
        let nfd_name = "cafe\u{0301}.txt";
        std::fs::write(root.path().join(nfd_name), b"x").expect("nfd-named file");

        let error = resolve(root.path(), &claim("café.txt"), 10)
            .expect_err("must be refused, not read as missing or as the wrong file");

        assert_eq!(
            error,
            UnsafePathCause::SpelledDifferently {
                at: "café.txt".to_owned(),
                on_disk: vec![nfd_name.to_owned()],
                claimed_spelling_present: false,
            }
        );
    }

    #[test]
    fn a_fuller_case_fold_variant_is_named_on_every_platform() {
        // `ß` folds to `ss` under full case folding, which a plain
        // lowercase comparison does not reach and neither APFS's nor
        // ext4's lookup applies. The listing's fold names it everywhere.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("straße.txt"), b"x").expect("eszett-named file");

        let error = resolve(root.path(), &claim("STRASSE.txt"), 10)
            .expect_err("a full-case-fold variant must be refused");

        assert_eq!(
            error,
            UnsafePathCause::SpelledDifferently {
                at: "STRASSE.txt".to_owned(),
                on_disk: vec!["straße.txt".to_owned()],
                claimed_spelling_present: false,
            }
        );
    }

    #[test]
    fn a_symlinked_directory_nested_two_levels_deep_names_the_full_path() {
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("a")).expect("intermediate directory");
        let real = root.path().join("elsewhere");
        std::fs::create_dir_all(&real).expect("link target");
        std::os::unix::fs::symlink(&real, root.path().join("a/b")).expect("create symlink");

        let error = resolve(root.path(), &claim("a/b/deep.yml"), 10)
            .expect_err("a symlinked directory two levels down must still be refused");
        assert_eq!(
            error,
            UnsafePathCause::SymbolicLinkAbove {
                at: "a/b".to_owned()
            }
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_directory_that_cannot_be_read_is_unreadable() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("scratch directory");
        let locked = root.path().join("locked");
        std::fs::create_dir_all(&locked).expect("directory to lock down");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("remove all permissions");

        // A process that bypasses permission checks — root, as in many CI
        // containers — can still list a mode-000 directory, so there is no
        // unreadable directory to resolve through. The premise is checked
        // rather than assumed: when it does not hold, this test has nothing
        // to exercise and says so on stderr instead of failing for a reason
        // that has nothing to do with `resolve`. Only a refusal on
        // permission establishes the premise; any other failure to list the
        // directory is a fault in the scratch directory and fails the test.
        match std::fs::read_dir(&locked) {
            Ok(_) => {
                std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
                    .expect("restore permissions so the temp directory can be cleaned up");
                eprintln!("skipped: this process can read a mode-000 directory");
                return;
            }
            Err(error) => assert_eq!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied,
                "listing the mode-000 directory failed, but not on permission: {error}"
            ),
        }

        let result = resolve(root.path(), &claim("locked/thing.txt"), 10);

        // Restored before any assertion can panic and leave a directory
        // `TempDir`'s own `Drop` cannot remove.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("restore permissions so the temp directory can be cleaned up");

        assert!(matches!(result, Err(UnsafePathCause::Unreadable { .. })));
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_non_utf8_entry_in_the_listing_is_ignored() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = TempDir::new().expect("scratch directory");
        // Not valid UTF-8: a lone continuation byte. The premise — this
        // filesystem accepts such a name at all — is checked at run time
        // rather than assumed from the platform: APFS (macOS) refuses to
        // create one outright, so this case is exercised on Linux (ext4
        // and most Linux filesystems) but not skipped by `cfg`.
        let invalid_name = OsStr::from_bytes(b"\xffinvalid");
        if let Err(error) = std::fs::write(root.path().join(invalid_name), b"unrelated") {
            // The refusal has to be of this name, not of writing here at all:
            // the same write under a UTF-8 name succeeds, or the failure is
            // the scratch directory's own and fails the test.
            std::fs::write(root.path().join("utf8-control"), b"unrelated")
                .expect("a UTF-8 name writes where the non-UTF-8 one was refused");
            eprintln!("skipped: this filesystem refuses a non-UTF-8 name ({error})");
            return;
        }
        std::fs::write(root.path().join("thing.txt"), b"hello").expect("the claimed file");

        let result = resolve(root.path(), &claim("thing.txt"), 10)
            .expect("must resolve past the non-utf8 entry");
        let OnDisk::File(bytes) = result else {
            panic!("expected a file")
        };
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn lists_exact_spelling_is_true_only_for_the_exact_entry() {
        // sync::write::verify's own paired assertion reads exactly this
        // function; it is tested directly here, once, rather than only
        // through verify's own test, since the two consumers should not
        // have to agree by coincidence.
        let root = TempDir::new().expect("scratch directory");
        std::fs::write(root.path().join("thing.txt"), b"x").expect("on-disk file");

        assert!(
            lists_exact_spelling(root.path(), "thing.txt").expect("directory must be readable"),
            "the claimed spelling is listed exactly"
        );
        assert!(
            !lists_exact_spelling(root.path(), "THING.TXT").expect("directory must be readable"),
            "no entry is spelled THING.TXT"
        );
    }

    /// The listing comparison with no ASCII shortcut: every entry that is
    /// not the exact spelling is folded in full and compared with the
    /// component's fold. The oracle `scan` is held to.
    fn scan_by_folding_everything(component: &str, names: &[String]) -> (bool, Vec<String>) {
        let component_folded = slow_fold(component);
        let mut exact = false;
        let mut variants = Vec::new();
        for name in names {
            if name == component {
                exact = true;
            } else if slow_fold(name) == component_folded {
                variants.push(name.clone());
            }
        }
        variants.sort();
        (exact, variants)
    }

    proptest::proptest! {
        #[test]
        fn the_listing_scan_gives_the_answers_folding_every_entry_gives(
            component in tricky_name(1..6),
            names in proptest::collection::vec(tricky_name(1..6), 0..10),
        ) {
            // Property: comparing ASCII names without folding them, and
            // folding the component once, changes no answer. Over listings
            // that mix ASCII with the characters that fold onto it, the scan
            // finds an exact entry exactly when `scan_by_folding_everything`
            // does, and the same variants.
            let listing = super::scan(
                &component,
                names.iter().map(|name| Ok(std::ffi::OsString::from(name))),
            )
            .expect("names cannot fail to read");
            let (exact, variants) = scan_by_folding_everything(&component, &names);

            proptest::prop_assert_eq!(listing.exact, exact);
            proptest::prop_assert_eq!(listing.variants, variants);
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

        #[test]
        fn listing_the_exact_spelling_finds_it_exactly_when_the_directory_lists_it(
            component in tricky_name(1..6),
            names in proptest::collection::vec(tricky_name(1..6), 0..8),
        ) {
            // Property: over a real directory holding whichever of these
            // names the filesystem keeps apart, `lists_exact_spelling`
            // answers what the directory's own listing says about that
            // spelling: its bytes are among the entries or they are not, and
            // no variant stands in for it. The entries are read back from the
            // directory, since a filesystem that folds keeps fewer names than
            // were written.
            let root = TempDir::new().expect("scratch directory");
            // `.` and `..` are directories every listing has, not files to make.
            for name in names.iter().filter(|name| !name.chars().all(|c| c == '.')) {
                std::fs::write(root.path().join(name), b"x").expect("a file to list");
            }
            let listed: Vec<String> = std::fs::read_dir(root.path())
                .expect("the directory lists")
                .map(|entry| {
                    entry
                        .expect("an entry")
                        .file_name()
                        .into_string()
                        .expect("every name written was UTF-8")
                })
                .collect();

            let found = lists_exact_spelling(root.path(), &component).expect("readable");

            proptest::prop_assert_eq!(found, listed.contains(&component));
        }
    }
}
