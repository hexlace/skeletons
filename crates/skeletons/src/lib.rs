#![doc = include_str!("../readme.md")]
//!
//! ## What this crate provides
//!
//! [`task()`] is the whole of this crate's public API, as it is for any
//! ritual task: the bundle a project's command line mounts, holding `check`,
//! `sync` and `wear` as its three commands.

use rituals::Task;

/// This bundle, for a command line to mount under whatever name imports it,
/// which is `skeletons` unless the project chooses otherwise.
//
// No `# Examples` section: a `task()` function has one call-site shape, a
// mount line in the command line that imports it, and no arguments to
// illustrate.
//
// No assertions of its own: the body hands on what the engine builds, and
// the engine's `Task::group` asserts at construction that the children are
// there, distinctly named, and none is `help`. The child list is held by the
// unit test below and by the integration tests that run a command line
// mounting this bundle.
#[must_use]
pub fn task() -> Task {
    skeletons_engine::task()
}

#[cfg(test)]
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
