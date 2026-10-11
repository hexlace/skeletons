//! One worn dependency: everything `check`/`sync` need about a single
//! `[package.metadata.skeletons.<key>]` table that named a real skeleton.

use std::path::PathBuf;

use super::pin::Pin;
use super::wearing_table::OptionShapeRefusal;
use crate::skeleton::Choices;

/// The identity of one worn dependency: the wearing manifest and the
/// dependency key under it — the same pair every output names a wearing by
/// (a member can wear more than one skeleton, and the same key can appear on
/// more than one member).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WornId {
    /// The wearing member's own `Cargo.toml`, relative to the workspace
    /// root, `/`-separated.
    pub(crate) manifest: String,
    pub(crate) key: String,
}

/// One `[package.metadata.skeletons.<key>]` table that named a real skeleton:
/// located, pinned, and either holding a valid set of choices or refused on
/// its own option values.
pub(crate) struct WornDependency {
    /// The wearing member's own `Cargo.toml`, relative to the workspace
    /// root, `/`-separated — the file a person opens to change anything.
    pub(crate) manifest: String,
    /// The dependency key the wearing table sits under.
    pub(crate) key: String,
    /// The skeleton's own package name, as locked.
    pub(crate) package: String,
    /// The skeleton's own locked version.
    pub(crate) version: semver::Version,
    /// The skeleton crate's own root directory: `manifest_path.parent()`.
    pub(crate) skeleton_directory: PathBuf,
    pub(crate) pin: Pin,
    /// The wearer's recorded option values, resolved into a shape the render
    /// accepts — or the one recorded value whose TOML shape it cannot
    /// represent. This dependency is still worn either way: an `Err` here
    /// becomes a refusal alongside this skeleton's own pin and behind fact,
    /// never a reason to drop the skeleton from the report entirely.
    pub(crate) choices: Result<Choices, OptionShapeRefusal>,
}

impl WornDependency {
    /// This worn dependency's own identity.
    pub(crate) fn id(&self) -> WornId {
        WornId {
            manifest: self.manifest.clone(),
            key: self.key.clone(),
        }
    }
}
