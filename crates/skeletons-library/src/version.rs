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

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::error::Error;

    use super::Version;

    type TestOutcome = Result<(), Box<dyn Error>>;

    fn version(text: &str) -> Result<Version, semver::Error> {
        semver::Version::parse(text).map(Version::new)
    }

    #[test]
    fn a_pre_release_sorts_before_its_release() -> TestOutcome {
        // `1.0.0-rc.1` has the same numbers as `1.0.0` and a pre-release part,
        // so it is lower under `Ord` and under `PartialOrd`, from either side.
        let pre_release = version("1.0.0-rc.1")?;
        let release = version("1.0.0")?;

        assert_eq!(pre_release.cmp(&release), Ordering::Less);
        assert_eq!(release.cmp(&pre_release), Ordering::Greater);
        assert_eq!(pre_release.partial_cmp(&release), Some(Ordering::Less));
        assert!(pre_release < release);
        Ok(())
    }

    #[test]
    fn precedence_decides_before_build_metadata() -> TestOutcome {
        // A lower version whose build metadata sorts higher is still lower, and
        // a pre-release is still before its release whatever either one's
        // build metadata is.
        let lower_with_larger_build = version("1.0.0+zzz")?;
        let higher = version("1.0.1+aaa")?;
        let pre_release_with_larger_build = version("1.0.0-rc.1+zzz")?;
        let release_with_smaller_build = version("1.0.0+aaa")?;
        let bare_release = version("1.0.0")?;

        assert_eq!(lower_with_larger_build.cmp(&higher), Ordering::Less);
        assert_eq!(
            pre_release_with_larger_build.cmp(&release_with_smaller_build),
            Ordering::Less
        );
        assert_eq!(
            pre_release_with_larger_build.cmp(&bare_release),
            Ordering::Less
        );
        Ok(())
    }

    #[test]
    fn versions_differing_only_in_build_metadata_are_different_and_ordered() -> TestOutcome {
        // Two versions with the same numbers and pre-release and different
        // build metadata are not equal, and `cmp` does not call them equal
        // either, from either side.
        let first = version("1.0.0+aaa")?;
        let second = version("1.0.0+bbb")?;
        let without = version("1.0.0")?;

        assert_ne!(first, second);
        assert_ne!(first.cmp(&second), Ordering::Equal);
        assert_eq!(first.cmp(&second), second.cmp(&first).reverse());
        assert_ne!(without, first);
        assert_ne!(without.cmp(&first), Ordering::Equal);
        Ok(())
    }

    #[test]
    fn the_order_agrees_with_equality() -> TestOutcome {
        // `cmp` is `Equal` exactly when `==` is true, over versions that are
        // identical, differ in build metadata, or differ in precedence.
        let texts = ["1.0.0", "1.0.0+aaa", "1.0.0+bbb", "1.0.0-rc.1", "1.0.1"];
        for left_text in texts {
            for right_text in texts {
                let left = version(left_text)?;
                let right = version(right_text)?;
                assert_eq!(
                    left.cmp(&right) == Ordering::Equal,
                    left == right,
                    "{left_text} against {right_text}"
                );
            }
        }
        assert_eq!(
            version("1.0.0+aaa")?.cmp(&version("1.0.0+aaa")?),
            Ordering::Equal
        );
        assert_eq!(version("1.0.0+aaa")?, version("1.0.0+aaa")?);
        Ok(())
    }
}
