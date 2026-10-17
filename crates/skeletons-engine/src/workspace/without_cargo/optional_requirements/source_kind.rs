//! Which versions of a package its source can give a resolve, and so which requirements it
//! cannot meet.

use semver::{Version, VersionReq};

use super::compatible_range::CompatibleRange;
use crate::workspace::without_cargo::package_manifest::DeclaredSource;

/// Where a worn package is locked from, as far as the versions it can give a resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceKind {
    /// A registry holds many versions, and Cargo keeps one of them for each compatible range.
    Registry,
    /// A directory holds one version.
    Path,
    /// A git repository, at one reference, holds one version.
    Git,
}

impl SourceKind {
    /// The kind of source `declared` asks for, which the lock agrees with when the declaration
    /// targets a worn package (`locked_source::agrees`).
    pub(super) const fn of(declared: &DeclaredSource) -> Self {
        match declared {
            DeclaredSource::Registry { .. } => Self::Registry,
            DeclaredSource::Path { .. } => Self::Path,
            DeclaredSource::Git { .. } => Self::Git,
        }
    }

    /// Why Cargo cannot resolve a workspace where the worn package, locked at `locked`, is asked
    /// for at `requirement`: the words that follow "does not meet", or nothing when Cargo can.
    ///
    /// A registry gives a second version beside the locked one for a requirement in another
    /// compatible range, so only a requirement some version in the locked version's range could
    /// meet is one Cargo cannot resolve: it keeps the one it has and finds it too old. A path or
    /// a git source holds one version, so any requirement it does not meet cannot be resolved,
    /// in whatever range it is written.
    pub(super) fn why_unmet(
        self,
        locked: &Version,
        requirement: &VersionReq,
    ) -> Option<&'static str> {
        if requirement.matches(locked) {
            return None;
        }
        match self {
            Self::Registry => CompatibleRange::of(locked)
                .admits_a_version_meeting(requirement)
                .then_some(" within its compatible range"),
            Self::Path => Some(", and its path source holds only that version"),
            Self::Git => Some(", and its git source holds only that version"),
        }
    }
}

#[cfg(test)]
mod tests {
    use semver::{Version, VersionReq};

    use super::SourceKind;
    use crate::workspace::without_cargo::canonical_git_url::CanonicalGitUrl;
    use crate::workspace::without_cargo::package_manifest::{DeclaredSource, GitReference};

    fn why(kind: SourceKind, locked: &str, requirement: &str) -> Option<&'static str> {
        kind.why_unmet(
            &Version::parse(locked).expect("a version"),
            &VersionReq::parse(requirement).expect("a requirement"),
        )
    }

    #[test]
    fn a_requirement_the_locked_version_meets_is_never_unmet() {
        // Every kind of source: the locked version is the one Cargo uses, so it is met.
        for kind in [SourceKind::Registry, SourceKind::Path, SourceKind::Git] {
            assert_eq!(why(kind, "1.2.3", "^1.2"), None, "{kind:?}");
        }
    }

    #[test]
    fn a_registry_is_unmet_only_by_a_requirement_in_the_locked_range() {
        // `^1.3` could be met by 1.3.0, which is in the range 1.2.3 is in, so Cargo keeps the
        // one version it has and finds it too old. `^2` is another range, which Cargo gives a
        // second version beside the lock, so it is not refused.
        assert_eq!(
            why(SourceKind::Registry, "1.2.3", "^1.3"),
            Some(" within its compatible range")
        );
        assert_eq!(why(SourceKind::Registry, "1.2.3", "^2"), None);
    }

    #[test]
    fn a_path_or_git_source_is_unmet_by_a_requirement_in_any_range() {
        for requirement in ["^1.3", "^2", "^0.9", ">=3, <4"] {
            assert_eq!(
                why(SourceKind::Path, "1.2.3", requirement),
                Some(", and its path source holds only that version"),
                "path, {requirement}"
            );
            assert_eq!(
                why(SourceKind::Git, "1.2.3", requirement),
                Some(", and its git source holds only that version"),
                "git, {requirement}"
            );
        }
    }

    #[test]
    fn the_kind_follows_the_declared_source() {
        assert_eq!(
            SourceKind::of(&DeclaredSource::Registry { name: None }),
            SourceKind::Registry
        );
        assert_eq!(
            SourceKind::of(&DeclaredSource::Path {
                directory: std::path::PathBuf::from("/a")
            }),
            SourceKind::Path
        );
        assert_eq!(
            SourceKind::of(&DeclaredSource::Git {
                url: CanonicalGitUrl::parse("https://example.test/g").expect("a url"),
                reference: GitReference::DefaultBranch,
            }),
            SourceKind::Git
        );
    }
}
