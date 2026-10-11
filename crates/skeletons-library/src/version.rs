//! A locked package's version.

use std::fmt;

/// A skeleton's version, with its parts and a total order.
///
/// To ask whether a version is at least 4.5, compare its parts as a tuple:
/// `(version.major(), version.minor()) >= (4, 5)`. To hand it to whatever
/// version type a caller holds itself, give that type the text `Display`
/// writes.
///
/// `Ord` is semantic-versioning precedence (a pre-release sorts before its
/// release), then build metadata, so that the order agrees with `Eq`: two versions that differ
/// only in build metadata are different, and never equal.
///
/// # Examples
///
/// ```no_run
/// let report = skeletons_library::check(".")?;
/// for skeleton in report.skeletons() {
///     let version = skeleton.version();
///     if (version.major(), version.minor()) >= (4, 5) {
///         println!("{} {version} is at least 4.5", skeleton.package());
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct Version(semver::Version);

impl Version {
    pub(crate) const fn new(version: semver::Version) -> Self {
        Self(version)
    }

    /// The major part.
    #[must_use]
    pub fn major(&self) -> u64 {
        self.0.major
    }

    /// The minor part.
    #[must_use]
    pub fn minor(&self) -> u64 {
        self.0.minor
    }

    /// The patch part.
    #[must_use]
    pub fn patch(&self) -> u64 {
        self.0.patch
    }

    /// The pre-release part, without its leading hyphen, or `None` when the
    /// version has none.
    #[must_use]
    pub fn pre_release(&self) -> Option<&str> {
        let text = self.0.pre.as_str();
        let present = !text.is_empty();
        present.then_some(text)
    }

    /// The build metadata, without its leading plus sign, or `None` when the
    /// version has none.
    #[must_use]
    pub fn build_metadata(&self) -> Option<&str> {
        let text = self.0.build.as_str();
        let present = !text.is_empty();
        present.then_some(text)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

/// Prints the version text, as a derived `Debug` of the wrapped type would
/// print its parts one field at a time and make a report unreadable.
impl fmt::Debug for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Version")
            .field(&format_args!("{}", self.0))
            .finish()
    }
}
