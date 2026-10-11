//! Rendering and comparing every worn skeleton: the one survey `check` and
//! `sync` both work from.

#[cfg(feature = "commands")]
pub(crate) mod count;
#[cfg(test)]
pub(crate) mod poison;
mod refusal;
#[cfg(feature = "commands")]
mod spelling;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::claim::{self, Claim, ClaimPath, Claimant, Drift};
use crate::skeleton;
use crate::workspace::{Wearing, Workspace, WornDependency, WornId};

pub(crate) use refusal::Refusal;
pub use refusal::{RefusalKind, RefusalRecord, RefusedWearing};
#[cfg(feature = "commands")]
pub(crate) use refusal::{join_and, option_shape_detail, unsafe_path_change_clause};
#[cfg(feature = "commands")]
pub(crate) use spelling::told_apart;

/// One file a worn skeleton claims: where it lives, who claims it, whether it
/// matches, and the bytes it should hold.
pub struct Row {
    /// Where the file lives, relative to the workspace root.
    pub path: ClaimPath,
    /// The worn dependency that claims the file.
    pub claimant: Claimant,
    /// Whether the file on disk matches the render, and if not, why not.
    pub drift: Drift,
    /// The bytes the skeleton renders for this path: writing them to the
    /// path makes the file match, whatever it held before.
    pub rendered: Vec<u8>,
}

/// What surveying a workspace found: every worn dependency (whether or not it
/// ended up refused), every file the worn skeletons claim, and every refusal.
pub struct Survey<'workspace> {
    /// The workspace root, as cargo reports it.
    pub root: PathBuf,
    /// Every worn dependency, refused or not.
    pub worn: Vec<&'workspace WornDependency>,
    /// One row per file a worn skeleton claims, in path order. A skeleton
    /// with a refusal of its own has none.
    pub rows: Vec<Row>,
    pub(crate) refusals: Vec<Refusal>,
}

impl Survey<'_> {
    /// Every refusal as data, in the order the survey found them.
    #[must_use]
    pub fn refusal_records(&self) -> Vec<RefusalRecord> {
        self.refusals.iter().map(Refusal::record).collect()
    }
}

/// Renders every worn skeleton `workspace` names, compares each claimed file
/// against what is really on disk, and collects every refusal along the
/// way — nothing here writes anything.
pub fn survey(workspace: &Workspace) -> Survey<'_> {
    let mut worn = Vec::new();
    let mut claims = Vec::new();
    let mut refusals = Vec::new();
    let mut construction_unsafe_paths: BTreeMap<WornId, Vec<Refusal>> = BTreeMap::new();

    for wearing in &workspace.wearing {
        match wearing {
            Wearing::Refused(wearing_refusal) => {
                refusals.push(Refusal::Wearing(wearing_refusal.clone()));
            }
            Wearing::Worn(dependency) => {
                worn.push(dependency);
                match survey_one(dependency) {
                    Ok(surveyed) => {
                        claims.extend(surveyed.claims);
                        if !surveyed.unsafe_paths.is_empty() {
                            construction_unsafe_paths
                                .insert(dependency.id(), surveyed.unsafe_paths);
                        }
                    }
                    Err(refusal) => refusals.push(*refusal),
                }
            }
        }
    }

    let (safe_claims, overlaps) = claim::find_overlaps(claims);
    refusals.extend(overlaps.into_iter().map(Refusal::Overlap));

    let (mut rows, mut location_refusals) =
        resolve_claims(&workspace.root, safe_claims, construction_unsafe_paths);
    refusals.append(&mut location_refusals);
    rows.sort_by(|left, right| left.path.cmp(&right.path));

    Survey {
        root: workspace.root.clone(),
        worn,
        rows,
        refusals,
    }
}

/// One worn dependency's own render, turned into claims — with any unsafe
/// path found while building them (a `.git` component, another name git will
/// not track, or a name too long) carried alongside, rather than stopping the
/// render's own other claims from being built at all.
struct SurveyedSkeleton {
    claims: Vec<Claim>,
    /// Every unsafe path found while turning this render's own paths into
    /// claims — always [`Refusal::UnsafePath`], structurally: [`survey_one`]
    /// pushes to this field from exactly one site, its own `Err(cause)` arm
    /// below, which only ever builds that variant. Resolving the surviving
    /// claims against the real filesystem, in [`resolve_claims`], can add
    /// more; both sets are this same skeleton's own refusals, reported
    /// together and re-checked to still be all-`unsafe-path` by
    /// `resolve_skeleton_claims`'s own assertion below.
    unsafe_paths: Vec<Refusal>,
}

/// Renders one worn dependency and turns its output into claims — or the one
/// refusal that stands in its place: the wearer's own recorded option
/// values were the wrong shape, or the render itself refused.
///
/// The error is boxed: `Refusal` carries a `WornId`, a skeleton name, a
/// version and (in its largest variant) a path and a cause, which together
/// make it too large to return unboxed from a `Result` without every `?` in
/// this function copying that whole size around on every call.
fn survey_one(worn: &WornDependency) -> Result<SurveyedSkeleton, Box<Refusal>> {
    let choices = worn.choices.as_ref().map_err(|refusal| {
        Box::new(Refusal::OptionShape {
            worn: worn.id(),
            skeleton: worn.package.clone(),
            version: worn.version.clone(),
            refusal: refusal.clone(),
        })
    })?;

    let rendering = skeleton::render(&worn.skeleton_directory, choices).map_err(|error| {
        Box::new(Refusal::Render {
            worn: worn.id(),
            skeleton: worn.package.clone(),
            version: worn.version.clone(),
            error,
        })
    })?;

    let claimant = Claimant {
        manifest: worn.manifest.clone(),
        dependency: worn.key.clone(),
        skeleton: worn.package.clone(),
        version: worn.version.clone(),
    };

    let mut claims = Vec::new();
    let mut unsafe_paths = Vec::new();
    for (path, bytes) in rendering.iter() {
        match ClaimPath::from_rendering_path(path) {
            Ok(claim_path) => claims.push(Claim {
                path: claim_path,
                claimant: claimant.clone(),
                rendered: bytes.to_vec(),
            }),
            Err(cause) => unsafe_paths.push(Refusal::UnsafePath {
                worn: worn.id(),
                skeleton: worn.package.clone(),
                version: worn.version.clone(),
                path: path.to_owned(),
                cause,
            }),
        }
    }
    Ok(SurveyedSkeleton {
        claims,
        unsafe_paths,
    })
}

/// Resolves every surviving claim against the real filesystem, grouped by
/// its own worn skeleton: every unsafe path found for a skeleton — whether
/// found while building its claims (`construction_unsafe_paths`) or while
/// resolving them here — refuses that whole skeleton (its other,
/// otherwise-fine rows are dropped with it), and every one of them is
/// reported, never only the first found.
fn resolve_claims(
    root: &std::path::Path,
    claims: Vec<Claim>,
    mut construction_unsafe_paths: BTreeMap<WornId, Vec<Refusal>>,
) -> (Vec<Row>, Vec<Refusal>) {
    let mut by_skeleton: BTreeMap<WornId, Vec<Claim>> = BTreeMap::new();
    for claim in claims {
        let id = WornId {
            manifest: claim.claimant.manifest.clone(),
            key: claim.claimant.dependency.clone(),
        };
        by_skeleton.entry(id).or_default().push(claim);
    }
    // A skeleton whose every claim was refused as an unsafe path when it was
    // built never contributed a claim at all, so without this it would never
    // appear here to carry its own construction-time refusal.
    for id in construction_unsafe_paths.keys() {
        by_skeleton.entry(id.clone()).or_default();
    }

    let mut rows = Vec::new();
    let mut refusals = Vec::new();
    for (id, skeleton_claims) in by_skeleton {
        let skeleton_refusals = construction_unsafe_paths.remove(&id).unwrap_or_default();
        let (mut skeleton_rows, mut skeleton_refusals) =
            resolve_skeleton_claims(root, &id, skeleton_claims, skeleton_refusals);
        rows.append(&mut skeleton_rows);
        refusals.append(&mut skeleton_refusals);
    }
    (rows, refusals)
}

/// Resolves one skeleton's own surviving claims against the real
/// filesystem, folding any unsafe path found here into `refusals` alongside
/// whatever `refusals` already carried from claim construction. A skeleton
/// with any refusal at all keeps none of its rows — its refusals are
/// returned instead, sorted in path order — since a skeleton that cannot be
/// fully trusted reports nothing partial about itself.
fn resolve_skeleton_claims(
    root: &std::path::Path,
    id: &WornId,
    mut claims: Vec<Claim>,
    mut refusals: Vec<Refusal>,
) -> (Vec<Row>, Vec<Refusal>) {
    claims.sort_by(|left, right| left.path.cmp(&right.path));
    let mut rows = Vec::with_capacity(claims.len());
    for claim in claims {
        let Claim {
            path,
            claimant,
            rendered,
        } = claim;
        match claim::resolve(root, &path, rendered.len()) {
            Ok(on_disk) => {
                let drift = claim::compare(&rendered, &on_disk);
                rows.push(Row {
                    path,
                    claimant,
                    drift,
                    rendered,
                });
            }
            Err(cause) => refusals.push(Refusal::UnsafePath {
                worn: id.clone(),
                skeleton: claimant.skeleton.clone(),
                version: claimant.version.clone(),
                path: path.as_str().to_owned(),
                cause,
            }),
        }
    }
    if refusals.is_empty() {
        return (rows, refusals);
    }
    // A skeleton's refusals here are always unsafe-path: `survey_one`
    // returns early, before ever producing a `SurveyedSkeleton`, for the two
    // refusal kinds that are about no single path (`OptionShape`, `Render`)
    // — asserted here, where they are built; `Report::refusals_for` asserts
    // the same thing again where they are read.
    assert!(
        refusals
            .iter()
            .all(|refusal| refusal.unsafe_path().is_some()),
        "a skeleton's construction-time and resolution-time refusals are always unsafe-path"
    );
    refusals.sort_by(|left, right| left.unsafe_path().cmp(&right.unsafe_path()));
    (Vec::new(), refusals)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{RefusalKind, RefusalRecord, RefusedWearing, survey};
    use crate::skeleton::Choices;
    #[cfg(feature = "commands")]
    use crate::workspace::Pin;
    use crate::workspace::{OptionShapeRefusal, ShapeFound, Wearing, Workspace, WornDependency};

    /// A workspace rooted at `root` that wears the one dependency `worn`.
    fn workspace_wearing(root: PathBuf, worn: WornDependency) -> Workspace {
        Workspace {
            root,
            wearing: vec![Wearing::Worn(worn)],
        }
    }

    /// A worn dependency named `ci` in `member/Cargo.toml`, over the skeleton
    /// in `directory`, holding `choices`.
    fn worn_over(
        directory: PathBuf,
        choices: Result<Choices, OptionShapeRefusal>,
    ) -> WornDependency {
        WornDependency {
            manifest: "member/Cargo.toml".to_owned(),
            key: "ci".to_owned(),
            package: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
            #[cfg(feature = "commands")]
            pin: Pin::from_source(None, &directory),
            skeleton_directory: directory,
            choices,
        }
    }

    #[test]
    fn a_wearing_whose_options_are_refused_is_still_worn_has_no_rows_and_is_named_by_its_record() {
        // A wearer who records an option value of the wrong shape still wears
        // the skeleton, so it stays in `worn`; nothing is rendered for it, so
        // it has no rows; and its refusal, read as data, names that wearing.
        // The workspace is built by hand over a directory that need not
        // exist, because the refusal is decided before the skeleton is read.
        let refused = OptionShapeRefusal {
            option: "cadence".to_owned(),
            found: ShapeFound::Number,
        };
        let workspace = workspace_wearing(
            PathBuf::from("/workspace"),
            worn_over(PathBuf::from("/skeleton"), Err(refused)),
        );

        let survey = survey(&workspace);

        let [worn] = survey.worn.as_slice() else {
            panic!(
                "the refused wearing must still be worn: {}",
                survey.worn.len()
            );
        };
        assert_eq!(
            (worn.manifest.as_str(), worn.key.as_str()),
            ("member/Cargo.toml", "ci")
        );
        assert!(survey.rows.is_empty(), "a refused wearing has no rows");
        assert_eq!(
            survey.refusal_records(),
            [RefusalRecord {
                kind: RefusalKind::OptionRefused,
                wearings: vec![RefusedWearing {
                    manifest: "member/Cargo.toml".to_owned(),
                    dependency: Some("ci".to_owned()),
                }],
                paths: Vec::new(),
            }]
        );
    }

    #[cfg(skeletons_checkout)]
    mod over_the_plain_fixture {
        use std::fs;
        use std::path::{Path, PathBuf};

        use tempfile::TempDir;

        use super::{survey, workspace_wearing, worn_over};
        use crate::acceptance::test_skeleton;
        use crate::claim::{Drift, DriftReason};
        use crate::skeleton::Choices;
        use crate::survey::Row;

        /// The skeleton these tests wear: one file, `plain.yml`, rendered
        /// exactly as it is written.
        const FIXTURE: &str = "renders/passthrough-plain";

        /// The one row of surveying `root` while wearing the fixture.
        fn only_row(root: &Path) -> Row {
            let workspace = workspace_wearing(
                root.to_owned(),
                worn_over(test_skeleton(FIXTURE), Ok(Choices::new())),
            );
            let mut rows = survey(&workspace).rows;
            assert_eq!(rows.len(), 1, "the fixture claims exactly one file");
            rows.remove(0)
        }

        fn fixture_bytes() -> Vec<u8> {
            fs::read(test_skeleton(FIXTURE).join("files/plain.yml"))
                .expect("the fixture skeleton holds files/plain.yml")
        }

        fn empty_root() -> TempDir {
            TempDir::new().expect("a temporary directory")
        }

        #[test]
        fn a_missing_claimed_file_is_a_row_that_carries_the_bytes_that_make_it_match() {
            // Surveying an empty root: the file the skeleton claims is not
            // there, so its row is `Missing`, and the row carries the bytes
            // the skeleton renders (the fixture's own file, read here from
            // disk, since the fixture renders it unchanged). Writing exactly
            // those bytes where the row says, then surveying again, makes it
            // `Matches`: the row alone is enough to bring the file in line.
            let root = empty_root();

            let row = only_row(root.path());

            assert_eq!(row.path.as_str(), "plain.yml");
            assert_eq!(row.drift, Drift::Drifted(DriftReason::Missing));
            assert_eq!(row.claimant.dependency, "ci");
            assert_eq!(row.rendered, fixture_bytes());

            fs::write(root.path().join(row.path.as_str()), &row.rendered)
                .expect("the temporary root is writable");

            assert_eq!(only_row(root.path()).drift, Drift::Matches);
        }

        #[test]
        fn a_claimed_file_with_other_bytes_is_changed() {
            // The same survey over a root that already holds the file, with
            // bytes that are not the render: `Changed`, and the row still
            // carries the render, not what is on disk.
            let root = empty_root();
            fs::write(root.path().join("plain.yml"), b"name: edited\n")
                .expect("the temporary root is writable");

            let row = only_row(root.path());

            assert_eq!(row.drift, Drift::Drifted(DriftReason::Changed));
            assert_eq!(row.rendered, fixture_bytes());
        }

        #[test]
        fn the_survey_names_the_root_it_was_given() {
            // `root` is the workspace root a caller joins a row's path onto.
            let root = empty_root();
            let workspace = workspace_wearing(
                root.path().to_owned(),
                worn_over(test_skeleton(FIXTURE), Ok(Choices::new())),
            );

            assert_eq!(survey(&workspace).root, PathBuf::from(root.path()));
        }
    }
}
