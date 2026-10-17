//! A locked package's identity, as `Cargo.lock` writes it.

use skeletons_engine::LockedPackageId;

/// One `[[package]]` entry of `Cargo.lock`, named as the lockfile writes it:
/// its `name`, its `version` and its `source`.
///
/// Those three together are Cargo's own identity for a package, so they stay
/// unambiguous when two versions of one name, or one version from two sources,
/// are locked. Each is the text of the lockfile, exactly: `source` is
/// `registry+https://github.com/rust-lang/crates.io-index` or
/// `git+https://…#<commit>`, never a path, since a package with no `source` is
/// a path package and is found from the manifests.
///
/// The fields are public and the type is not `#[non_exhaustive]`, which is
/// this crate's one exception to both. Three strings in a constructor can be
/// passed in the wrong order and still compile, and a struct literal names each
/// one. The three fields are the whole of a lockfile's identity for a package,
/// so there is no field a later version would add, and nothing to keep valid.
///
/// # Examples
///
/// ```
/// use skeletons_library::LockedPackage;
///
/// let package = LockedPackage {
///     name: "house-rules".to_owned(),
///     version: "4.5.6".to_owned(),
///     source: "registry+https://github.com/rust-lang/crates.io-index".to_owned(),
/// };
/// assert_eq!(package.name, "house-rules");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockedPackage {
    /// The package's name.
    pub name: String,
    /// The package's version, as the lockfile writes it.
    pub version: String,
    /// The package's source, as the lockfile writes it.
    pub source: String,
}

impl LockedPackage {
    // Functions of the crate's own rather than `From`: a trait impl naming an
    // engine type would be public, and put `skeletons-engine` in this crate's
    // interface.
    pub(crate) fn from_engine(identity: LockedPackageId) -> Self {
        Self {
            name: identity.name,
            version: identity.version,
            source: identity.source,
        }
    }

    pub(crate) fn to_engine(&self) -> LockedPackageId {
        LockedPackageId {
            name: self.name.clone(),
            version: self.version.clone(),
            source: self.source.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use skeletons_engine::LockedPackageId;

    use super::LockedPackage;

    fn identity() -> LockedPackageId {
        LockedPackageId {
            name: "house-rules".to_owned(),
            version: "4.5.6".to_owned(),
            source: "git+https://example.test/house-rules#abc".to_owned(),
        }
    }

    #[test]
    fn an_identity_keeps_its_three_fields_in_both_directions() {
        // Each field is given a different value, so a swap between the
        // fields in either conversion changes what comes back.
        let package = LockedPackage::from_engine(identity());

        assert_eq!(package.name, "house-rules");
        assert_eq!(package.version, "4.5.6");
        assert_eq!(package.source, "git+https://example.test/house-rules#abc");
        assert_eq!(package.to_engine(), identity());
    }
}
