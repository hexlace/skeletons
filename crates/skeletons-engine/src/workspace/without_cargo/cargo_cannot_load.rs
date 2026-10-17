//! The manifests this reader knows Cargo cannot load either.

use super::text_error::RefusedShape;

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

impl From<RefusedShape> for CargoCannotLoad {
    fn from(shape: RefusedShape) -> Self {
        match shape {
            RefusedShape::GitAndPath => Self::GitAndPath,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CargoCannotLoad;
    use crate::workspace::without_cargo::text_error::RefusedShape;

    #[test]
    fn a_shape_of_text_converts_to_the_shape_of_the_same_name() {
        assert_eq!(
            CargoCannotLoad::from(RefusedShape::GitAndPath),
            CargoCannotLoad::GitAndPath
        );
    }
}
