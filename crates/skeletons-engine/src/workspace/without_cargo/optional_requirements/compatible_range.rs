//! The versions Cargo takes to be one package, and whether any of them meets a requirement.
//!
//! Cargo keeps one version of a package for each range of versions it calls compatible: it
//! looks at the first part of a version that is not zero, so 4.5.6 and 4.6.0 are one package,
//! 0.3.1 and 0.3.2 are one, 0.3.1 and 0.4.0 are two, and so are 0.0.3 and 0.0.4. A pre-release
//! does not count: 4.0.0-alpha is in the range of 4.5.6.

use semver::{Comparator, Op, Prerelease, Version, VersionReq};

/// The versions Cargo takes to be the one package a version is, named by the first part of it
/// that is not zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompatibleRange {
    /// Every version with this major, which is not zero.
    Major(u64),
    /// Every version with a major of zero and this minor, which is not zero.
    Minor(u64),
    /// Every version with a major and a minor of zero and this patch.
    Patch(u64),
}

impl CompatibleRange {
    /// The range `version` is in, whatever its pre-release.
    pub(super) fn of(version: &Version) -> Self {
        let range = if version.major > 0 {
            Self::Major(version.major)
        } else if version.minor > 0 {
            Self::Minor(version.minor)
        } else {
            Self::Patch(version.patch)
        };
        assert!(
            range.contains(version),
            "a version is in the range it names"
        );
        range
    }

    /// Whether some version in this range meets `requirement`, or may: a registry may hold one,
    /// and this cannot see what a registry holds.
    ///
    /// A requirement is an intersection of intervals of versions, and an interval's ends are
    /// the versions its comparators write, so an intersection that holds a version of the range
    /// holds the lowest version of the range or one of a closed set built from each comparator:
    /// the version it writes, with and without its pre-release, the version just after it at the
    /// precision it is written in, and, when it writes a pre-release, the pre-releases just after
    /// that one and the lowest one of its release. Each is tried with `VersionReq::matches`,
    /// which is the matcher Cargo uses.
    pub(super) fn admits_a_version_meeting(&self, requirement: &VersionReq) -> bool {
        // `Op` is non-exhaustive, so a later `semver` may add an operator whose versions are not
        // an interval with ends the comparator writes, and the closed set above would miss
        // them. TS-ASSERT-SPACE: the domain is open, and the unknown operator is handled
        // explicitly, on the side that refuses, which is the side that cannot report a package
        // wrongly.
        let all_known = requirement
            .comparators
            .iter()
            .all(|comparator| is_an_interval_operator(comparator.op));
        if all_known {
            let lowest = self.lowest();
            assert!(
                self.contains(&lowest),
                "the lowest version of a range is in it"
            );
            std::iter::once(lowest)
                .chain(requirement.comparators.iter().flat_map(candidates_of))
                .any(|candidate| self.contains(&candidate) && requirement.matches(&candidate))
        } else {
            true
        }
    }

    /// The lowest version of this range, with no pre-release.
    const fn lowest(self) -> Version {
        match self {
            Self::Major(major) => Version::new(major, 0, 0),
            Self::Minor(minor) => Version::new(0, minor, 0),
            Self::Patch(patch) => Version::new(0, 0, patch),
        }
    }

    /// Whether `candidate` is in this range.
    const fn contains(self, candidate: &Version) -> bool {
        match self {
            Self::Major(major) => candidate.major == major,
            Self::Minor(minor) => candidate.major == 0 && candidate.minor == minor,
            Self::Patch(patch) => {
                candidate.major == 0 && candidate.minor == 0 && candidate.patch == patch
            }
        }
    }
}

/// Whether `operator` is one of the eight `semver` has, each of which matches an interval of
/// versions.
const fn is_an_interval_operator(operator: Op) -> bool {
    matches!(
        operator,
        Op::Exact
            | Op::Greater
            | Op::GreaterEq
            | Op::Less
            | Op::LessEq
            | Op::Tilde
            | Op::Caret
            | Op::Wildcard
    )
}

/// The versions worth trying for `comparator`: the ones in the closed set
/// [`CompatibleRange::admits_a_version_meeting`] describes.
fn candidates_of(comparator: &Comparator) -> Vec<Version> {
    let (minor, patch) = (comparator.minor.unwrap_or(0), comparator.patch.unwrap_or(0));
    let written = Version::new(comparator.major, minor, patch);
    // The version just after the one written, at the precision it is written in: `>4.2.3` first
    // admits 4.2.4, `>4.2` first admits 4.3.0, and `>4` first admits 5.0.0. A part at the
    // largest value has no successor, and the version written is tried again, which is harmless.
    let after = match (comparator.minor, comparator.patch) {
        (Some(minor), Some(patch)) => {
            Version::new(comparator.major, minor, patch.saturating_add(1))
        }
        (Some(minor), None) => Version::new(comparator.major, minor.saturating_add(1), 0),
        (None, _) => Version::new(comparator.major.saturating_add(1), 0, 0),
    };
    let mut candidates = vec![written.clone(), after];
    if !comparator.pre.is_empty() {
        // A pre-release meets a requirement only at the release a comparator writes one for, and
        // there the interval's lowest end is the pre-release written, the one just after it
        // (the same followed by `.0`), or the lowest pre-release of the release, `0`.
        for pre in [
            comparator.pre.clone(),
            pre_release(&format!("{}.0", comparator.pre.as_str())),
            pre_release("0"),
        ] {
            candidates.push(Version {
                pre,
                ..written.clone()
            });
        }
    }
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.build.is_empty()),
        "a candidate has no build metadata, which no ordering or matching reads"
    );
    candidates
}

/// The pre-release `text`, which is one of the texts in this module: a valid one.
fn pre_release(text: &str) -> Prerelease {
    let Ok(pre) = Prerelease::new(text) else {
        unreachable!("a pre-release followed by `.0`, and `0`, are pre-releases: {text}")
    };
    pre
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use semver::{Version, VersionReq};

    use super::CompatibleRange;

    /// Whether the range of the compatible versions of `locked` admits one that meets
    /// `requirement`.
    fn admits(locked: &str, requirement: &str) -> bool {
        let locked = Version::parse(locked).expect("a version written in this test");
        let requirement = VersionReq::parse(requirement).expect("a requirement written here");
        CompatibleRange::of(&locked).admits_a_version_meeting(&requirement)
    }

    #[test]
    fn a_requirement_the_range_of_a_major_can_meet_with_a_newer_version_is_admitted() {
        // With 4.5.6 locked, every version from 4.0.0 up to but not including 5.0.0 is the
        // same package to Cargo. Each requirement here is met by one of them that is not
        // 4.5.6, which is a version a registry may hold, or is met by 4.5.6 itself.
        for requirement in [
            "^4.6",
            "4.6",
            "=4.5.5",
            "~4.5.7",
            "~4.6",
            ">=4.6, <4.7",
            ">4.5.6",
            "<4.5.6",
            ">=4",
            "4.*",
            "4.6.*",
            "=4.0.0",
            "^4.5",
            "^4",
        ] {
            assert!(admits("4.5.6", requirement), "4.5.6 against {requirement}");
        }
    }

    #[test]
    fn a_requirement_no_version_of_the_range_can_meet_is_not_admitted() {
        // Requirements that sit entirely in another major, or that no version meets at all.
        for requirement in [
            "^5",
            "5",
            "^3",
            "=5.0.0",
            ">=5",
            ">=5.0.0, <6",
            "<4",
            "<4.0.0",
            "5.*",
            ">4.2.3, <4.2.4",
            ">=4.6, <4.6",
        ] {
            assert!(!admits("4.5.6", requirement), "4.5.6 against {requirement}");
        }
    }

    #[test]
    fn the_range_of_a_zero_major_is_its_minor_and_of_a_zero_minor_its_patch() {
        // Cargo calls 0.3.1 and 0.3.2 one package, and 0.3.1 and 0.4.0 two. Below that, only
        // the patch is the same: 0.0.3 has no compatible version but itself.
        assert!(!admits("0.3.1", "^0.4"));
        assert!(admits("0.3.1", "^0.3.2"));
        assert!(admits("0.3.1", "^0.3"));
        assert!(!admits("0.3.1", "^0.2"));
        assert!(!admits("0.0.3", "^0.0.4"));
        assert!(admits("0.0.3", "^0.0.3"));
        assert!(!admits("0.0.3", "^0.1"));
        assert!(!admits("0.0.3", ">=0.0.4"));
    }

    #[test]
    fn a_pre_release_is_in_the_range_of_its_release_and_the_locked_pre_release_is_ignored() {
        // Cargo's compatibility looks at the major, minor and patch and not at a pre-release, so
        // 4.0.0-alpha is the same package as 4.5.6, and a locked 4.5.6-rc.1 has the range of
        // 4.5.6. A pre-release meets a requirement only when the requirement names one at its
        // own major, minor and patch.
        assert!(admits("4.5.6", "=4.0.0-alpha"));
        assert!(admits("4.5.6-rc.1", "^4.6"));
        assert!(!admits("4.5.6-rc.1", "^5"));
        assert!(admits("4.5.6", ">4.6.0-alpha, <4.6.0-beta"));
        assert!(!admits("4.5.6", "=5.0.0-alpha"));
        // Only a pre-release lies above one end and below the other: 4.0.1-0, the lowest one of
        // its release, is the version nothing else in the range is.
        assert!(admits("4.5.6", ">4.0.0, <4.0.1-alpha"));
        assert!(!admits("4.5.6", ">4.0.0, <4.0.1"));
    }

    /// The largest major, minor and patch a requirement or a locked version is written with.
    /// The numbers are small so that comparators meet each other often.
    const WRITTEN_PART_MAX: u64 = 3;

    /// The largest major, minor and patch the grid looks at: one more than a requirement is
    /// written with, which is as far as the version just after one it writes goes.
    const PART_MAX: u64 = WRITTEN_PART_MAX + 2;

    /// The pre-releases the brute force looks at, which hold every one the range builds a
    /// candidate from and others besides: a numeric identifier, and an identifier followed by
    /// the one that would come next.
    const PRE_RELEASES: [&str; 7] = ["0", "1", "alpha", "alpha.0", "alpha.1", "beta", "beta.0"];

    /// Every version up to `PART_MAX` in each part with no pre-release and with each of
    /// [`PRE_RELEASES`].
    fn grid() -> Vec<Version> {
        let mut versions = Vec::new();
        for major in 0..=PART_MAX {
            for minor in 0..=PART_MAX {
                for patch in 0..=PART_MAX {
                    versions.push(Version::new(major, minor, patch));
                    for pre in PRE_RELEASES {
                        versions.push(
                            Version::parse(&format!("{major}.{minor}.{patch}-{pre}"))
                                .expect("a version this test builds"),
                        );
                    }
                }
            }
        }
        versions
    }

    /// What Cargo compares to tell whether two versions of a package are one: the first part
    /// that is not zero, with the parts before it.
    fn cargo_compatibility(version: &Version) -> (u8, u64) {
        if version.major > 0 {
            (1, version.major)
        } else if version.minor > 0 {
            (2, version.minor)
        } else {
            (3, version.patch)
        }
    }

    /// The text of one comparator over the parts the grid holds: an operator and a version of
    /// one, two or three parts, with a pre-release on three; or a wildcard.
    fn comparator_text() -> impl Strategy<Value = String> {
        let version = (
            0..=WRITTEN_PART_MAX,
            prop::option::of((
                0..=WRITTEN_PART_MAX,
                prop::option::of((
                    0..=WRITTEN_PART_MAX,
                    prop::sample::select(vec!["", "-alpha", "-beta"]),
                )),
            )),
        )
            .prop_map(|(major, rest)| match rest {
                None => format!("{major}"),
                Some((minor, None)) => format!("{major}.{minor}"),
                Some((minor, Some((patch, pre)))) => format!("{major}.{minor}.{patch}{pre}"),
            });
        let operated = (
            prop::sample::select(vec!["=", ">", ">=", "<", "<=", "~", "^", ""]),
            version,
        )
            .prop_map(|(operator, version)| format!("{operator}{version}"));
        let wildcard = (0..=WRITTEN_PART_MAX, prop::option::of(0..=WRITTEN_PART_MAX)).prop_map(
            |(major, minor)| {
                minor.map_or_else(
                    || format!("{major}.*"),
                    |minor| format!("{major}.{minor}.*"),
                )
            },
        );
        prop_oneof![4 => operated, 1 => wildcard]
    }

    /// The text of a requirement that closes in on one release: a lower end and an upper end
    /// written within two patches of each other, either with a pre-release. These are the
    /// shapes whose only versions are pre-releases, which two independent comparators seldom
    /// make.
    fn interval_text() -> impl Strategy<Value = String> {
        (
            (0..=WRITTEN_PART_MAX, 0..=WRITTEN_PART_MAX, 0..=1u64),
            prop::sample::select(vec![">", ">="]),
            prop::sample::select(vec!["", "-alpha", "-beta"]),
            0..=2u64,
            prop::sample::select(vec!["<", "<="]),
            prop::sample::select(vec!["", "-alpha", "-beta"]),
        )
            .prop_map(
                |((major, minor, patch), lower, lower_pre, extra, upper, upper_pre)| {
                    let upper_patch = patch + extra;
                    format!(
                        "{lower}{major}.{minor}.{patch}{lower_pre}, \
                         {upper}{major}.{minor}.{upper_patch}{upper_pre}"
                    )
                },
            )
    }

    /// The text of a requirement: comparators written independently, or an interval.
    fn requirement_text() -> impl Strategy<Value = String> {
        prop_oneof![
            2 => prop::collection::vec(comparator_text(), 1..=3)
                .prop_map(|comparators| comparators.join(", ")),
            1 => interval_text(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4096))]

        /// Compares `admits_a_version_meeting` with looking at every version of a grid: the
        /// range admits when one of the grid has the locked version's compatibility and meets
        /// the requirement. The grid holds every version the range tries, and others besides
        /// that it never tries, such as more pre-releases, so a requirement only one of those
        /// meets is found by looking and missed by the range. The meeting is
        /// `VersionReq::matches`, the matcher Cargo uses. The numbers are small and the cases
        /// many so that two comparators close in on one pre-release often enough.
        #[test]
        fn admitting_agrees_with_looking_at_every_version_of_a_grid(
            locked in (
                0..=WRITTEN_PART_MAX,
                0..=WRITTEN_PART_MAX,
                0..=WRITTEN_PART_MAX,
            ),
            text in requirement_text(),
        ) {
            let Ok(requirement) = VersionReq::parse(&text) else {
                return Err(TestCaseError::reject("a text the grammar refuses"));
            };
            let locked = Version::new(locked.0, locked.1, locked.2);

            let by_range = CompatibleRange::of(&locked).admits_a_version_meeting(&requirement);
            let by_looking = grid().iter().any(|candidate| {
                cargo_compatibility(candidate) == cargo_compatibility(&locked)
                    && requirement.matches(candidate)
            });

            prop_assert_eq!(by_range, by_looking, "{} against {}", locked, text);
        }
    }
}
