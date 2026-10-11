#![doc = include_str!("../readme.md")]
//!
//! ## What this crate provides
//!
//! Without a feature, the public items read a workspace and report on it as
//! data. [`read`] reads what a workspace wears, and lets cargo reach the
//! network only when it is handed [`Network::Allowed`]. [`survey()`] renders
//! every worn skeleton and compares what it renders with the files on disk,
//! and writes nothing. The items they take and return carry the answer: each
//! worn skeleton, each file it claims with its state, and each refusal with
//! the wearings and paths it concerns.
//!
//! ## Features
//!
//! - `commands` adds `task()`, the group of the three commands `check`,
//!   `sync` and `wear` that `skeletons` hands on as its own bundle, with the
//!   command-line framework, the network client and the manifest editor they
//!   are built on. It is off by default, and `skeletons` is the one that turns
//!   it on.
//! - `test-util` compiles in the seams that let a dependent's tests observe or
//!   redirect a real network request or a real wait. Every one of them sits in
//!   the commands, so it changes nothing without `commands`.

// Every module in this crate is private, and every item shared between
// modules is `pub(crate)`: the visibility that is true, and what rustc's
// `unreachable_pub` (warned in the workspace lints) asks for. The public items
// are the ones re-exported from this root, so this list is the whole public
// surface. Clippy's nursery `redundant_pub_crate` asks for `pub` instead, for
// every `pub(crate)` item in a private module, so the two lints disagree
// crate-wide. `expect` rather than `allow`, so the suppression goes stale
// loudly if the crate ever stops needing it — one explanation here, rather
// than one repeated at every module that would otherwise need its own
// `#[allow]`.
#![expect(
    clippy::redundant_pub_crate,
    reason = "every module is private and the public items are re-exported from the root; \
              pub(crate) is true for the rest, unreachable_pub wants it"
)]

#[cfg(feature = "commands")]
use rituals::Task;

#[cfg(feature = "commands")]
mod behind;
mod cargo;
#[cfg(feature = "commands")]
mod check;
mod claim;
#[cfg(feature = "commands")]
mod current_directory;
#[cfg(feature = "commands")]
mod git;
mod skeleton;
mod subprocess;
mod survey;
#[cfg(feature = "commands")]
mod sync;
#[cfg(feature = "commands")]
mod wear;
#[cfg(feature = "commands")]
mod work_tree;
mod workspace;

pub use claim::{ClaimPath, Claimant, Drift, DriftReason};
pub use survey::{RefusalKind, RefusalRecord, RefusedWearing, Row, Survey, survey};
pub use workspace::{Network, ReadWorkspaceError, Workspace, WornDependency, read};

// Tests for `skeleton::render`, read from real skeleton crates under
// `test-skeletons/`, which `exclude = ["/test-skeletons"]` in this crate's manifest
// keeps out of the published package. See `acceptance.rs` for how they
// call `render`, and which of them the package still runs.
#[cfg(test)]
mod acceptance;

// Keeps `.docs/design.md`'s own `` `file` → `name` `` citations pointing at
// code that still exists. Test only: the check reads the doc and the
// workspace's other files, neither of which the published crate carries any
// obligation to, so those tests compile only in a checkout
// (`skeletons_checkout`, set by `build.rs`); the citation parser's own tests
// run everywhere.
#[cfg(test)]
mod doc_citations;

/// The group of the three commands `check`, `sync` and `wear`.
///
/// This is what `skeletons` hands on as its own `task()`, for a command line to
/// mount under whatever name imports it.
//
// No `# Examples` section: a `task()` function has one call-site shape, a
// mount line in the command line that imports it, and no arguments to
// illustrate.
//
// No assertions of its own either. The body is one `Task::group` call,
// which asserts at construction that there is at least one child, that the
// names are distinct and that none is `help`. The child list this function
// adds is held by the unit test below and by the integration tests that run
// a command line mounting it.
#[cfg(feature = "commands")]
#[must_use]
pub fn task() -> Task {
    Task::group(
        "keep this repository from drifting apart from its siblings",
        [
            ("check", check::task()),
            ("sync", sync::task()),
            ("wear", wear::task()),
        ],
    )
}

#[cfg(test)]
#[cfg(feature = "commands")]
mod tests {
    #[test]
    fn the_bundle_holds_exactly_check_sync_and_wear() {
        // Task's own Debug impl prints a bundle's `children: [names]`, and
        // is the only door onto them from outside `rituals`. What breaks if
        // this fails is every command line that mounts it: its children are
        // the commands under `skeletons`, child for child.
        let rendered = format!("{:?}", super::task());
        assert!(
            rendered.contains(r#"children: ["check", "sync", "wear"]"#),
            "expected exactly the check, sync and wear children; Debug was: {rendered}"
        );
    }
}

#[cfg(all(test, not(skeletons_checkout)))]
mod packaged {
    // Runs only where `build.rs` found no `test-skeletons/` beside the
    // manifest, which is an unpacked package and nothing else. The tests that
    // read the files the package leaves out are compiled away there, so this
    // one says so in the skipped list CI prints: a checkout that lost the
    // `skeletons_checkout` cfg would then show up as a change in that list,
    // not as tests that quietly stopped existing.
    #[test]
    fn checkout_only_tests_are_not_built_without_the_test_skeletons() {
        eprintln!("skipped: this is the published package, which carries no test-skeletons/");
    }
}
