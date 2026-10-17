//! A locked package's identity as `Cargo.lock` writes it.

/// One `[[package]]` entry of `Cargo.lock`, named as the lockfile writes it.
///
/// The text of `name`, `version` and `source` is Cargo's own package
/// identity, which stays unambiguous when two versions of one name, or one
/// version from two sources, are locked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockedPackageId {
    /// The package's name.
    pub name: String,
    /// The package's version, as the lockfile writes it.
    pub version: String,
    /// The package's source, such as `registry+https://…` or `git+https://…`.
    pub source: String,
}
