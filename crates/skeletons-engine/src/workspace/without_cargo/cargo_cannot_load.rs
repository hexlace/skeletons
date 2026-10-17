//! The manifests this reader knows Cargo cannot load either.

/// A shape of manifest that Cargo itself fails to load, each one measured against Cargo.
///
/// A read may pass over a manifest only where Cargo fails on that manifest too. Then a refusal
/// there would fire on a workspace Cargo cannot resolve once the package is enabled, and while
/// it is off Cargo never loads it. Where Cargo loads the manifest, passing over it would hide
/// requirements Cargo binds. Cargo loads a package outside the workspace only once a feature
/// enables it, and this reader cannot tell whether one does. The set is closed: a refusal added
/// to the manifest reader later carries no shape, so a read that matches on this type refuses it
/// by name rather than passing over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoCannotLoad {
    /// The manifest is not there: its directory or the `Cargo.toml` in it is gone.
    Missing,
    /// A dependency of the package writes both `git` and `path`, which Cargo refuses as
    /// ambiguous. An entry of `[workspace.dependencies]` that does is not this shape: Cargo
    /// loads it until a member inherits it.
    GitAndPath,
}
