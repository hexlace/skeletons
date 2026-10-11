//! Something the check could not determine, and what it concerns.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use skeletons_engine::{RefusalRecord, RefusedWearing};

/// What kind of refusal one is, by what it says is wrong.
///
/// The names are the `kind` words `check --json` reports, in `UpperCamelCase`.
/// The first six are about a wearing that resolved to no skeleton at all. The
/// last four name one or more worn skeletons, and each of those skeletons
/// holds the refusal in its [`Files::Refused`](crate::Files::Refused).
///
/// A kind has no `Display`, because this library says nothing in sentences:
/// what a refusal means to a caller's users is the caller's to write. For a
/// kind a caller's version of this crate does not know, [`as_str`] is what to
/// print, log or key on.
///
/// [`as_str`]: RefusalKind::as_str
///
/// # Examples
///
/// ```
/// use skeletons_library::RefusalKind;
///
/// fn say(kind: RefusalKind) -> String {
///     match kind {
///         RefusalKind::Overlap => "two skeletons claim the same path".to_owned(),
///         // A kind a later version adds still has a word of its own.
///         other => format!("refused: {}", other.as_str()),
///     }
/// }
///
/// assert_eq!(say(RefusalKind::Overlap), "two skeletons claim the same path");
/// assert_eq!(say(RefusalKind::Unresolved), "refused: unresolved");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefusalKind {
    /// A wearing table, or a manifest's whole `skeletons` table, is not a
    /// table.
    NotATable,
    /// A real dependency is declared under a key a skeleton keeps for its own
    /// declarations.
    ReservedKey,
    /// A wearing table is named for a key that no dependency of its manifest
    /// is declared under.
    NamesNoDependency,
    /// The dependency resolved to no locked package.
    Unresolved,
    /// The dependency resolved to more than one locked package.
    Ambiguous,
    /// The package the dependency resolved to is not a skeleton.
    NotASkeleton,
    /// A value recorded for one of the skeleton's options was refused.
    OptionRefused,
    /// The skeleton itself is defective.
    SkeletonInvalid,
    /// Two or more worn skeletons claim colliding paths.
    Overlap,
    /// A claimed path cannot safely be read or written.
    UnsafePath,
}

impl RefusalKind {
    /// The word for this kind: a stable identifier, lowercase and hyphenated,
    /// and the same one the command line's JSON reports as `kind`.
    ///
    /// It is an identifier and not prose, so it is safe to log, compare or
    /// key on, and it will not be reworded. A kind added later has a word of
    /// its own.
    ///
    /// # Examples
    ///
    /// ```
    /// use skeletons_library::RefusalKind;
    ///
    /// assert_eq!(RefusalKind::NamesNoDependency.as_str(), "names-no-dependency");
    /// ```
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotATable => "not-a-table",
            Self::ReservedKey => "reserved-key",
            Self::NamesNoDependency => "names-no-dependency",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::NotASkeleton => "not-a-skeleton",
            Self::OptionRefused => "option-refused",
            Self::SkeletonInvalid => "skeleton-invalid",
            Self::Overlap => "overlap",
            Self::UnsafePath => "unsafe-path",
        }
    }

    /// Whether this kind is about a wearing that resolved to no skeleton, which
    /// no worn skeleton can carry.
    pub(crate) fn concerns_unidentified_wearing(self) -> bool {
        match self {
            Self::NotATable
            | Self::ReservedKey
            | Self::NamesNoDependency
            | Self::Unresolved
            | Self::Ambiguous
            | Self::NotASkeleton => true,
            Self::OptionRefused | Self::SkeletonInvalid | Self::Overlap | Self::UnsafePath => false,
        }
    }
}

impl From<skeletons_engine::RefusalKind> for RefusalKind {
    fn from(kind: skeletons_engine::RefusalKind) -> Self {
        match kind {
            skeletons_engine::RefusalKind::NotATable => Self::NotATable,
            skeletons_engine::RefusalKind::ReservedKey => Self::ReservedKey,
            skeletons_engine::RefusalKind::NamesNoDependency => Self::NamesNoDependency,
            skeletons_engine::RefusalKind::Unresolved => Self::Unresolved,
            skeletons_engine::RefusalKind::Ambiguous => Self::Ambiguous,
            skeletons_engine::RefusalKind::NotASkeleton => Self::NotASkeleton,
            skeletons_engine::RefusalKind::OptionRefused => Self::OptionRefused,
            skeletons_engine::RefusalKind::SkeletonInvalid => Self::SkeletonInvalid,
            skeletons_engine::RefusalKind::Overlap => Self::Overlap,
            skeletons_engine::RefusalKind::UnsafePath => Self::UnsafePath,
        }
    }
}

/// Where a project asks to wear a skeleton: a manifest, and perhaps a
/// dependency key.
///
/// A [`WornSkeleton`](crate::WornSkeleton) is a wearing that resolved to a
/// skeleton. A `Wearing` is only the place the project asks, which is all some
/// refusals have, and the package its dependency names when the manifest
/// declares one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct Wearing {
    manifest: PathBuf,
    dependency_key: Option<String>,
    package: Option<String>,
}

impl Wearing {
    fn from_engine(wearing: &RefusedWearing) -> Self {
        Self {
            manifest: PathBuf::from(&wearing.manifest),
            dependency_key: wearing.dependency.clone(),
            package: wearing.package.clone(),
        }
    }

    /// The wearing member's `Cargo.toml`, relative to the workspace root.
    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// The dependency key the wearing table sits under, or `None` when the
    /// refusal is about the manifest's whole `skeletons` table.
    #[must_use]
    pub fn dependency_key(&self) -> Option<&str> {
        self.dependency_key.as_deref()
    }

    /// The package the dependency under this key names, as the manifest
    /// declares it. `None` when the manifest declares no dependency under the
    /// key, or the refusal is about the whole `skeletons` table. For a wearing
    /// that resolved to a skeleton it is that skeleton's package name.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
}

/// One thing the check could not determine, and what it concerns.
///
/// A refusal is a finding, not a failure of the call: every other wearing in
/// the workspace is still reported.
#[derive(Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Refusal {
    data: Arc<RefusalData>,
}

/// What a [`Refusal`] holds, built once. A refusal is handed to every worn
/// skeleton it names and to every package it could concern, and each of those
/// holds the same data rather than a copy of its wearings and paths.
#[derive(PartialEq, Eq, Hash)]
struct RefusalData {
    kind: RefusalKind,
    wearings: Vec<Wearing>,
    paths: Vec<PathBuf>,
}

impl Refusal {
    pub(crate) fn from_record(record: &RefusalRecord) -> Self {
        let data = RefusalData {
            kind: RefusalKind::from(record.kind),
            wearings: record.wearings.iter().map(Wearing::from_engine).collect(),
            paths: record.paths.iter().map(PathBuf::from).collect(),
        };
        assert!(
            !data.wearings.is_empty(),
            "every refusal concerns at least one wearing"
        );
        Self {
            data: Arc::new(data),
        }
    }

    /// Whether `other` holds the very data this refusal holds, which two
    /// refusals that merely compare equal do not.
    #[cfg(test)]
    pub(crate) fn shares_data_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    /// What kind of refusal this is.
    #[must_use]
    pub fn kind(&self) -> RefusalKind {
        self.data.kind
    }

    /// The wearings this refusal concerns: one, or every claimant for an
    /// overlap. Never empty.
    #[must_use]
    pub fn wearings(&self) -> &[Wearing] {
        &self.data.wearings
    }

    /// The paths a skeleton renders that this refusal concerns, relative to the
    /// workspace root. May be empty.
    ///
    /// An overlap names the wearings and the paths separately, and does not
    /// say which wearing claims which path. An unsafe path can name a path that
    /// never became a claimed file.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.data.paths
    }

    /// The package named by the one wearing of a refusal about a wearing that
    /// resolved to no skeleton, or `None` when that wearing names none.
    pub(crate) fn package_named(&self) -> Option<&str> {
        assert!(
            self.data.kind.concerns_unidentified_wearing(),
            "only a refusal about an unidentified wearing has exactly one wearing"
        );
        let [wearing] = self.data.wearings.as_slice() else {
            unreachable!("a refusal about an unidentified wearing concerns one wearing")
        };
        wearing.package()
    }
}

impl fmt::Debug for Refusal {
    // Written out so that the shared data is not a level of its own in what
    // `{:?}` and `{:#?}` print: a refusal shows its kind, wearings and paths.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Refusal")
            .field("kind", &self.data.kind)
            .field("wearings", &self.data.wearings)
            .field("paths", &self.data.paths)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::RefusalKind;

    /// Every kind, in the order `position` numbers them.
    const KINDS: [RefusalKind; 10] = [
        RefusalKind::NotATable,
        RefusalKind::ReservedKey,
        RefusalKind::NamesNoDependency,
        RefusalKind::Unresolved,
        RefusalKind::Ambiguous,
        RefusalKind::NotASkeleton,
        RefusalKind::OptionRefused,
        RefusalKind::SkeletonInvalid,
        RefusalKind::Overlap,
        RefusalKind::UnsafePath,
    ];

    /// The place of `kind` in [`KINDS`]. The match has no wildcard arm, so a
    /// variant added without an arm here does not compile.
    const fn position(kind: RefusalKind) -> usize {
        match kind {
            RefusalKind::NotATable => 0,
            RefusalKind::ReservedKey => 1,
            RefusalKind::NamesNoDependency => 2,
            RefusalKind::Unresolved => 3,
            RefusalKind::Ambiguous => 4,
            RefusalKind::NotASkeleton => 5,
            RefusalKind::OptionRefused => 6,
            RefusalKind::SkeletonInvalid => 7,
            RefusalKind::Overlap => 8,
            RefusalKind::UnsafePath => 9,
        }
    }

    /// The word the documentation gives `kind`, written out separately from
    /// `as_str`. The match has no wildcard arm, so a variant added without a
    /// word here does not compile.
    fn documented_word(kind: RefusalKind) -> &'static str {
        match kind {
            RefusalKind::NotATable => "not-a-table",
            RefusalKind::ReservedKey => "reserved-key",
            RefusalKind::NamesNoDependency => "names-no-dependency",
            RefusalKind::Unresolved => "unresolved",
            RefusalKind::Ambiguous => "ambiguous",
            RefusalKind::NotASkeleton => "not-a-skeleton",
            RefusalKind::OptionRefused => "option-refused",
            RefusalKind::SkeletonInvalid => "skeleton-invalid",
            RefusalKind::Overlap => "overlap",
            RefusalKind::UnsafePath => "unsafe-path",
        }
    }

    #[test]
    fn every_kind_is_listed_once_in_order() {
        // `position` cannot be written without naming every variant, so a
        // list that is shorter than it, or out of its order, is a variant the
        // tests below would skip.
        for (index, kind) in KINDS.into_iter().enumerate() {
            assert_eq!(position(kind), index, "{kind:?}");
        }
    }

    #[test]
    fn every_kind_has_its_documented_word() {
        // Each variant's `as_str` is compared with the word written out in
        // `documented_word`, whose match is exhaustive.
        for kind in KINDS {
            assert_eq!(kind.as_str(), documented_word(kind), "{kind:?}");
        }
    }

    #[test]
    fn no_two_kinds_share_a_word() {
        // A word that named two kinds could not be keyed on.
        for (index, kind) in KINDS.into_iter().enumerate() {
            for other in KINDS.into_iter().skip(index + 1) {
                assert_ne!(kind.as_str(), other.as_str(), "{kind:?} and {other:?}");
            }
        }
    }
}
