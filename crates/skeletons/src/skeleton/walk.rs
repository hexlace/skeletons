//! Walking a skeleton's `files/` and `partials/` trees into path sets, in an
//! order that depends only on the tree's own structure, never on how the
//! filesystem happens to list a directory's entries.
//!
//! Each directory's entries are listed once — each one charged against the
//! render's entry budget the moment its name is read, so no more of a
//! listing is ever held in memory than the budget allows — then sorted, then
//! checked as names, then visited in ascending name order, each directory
//! among them descended into before its next sibling is visited — with an
//! explicit stack rather than recursion, so a deeply nested skeleton cannot
//! overflow the call stack. Every entry is classified from its own
//! [`std::fs::symlink_metadata`], never a followed link's: a symbolic link
//! is refused outright, whatever it points at.
//!
//! A directory's names are checked as a listing, when it is listed and so
//! before any entry in it is visited, in two passes over the whole listing:
//! first every name for valid UTF-8, refusing the first that is not, in
//! sorted order, ahead of any other rule; then, once every name is valid
//! UTF-8, every name again, in sorted order, against two more rules — none
//! may be `Cargo.toml` or a name that folds to it, since Cargo leaves a
//! directory holding one out of the package; and no two may be one name to
//! a filesystem that ignores case or Unicode normalization (see
//! [`super::folding`]) — a property of the tree, which renders two files on
//! one machine and one on another — refusing the first name that breaks
//! either.
//!
//! Both passes finish before any entry of the directory is visited, so the
//! first refusal is fixed by the tree alone: a name defect anywhere in a
//! directory's listing is reported before anything found by visiting that
//! directory's entries, even an entry sorted ahead of the defective name —
//! and a non-UTF-8 name anywhere in the listing is reported ahead of a
//! manifest name or collision anywhere in it, even one that sorts earlier,
//! since the first pass runs over the whole listing before the second
//! begins.

use std::borrow::Borrow;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use super::error::{Reason, RenderError, SkeletonIdentity};
use super::folding::names_collide;
use super::limits::EntryBudget;
use super::manifest::MANIFEST_NAME;
use super::siblings::Siblings;

/// Which of a skeleton's two walked trees a path lies under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tree {
    /// `files/`: whole files a wearing repository should hold.
    Files,
    /// `partials/`: fragments a `set` option's directives insert.
    Partials,
}

impl Tree {
    /// The tree's directory name, directly under the skeleton's own directory.
    pub(crate) const fn directory_name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Partials => "partials",
        }
    }

    /// The path of the entry at `components` under this tree, relative to
    /// the skeleton's directory with `/` separators — the one spelling every
    /// walk refusal uses for a path, whether it names the entry at fault or
    /// the other entry a refusal is about.
    fn skeleton_relative(self, components: &[String]) -> String {
        let mut path = self.directory_name().to_owned();
        for component in components {
            path.push('/');
            path.push_str(component);
        }
        path
    }
}

/// A validated path relative to a skeleton's `files/` or `partials/` directory:
/// non-empty `/`-joined components, each itself non-empty. `Rendering` keys
/// its files by this, and it is what a set option's declared `partial`
/// string is compared against.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TreePath(String);

impl TreePath {
    /// Builds a tree path from components discovered walking a tree —
    /// never from a skeleton author's or a wearer's own text, which is why this
    /// is a private constructor rather than a `parse`.
    fn from_components(components: &[String]) -> Self {
        assert!(
            !components.is_empty(),
            "a tree path always has at least one component"
        );
        assert!(
            components.iter().all(|component| !component.is_empty()),
            "a path component discovered by the walk is never empty"
        );
        Self(components.join("/"))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds a tree path directly from component strings, for a test in
    /// another module that needs one without walking a real directory.
    #[cfg(test)]
    pub(crate) fn for_test(components: &[&str]) -> Self {
        Self::from_components(
            &components
                .iter()
                .map(|component| (*component).to_owned())
                .collect::<Vec<_>>(),
        )
    }
}

impl fmt::Display for TreePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for TreePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// Sound: `Eq` and `Ord` are both derived from the single `String` field, so
// they agree exactly with `str`'s own — the property `Borrow` requires of
// any type that implements it for more than one target. This is what lets a
// `BTreeMap<TreePath, _>`/`BTreeSet<TreePath>` be looked up by a plain
// `&str`, without building a `TreePath` (a walk-only type) just to ask.
impl Borrow<str> for TreePath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// One walk-phase refusal: which tree it was found in, the entry at fault,
/// and why.
#[derive(Debug)]
pub(crate) struct WalkFailure {
    pub(crate) tree: Tree,
    /// The failing entry's path components, relative to the walked root.
    /// Empty only when the failure is about the root itself.
    pub(crate) components: Vec<String>,
    pub(crate) reason: Reason,
}

impl WalkFailure {
    const fn new(tree: Tree, components: Vec<String>, reason: Reason) -> Self {
        Self {
            tree,
            components,
            reason,
        }
    }

    /// The failing path, relative to the skeleton's directory — what
    /// [`super::error::RenderError::file`] reports.
    pub(crate) fn file(&self) -> String {
        self.tree.skeleton_relative(&self.components)
    }
}

/// Walks every entry under `root`, returning the [`TreePath`] of every
/// regular file found, or the first refusal the walk meets: each
/// directory's listing is checked as names (see [`check_listing`]) when the
/// directory is listed, before any of its entries are visited; its entries
/// are then visited in ascending name order, a subdirectory in full before
/// its next sibling.
///
/// `root` itself is assumed to already be a directory (the caller checks
/// this, since the top-level `files`/`partials` distinction between "absent"
/// and "not a directory" needs its own wording); this only walks what is
/// inside it.
pub(crate) fn walk_tree(
    root: &Path,
    tree: Tree,
    budget: &mut EntryBudget,
) -> Result<BTreeSet<TreePath>, WalkFailure> {
    let mut discovered = BTreeSet::new();
    // Each stack entry is one entry's path components, relative to `root`,
    // not yet classified. The root itself is represented by the empty
    // component list pushed here first.
    let mut stack: Vec<Vec<String>> = vec![Vec::new()];

    while let Some(components) = stack.pop() {
        let absolute = join(root, &components);

        if !components.is_empty() {
            let metadata = std::fs::symlink_metadata(&absolute).map_err(|cause| {
                WalkFailure::new(tree, components.clone(), Reason::Unreadable { cause })
            })?;

            if metadata.is_symlink() {
                return Err(WalkFailure::new(tree, components, Reason::SymbolicLink));
            }
            if metadata.is_file() {
                discovered.insert(TreePath::from_components(&components));
                continue;
            }
            if !metadata.is_dir() {
                return Err(WalkFailure::new(
                    tree,
                    components,
                    Reason::NotAFileOrDirectory,
                ));
            }
            // A directory: fall through and list it below.
        }

        let listing = std::fs::read_dir(&absolute)
            .map_err(|cause| {
                WalkFailure::new(tree, components.clone(), Reason::Unreadable { cause })
            })?
            .map(|entry| entry.map(|entry| entry.file_name()));
        let names = list_names(listing, budget)
            .map_err(|reason| WalkFailure::new(tree, components.clone(), reason))?;
        let children = check_listing(tree, &components, names)?;

        // Pushed in reverse so that popping the stack (last in, first out)
        // visits this directory's own entries in ascending name order —
        // and, because a directory pushed here is itself fully expanded
        // before the loop returns to its next sibling, each subdirectory is
        // walked in full before the next sibling is visited. Its names were
        // all checked above, before any of them is visited.
        for name in children.into_iter().rev() {
            let mut child = components.clone();
            child.push(name);
            stack.push(child);
        }
    }

    Ok(discovered)
}

/// Checks one directory's `names` as a listing, before any of its entries
/// are visited, in the two passes [`walk_tree`] relies on: first every name
/// for valid UTF-8 (see [`utf8_names`]), refusing the first that is not, in
/// sorted order, ahead of any other rule; then, once every name is valid
/// UTF-8, the manifest-name and collision rules (see [`check_names`]),
/// refusing the first name that breaks either. Returns the listing as
/// validated names, sorted, once both passes admit it.
fn check_listing(
    tree: Tree,
    components: &[String],
    names: Vec<OsString>,
) -> Result<Vec<String>, WalkFailure> {
    let children = utf8_names(tree, components, names)?;
    check_names(tree, components, &children)?;
    Ok(children)
}

/// Converts one directory's sorted `names` to UTF-8, in ascending order, so
/// the first non-UTF-8 name in sorted order is what gets refused — never a
/// name that only looks first because of the reversed push in
/// [`walk_tree`].
fn utf8_names(
    tree: Tree,
    components: &[String],
    names: Vec<OsString>,
) -> Result<Vec<String>, WalkFailure> {
    let mut children = Vec::with_capacity(names.len());
    for name in names {
        match name.into_string() {
            Ok(name) => children.push(name),
            Err(raw_name) => {
                let child = child_of(components, &raw_name.to_string_lossy());
                return Err(WalkFailure::new(tree, child, Reason::PathNotUtf8));
            }
        }
    }
    Ok(children)
}

/// Checks one directory's listed `children` — already validated as UTF-8 by
/// [`utf8_names`] — as names, in ascending order, refusing the first name
/// that breaks either rule, whichever it breaks: a name that is
/// `Cargo.toml`, or one name with it, is [`Reason::NestedManifest`]; a name
/// that is one name with an earlier sibling to a filesystem that ignores
/// case or Unicode normalization is [`Reason::NamesCollide`], naming the
/// earlier of the two as the entry at fault and carrying the later as the
/// other, both relative to the skeleton's directory.
fn check_names(tree: Tree, components: &[String], children: &[String]) -> Result<(), WalkFailure> {
    let mut siblings = Siblings::new();
    for name in children {
        if names_collide(name, MANIFEST_NAME) {
            return Err(WalkFailure::new(
                tree,
                child_of(components, name),
                Reason::NestedManifest,
            ));
        }
        if let Err(earlier) = siblings.admit(name) {
            let other = tree.skeleton_relative(&child_of(components, name));
            return Err(WalkFailure::new(
                tree,
                child_of(components, earlier),
                Reason::NamesCollide { other },
            ));
        }
    }
    Ok(())
}

/// The components of the entry `name` inside the directory at `components`.
fn child_of(components: &[String], name: &str) -> Vec<String> {
    let mut child = Vec::with_capacity(components.len() + 1);
    child.extend_from_slice(components);
    child.push(name.to_owned());
    child
}

/// Reads one directory's entry names, charging each against `budget` the
/// moment the listing yields it, so no more of a listing is ever held in
/// memory than the budget allows; returns the names sorted.
///
/// The budget is consumed before an item's own `io::Result` is unwrapped, so
/// the item that would cross the budget is refused before it is ever kept —
/// a directory with a million entries never has more than
/// [`super::limits::ENTRIES_MAX`] of them collected at once, whichever one
/// happens to be unreadable.
fn list_names(
    listing: impl Iterator<Item = std::io::Result<OsString>>,
    budget: &mut EntryBudget,
) -> Result<Vec<OsString>, Reason> {
    let mut names = Vec::new();
    for entry in listing {
        budget.consume()?;
        let name = entry.map_err(|cause| Reason::Unreadable { cause })?;
        names.push(name);
    }
    names.sort();

    // Postcondition: a listing this function returns is always sorted, and
    // never holds more names than the budget it was charged against could
    // ever allow through in total.
    assert!(
        names.windows(2).all(|pair| pair[0] <= pair[1]),
        "list_names must return names in sorted order"
    );
    assert!(
        names.len() as u64 <= u64::from(super::limits::ENTRIES_MAX),
        "list_names must never return more names than the entry budget allows"
    );
    Ok(names)
}

/// Walks a skeleton's `files/` directory: it must exist, be a directory, and
/// hold at least one file, or it is refused as [`Reason::NoFiles`] — a skeleton
/// with no file under `files/` has nothing to render.
pub(crate) fn walk_files(
    skeleton_directory: &Path,
    skeleton: &SkeletonIdentity,
    budget: &mut EntryBudget,
) -> Result<BTreeSet<TreePath>, RenderError> {
    let files_root = skeleton_directory.join(Tree::Files.directory_name());
    match top_level_kind(&files_root) {
        TopLevelKind::Absent => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Files.directory_name(),
                Reason::NoFiles,
            ));
        }
        TopLevelKind::NotADirectory => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Files.directory_name(),
                Reason::NotADirectory,
            ));
        }
        TopLevelKind::Symlink => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Files.directory_name(),
                Reason::SymbolicLink,
            ));
        }
        TopLevelKind::Unreadable(cause) => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Files.directory_name(),
                Reason::Unreadable { cause },
            ));
        }
        TopLevelKind::Directory => {}
    }

    let discovered = walk_tree(&files_root, Tree::Files, budget)
        .map_err(|failure| to_render_error(skeleton, failure))?;
    if discovered.is_empty() {
        return Err(RenderError::about_file(
            skeleton.clone(),
            Tree::Files.directory_name(),
            Reason::NoFiles,
        ));
    }
    Ok(discovered)
}

/// Walks a skeleton's `partials/` directory. Unlike `files/`, it may simply be
/// absent — a skeleton with no `set` options has no partials to ship.
pub(crate) fn walk_partials(
    skeleton_directory: &Path,
    skeleton: &SkeletonIdentity,
    budget: &mut EntryBudget,
) -> Result<BTreeSet<TreePath>, RenderError> {
    let partials_root = skeleton_directory.join(Tree::Partials.directory_name());
    match top_level_kind(&partials_root) {
        TopLevelKind::Absent => return Ok(BTreeSet::new()),
        TopLevelKind::NotADirectory => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Partials.directory_name(),
                Reason::NotADirectory,
            ));
        }
        TopLevelKind::Symlink => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Partials.directory_name(),
                Reason::SymbolicLink,
            ));
        }
        TopLevelKind::Unreadable(cause) => {
            return Err(RenderError::about_file(
                skeleton.clone(),
                Tree::Partials.directory_name(),
                Reason::Unreadable { cause },
            ));
        }
        TopLevelKind::Directory => {}
    }

    walk_tree(&partials_root, Tree::Partials, budget)
        .map_err(|failure| to_render_error(skeleton, failure))
}

/// What a top-level `files`/`partials` path turned out to be, decided from
/// its own metadata without following a link.
enum TopLevelKind {
    Absent,
    Directory,
    NotADirectory,
    Symlink,
    Unreadable(std::io::Error),
}

fn top_level_kind(path: &Path) -> TopLevelKind {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => TopLevelKind::Absent,
        Err(error) => TopLevelKind::Unreadable(error),
        Ok(metadata) if metadata.is_symlink() => TopLevelKind::Symlink,
        Ok(metadata) if metadata.is_dir() => TopLevelKind::Directory,
        Ok(_not_a_directory) => TopLevelKind::NotADirectory,
    }
}

fn to_render_error(skeleton: &SkeletonIdentity, failure: WalkFailure) -> RenderError {
    RenderError::about_file(skeleton.clone(), failure.file(), failure.reason)
}

/// Joins `root` with a walk's relative path components, `/` or not — this
/// crate never compares the result against another `Path`, only ever
/// reopens it, so the host's own separator is fine here.
fn join(root: &Path, components: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in components {
        path.push(component);
    }
    path
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        Reason, SkeletonIdentity, Tree, TreePath, check_listing, check_names, walk_files,
        walk_partials, walk_tree,
    };
    use crate::skeleton::limits::{ENTRIES_MAX, EntryBudget};

    /// A fresh temporary directory for one test, removed when the returned
    /// value drops, whether the test passes or fails.
    fn scratch_directory() -> TempDir {
        TempDir::new().expect("create scratch directory")
    }

    #[test]
    fn every_regular_file_is_discovered_keyed_by_its_relative_path() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::create_dir_all(root.join("a/b")).expect("nested directories");
        std::fs::write(root.join("root.txt"), "").expect("root file");
        std::fs::write(root.join("a/b/deep.txt"), "").expect("nested file");

        let mut budget = EntryBudget::new();
        let discovered =
            walk_tree(root, Tree::Files, &mut budget).expect("a plain tree must walk cleanly");

        assert!(discovered.contains(&TreePath::from_components(&["root.txt".to_owned()])));
        assert!(discovered.contains(&TreePath::from_components(&[
            "a".to_owned(),
            "b".to_owned(),
            "deep.txt".to_owned()
        ])));
    }

    #[test]
    fn an_empty_tree_discovers_nothing() {
        let scratch = scratch_directory();
        let root = scratch.path();
        let mut budget = EntryBudget::new();
        let discovered = walk_tree(root, Tree::Files, &mut budget)
            .expect("an empty directory must walk cleanly");
        assert!(discovered.is_empty());
    }

    #[test]
    fn a_symlinked_file_is_refused_naming_it() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::write(root.join("target.txt"), "").expect("link target");
        std::os::unix::fs::symlink(root.join("target.txt"), root.join("link.txt"))
            .expect("create a symlink");

        let mut budget = EntryBudget::new();
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("a symlink must be refused");
        assert_eq!(failure.file(), "files/link.txt");
        assert!(matches!(failure.reason, Reason::SymbolicLink));
    }

    #[test]
    fn a_fifo_is_refused_as_neither_a_file_nor_a_directory() {
        // A FIFO is not something git can store, so no committed test skeleton
        // exercises this; a real one, made with `mkfifo`, is the only way
        // to test the walk's handling of "something else entirely".
        let scratch = scratch_directory();
        let root = scratch.path();
        let fifo_path = root.join("pipe");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo_path)
            .status()
            .expect("run mkfifo");
        assert!(
            status.success(),
            "mkfifo must succeed for this test to mean anything"
        );

        let mut budget = EntryBudget::new();
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("a FIFO must be refused");
        assert_eq!(failure.file(), "files/pipe");
        assert!(matches!(failure.reason, Reason::NotAFileOrDirectory));
    }

    #[test]
    fn the_first_walk_refusal_is_the_first_entry_in_sorted_order() {
        // `b-link` is created before `a-link`, so a walk following creation
        // or a typical directory listing would meet `b-link` first; sorted
        // order visits `a-link` first instead. Both are refused as symbolic
        // links, so which one the walk names first is the only observable
        // effect of its own visiting order, independent of the filesystem.
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::write(root.join("target.txt"), "").expect("link target");
        std::os::unix::fs::symlink(root.join("target.txt"), root.join("b-link"))
            .expect("create b-link");
        std::os::unix::fs::symlink(root.join("target.txt"), root.join("a-link"))
            .expect("create a-link");

        let mut budget = EntryBudget::new();
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("a symlink must be refused");
        assert_eq!(
            failure.file(),
            "files/a-link",
            "the walk must visit and refuse a-link before b-link, in sorted order"
        );
    }

    #[test]
    fn the_entry_budget_is_exhausted_by_a_tree_deep_enough_to_reach_it() {
        let scratch = scratch_directory();
        let root = scratch.path();
        for index in 0..4u32 {
            std::fs::write(root.join(format!("file-{index}.txt")), "").expect("fixture file");
        }

        let mut budget = EntryBudget::with_remaining(2);
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("the budget must be exhausted");
        // `entries_max` always reports the crate-wide `ENTRIES_MAX`
        // constant, not whatever a test started `remaining` at — that field
        // tells a skeleton author the real configured limit, not this test's
        // shortcut for reaching it quickly.
        assert!(matches!(
            failure.reason,
            Reason::TooManyEntries { entries_max } if entries_max == ENTRIES_MAX
        ));
    }

    #[test]
    fn list_names_stops_pulling_the_listing_at_the_budget() {
        // A synthetic listing that never touches a filesystem: counting how
        // many `next()` calls actually happen is the filesystem-independent
        // proof that `list_names` stops pulling from the listing the moment
        // the budget is crossed, rather than materialising the whole
        // listing first and refusing only afterwards.
        let pulled = std::cell::Cell::new(0u32);
        let listing = (0..1_000_000u32).map(|index| {
            pulled.set(pulled.get() + 1);
            Ok(std::ffi::OsString::from(format!("entry-{index}")))
        });

        let mut budget = EntryBudget::new();
        let error = super::list_names(listing, &mut budget)
            .expect_err("a listing past the budget must be refused");

        assert!(matches!(
            error,
            Reason::TooManyEntries { entries_max } if entries_max == ENTRIES_MAX
        ));
        assert_eq!(
            pulled.get(),
            ENTRIES_MAX + 1,
            "list_names must stop pulling from the listing the moment the budget is crossed"
        );
    }

    #[test]
    fn a_listing_past_the_budget_is_refused_naming_the_directory() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::create_dir_all(root.join("nested")).expect("nested directory");
        for index in 0..10u32 {
            std::fs::write(root.join(format!("nested/file-{index}.txt")), "")
                .expect("fixture file");
        }

        let mut budget = EntryBudget::with_remaining(3);
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("the budget must be exhausted");
        assert_eq!(
            failure.file(),
            "files/nested",
            "the refusal must name the directory whose listing crossed the budget, not one entry \
             in it"
        );
        assert!(matches!(failure.reason, Reason::TooManyEntries { .. }));
    }

    #[test]
    fn a_name_defect_in_a_listing_is_reported_before_a_defect_under_an_earlier_entry() {
        // The walk checks a directory's whole listing as names when it lists
        // the directory, before it visits any entry in it. Here `files/`
        // lists `a` and `cargo.toml`: `a` sorts first and holds a symbolic
        // link, and `cargo.toml` folds to a Cargo manifest's name. The walk
        // reports the manifest name, which it checked on listing `files/`,
        // not the link under `a`, which it would reach only by visiting `a`.
        // A walk that checked each name only as it visited the entry (a
        // true preorder), or that did not check the name at all, reports
        // the link instead.
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::create_dir_all(root.join("a")).expect("files/a");
        std::os::unix::fs::symlink("elsewhere", root.join("a/link")).expect("files/a/link");
        std::fs::write(root.join("cargo.toml"), "").expect("files/cargo.toml");

        let mut budget = EntryBudget::new();
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("the tree must be refused");
        assert_eq!(failure.file(), "files/cargo.toml");
        assert!(
            matches!(failure.reason, Reason::NestedManifest),
            "expected the manifest name checked on listing files/, got {:?}",
            failure.reason
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a test that cannot establish its premise says so rather than passing silently"
    )]
    fn a_name_collision_in_a_listing_is_reported_before_a_defect_under_an_entry_between() {
        // The same order for the other name check: `files/` lists `B.txt`,
        // `a` and `b.txt`, and `B.txt` and `b.txt` are one name to a
        // filesystem that ignores case, while `a`, sorted between them,
        // holds a symbolic link. The collision is reported, naming `B.txt`
        // and carrying `b.txt`, before the link under `a` is ever reached.
        //
        // Only a filesystem that keeps `B.txt` and `b.txt` apart can hold
        // this tree. On one that folds them together, such as a default
        // macOS volume, there is nothing to walk and the test asserts
        // nothing; CI's Linux filesystem holds it.
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::write(root.join("B.txt"), "upper").expect("files/B.txt");
        std::fs::write(root.join("b.txt"), "lower").expect("files/b.txt");
        // Only reading `lower` back means the two names are one; a failed
        // read is a fault in the scratch directory, not a fold, and fails.
        let read_back = std::fs::read(root.join("B.txt")).expect("read files/B.txt back");
        assert!(
            read_back == b"upper" || read_back == b"lower",
            "files/B.txt read back as neither write: {read_back:?}"
        );
        let names_are_kept_apart = read_back == b"upper";
        if !names_are_kept_apart {
            eprintln!("skipped: this filesystem folds `B.txt` and `b.txt` into one name");
            return;
        }
        std::fs::create_dir_all(root.join("a")).expect("files/a");
        std::os::unix::fs::symlink("elsewhere", root.join("a/link")).expect("files/a/link");

        let mut budget = EntryBudget::new();
        let failure =
            walk_tree(root, Tree::Files, &mut budget).expect_err("the tree must be refused");
        assert_eq!(failure.file(), "files/B.txt");
        assert!(
            matches!(&failure.reason, Reason::NamesCollide { other } if other == "files/b.txt"),
            "expected the collision checked on listing files/, got {:?}",
            failure.reason
        );
    }

    // `walk_files`/`walk_partials` themselves — the "is `files/`/`partials/`
    // even there, and is it a directory" checks no committed test skeleton can
    // express (a skeleton is a real crate directory that always ships `files/`),
    // so these are exercised directly against a bare scratch directory.

    #[test]
    fn walk_files_refuses_a_skeleton_with_no_files_directory_at_all() {
        let scratch = scratch_directory();
        let root = scratch.path();
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let mut budget = EntryBudget::new();

        let error = walk_files(root, &skeleton, &mut budget)
            .expect_err("a skeleton with no files/ must be refused");
        assert_eq!(error.file(), Some("files"));
        assert!(matches!(error.reason(), Reason::NoFiles));
    }

    #[test]
    fn walk_files_refuses_a_files_directory_that_holds_no_file_at_all() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::create_dir_all(root.join("files/empty")).expect("an empty nested directory");
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let mut budget = EntryBudget::new();

        let error = walk_files(root, &skeleton, &mut budget)
            .expect_err("a files/ tree with no file anywhere in it must be refused");
        assert_eq!(error.file(), Some("files"));
        assert!(matches!(error.reason(), Reason::NoFiles));
    }

    #[test]
    fn walk_files_refuses_a_files_path_that_is_a_plain_file() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::write(root.join("files"), "not a directory").expect("a plain files file");
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let mut budget = EntryBudget::new();

        let error = walk_files(root, &skeleton, &mut budget)
            .expect_err("files/ that is a plain file must be refused");
        assert_eq!(error.file(), Some("files"));
        assert!(matches!(error.reason(), Reason::NotADirectory));
    }

    #[test]
    fn walk_partials_is_empty_when_the_directory_is_entirely_absent() {
        let scratch = scratch_directory();
        let root = scratch.path();
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let mut budget = EntryBudget::new();

        let discovered = walk_partials(root, &skeleton, &mut budget)
            .expect("an absent partials/ is not a refusal");
        assert!(discovered.is_empty());
    }

    #[test]
    fn walk_partials_refuses_a_partials_path_that_is_a_plain_file() {
        let scratch = scratch_directory();
        let root = scratch.path();
        std::fs::write(root.join("partials"), "not a directory").expect("a plain partials file");
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let mut budget = EntryBudget::new();

        let error = walk_partials(root, &skeleton, &mut budget)
            .expect_err("partials/ that is a plain file must be refused");
        assert_eq!(error.file(), Some("partials"));
        assert!(matches!(error.reason(), Reason::NotADirectory));
    }

    // `check_names` directly, on names rather than a real directory: two
    // names that fold to one cannot both be written to a filesystem that
    // ignores case or normalization, which is the filesystem these tests
    // may well be running on, so the listing is handed in as text.

    #[test]
    fn check_names_refuses_a_collision_naming_both_paths_relative_to_the_skeleton() {
        let components = vec!["nested".to_owned(), "deeper".to_owned()];
        let children = vec!["Dependabot.yml".to_owned(), "dependabot.yml".to_owned()];

        let failure = check_names(Tree::Partials, &components, &children)
            .expect_err("two names differing only in case must be refused");

        assert_eq!(failure.file(), "partials/nested/deeper/Dependabot.yml");
        assert!(
            matches!(
                &failure.reason,
                Reason::NamesCollide { other } if other == "partials/nested/deeper/dependabot.yml"
            ),
            "expected a names-collide refusal naming the later name, got {:?}",
            failure.reason
        );
    }

    #[test]
    fn check_names_refuses_nfc_and_nfd_spellings_at_the_top_of_a_tree() {
        let children = vec!["e\u{301}.txt".to_owned(), "\u{e9}.txt".to_owned()];

        let failure = check_names(Tree::Files, &[], &children)
            .expect_err("NFC and NFD spellings of one name must be refused");

        assert_eq!(failure.file(), "files/e\u{301}.txt");
        assert!(matches!(
            &failure.reason,
            Reason::NamesCollide { other } if other == "files/\u{e9}.txt"
        ));
    }

    #[test]
    fn check_names_admits_a_listing_with_no_two_names_alike() {
        let children = vec![
            "A".to_owned(),
            "b".to_owned(),
            "dependabot.yaml".to_owned(),
            "dependabot.yml".to_owned(),
        ];
        check_names(Tree::Files, &[], &children).expect("no two of these names collide");
    }

    #[test]
    fn check_names_refuses_a_cargo_manifest_naming_it() {
        let components = vec!["sub".to_owned()];
        let children = vec!["Cargo.toml".to_owned(), "x.yml".to_owned()];

        let failure = check_names(Tree::Files, &components, &children)
            .expect_err("a nested Cargo.toml must be refused");

        assert_eq!(failure.file(), "files/sub/Cargo.toml");
        assert!(matches!(&failure.reason, Reason::NestedManifest));
    }

    #[test]
    fn check_names_refuses_any_name_that_folds_to_the_manifest_name() {
        for name in ["cargo.toml", "CARGO.TOML", "Cargo.TOML"] {
            let children = vec![name.to_owned()];
            let failure = check_names(Tree::Files, &[], &children)
                .expect_err("a name that folds to Cargo.toml must be refused");
            assert!(
                matches!(&failure.reason, Reason::NestedManifest),
                "expected {name:?} to be refused as a nested manifest, got {:?}",
                failure.reason
            );
        }
    }

    #[test]
    fn check_names_admits_names_that_only_resemble_the_manifest() {
        let children = vec![
            "Cargo.lock".to_owned(),
            "Cargo.tom".to_owned(),
            "Cargo.toml.bak".to_owned(),
            "cargo-toml".to_owned(),
        ];
        check_names(Tree::Files, &[], &children)
            .expect("names that merely resemble Cargo.toml must not be refused");
    }

    #[test]
    fn check_names_refuses_a_collision_before_a_later_manifest_name() {
        // Neither rule outranks the other: the names are checked in
        // ascending order, each against both rules, so `a` colliding with
        // `A` is refused before `cargo.toml`, which sorts after both, is
        // reached.
        let children = vec!["A".to_owned(), "a".to_owned(), "cargo.toml".to_owned()];
        let failure = check_names(Tree::Files, &[], &children)
            .expect_err("the A/a collision must be refused before the later manifest name");
        assert_eq!(failure.file(), "files/A");
        assert!(
            matches!(&failure.reason, Reason::NamesCollide { other } if other == "files/a"),
            "expected the A/a collision, got {:?}",
            failure.reason
        );
    }

    #[test]
    fn check_names_refuses_the_first_offending_name_in_path_order() {
        let children = vec!["A".to_owned(), "Cargo.toml".to_owned(), "a".to_owned()];
        let failure = check_names(Tree::Files, &[], &children)
            .expect_err("the nested manifest must be refused before the A/a collision");
        assert_eq!(failure.file(), "files/Cargo.toml");
        assert!(matches!(&failure.reason, Reason::NestedManifest));

        let children = vec!["Cargo.toml".to_owned(), "cargo.toml".to_owned()];
        let failure = check_names(Tree::Files, &[], &children).expect_err(
            "the first Cargo.toml must be refused as a nested manifest, not a collision",
        );
        assert_eq!(failure.file(), "files/Cargo.toml");
        assert!(matches!(&failure.reason, Reason::NestedManifest));
    }

    // `check_listing` directly, on the raw names a directory listing would
    // hand it: it runs `utf8_names` over the whole listing before it ever
    // runs `check_names`, so a non-UTF-8 name anywhere in the listing is
    // refused ahead of a manifest name or collision anywhere in it, even
    // one that sorts earlier than the non-UTF-8 name.

    #[test]
    fn check_listing_refuses_a_non_utf8_name_before_a_manifest_name_that_sorts_earlier() {
        use std::os::unix::ffi::OsStringExt;

        // `cargo.toml` sorts before the non-UTF-8 name (0x63 < 0xFF), so a
        // single pass over the sorted listing would meet it first and
        // refuse it as a nested manifest. The listing is checked for valid
        // UTF-8 in full before it is checked against the manifest-name and
        // collision rules at all, so the non-UTF-8 name is what is refused.
        let names = vec![
            std::ffi::OsString::from("cargo.toml"),
            std::ffi::OsString::from_vec(vec![0xFF, b'z']),
        ];

        let failure = check_listing(Tree::Files, &[], names)
            .expect_err("a non-UTF-8 name must be refused even though cargo.toml sorts first");

        assert!(
            matches!(&failure.reason, Reason::PathNotUtf8),
            "expected the non-UTF-8 name to be refused ahead of the manifest name, got {:?}",
            failure.reason
        );
    }

    #[test]
    fn check_listing_refuses_a_manifest_name_once_every_name_is_valid_utf8() {
        let names = vec![std::ffi::OsString::from("cargo.toml")];

        let failure = check_listing(Tree::Files, &[], names)
            .expect_err("a manifest name must still be refused once the UTF-8 pass admits it");

        assert_eq!(failure.file(), "files/cargo.toml");
        assert!(matches!(&failure.reason, Reason::NestedManifest));
    }
}
