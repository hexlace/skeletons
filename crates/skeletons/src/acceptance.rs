//! Behaviour tests for `crate::skeleton::render`.
//!
//! These are written against render's behaviour, not against any particular
//! implementation shape: each test says, in a comment, which behaviour it
//! holds render to. They read real skeleton crates from `test-skeletons/`, the way a
//! skeleton author would actually write one; only `names` builds its
//! skeletons in test code, as described below.
//!
//! Every submodule reaches the seam through [`test_skeleton`] and the re-exports
//! of `crate::skeleton`: `render`, `Choice`, `Choices`, `Reason`,
//! `RenderError`, `Rendering`, `SkeletonIdentity`, `OptionKind`.
//!
//! The published package leaves `test-skeletons/` out, so a submodule that
//! reads it is compiled only in a checkout (`skeletons_checkout`, set by this
//! crate's `build.rs`). `names` builds its skeletons in a temporary directory
//! and keeps those tests in the package too.

#[cfg(skeletons_checkout)]
mod budget;
#[cfg(skeletons_checkout)]
mod byte_order_mark;
#[cfg(skeletons_checkout)]
mod choices;
#[cfg(skeletons_checkout)]
mod determinism;
#[cfg(skeletons_checkout)]
mod directives;
#[cfg(skeletons_checkout)]
mod fill;
#[cfg(skeletons_checkout)]
mod hidden_directives;
#[cfg(skeletons_checkout)]
mod hidden_markers;
#[cfg(skeletons_checkout)]
mod indentation;
#[cfg(skeletons_checkout)]
mod manifests;
mod names;
#[cfg(skeletons_checkout)]
mod optional_text;
#[cfg(skeletons_checkout)]
mod placeholders;
#[cfg(skeletons_checkout)]
mod schema;
#[cfg(skeletons_checkout)]
mod select;
#[cfg(skeletons_checkout)]
mod text;
#[cfg(skeletons_checkout)]
mod tree;
#[cfg(skeletons_checkout)]
mod validation;
#[cfg(skeletons_checkout)]
mod verbatim;

/// The directory of the test skeleton at `relative` under `crates/skeletons/test-skeletons`.
///
/// Test skeletons are read from disk at run time, from `CARGO_MANIFEST_DIR`,
/// never baked into the test binary with `include_str!`/`include_bytes!`:
/// they are meant to be read the same way `render` will read a real skeleton's
/// directory.
#[cfg(skeletons_checkout)]
fn test_skeleton(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-skeletons")
        .join(relative)
}
