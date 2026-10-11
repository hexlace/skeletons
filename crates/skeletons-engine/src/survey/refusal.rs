//! Why something could not be determined — every kind of refusal `check` and
//! `sync` report, as data. The sentences they are reported in are in
//! `message`, which only the commands compile.

#[cfg(feature = "commands")]
mod message;

use crate::claim::{Overlap, UnsafePathCause};
use crate::skeleton::RenderError;
use crate::workspace::{OptionShapeRefusal, WearingRefusal, WornId};

#[cfg(feature = "commands")]
pub(crate) use message::{
    CollisionWords, ONE_SKELETON_PER_FILE, join_and, many_paths_words, option_shape_detail,
    two_paths_words, unsafe_path_change_clause,
};

/// One refusal: something `check`/`sync` could not determine, either about
/// one worn skeleton or about no single one.
#[cfg_attr(
    not(feature = "commands"),
    expect(
        dead_code,
        reason = "the detail fields are built when a workspace is surveyed and read only by \
                  the sentences the commands print"
    )
)]
pub(crate) enum Refusal {
    /// A manifest's own wearing table could not name a worn skeleton at all.
    Wearing(WearingRefusal),
    /// A recorded option value's TOML shape a `Choice` cannot represent.
    OptionShape {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        refusal: OptionShapeRefusal,
    },
    /// The skeleton's own render refused — a defect in the skeleton itself
    /// (`error.file()` is `Some`) or an undeclared/malformed choice the
    /// render itself caught (`error.file()` is `None`).
    Render {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        error: RenderError,
    },
    /// Two or more claims collide.
    Overlap(Overlap),
    /// A claimed path could not be safely resolved on disk, would sit inside
    /// `.git`, holds a name git will not track, or holds a name too long.
    UnsafePath {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        path: String,
        cause: UnsafePathCause,
    },
}

/// What kind of refusal one is, by what it says is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefusalKind {
    /// A wearing table, or the whole `skeletons` table, is not a table.
    NotATable,
    /// A real dependency is declared under a key a skeleton keeps for its own
    /// declarations.
    ReservedKey,
    /// A wearing table is named for a key no dependency of its manifest is
    /// declared under.
    NamesNoDependency,
    /// The dependency resolved to no locked package.
    Unresolved,
    /// The dependency resolved to more than one locked package.
    Ambiguous,
    /// The resolved package is not a skeleton.
    NotASkeleton,
    /// What was recorded for the skeleton's options was refused: a value of
    /// the wrong shape or outside what the option allows, a value for an
    /// option the skeleton does not declare, or values whose render is too
    /// large.
    OptionRefused,
    /// The skeleton itself is defective.
    SkeletonInvalid,
    /// Two or more worn skeletons claim colliding paths.
    Overlap,
    /// A claimed path cannot safely be read or written.
    UnsafePath,
}

#[cfg(any(test, feature = "commands"))]
impl RefusalKind {
    /// The word this kind is reported as: the `kind` of `check --json` and
    /// the word `check`'s human output classifies a refusal by.
    pub(crate) const fn word(self) -> &'static str {
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
}

/// One wearing a refusal concerns: where a project asks to wear a skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedWearing {
    /// The wearing member's `Cargo.toml`, relative to the workspace root.
    pub manifest: String,
    /// The dependency key the wearing table sits under, or `None` when the
    /// refusal concerns the manifest's whole `skeletons` table.
    pub dependency: Option<String>,
    /// The package the dependency under this key names, as the manifest
    /// declares it; `None` when the manifest declares no dependency under the
    /// key, or the refusal concerns the whole table.
    pub package: Option<String>,
}

/// One refusal as data: what kind it is and what it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalRecord {
    /// What kind of refusal it is.
    pub kind: RefusalKind,
    /// The wearings it concerns: one, or every claimant for an overlap.
    pub wearings: Vec<RefusedWearing>,
    /// The claimed paths it concerns, relative to the workspace root; empty
    /// when it concerns none.
    pub paths: Vec<String>,
}

impl Refusal {
    /// Which kind of refusal this is.
    pub(crate) fn classify(&self) -> RefusalKind {
        match self {
            Self::Wearing(wearing) => match wearing {
                WearingRefusal::NotATable { .. } => RefusalKind::NotATable,
                WearingRefusal::ReservedKey { .. } => RefusalKind::ReservedKey,
                WearingRefusal::NamesNoDependency { .. } => RefusalKind::NamesNoDependency,
                WearingRefusal::Unresolved { .. } => RefusalKind::Unresolved,
                WearingRefusal::Ambiguous { .. } => RefusalKind::Ambiguous,
                WearingRefusal::NotASkeleton { .. } => RefusalKind::NotASkeleton,
            },
            Self::OptionShape { .. } => RefusalKind::OptionRefused,
            // A render refusal about a specific file is a defect in the
            // skeleton itself; one about no file at all (`error.file()` is
            // `None`) is about the wearer's own choices instead — the
            // render's own documented invariant (`skeleton-format.md`,
            // §Refusals) that a choice refusal never names a file.
            Self::Render { error, .. } => {
                if error.file().is_some() {
                    RefusalKind::SkeletonInvalid
                } else {
                    RefusalKind::OptionRefused
                }
            }
            Self::Overlap(_) => RefusalKind::Overlap,
            Self::UnsafePath { .. } => RefusalKind::UnsafePath,
        }
    }

    /// The `kind` this refusal reports in `--json`, and the word `check`'s
    /// human output classifies it by.
    #[cfg(any(test, feature = "commands"))]
    pub(crate) fn kind(&self) -> &'static str {
        self.classify().word()
    }

    /// This refusal as data: its kind, the wearings it concerns and the
    /// claimed paths it concerns.
    pub(crate) fn record(&self) -> RefusalRecord {
        let (wearings, paths) = match self {
            Self::Wearing(wearing) => (vec![wearing_of(wearing)], Vec::new()),
            Self::OptionShape { worn, skeleton, .. } | Self::Render { worn, skeleton, .. } => (
                vec![refused_wearing(
                    &worn.manifest,
                    Some(&worn.key),
                    Some(skeleton),
                )],
                Vec::new(),
            ),
            Self::Overlap(overlap) => (
                overlap
                    .claimants
                    .iter()
                    .map(|claimant| {
                        refused_wearing(
                            &claimant.manifest,
                            Some(&claimant.dependency),
                            Some(&claimant.skeleton),
                        )
                    })
                    .collect(),
                overlap
                    .paths
                    .iter()
                    .map(|path| path.as_str().to_owned())
                    .collect(),
            ),
            Self::UnsafePath {
                worn,
                skeleton,
                path,
                ..
            } => (
                vec![refused_wearing(
                    &worn.manifest,
                    Some(&worn.key),
                    Some(skeleton),
                )],
                vec![path.clone()],
            ),
        };
        RefusalRecord {
            kind: self.classify(),
            wearings,
            paths,
        }
    }

    /// The claimed path an `unsafe-path` refusal is about — the order more
    /// than one of a skeleton's own refusals are shown in (`check`'s own
    /// `refusals_for`). `None` for every other kind, which a skeleton never
    /// carries more than one of at once (see `Self::about_one_skeleton`).
    pub(crate) fn unsafe_path(&self) -> Option<&str> {
        match self {
            Self::UnsafePath { path, .. } => Some(path.as_str()),
            _ => None,
        }
    }
}

/// The wearing a [`WearingRefusal`] is about: the manifest, the dependency
/// key it names, or no key where it names the manifest's whole table, and the
/// package that dependency names where one is known.
fn wearing_of(wearing: &WearingRefusal) -> RefusedWearing {
    match wearing {
        WearingRefusal::NotATable {
            manifest,
            key,
            package,
        } => refused_wearing(manifest, key.as_deref(), package.as_deref()),
        WearingRefusal::ReservedKey {
            manifest,
            key,
            crate_name,
        } => refused_wearing(manifest, Some(key), Some(crate_name)),
        WearingRefusal::NamesNoDependency {
            manifest,
            dependency,
        } => refused_wearing(manifest, Some(dependency), None),
        WearingRefusal::Unresolved {
            manifest,
            dependency,
            package,
        } => refused_wearing(manifest, Some(dependency), Some(package)),
        WearingRefusal::Ambiguous {
            manifest,
            dependency,
            packages,
        } => {
            let Some((name, _)) = packages.first() else {
                unreachable!("a dependency is ambiguous only between several packages")
            };
            refused_wearing(manifest, Some(dependency), Some(name))
        }
        WearingRefusal::NotASkeleton {
            manifest,
            dependency,
            package: (name, _),
        } => refused_wearing(manifest, Some(dependency), Some(name)),
    }
}

fn refused_wearing(
    manifest: &str,
    dependency: Option<&str>,
    package: Option<&str>,
) -> RefusedWearing {
    RefusedWearing {
        manifest: manifest.to_owned(),
        dependency: dependency.map(str::to_owned),
        package: package.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::{Refusal, RefusalKind, RefusalRecord, RefusedWearing};
    use crate::claim::{Claimant, Overlap, UnsafePathCause};
    use crate::skeleton::{Reason, RenderError, SkeletonIdentity};
    use crate::survey::poison::{REFUSAL_KINDS, assert_every_kind, claim, kind_index};
    use crate::workspace::{OptionShapeRefusal, ShapeFound, WearingRefusal, WornId};

    fn wearing(manifest: &str, dependency: Option<&str>, package: Option<&str>) -> RefusedWearing {
        RefusedWearing {
            manifest: manifest.to_owned(),
            dependency: dependency.map(str::to_owned),
            package: package.map(str::to_owned),
        }
    }

    fn worn() -> WornId {
        WornId {
            manifest: "member/Cargo.toml".to_owned(),
            key: "ci".to_owned(),
        }
    }

    fn claimant(manifest: &str, dependency: &str) -> Claimant {
        Claimant {
            manifest: manifest.to_owned(),
            dependency: dependency.to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        }
    }

    fn record(kind: RefusalKind, wearings: Vec<RefusedWearing>, paths: &[&str]) -> RefusalRecord {
        RefusalRecord {
            kind,
            wearings,
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    type Sample = (&'static str, Refusal, RefusalRecord);

    /// The wearing member every sample but the overlap concerns.
    fn on_member(dependency: Option<&str>, package: Option<&str>) -> Vec<RefusedWearing> {
        vec![wearing("member/Cargo.toml", dependency, package)]
    }

    /// Every shape of refusal there is, each with the record it must report.
    fn samples() -> Vec<Sample> {
        let mut samples = wearing_samples();
        samples.extend(survey_samples());
        samples
    }

    /// The refusals of a manifest's wearing table, before any skeleton is read.
    fn wearing_samples() -> Vec<Sample> {
        vec![
            (
                "a whole skeletons table that is not a table",
                Refusal::Wearing(WearingRefusal::NotATable {
                    manifest: "member/Cargo.toml".to_owned(),
                    key: None,
                    package: None,
                }),
                record(RefusalKind::NotATable, on_member(None, None), &[]),
            ),
            (
                "one wearing table that is not a table",
                Refusal::Wearing(WearingRefusal::NotATable {
                    manifest: "member/Cargo.toml".to_owned(),
                    key: Some("ci".to_owned()),
                    package: Some("a-crate".to_owned()),
                }),
                record(
                    RefusalKind::NotATable,
                    on_member(Some("ci"), Some("a-crate")),
                    &[],
                ),
            ),
            (
                "a dependency under a reserved key",
                Refusal::Wearing(WearingRefusal::ReservedKey {
                    manifest: "member/Cargo.toml".to_owned(),
                    key: "options".to_owned(),
                    crate_name: "a-crate".to_owned(),
                }),
                record(
                    RefusalKind::ReservedKey,
                    on_member(Some("options"), Some("a-crate")),
                    &[],
                ),
            ),
            (
                "a key that names no dependency",
                Refusal::Wearing(WearingRefusal::NamesNoDependency {
                    manifest: "member/Cargo.toml".to_owned(),
                    dependency: "ci".to_owned(),
                }),
                record(
                    RefusalKind::NamesNoDependency,
                    on_member(Some("ci"), None),
                    &[],
                ),
            ),
            (
                "a dependency resolved to no package",
                Refusal::Wearing(WearingRefusal::Unresolved {
                    manifest: "member/Cargo.toml".to_owned(),
                    dependency: "ci".to_owned(),
                    package: "a-crate".to_owned(),
                }),
                record(
                    RefusalKind::Unresolved,
                    on_member(Some("ci"), Some("a-crate")),
                    &[],
                ),
            ),
            (
                "a dependency resolved to several packages",
                Refusal::Wearing(WearingRefusal::Ambiguous {
                    manifest: "member/Cargo.toml".to_owned(),
                    dependency: "ci".to_owned(),
                    packages: vec![
                        ("a-skeleton".to_owned(), "0.1.0".to_owned()),
                        ("a-skeleton".to_owned(), "0.2.0".to_owned()),
                    ],
                }),
                record(
                    RefusalKind::Ambiguous,
                    on_member(Some("ci"), Some("a-skeleton")),
                    &[],
                ),
            ),
            (
                "a package that is no skeleton",
                Refusal::Wearing(WearingRefusal::NotASkeleton {
                    manifest: "member/Cargo.toml".to_owned(),
                    dependency: "ci".to_owned(),
                    package: ("a-crate".to_owned(), "0.1.0".to_owned()),
                }),
                record(
                    RefusalKind::NotASkeleton,
                    on_member(Some("ci"), Some("a-crate")),
                    &[],
                ),
            ),
        ]
    }

    /// The refusals made once a skeleton has been located and rendered.
    fn survey_samples() -> Vec<Sample> {
        let version = semver::Version::new(0, 1, 0);
        let identity = || SkeletonIdentity::Named("a-skeleton".to_owned());
        vec![
            (
                "an option value of the wrong shape",
                Refusal::OptionShape {
                    worn: worn(),
                    skeleton: "a-skeleton".to_owned(),
                    version: version.clone(),
                    refusal: OptionShapeRefusal {
                        option: "cadence".to_owned(),
                        found: ShapeFound::Number,
                    },
                },
                record(
                    RefusalKind::OptionRefused,
                    on_member(Some("ci"), Some("a-skeleton")),
                    &[],
                ),
            ),
            (
                "a render that refuses a choice and names no file",
                Refusal::Render {
                    worn: worn(),
                    skeleton: "a-skeleton".to_owned(),
                    version: version.clone(),
                    error: RenderError::about_choice(
                        identity(),
                        Reason::OptionNameInvalid {
                            option: "cadence".to_owned(),
                        },
                    ),
                },
                record(
                    RefusalKind::OptionRefused,
                    on_member(Some("ci"), Some("a-skeleton")),
                    &[],
                ),
            ),
            (
                "a render that refuses a file of the skeleton",
                Refusal::Render {
                    worn: worn(),
                    skeleton: "a-skeleton".to_owned(),
                    version: version.clone(),
                    error: RenderError::about_file(identity(), "files/a.yml", Reason::NotUtf8),
                },
                record(
                    RefusalKind::SkeletonInvalid,
                    on_member(Some("ci"), Some("a-skeleton")),
                    &[],
                ),
            ),
            (
                "two claimants of colliding paths",
                Refusal::Overlap(Overlap {
                    paths: vec![claim("a/B.yml"), claim("a/b.yml")],
                    claimants: vec![
                        claimant("one/Cargo.toml", "first"),
                        claimant("two/Cargo.toml", "second"),
                    ],
                }),
                record(
                    RefusalKind::Overlap,
                    vec![
                        wearing("one/Cargo.toml", Some("first"), Some("a-skeleton")),
                        wearing("two/Cargo.toml", Some("second"), Some("a-skeleton")),
                    ],
                    &["a/B.yml", "a/b.yml"],
                ),
            ),
            (
                "an unsafe path",
                Refusal::UnsafePath {
                    worn: worn(),
                    skeleton: "a-skeleton".to_owned(),
                    version,
                    path: "a/b.yml".to_owned(),
                    cause: UnsafePathCause::Symlink,
                },
                record(
                    RefusalKind::UnsafePath,
                    on_member(Some("ci"), Some("a-skeleton")),
                    &["a/b.yml"],
                ),
            ),
        ]
    }

    #[test]
    fn every_refusal_reports_its_kind_the_wearings_it_concerns_and_its_paths() {
        // One refusal of every shape the survey can produce, built by hand,
        // each paired with the record the issue's three questions give it:
        // its kind, the wearings it concerns (a whole-table refusal names no
        // dependency key, an overlap names every claimant) and the paths it
        // concerns (only an overlap and an unsafe path name any). The kinds
        // are counted through an exhaustive match, so a kind no sample
        // reaches cannot go untested.
        let samples = samples();
        assert_every_kind(
            samples
                .iter()
                .map(|(_, _, expected)| kind_index(expected.kind)),
            REFUSAL_KINDS,
            "RefusalKind",
        );

        for (name, refusal, expected) in samples {
            assert_eq!(refusal.record(), expected, "{name}");
        }
    }

    #[test]
    fn the_kind_a_refusal_is_classified_as_is_the_kind_its_record_reports() {
        // `Refusal::kind` is the word of `classify`, and a record carries
        // `classify`'s answer, so the classification is made once: the word
        // `check` prints and the kind a caller reads cannot disagree.
        for (name, refusal, expected) in samples() {
            assert_eq!(refusal.classify(), expected.kind, "{name}");
            assert_eq!(refusal.kind(), expected.kind.word(), "{name}");
        }
    }
}
