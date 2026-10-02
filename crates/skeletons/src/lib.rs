#![doc = include_str!("../readme.md")]
//!
//! ## What this crate provides
//!
//! [`task()`] is the whole of this crate's public API, as it is for any
//! ritual task: the bundle a project's command line mounts, holding `check`
//! and `sync` as its two commands. Everything else in the crate is private.

// Every module in this crate is private, and every item shared between
// modules is `pub(crate)`: the visibility that is true, and what rustc's
// `unreachable_pub` (warned in the workspace lints) asks for. Clippy's
// nursery `redundant_pub_crate` asks for `pub` instead, for every such
// item, so the two lints disagree crate-wide. `expect` rather than `allow`,
// so the suppression goes stale loudly if the crate ever stops needing it —
// one explanation here, rather than one repeated at every module that would
// otherwise need its own `#[allow]`.
#![expect(
    clippy::redundant_pub_crate,
    reason = "every module is private; pub(crate) is true, unreachable_pub wants it"
)]

use rituals::Task;

mod behind;
mod check;
mod claim;
mod git;
mod skeleton;
mod subprocess;
mod survey;
mod sync;
mod work_tree;
mod workspace;

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

/// This bundle, for a command line to mount under whatever name imports it,
/// which is `skeletons` unless the project chooses otherwise.
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
#[must_use]
pub fn task() -> Task {
    Task::group(
        "keep this repository from drifting apart from its siblings",
        [("check", check::task()), ("sync", sync::task())],
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bundle_holds_exactly_check_and_sync() {
        // Task's own Debug impl prints a bundle's `children: [names]`, and
        // is the only door onto them from outside `rituals`. What breaks if
        // this fails is every command line that mounts it: its children are
        // the commands under `skeletons`, child for child.
        let rendered = format!("{:?}", super::task());
        assert!(
            rendered.contains(r#"children: ["check", "sync"]"#),
            "expected exactly the check and sync children; Debug was: {rendered}"
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
