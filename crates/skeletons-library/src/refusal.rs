//! Something the check could not determine, and what it concerns.

use std::path::{Path, PathBuf};

use skeletons_engine::{RefusalRecord, RefusedWearing};

/// What kind of refusal one is, by what it says is wrong.
///
/// The names are the `kind` words `check --json` reports, in `UpperCamelCase`.
/// The first six are about a wearing that resolved to no skeleton at all. The
/// last four name one or more worn skeletons, and each of those skeletons
/// holds the refusal in its [`Files::Refused`](crate::Files::Refused).
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Refusal {
    kind: RefusalKind,
    wearings: Vec<Wearing>,
    paths: Vec<PathBuf>,
}

impl Refusal {
    pub(crate) fn from_record(record: &RefusalRecord) -> Self {
        let refusal = Self {
            kind: RefusalKind::from(record.kind),
            wearings: record.wearings.iter().map(Wearing::from_engine).collect(),
            paths: record.paths.iter().map(PathBuf::from).collect(),
        };
        assert!(
            !refusal.wearings.is_empty(),
            "every refusal concerns at least one wearing"
        );
        refusal
    }

    /// What kind of refusal this is.
    #[must_use]
    pub fn kind(&self) -> RefusalKind {
        self.kind
    }

    /// The wearings this refusal concerns: one, or every claimant for an
    /// overlap. Never empty.
    #[must_use]
    pub fn wearings(&self) -> &[Wearing] {
        &self.wearings
    }

    /// The paths a skeleton renders that this refusal concerns, relative to the
    /// workspace root. May be empty.
    ///
    /// An overlap names the wearings and the paths separately, and does not
    /// say which wearing claims which path. An unsafe path can name a path that
    /// never became a claimed file.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The package named by the one wearing of a refusal about a wearing that
    /// resolved to no skeleton, or `None` when that wearing names none.
    pub(crate) fn package_named(&self) -> Option<&str> {
        assert!(
            self.kind.concerns_unidentified_wearing(),
            "only a refusal about an unidentified wearing has exactly one wearing"
        );
        let [wearing] = self.wearings.as_slice() else {
            unreachable!("a refusal about an unidentified wearing concerns one wearing")
        };
        wearing.package()
    }
}
