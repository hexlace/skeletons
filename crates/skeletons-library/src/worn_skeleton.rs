//! One wearing that resolved to a skeleton.

use std::path::{Path, PathBuf};

use skeletons_engine::WornDependency;

use crate::claimed_file::ClaimedFile;
use crate::files::Files;
use crate::refusal::Refusal;
use crate::version::Version;

/// A wearing that resolved to a skeleton, refused or not.
///
/// A skeleton worn by two members, or under two dependency keys, is two of
/// these.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct WornSkeleton {
    package: String,
    version: Version,
    manifest: PathBuf,
    dependency_key: String,
    claimed: Vec<ClaimedFile>,
    refusals: Vec<Refusal>,
}

impl WornSkeleton {
    pub(crate) fn from_worn(worn: &WornDependency) -> Self {
        Self {
            package: worn.package.clone(),
            version: Version::new(worn.version.clone()),
            manifest: PathBuf::from(&worn.manifest),
            dependency_key: worn.key.clone(),
            claimed: Vec::new(),
            refusals: Vec::new(),
        }
    }

    pub(crate) fn push_file(&mut self, file: ClaimedFile) {
        self.claimed.push(file);
    }

    pub(crate) fn push_refusal(&mut self, refusal: Refusal) {
        self.refusals.push(refusal);
    }

    /// The skeleton's package name, which is how a caller finds it.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The version as locked. For a skeleton taken by `path`, the version in
    /// its own manifest.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// The wearing member's `Cargo.toml`, relative to the workspace root.
    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// The dependency key the wearing member declares the skeleton under. It
    /// belongs to the wearer and is not the package name.
    #[must_use]
    pub fn dependency_key(&self) -> &str {
        &self.dependency_key
    }

    /// The files this skeleton claims, read by matching [`Files`].
    ///
    /// This is the only way to a skeleton's files. The refusals that name it
    /// are here and in [`Report::refusals`](crate::Report::refusals).
    /// `Checked` is the only variant that holds every file the
    /// skeleton claims, so "no drifted file" can be concluded from `Checked`
    /// and from nothing else.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use skeletons_library::Files;
    ///
    /// let report = skeletons_library::check(".")?;
    /// for skeleton in report.skeletons() {
    ///     let Files::Checked(files) = skeleton.files() else {
    ///         return Err(format!("{} was refused", skeleton.dependency_key()).into());
    ///     };
    ///     for file in files.iter().filter(|file| file.state().is_drifted()) {
    ///         println!("{} is {:?}", file.path().display(), file.state());
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn files(&self) -> Files<'_> {
        if self.refusals.is_empty() {
            Files::Checked(&self.claimed)
        } else {
            Files::Refused {
                refusals: &self.refusals,
                compared: &self.claimed,
            }
        }
    }
}
