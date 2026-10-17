//! The manifests this reader knows Cargo cannot load either.

/// A shape of manifest that Cargo itself fails to load, each one measured against Cargo.
///
/// Cargo loads the manifest of a package that sits outside the workspace only once a feature
/// enables it, and this reader cannot tell whether one does. A read that judges such a package
/// whether or not a feature enables it may therefore pass over a manifest it cannot read only
/// where Cargo fails on that manifest too, since there a refusal would fire on a workspace Cargo
/// reads, and the package is not part of any resolution that succeeds. The set is closed: a
/// refusal added to the manifest reader later carries no shape, so a read that matches on this
/// type refuses it by name rather than passing over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoCannotLoad {
    /// The manifest is not there: its directory or the `Cargo.toml` in it is gone.
    Missing,
    /// A dependency writes both `git` and `path`, which Cargo refuses as ambiguous.
    GitAndPath,
}
