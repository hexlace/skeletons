//! Asking the filesystem whether it takes an index entry for a file `sync`
//! has just created.
//!
//! An index entry hidden under another spelling of a claim
//! ([`crate::sync::fold_variant`]) only matters where the filesystem folds the
//! two spellings into one name. That is a fact about the directory being
//! written into, which no platform test can state and no configuration git
//! reads describes (`core.ignorecase` is git's measurement when the
//! repository was made, and blind to a case-folded directory on a filesystem
//! that is otherwise exact). So it is asked directly, once the file exists:
//! looking the entry's spelling up on disk either finds the very file `sync`
//! just created, and the two are one name, or finds nothing, and they are two.
//!
//! On a filesystem that keeps the spellings apart (ext4, tmpfs), the lookup
//! finds nothing and nothing is refused: a Unicode variant that git keeps
//! visible is not a hazard there: git lists the new file as untracked and
//! leaves the hidden entry alone. Where the filesystem folds (APFS), the
//! lookup finds the file, and the write is refused with nothing written.
//!
//! Tested against whatever filesystem the tests run on, by
//! `a_variant_the_filesystem_folds_is_refused_and_one_it_keeps_apart_is_not`,
//! below, which checks its own premise at run time rather than by platform.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::identity::FileIdentity;
use crate::claim::{ClaimPath, staging_name};
use crate::sync::fold_variant::{FoldRelation, FoldVariant};

/// Whether the filesystem under `root` takes `variant` for the file `sync` is
/// staging `claim` in, or for the directory above the claim it stands for.
///
/// For [`FoldRelation::SamePath`] the lookup is of the entry's spelling in
/// its staging-name form, `.<name>.skeletons-sync`, since that is the file that
/// exists, and its affixes are ASCII that folds as itself, so the
/// filesystem folds it exactly as it would fold the target's name: `staging`
/// is the identity of that file, read from its open handle. For
/// [`FoldRelation::DirectoryAbove`] it is the entry's own spelling against
/// the claim's directory, compared by identity.
///
/// `Ok(false)` is two names: nothing is at the spelling, or a directory above
/// it is not one there. Any other error is returned for the caller to refuse
/// on, since a lookup that failed for another reason says nothing about
/// whether the names are one.
///
/// # Errors
///
/// The operating system's own error for a lookup that failed for a reason
/// other than there being nothing at the path.
///
/// # Panics
///
/// If `variant` is not a fold variant of `claim`, which [`crate::sync::proof`]
/// pairs them for, and `fold_variants` asserts the same thing of what it returns.
pub(super) fn takes_for_one_name(
    root: &Path,
    claim: &ClaimPath,
    variant: &FoldVariant,
    staging: FileIdentity,
) -> std::io::Result<bool> {
    assert!(
        variant.relates_to(claim),
        "{variant:?} must be a fold variant of {claim}"
    );
    let (spelling, created) = match variant.relation() {
        FoldRelation::SamePath => (staging_spelling_of(root, variant.git_path()), Some(staging)),
        FoldRelation::DirectoryAbove { claimed } => {
            let directory = identity_at(&claimed.to_path(root))?;
            (path_of(root, variant.git_path()), directory)
        }
    };
    Ok(identity_at(&spelling)?.is_some_and(|found| Some(found) == created))
}

/// The identity of what is at `path`, without following a link, or `None`
/// when nothing is there, or a directory above it is not one: the two
/// answers that mean "two names".
fn identity_at(path: &Path) -> std::io::Result<Option<FileIdentity>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(FileIdentity::of(&metadata))),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// `git_path`, an index entry's workspace-relative path, under `root`.
fn path_of(root: &Path, git_path: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(git_path.split('/'));
    path
}

/// The staging file's path for an entry spelled `git_path`: the same
/// directory, and the last name wrapped as staging wraps it.
fn staging_spelling_of(root: &Path, git_path: &str) -> PathBuf {
    let (parent, name) = git_path
        .rsplit_once('/')
        .map_or((None, git_path), |(parent, name)| (Some(parent), name));
    let mut path = parent.map_or_else(|| root.to_path_buf(), |parent| path_of(root, parent));
    path.push(staging_name(name));
    path
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::{FileIdentity, takes_for_one_name};
    use crate::claim::ClaimPath;
    use crate::sync::fold_variant::{FoldVariant, fold_variants};

    fn claim(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// The one variant `entry` is of `claimed`, built the way production
    /// builds it: from a listing.
    fn variant(claimed: &ClaimPath, entry: &str) -> FoldVariant {
        let listing = format!("{entry}\0");
        let mut all = fold_variants(listing.as_bytes(), &[claimed]);
        let mut variants = all.remove(0);
        assert_eq!(variants.len(), 1, "{entry} must be a variant of {claimed}");
        variants.remove(0)
    }

    fn identity_of(path: &Path) -> FileIdentity {
        FileIdentity::of(&std::fs::symlink_metadata(path).expect("metadata"))
    }

    /// Whether looking `name` up under `root` finds something: how a test
    /// learns whether this filesystem folds a spelling onto one it has made,
    /// by asking it, never by platform. Only "not found" is a no; any other
    /// failure of the lookup is a fault in the test's scratch directory, and
    /// fails the test rather than reading as a filesystem that keeps the
    /// spellings apart.
    fn resolves(root: &Path, name: &str) -> bool {
        match std::fs::symlink_metadata(root.join(name)) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => panic!("looking up {name} failed, but not as absent: {error}"),
        }
    }

    /// Stages `.a.yml.skeletons-sync` under `root` and makes sure the entry
    /// spelling `.A.yml.skeletons-sync` finds it: a filesystem that folds case
    /// already does, and on one that does not a hard link stands in for the
    /// lookup that would have folded. Returns the staging file's identity.
    fn staged_where_the_variant_resolves(root: &Path) -> FileIdentity {
        let staged = root.join(".a.yml.skeletons-sync");
        std::fs::write(&staged, b"").expect("the staging file");
        if !resolves(root, ".A.yml.skeletons-sync") {
            std::fs::hard_link(&staged, root.join(".A.yml.skeletons-sync")).expect("the fold");
        }
        identity_of(&staged)
    }

    // The tests that need a spelling to find nothing (the controls) or to
    // find another file cannot make a case-folding filesystem say so, so
    // they check the premise first and say `skipped:` where it does not
    // hold. The tests that need it to find the file make it so, so both
    // outcomes are exercised on either kind of filesystem.

    #[test]
    fn a_spelling_that_resolves_to_the_staging_file_is_one_name_with_the_claim() {
        // The entry is `A.yml` and the claim `a.yml`; the file is staged as
        // `.a.yml.skeletons-sync`, and the entry's staging form finds it.
        let root = TempDir::new().expect("scratch directory");
        let staging = staged_where_the_variant_resolves(root.path());
        let claimed = claim("a.yml");

        let folds = takes_for_one_name(root.path(), &claimed, &variant(&claimed, "A.yml"), staging)
            .expect("the lookup works");

        assert!(folds);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_spelling_with_nothing_at_it_is_a_different_name() {
        // The control for the test above: the same staging file, and nothing
        // at the entry's spelling.
        let root = TempDir::new().expect("scratch directory");
        let staged = root.path().join(".a.yml.skeletons-sync");
        std::fs::write(&staged, b"").expect("the staging file");
        if resolves(root.path(), ".A.yml.skeletons-sync") {
            eprintln!(
                "skipped: this filesystem finds `.A.yml.skeletons-sync` for `.a.yml.skeletons-sync`"
            );
            return;
        }
        let claimed = claim("a.yml");

        let folds = takes_for_one_name(
            root.path(),
            &claimed,
            &variant(&claimed, "A.yml"),
            identity_of(&staged),
        )
        .expect("the lookup works");

        assert!(!folds);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_spelling_that_resolves_to_another_file_is_not_the_staging_file() {
        // Something else answers to the entry's staging name: a different
        // file, so a different identity, and the names are two.
        let root = TempDir::new().expect("scratch directory");
        let staged = root.path().join(".a.yml.skeletons-sync");
        std::fs::write(&staged, b"").expect("the staging file");
        if resolves(root.path(), ".A.yml.skeletons-sync") {
            eprintln!(
                "skipped: this filesystem finds `.A.yml.skeletons-sync` for `.a.yml.skeletons-sync`"
            );
            return;
        }
        std::fs::write(root.path().join(".A.yml.skeletons-sync"), b"another")
            .expect("another file");
        let claimed = claim("a.yml");

        let folds = takes_for_one_name(
            root.path(),
            &claimed,
            &variant(&claimed, "A.yml"),
            identity_of(&staged),
        )
        .expect("the lookup works");

        assert!(!folds);
    }

    #[test]
    fn a_file_above_that_resolves_to_the_claimed_directory_is_one_name_with_it() {
        // The claim is `A/b/x.yml` and git tracks `a/b`, a file, where the
        // claim needs the directory `A/b`. `a/b` resolves to the claimed
        // `A/b` where `a` and `A` are one directory: naturally on a
        // filesystem that folds case, and through a link `a` to `A` where it
        // does not (the link is what a folding lookup would have done).
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("A/b")).expect("the claimed directories");
        if !resolves(root.path(), "a") {
            std::os::unix::fs::symlink("A", root.path().join("a"))
                .expect("the link that stands in for the fold");
        }
        let claimed = claim("A/b/x.yml");
        let staging = identity_of(&root.path().join("A/b"));

        let folds = takes_for_one_name(root.path(), &claimed, &variant(&claimed, "a/b"), staging)
            .expect("the lookup works");

        assert!(folds);
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_file_above_that_resolves_to_nothing_is_a_different_name() {
        // The control for the test above: no `a` at all, so `a/b` is
        // nothing, not an error.
        let root = TempDir::new().expect("scratch directory");
        std::fs::create_dir_all(root.path().join("A/b")).expect("the claimed directories");
        if resolves(root.path(), "a") {
            eprintln!("skipped: this filesystem finds `a` for `A`");
            return;
        }
        let claimed = claim("A/b/x.yml");
        let staging = identity_of(&root.path().join("A/b"));

        let folds = takes_for_one_name(root.path(), &claimed, &variant(&claimed, "a/b"), staging)
            .expect("the lookup works");

        assert!(!folds);
    }

    #[test]
    fn a_lookup_that_fails_for_another_reason_is_an_error_not_two_names() {
        // A directory that is a link to itself makes every lookup through it
        // fail with a loop error, on every platform and for every user: that
        // says nothing about whether the spelling is the claim's name, so it
        // is neither `true` nor `false`. The claim and the entry are one name
        // (case apart), so the pairing holds.
        let root = TempDir::new().expect("scratch directory");
        let scratch = root.path().join("scratch");
        std::fs::write(&scratch, b"").expect("a file to take an identity from");
        std::os::unix::fs::symlink("d", root.path().join("d")).expect("a link to itself");
        let claimed = claim("d/A.yml");

        let outcome = takes_for_one_name(
            root.path(),
            &claimed,
            &variant(&claimed, "d/a.yml"),
            identity_of(&scratch),
        );

        assert!(outcome.is_err(), "got {outcome:?}");
    }

    #[test]
    #[should_panic(expected = "must be a fold variant of")]
    fn a_variant_paired_with_a_claim_it_does_not_belong_to_is_a_defect() {
        let root = TempDir::new().expect("scratch directory");
        let scratch = root.path().join("scratch");
        std::fs::write(&scratch, b"").expect("a file to take an identity from");
        let claimed = claim("a.yml");
        let another = claim("b.yml");

        let _outcome = takes_for_one_name(
            root.path(),
            &another,
            &variant(&claimed, "A.yml"),
            identity_of(&scratch),
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_variant_the_filesystem_folds_is_refused_and_one_it_keeps_apart_is_not() {
        // The real question, asked of the real filesystem: the claim is
        // `café.yml` precomposed and the index entry `café.yml` decomposed.
        // The premise is measured here rather than assumed from the platform:
        // whether this filesystem finds one spelling by looking up the other.
        // The answer must match it, in both directions: a folding filesystem
        // (APFS) is refused, and one that keeps them apart (ext4) is not.
        let root = TempDir::new().expect("scratch directory");
        let composed = "caf\u{e9}.yml";
        let decomposed = "cafe\u{301}.yml";
        let claimed = claim(composed);
        let staged = root.path().join(format!(".{composed}.skeletons-sync"));
        std::fs::write(&staged, b"").expect("the staging file");
        let filesystem_folds = resolves(root.path(), &format!(".{decomposed}.skeletons-sync"));
        eprintln!("this filesystem folds Unicode normalization: {filesystem_folds}");

        let folds = takes_for_one_name(
            root.path(),
            &claimed,
            &variant(&claimed, decomposed),
            identity_of(&staged),
        )
        .expect("the lookup works");

        assert_eq!(folds, filesystem_folds);
    }
}
