//! A file a worn skeleton claims: its rendered bytes, who claims it, and the
//! workspace-relative path it should live at.

mod drift;
mod location;
mod overlap;
mod untrackable;

use std::path::{Path, PathBuf};

pub(crate) use drift::{Drift, DriftReason, compare};
pub(crate) use location::{
    Located, OnDisk, TooLongName, UnsafePathCause, lists_exact_spelling, look_up, read_at_most,
    resolve,
};
pub(crate) use overlap::{
    FoldedRelation, Overlap, find_overlaps, folded_components, folded_relation,
    proper_folded_prefixes,
};

/// The suffix every staging file carries: `.<file-name>.skeletons-sync`, beside
/// its target.
const STAGING_SUFFIX: &str = ".skeletons-sync";

/// The most bytes a file name can hold: 255 on ext4, and the one bound APFS
/// (255 UTF-16 code units, which is never more than the UTF-8 byte count)
/// honours in its own terms as well. A constant rather than the filesystem's
/// own answer (`pathconf`), which is not in the standard library, differs per
/// directory, and would make `check` answer differently on different
/// machines.
pub(crate) const NAME_BYTES_MAX: usize = 255;

/// What `.<file-name>.skeletons-sync` adds to a file name: the leading dot and
/// [`STAGING_SUFFIX`], 16 bytes. Computed from the two, so the limit on a
/// claim's last component cannot drift from the name staging builds.
const STAGING_AFFIX_BYTES: usize = ".".len() + STAGING_SUFFIX.len();

/// The longest last component a claim may have, so that the name `sync`
/// stages it under still fits: [`NAME_BYTES_MAX`] less [`STAGING_AFFIX_BYTES`],
/// which is 239.
const CLAIMED_NAME_BYTES_MAX: usize = NAME_BYTES_MAX - STAGING_AFFIX_BYTES;

/// Whether `name` is a `.git` entry, in any ASCII case: the one rule that
/// decides both which claim names are a write into git's own directory
/// ([`ClaimPath::from_rendering_path`]) and what marks a directory as another
/// repository's own ([`look_up`]), so the two cannot disagree about which
/// spellings count. It is only the exact name: the other spellings git reads
/// as `.git` cannot hold a repository, and a claim at one is refused as a
/// different cause.
pub(crate) fn is_git_directory_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git")
}

/// The name a staging file carries beside a target named `file_name`:
/// `.<file_name>.skeletons-sync`. The one place that wrapping is written, so a
/// path built to ask whether the filesystem takes a spelling for a staging
/// name (`sync::write`'s fold probe) wraps it exactly as staging does.
pub(crate) fn staging_name(file_name: &str) -> String {
    format!(".{file_name}{STAGING_SUFFIX}")
}

/// A validated path relative to the workspace root: built only from a
/// [`crate::skeleton::Rendering`] path, so its components are always non-empty
/// real names, `/`-joined, and never `..`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ClaimPath(String);

impl ClaimPath {
    /// Builds a claim path from one of a rendering's own paths, refusing it
    /// outright when it holds a name git refuses to track, or one no file
    /// could have or `sync` could stage. Decided here, from the name alone,
    /// so `check` and `sync` give the same answer on every machine and before
    /// anything is created.
    ///
    /// A component is refused when it is `.git` in any ASCII case
    /// ([`UnsafePathCause::InsideGitDirectory`]: writing there would be a git
    /// write, and `sync` never makes one), or any other spelling git reads
    /// as `.git` ([`UnsafePathCause::UntrackableName`]): `.git.`, `git~1`, a
    /// name with a space, period, colon or backslash after either, or with
    /// one of a set of invisible code points anywhere in it. Git refuses
    /// every one of them with `invalid path`, so a file at such a name could
    /// be written and never committed. The set is git's own, from its source
    /// (v2.53.0), and is written out in the `untrackable` module.
    ///
    /// A path is also refused for a component of more than
    /// [`NAME_BYTES_MAX`] bytes, or a last component whose staging name
    /// (`.<name>.skeletons-sync`) would be
    /// ([`UnsafePathCause::NameTooLong`]). The name check comes first, so a
    /// path that is both is refused as the name it is.
    ///
    /// # Panics
    ///
    /// If `path` is empty, holds an empty component, or holds a `..`
    /// component — every one of these is a `Rendering` invariant that a
    /// render change breaking would be a defect in `skeleton::render`, not
    /// something a wearer's own repository could ever cause.
    pub(crate) fn from_rendering_path(path: &str) -> Result<Self, UnsafePathCause> {
        assert!(!path.is_empty(), "a Rendering path is never empty");
        refuse_a_name_git_will_not_track(path)?;
        refuse_a_name_too_long(path)?;
        Ok(Self(path.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The staging path this claim is written at before it lands: the same
    /// directory, and the file name wrapped as `.<file-name>.skeletons-sync`.
    /// `a/b.yml` stages at `a/.b.yml.skeletons-sync`, and `b.yml` at
    /// `.b.yml.skeletons-sync`.
    ///
    /// It is a [`ClaimPath`] like any other, so the walk that vets a claimed
    /// path ([`look_up`]) can vet the path of something `sync` created, and
    /// `sync` never has to build a staging name out of a string.
    ///
    /// # Panics
    ///
    /// If the name it built is not a dotted `.skeletons-sync` name, or is
    /// `.git` — neither can happen for a claim path, whose last component is
    /// never empty and never `.git`. A staged name may read as one of the other
    /// names git refuses to track, as `.git:x.yml.skeletons-sync` does for the
    /// claim `git:x.yml`; a staging file is never tracked, so that harms
    /// nothing.
    pub(crate) fn staging(&self) -> Self {
        let staged = match self.0.rsplit_once('/') {
            Some((parent, file_name)) => format!("{parent}/{}", staging_name(file_name)),
            None => staging_name(&self.0),
        };
        let last = staged.rsplit('/').next().unwrap_or(&staged);
        assert!(
            last.starts_with('.'),
            "a staging name starts with a dot: {staged:?}"
        );
        assert!(
            last.ends_with(STAGING_SUFFIX),
            "a staging name ends with {STAGING_SUFFIX}: {staged:?}"
        );
        assert!(
            !is_git_directory_name(last),
            "a staging name is never `.git`: {staged:?}"
        );
        assert!(
            last.len() <= NAME_BYTES_MAX,
            "a staging name fits in a file name, which `from_rendering_path` guarantees for \
             every claim: {} bytes in {staged:?}",
            last.len()
        );
        Self(staged)
    }

    /// The directory this path sits in, or `None` for a path at the
    /// workspace root: `a/b/c.yml` is in `a/b`, and `c.yml` in no directory.
    pub(crate) fn parent(&self) -> Option<Self> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _name)| Self(parent.to_owned()))
    }

    /// How many components this path has: `a/b/c.yml` is 3.
    pub(crate) fn depth(&self) -> usize {
        self.0.split('/').count()
    }

    /// Every proper prefix of this path, root to leaf: the directories above
    /// it. `a/b/c.yml` gives `a` and `a/b`; a path at the workspace root
    /// gives none.
    ///
    /// # Panics
    ///
    /// If it does not hold one prefix per component above the last, which
    /// only a defect in this function could cause.
    pub(crate) fn ancestors(&self) -> Vec<Self> {
        let above = self.depth() - 1;
        let mut ancestors = Vec::with_capacity(above);
        let mut end = 0;
        for component in self.0.split('/').take(above) {
            end += component.len();
            ancestors.push(Self(self.0[..end].to_owned()));
            // The `/` between this component and the next.
            end += 1;
        }
        assert_eq!(
            ancestors.len(),
            above,
            "one ancestor for every component above the last"
        );
        ancestors
    }

    /// This claim's own path, joined onto `root` as a real filesystem path —
    /// the same join [`resolve`] walks component by component while
    /// checking for symlinks. Used by `sync::write` to name the target it
    /// prepares and renames into, once that walk has already vetted every
    /// claim `sync` goes on to write.
    pub(crate) fn to_path(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for component in self.0.split('/') {
            path.push(component);
        }
        path
    }
}

/// Refuses `path` when a component of it is a name git refuses to track,
/// naming the first such component. `.git` in any ASCII case is
/// [`UnsafePathCause::InsideGitDirectory`], and every other spelling git
/// reads as `.git` is [`UnsafePathCause::UntrackableName`]; see
/// [`ClaimPath::from_rendering_path`].
fn refuse_a_name_git_will_not_track(path: &str) -> Result<(), UnsafePathCause> {
    let mut end = 0;
    for component in path.split('/') {
        assert!(
            !component.is_empty(),
            "a Rendering path component is never empty"
        );
        assert_ne!(component, "..", "a Rendering path component is never `..`");
        end += component.len();
        if is_git_directory_name(component) {
            return Err(UnsafePathCause::InsideGitDirectory);
        }
        if untrackable::is_untrackable_name(component) {
            return Err(UnsafePathCause::UntrackableName {
                at: path[..end].to_owned(),
            });
        }
        // The `/` between this component and the next.
        end += 1;
    }
    Ok(())
}

/// Refuses `path` when a component of it is longer than a file name can be,
/// or its last component is too long for `sync` to stage; see
/// [`ClaimPath::from_rendering_path`].
fn refuse_a_name_too_long(path: &str) -> Result<(), UnsafePathCause> {
    let mut end = 0;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        end += component.len();
        let bytes = component.len();
        if bytes > NAME_BYTES_MAX {
            return Err(UnsafePathCause::NameTooLong {
                at: path[..end].to_owned(),
                bytes,
                name: TooLongName::Claimed,
            });
        }
        // The staging name only wraps the last component.
        if components.peek().is_none() && bytes > CLAIMED_NAME_BYTES_MAX {
            return Err(UnsafePathCause::NameTooLong {
                at: path[..end].to_owned(),
                bytes: bytes + STAGING_AFFIX_BYTES,
                name: TooLongName::Staging,
            });
        }
        // The `/` between this component and the next.
        end += 1;
    }
    Ok(())
}

impl std::fmt::Display for ClaimPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Which worn dependency claims a file — everything a message needs to name
/// it: `<dependency> in <manifest> (<skeleton> <version>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Claimant {
    pub(crate) manifest: String,
    pub(crate) dependency: String,
    pub(crate) skeleton: String,
    pub(crate) version: semver::Version,
}

/// One claimed file: where it should live, who claims it, and the bytes it
/// should hold.
pub(crate) struct Claim {
    pub(crate) path: ClaimPath,
    pub(crate) claimant: Claimant,
    pub(crate) rendered: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        ClaimPath, NAME_BYTES_MAX, TooLongName, UnsafePathCause, is_git_directory_name,
        staging_name,
    };

    #[test]
    fn a_plain_rendering_path_becomes_a_claim_path_unchanged() {
        let path = ClaimPath::from_rendering_path(".github/dependabot.yml")
            .expect("a plain path must not be refused");
        assert_eq!(path.as_str(), ".github/dependabot.yml");
    }

    #[test]
    fn a_dotgit_component_is_refused_case_insensitively() {
        for path in [".git/hooks/pre-commit", "a/.GIT/b", "a/.Git"] {
            let error = ClaimPath::from_rendering_path(path)
                .expect_err("a `.git` component must be refused");
            assert!(matches!(error, UnsafePathCause::InsideGitDirectory));
        }
    }

    #[test]
    fn a_git_directory_name_is_dotgit_in_any_ascii_case_and_nothing_else() {
        // The one predicate the claim rule and the nested-repository walk
        // share. The negative rows are the near misses: `.github` and
        // `.gitignore` start with it, `git` and `.gi` are part of it, and a
        // trailing space or dot is a different name to every supported
        // filesystem.
        for name in [".git", ".GIT", ".Git", ".gIt"] {
            assert!(is_git_directory_name(name), "{name:?}");
        }
        for name in [
            ".github",
            ".gitignore",
            "git",
            ".gi",
            ".git ",
            ".git.",
            "",
            "_git",
        ] {
            assert!(!is_git_directory_name(name), "{name:?}");
        }
    }

    #[test]
    fn a_staging_name_is_built_for_a_claim_git_accepts_even_when_git_would_refuse_it() {
        // `git:x.yml` and `git\x.yml` are names git tracks, and stage as
        // `.git:x.yml.skeletons-sync` and `.git\x.yml.skeletons-sync`, which begin
        // `.git` and then a colon or a backslash. A staging file is never
        // tracked, so building that name must not panic.
        for (claim, staged) in [
            ("git:x.yml", ".git:x.yml.skeletons-sync"),
            ("git\\x.yml", ".git\\x.yml.skeletons-sync"),
            ("a/git:x.yml", "a/.git:x.yml.skeletons-sync"),
        ] {
            let path = ClaimPath::from_rendering_path(claim)
                .expect("git tracks this name, so it is a valid claim");
            assert_eq!(path.staging().as_str(), staged, "{claim:?}");
        }
    }

    #[test]
    fn no_name_is_both_a_git_directory_name_and_an_untrackable_name() {
        // One owner per name: the exact `.git` is a git directory name and
        // not an untrackable one, and every other spelling git refuses is the
        // reverse, so neither answer depends on which is asked first.
        for name in [".git", ".GIT", ".Git"] {
            assert!(is_git_directory_name(name), "{name:?}");
            assert!(!super::untrackable::is_untrackable_name(name), "{name:?}");
        }
        for name in [".git.", "git~1", ".git:x", ".g\u{200c}it", "a\\.git"] {
            assert!(!is_git_directory_name(name), "{name:?}");
            assert!(super::untrackable::is_untrackable_name(name), "{name:?}");
        }
    }

    /// What `from_rendering_path` refuses `path` as, expecting a refusal.
    fn refusal_of(path: &str) -> UnsafePathCause {
        ClaimPath::from_rendering_path(path)
            .expect_err(&format!("{path:?} is a name git refuses to track"))
    }

    /// Asserts `path` is refused as an untrackable name, and that the refusal
    /// points at `at`, the claimed path through the offending component.
    fn assert_untrackable(path: &str, at: &str) {
        assert_eq!(
            refusal_of(path),
            UnsafePathCause::UntrackableName { at: at.to_owned() },
            "{path:?}"
        );
    }

    #[test]
    fn dotgit_followed_by_a_period_is_untrackable() {
        // `.git.` is the same name to git as `.git`.
        assert_untrackable(".git.", ".git.");
    }

    #[test]
    fn dotgit_followed_by_a_space_is_untrackable() {
        assert_untrackable(".git ", ".git ");
    }

    #[test]
    fn dotgit_followed_by_a_run_of_spaces_and_periods_is_untrackable() {
        assert_untrackable(".git . .", ".git . .");
        assert_untrackable(".GIT...   ..", ".GIT...   ..");
    }

    #[test]
    fn git_tilde_one_is_untrackable_in_any_ascii_case() {
        for name in ["git~1", "GIT~1", "Git~1", "gIT~1"] {
            assert_untrackable(name, name);
        }
    }

    #[test]
    fn git_tilde_one_followed_by_spaces_and_periods_is_untrackable() {
        assert_untrackable("git~1 .", "git~1 .");
        assert_untrackable("git~1.", "git~1.");
    }

    #[test]
    fn dotgit_followed_by_a_colon_and_a_stream_name_is_untrackable() {
        assert_untrackable(".git::$INDEX_ALLOCATION", ".git::$INDEX_ALLOCATION");
        assert_untrackable(".git:x", ".git:x");
        assert_untrackable("git~1:x", "git~1:x");
        assert_untrackable(".git. :x", ".git. :x");
    }

    #[test]
    fn dotgit_followed_by_a_backslash_is_untrackable() {
        assert_untrackable(".git\\config", ".git\\config");
        assert_untrackable("git~1\\config", "git~1\\config");
        assert_untrackable(".git\\", ".git\\");
    }

    #[test]
    fn dotgit_after_a_backslash_is_untrackable() {
        assert_untrackable("sub\\.git", "sub\\.git");
        assert_untrackable("sub\\git~1", "sub\\git~1");
        assert_untrackable("a\\b\\.git.", "a\\b\\.git.");
        assert_untrackable("\\\\.git", "\\\\.git");
    }

    /// The sixteen code points git skips when it reads a name as `.git`.
    const IGNORABLE: [char; 16] = [
        '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
        '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}', '\u{feff}',
    ];

    #[test]
    fn dotgit_with_an_ignorable_code_point_inside_it_is_untrackable() {
        for character in IGNORABLE {
            let name = format!(".g{character}it");
            assert_untrackable(&name, &name);
        }
    }

    #[test]
    fn dotgit_with_an_ignorable_code_point_before_the_dot_is_untrackable() {
        assert_untrackable("\u{200d}.git", "\u{200d}.git");
    }

    #[test]
    fn dotgit_with_an_ignorable_code_point_between_the_dot_and_the_g_is_untrackable() {
        assert_untrackable(".\u{202e}git", ".\u{202e}git");
    }

    #[test]
    fn dotgit_with_an_ignorable_code_point_after_the_t_is_untrackable() {
        assert_untrackable(".git\u{feff}", ".git\u{feff}");
    }

    #[test]
    fn dotgit_in_capitals_with_ignorable_code_points_is_untrackable() {
        assert_untrackable(".G\u{200c}IT", ".G\u{200c}IT");
        assert_untrackable(
            "\u{200c}.G\u{200d}i\u{200e}T\u{200f}",
            "\u{200c}.G\u{200d}i\u{200e}T\u{200f}",
        );
    }

    #[test]
    fn a_directory_that_is_untrackable_is_refused_at_that_directory() {
        assert_untrackable(".git./hooks/pre-commit.yml", ".git.");
        assert_untrackable("git~1/config.yml", "git~1");
        assert_untrackable("a/b/.g\u{200c}it/c.yml", "a/b/.g\u{200c}it");
    }

    #[test]
    fn the_first_untrackable_component_is_the_one_named() {
        assert_untrackable("a/git~1/.git./x.yml", "a/git~1");
    }

    #[test]
    fn an_untrackable_name_is_refused_before_a_length_is_read() {
        assert_untrackable(&format!(".git./{}", file_of_bytes(300)), ".git.");
    }

    #[test]
    fn the_exact_dotgit_spelling_keeps_its_own_cause() {
        // Writing at `.git` is a write into git's own directory, which is a
        // different fact from a name git merely refuses to track.
        for path in [".git", ".GIT", "a/.Git/b.yml"] {
            assert_eq!(refusal_of(path), UnsafePathCause::InsideGitDirectory);
        }
    }

    #[test]
    fn names_git_tracks_are_accepted() {
        // Every one of these is a name git adds without complaint: it looks
        // like a refused name and is not one.
        let accepted = [
            ".github",
            ".gitignore",
            ".gitattributes",
            "git",
            "a.git",
            "gitx",
            ".git-blame-ignore-revs",
            "git~2",
            "git~10",
            "git~1x",
            "git~",
            ".git~1",
            "..git",
            ".gitx",
            "a:b",
            "x\\y",
            ".g it",
            ".gi t",
            ".gi",
            "\u{200c}",
            ".git.\u{200c}",
            ".git\u{200c}x",
            "git\u{200c}~1",
            "x\\.gitx",
            "x\\git~2",
            // A backslash that is a component's first character is not a
            // separator to git: it consumes that character before it looks
            // for one.
            "\\.git",
            "\\git~1",
        ];
        for name in accepted {
            let path = ClaimPath::from_rendering_path(name)
                .unwrap_or_else(|cause| panic!("{name:?} must be accepted, was {cause:?}"));
            assert_eq!(path.as_str(), name);
        }
    }

    /// One name git spells `.git`: `.git` in mixed ASCII case, with zero or
    /// more of git's ignorable code points before the dot, after each letter,
    /// and (because each slot is independent) anywhere in it.
    fn ignorable_dotgit() -> impl Strategy<Value = (String, bool)> {
        let slot = proptest::collection::vec(proptest::sample::select(IGNORABLE.to_vec()), 0..3);
        (
            proptest::collection::vec(slot, 5),
            proptest::collection::vec(proptest::bool::ANY, 3),
        )
            .prop_map(|(slots, uppers)| {
                let letters: Vec<char> = ['g', 'i', 't']
                    .into_iter()
                    .zip(&uppers)
                    .map(|(letter, upper)| {
                        if *upper {
                            letter.to_ascii_uppercase()
                        } else {
                            letter
                        }
                    })
                    .collect();
                let pieces = ['.', letters[0], letters[1], letters[2]];
                let mut name = String::new();
                let mut any_ignorable = false;
                for (index, slot) in slots.iter().enumerate() {
                    for character in slot {
                        name.push(*character);
                        any_ignorable = true;
                    }
                    if let Some(piece) = pieces.get(index) {
                        name.push(*piece);
                    }
                }
                (name, any_ignorable)
            })
    }

    proptest! {
        // Property: every spelling of `.git` made of ASCII case and git's
        // ignorable code points is refused, as the exact-name cause when
        // nothing ignorable is in it and as an untrackable name otherwise.
        // The input space is the whole product: five slots (before the dot,
        // and after each of the four letters) of up to two of the sixteen
        // code points each, and three independent letter cases.
        #[test]
        fn every_ignorable_spelling_of_dotgit_is_refused(
            (name, any_ignorable) in ignorable_dotgit(),
        ) {
            let refusal = ClaimPath::from_rendering_path(&name)
                .expect_err("a spelling of `.git` must be refused");
            if any_ignorable {
                prop_assert_eq!(refusal, UnsafePathCause::UntrackableName { at: name });
            } else {
                prop_assert_eq!(refusal, UnsafePathCause::InsideGitDirectory);
            }
        }

        // Property: `.git` followed by any run of spaces and periods, then
        // optionally a colon and anything after it, is refused; `git~1` the
        // same. The run is where the two families share a tail.
        #[test]
        fn a_run_of_spaces_and_periods_after_the_name_is_refused(
            head in proptest::sample::select(vec![".git", ".GIT", "git~1", "GiT~1"]),
            run in "[ .]{1,6}",
            stream in proptest::option::of("[a-zA-Z$_:]{0,8}"),
        ) {
            let name = stream.map_or_else(
                || format!("{head}{run}"),
                |stream| format!("{head}{run}:{stream}"),
            );
            prop_assert_eq!(refusal_of(&name), UnsafePathCause::UntrackableName { at: name });
        }

        // Property: a name that has any ordinary letter or digit before
        // `.git`, or after it, is not `.git` in any of git's readings and is
        // accepted. The alphabet holds no space, period, colon, backslash,
        // tilde or ignorable code point, which are the only things git's
        // readings skip or end a name at.
        #[test]
        fn a_name_with_an_ordinary_letter_around_dotgit_is_accepted(
            before in "[a-z0-9_-]{0,4}",
            after in "[a-z0-9_-]{0,4}",
        ) {
            prop_assume!(!before.is_empty() || !after.is_empty());
            let name = format!("{before}.git{after}");
            let path = ClaimPath::from_rendering_path(&name).expect("git tracks this name");
            prop_assert_eq!(path.as_str(), name.as_str());
        }
    }

    #[test]
    fn a_path_component_that_merely_contains_git_is_not_refused() {
        let path = ClaimPath::from_rendering_path("gitignore/thing.txt")
            .expect("a component that only contains `git` as a substring is not `.git` itself");
        assert_eq!(path.as_str(), "gitignore/thing.txt");
    }

    /// A last component of exactly `bytes` bytes ending in `.yml`, in ASCII.
    fn file_of_bytes(bytes: usize) -> String {
        format!("{}.yml", "n".repeat(bytes - ".yml".len()))
    }

    #[test]
    fn a_last_component_of_239_bytes_is_the_longest_that_can_be_staged() {
        // `.` + 239 + `.skeletons-sync` is exactly 255 bytes, the most a file name
        // holds, so 239 is accepted and stages, and 240 is refused because
        // its staging name is 256.
        let fits = ClaimPath::from_rendering_path(&file_of_bytes(239)).expect("239 fits");
        assert_eq!(fits.staging().as_str().len(), 255);

        let refused = ClaimPath::from_rendering_path(&file_of_bytes(240))
            .expect_err("240 bytes cannot be staged");
        assert_eq!(
            refused,
            UnsafePathCause::NameTooLong {
                at: file_of_bytes(240),
                bytes: 256,
                name: TooLongName::Staging,
            }
        );
    }

    #[test]
    fn a_component_of_more_than_255_bytes_is_refused_as_a_name_that_cannot_exist() {
        let long = file_of_bytes(256);
        let refused = ClaimPath::from_rendering_path(&format!("d/{long}"))
            .expect_err("256 bytes is not a file name");
        assert_eq!(
            refused,
            UnsafePathCause::NameTooLong {
                at: format!("d/{long}"),
                bytes: 256,
                name: TooLongName::Claimed,
            }
        );
    }

    #[test]
    fn a_directory_may_be_255_bytes_but_not_256() {
        // Only the last component is wrapped by the staging name, so a
        // directory has the whole 255.
        let directory = "d".repeat(NAME_BYTES_MAX);
        ClaimPath::from_rendering_path(&format!("{directory}/x.yml"))
            .expect("a directory of 255 bytes fits");

        let too_long = "d".repeat(NAME_BYTES_MAX + 1);
        let refused = ClaimPath::from_rendering_path(&format!("a/{too_long}/x.yml"))
            .expect_err("a directory of 256 bytes cannot exist");
        assert_eq!(
            refused,
            UnsafePathCause::NameTooLong {
                at: format!("a/{too_long}"),
                bytes: 256,
                name: TooLongName::Claimed,
            }
        );
    }

    #[test]
    fn the_limit_counts_bytes_not_characters() {
        // `é` is two bytes: 119 of them are 238 bytes and fit as a last
        // component, 120 are 240 and do not, and 128 are 256, too long for
        // any name. As a directory 120 fit.
        let of = |characters: usize| "\u{e9}".repeat(characters);
        ClaimPath::from_rendering_path(&of(119)).expect("238 bytes fits");
        let staging = ClaimPath::from_rendering_path(&of(120)).expect_err("240 bytes");
        assert!(matches!(
            staging,
            UnsafePathCause::NameTooLong {
                bytes: 256,
                name: TooLongName::Staging,
                ..
            }
        ));
        ClaimPath::from_rendering_path(&format!("{}/x.yml", of(120)))
            .expect("240 bytes fits as a directory");
        let claimed = ClaimPath::from_rendering_path(&of(128)).expect_err("256 bytes");
        assert!(matches!(
            claimed,
            UnsafePathCause::NameTooLong {
                bytes: 256,
                name: TooLongName::Claimed,
                ..
            }
        ));
    }

    #[test]
    fn a_dotgit_component_is_still_refused_as_inside_git_before_a_length_is_read() {
        let error = ClaimPath::from_rendering_path(&format!(".git/{}", file_of_bytes(300)))
            .expect_err("a `.git` component is refused");
        assert_eq!(error, UnsafePathCause::InsideGitDirectory);
    }

    #[test]
    fn staging_is_the_dotted_name_beside_the_target() {
        let staged = |path: &str| {
            ClaimPath::from_rendering_path(path)
                .expect("valid")
                .staging()
        };
        assert_eq!(staged("a/b.yml").as_str(), "a/.b.yml.skeletons-sync");
        assert_eq!(staged("b.yml").as_str(), ".b.yml.skeletons-sync");
    }

    #[test]
    fn a_staging_name_wraps_the_file_name_the_way_a_claims_staging_path_does() {
        // One function writes the wrapping, so a path built to ask whether
        // the filesystem takes a spelling for a staging name cannot differ
        // from the staging path itself.
        assert_eq!(staging_name("b.yml"), ".b.yml.skeletons-sync");
        let claim = ClaimPath::from_rendering_path("a/b.yml").expect("valid");
        assert!(claim.staging().as_str().ends_with(&staging_name("b.yml")));
    }

    #[test]
    fn ancestors_are_the_proper_prefixes_root_to_leaf() {
        let ancestors = |path: &str| -> Vec<String> {
            ClaimPath::from_rendering_path(path)
                .expect("valid")
                .ancestors()
                .iter()
                .map(|ancestor| ancestor.as_str().to_owned())
                .collect()
        };
        assert_eq!(ancestors("a/b/c.yml"), ["a", "a/b"]);
        assert_eq!(ancestors("a/b.yml"), ["a"]);
        assert!(ancestors("b.yml").is_empty());
    }

    #[test]
    fn the_parent_is_the_directory_above_or_nothing_at_the_root() {
        let parent = |path: &str| {
            ClaimPath::from_rendering_path(path)
                .expect("valid")
                .parent()
                .map(|parent| parent.as_str().to_owned())
        };
        assert_eq!(parent("a/b/c.yml").as_deref(), Some("a/b"));
        assert_eq!(parent("a/b.yml").as_deref(), Some("a"));
        assert_eq!(parent("b.yml"), None);
    }

    #[test]
    fn depth_counts_components() {
        let depth = |path: &str| ClaimPath::from_rendering_path(path).expect("valid").depth();
        assert_eq!(depth("b.yml"), 1);
        assert_eq!(depth("a/b/c.yml"), 3);
    }

    #[test]
    fn to_path_joins_every_component_onto_root() {
        let path = ClaimPath::from_rendering_path(".github/dependabot.yml").expect("valid");
        assert_eq!(
            path.to_path(std::path::Path::new("/workspace")),
            std::path::PathBuf::from("/workspace/.github/dependabot.yml")
        );
    }
}
