//! Rendering and comparing every worn skeleton: the one survey `check` and
//! `sync` both work from.

pub(crate) mod count;
#[cfg(test)]
pub(crate) mod poison;
mod refusal;
mod spelling;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::claim::{self, Claim, ClaimPath, Claimant, Drift};
use crate::skeleton;
use crate::workspace::{Wearing, Workspace, WornDependency, WornId};

pub(crate) use refusal::{Refusal, join_and, option_shape_detail, unsafe_path_change_clause};
pub(crate) use spelling::told_apart;

/// One bone's own drift fact — the row `check`'s output prints one
/// line for, and the row `sync` writes from when it is drifted.
pub(crate) struct Row {
    pub(crate) path: ClaimPath,
    pub(crate) claimant: Claimant,
    pub(crate) drift: Drift,
    /// The bytes the skeleton renders for this path — carried from the one
    /// render `survey_one` already did, so `sync` never renders a skeleton a
    /// second time just to get the bytes it already compared.
    pub(crate) rendered: Vec<u8>,
}

/// Everything the render-and-compare phase determined about a workspace:
/// every worn dependency (whether or not it ended up refused), every row a
/// claim resolved to, and every refusal.
pub(crate) struct Survey<'workspace> {
    pub(crate) root: PathBuf,
    pub(crate) worn: Vec<&'workspace WornDependency>,
    pub(crate) rows: Vec<Row>,
    pub(crate) refusals: Vec<Refusal>,
}

/// Renders every worn skeleton `workspace` names, compares each claimed file
/// against what is really on disk, and collects every refusal along the
/// way — nothing here writes anything.
pub(crate) fn survey(workspace: &Workspace) -> Survey<'_> {
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
