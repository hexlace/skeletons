//! `check --json`'s own document: `#[derive(Serialize)]` shapes, one `report`
//! call, format version 1.
//!
//! Every object's fields are declared here in the order `.docs/wearing.md`
//! lists them; that order is not itself promised (a reader may see any
//! order), but this is the one this crate actually produces.
//!
//! Every object with more than one shape (a pin, a drift fact, a behind
//! fact, a newer value, a refusal) is a `#[serde(untagged)] enum` of struct
//! variants, one variant per shape, each declaring exactly its own fields —
//! never `skip_serializing_if`, never a shared `Default` to build from. A
//! field a variant's own struct does not declare is a field it cannot hold,
//! so the wrong shape does not compile, let alone serialize: one object
//! carries exactly its variant's fields, and a listed field with no value
//! this time is `null`, never absent.

use std::path::Path;

use serde::Serialize;

use super::behind_words::{newer_detail, undetermined_detail};
use super::pin_words;
use super::report::{Filter, Report, Summary};
use crate::behind::{Behind, Newer, UndeterminedReason};
use crate::claim::{Drift, DriftReason, Overlap, UnsafePathCause};
use crate::git::ObjectId;
use crate::skeleton::RenderError;
use crate::survey::{Refusal, option_shape_detail};
use crate::workspace::{
    CRATES_IO_SOURCE, Pin, ReadWorkspaceError, WearingRefusal, WornDependency, WornId,
};

/// `--json`'s own format-version marker: a reader checks this is a version
/// it knows and ignores every field it does not, and a value it does not
/// know in an *open* field still travels with a human `detail`/`message` it
/// can show. See `.docs/wearing.md`'s own Versioning section for the full
/// rule.
pub(crate) const JSON_FORMAT_VERSION: u32 = 1;

/// Renders `report`, narrowed by `filter`, as the pretty-printed JSON
/// document `check --json` prints — one `report()` call's worth of text.
/// `fail_behind` decides `summary.failed`, the same as it decides the actual
/// exit status.
pub(crate) fn document(report: &Report<'_>, filter: Filter, fail_behind: bool) -> String {
    let root = report.root();
    let summary = report.summary(fail_behind);

    let skeletons: Vec<SkeletonJson> = report
        .skeletons()
        .into_iter()
        .map(|worn| skeleton_json(report, worn, root))
        .collect();

    let bones: Vec<BoneJson> = report
        .skeletons()
        .into_iter()
        .flat_map(|worn| {
            let behind = report.behind_for(worn);
            report
                .rows_for(worn)
                .into_iter()
                .filter(move |row| filter.shows(row, behind))
                .map(move |row| BoneJson {
                    path: row.path.as_str().to_owned(),
                    manifest: worn.manifest.clone(),
                    dependency: worn.key.clone(),
                    skeleton: worn.package.clone(),
                    version: worn.version.to_string(),
                    pin: pin_json(&worn.pin, root),
                    drift: drift_json(row.drift),
                    behind: behind_json(&worn.pin, behind),
                })
        })
        .collect();

    // Sorted before it is turned into JSON: `RefusalJson` is untagged, so it
    // carries no field this sort could read back out without matching every
    // variant, and `Refusal` already has both keys this order needs.
    let mut all_refusals: Vec<&Refusal> = report.all_refusals().iter().collect();
    all_refusals.sort_by(|left, right| {
        left.kind()
            .cmp(right.kind())
            .then_with(|| left.message().cmp(&right.message()))
    });
    let refusals: Vec<RefusalJson> = all_refusals.into_iter().map(refusal_json).collect();

    // Re-verified here, alongside `Report::summary`'s own assertion of the same
    // property: `skeletons[]` is built from the exact same worn dependencies
    // the behind counts were tallied from, so the two must always agree.
    assert_eq!(
        skeletons.len(),
        summary.current + summary.behind + summary.pinned + summary.undetermined,
        "skeletons[] must list exactly as many entries as summary's four behind counts sum to"
    );

    let document = DocumentJson {
        format_version: JSON_FORMAT_VERSION,
        filters: filters_json(filter),
        summary: summary_json(&summary),
        skeletons,
        bones,
        refusals,
        aborted: None,
    };

    // A newline-terminated document, as every other line `report()` writes
    // is (`writeln!`): `to_string_pretty` itself never adds a trailing
    // newline, and this is the one call site that decides the whole
    // document is exactly one `report()` line, however many lines of text
    // it contains.
    to_pretty_json(&document)
}

/// The document `check --json` prints when the workspace itself could not be
/// read: every count zero, `skeletons`/`bones`/`refusals` empty, `aborted`
/// naming what stopped it.
pub(crate) fn aborted(error: &ReadWorkspaceError) -> String {
    let document = DocumentJson {
        format_version: JSON_FORMAT_VERSION,
        filters: Vec::new(),
        summary: SummaryJson {
            bones: 0,
            matches: 0,
            drifted: 0,
            skeletons: 0,
            current: 0,
            behind: 0,
            pinned: 0,
            undetermined: 0,
            refusals: 0,
            failed: true,
        },
        skeletons: Vec::new(),
        bones: Vec::new(),
        refusals: Vec::new(),
        aborted: Some(aborted_json(error)),
    };
    to_pretty_json(&document)
}

fn to_pretty_json(document: &DocumentJson) -> String {
    // `DocumentJson` is built entirely from this crate's own owned data and
    // `#[derive(Serialize)]` shapes with no custom `serialize` impl, so
    // serialization can only fail on a type that cannot happen here (a map
    // key that is not a string, or a `NaN`/`Infinity` float — this document
    // has neither).
    serde_json::to_string_pretty(document)
        .unwrap_or_else(|error| unreachable!("DocumentJson always serializes: {error}"))
}

#[derive(Serialize)]
struct DocumentJson {
    format_version: u32,
    filters: Vec<&'static str>,
    summary: SummaryJson,
    skeletons: Vec<SkeletonJson>,
    bones: Vec<BoneJson>,
    refusals: Vec<RefusalJson>,
    aborted: Option<AbortedJson>,
}

#[derive(Serialize)]
struct SummaryJson {
    bones: u64,
    matches: u64,
    drifted: u64,
    skeletons: u64,
    current: u64,
    behind: u64,
    pinned: u64,
    undetermined: u64,
    refusals: u64,
    failed: bool,
}

// `usize as u64` here, not `u64::try_from`, because `summary_json` is `const`
// and `TryFrom` is not callable in a const context. The cast is lossless
// regardless: `usize` is at most 64 bits wide on every target this crate
// supports, so it always fits in a `u64`.
const fn summary_json(summary: &Summary) -> SummaryJson {
    SummaryJson {
        bones: summary.bones as u64,
        matches: summary.matches as u64,
        drifted: summary.drifted as u64,
        skeletons: summary.skeletons as u64,
        current: summary.current as u64,
        behind: summary.behind as u64,
        pinned: summary.pinned as u64,
        undetermined: summary.undetermined as u64,
        refusals: summary.refusals as u64,
        failed: summary.failed,
    }
}

fn filters_json(filter: Filter) -> Vec<&'static str> {
    let mut filters = Vec::new();
    if filter.drifted {
        filters.push("drifted");
    }
    if filter.behind {
        filters.push("behind");
    }
    filters
}

#[derive(Serialize)]
struct SkeletonJson {
    manifest: String,
    dependency: String,
    skeleton: String,
    version: String,
    pin: PinJson,
    behind: BehindJson,
    refused: bool,
}

fn skeleton_json(report: &Report<'_>, worn: &WornDependency, root: &Path) -> SkeletonJson {
    SkeletonJson {
        manifest: worn.manifest.clone(),
        dependency: worn.key.clone(),
        skeleton: worn.package.clone(),
        version: worn.version.to_string(),
        pin: pin_json(&worn.pin, root),
        behind: behind_json(&worn.pin, report.behind_for(worn)),
        refused: !report.refusals_for(worn).is_empty(),
    }
}

#[derive(Serialize)]
struct BoneJson {
    path: String,
    manifest: String,
    dependency: String,
    skeleton: String,
    version: String,
    pin: PinJson,
    drift: DriftJson,
    behind: BehindJson,
}

/// `pin` (`kind` open — see `.docs/wearing.md`'s Versioning section): one
/// variant per [`Pin`], each declaring exactly the fields that document's
/// "Every object, field by field" section lists for that `kind`. `detail` is
/// [`pin_words::pin_detail`] on every variant, the same words `check`'s human
/// header prints after `from` — one function feeding both outputs, so they
/// cannot describe the same pin two different ways.
#[derive(Serialize)]
#[serde(untagged)]
enum PinJson {
    CratesIo {
        kind: &'static str,
        source: String,
        detail: String,
    },
    OtherRegistry {
        kind: &'static str,
        source: String,
        detail: String,
    },
    Tag {
        kind: &'static str,
        url: String,
        tag: String,
        commit: String,
        detail: String,
    },
    Branch {
        kind: &'static str,
        url: String,
        branch: String,
        commit: String,
        detail: String,
    },
    DefaultBranch {
        kind: &'static str,
        url: String,
        commit: String,
        detail: String,
    },
    Rev {
        kind: &'static str,
        url: String,
        rev: String,
        commit: String,
        detail: String,
    },
    Path {
        kind: &'static str,
        path: String,
        detail: String,
    },
    Unrecognised {
        kind: &'static str,
        source: String,
        detail: String,
    },
}

fn pin_json(pin: &Pin, root: &Path) -> PinJson {
    let detail = pin_words::pin_detail(pin, root);
    match pin {
        Pin::CratesIo => PinJson::CratesIo {
            kind: "registry",
            source: CRATES_IO_SOURCE.to_owned(),
            detail,
        },
        Pin::OtherRegistry { source } => PinJson::OtherRegistry {
            kind: "registry",
            source: source.clone(),
            detail,
        },
        Pin::Tag { url, tag, commit } => PinJson::Tag {
            kind: "tag",
            url: url.clone(),
            tag: tag.clone(),
            commit: commit.to_string(),
            detail,
        },
        Pin::Branch {
            url,
            branch,
            commit,
        } => PinJson::Branch {
            kind: "branch",
            url: url.clone(),
            branch: branch.clone(),
            commit: commit.to_string(),
            detail,
        },
        Pin::DefaultBranch { url, commit } => PinJson::DefaultBranch {
            kind: "default-branch",
            url: url.clone(),
            commit: commit.to_string(),
            detail,
        },
        Pin::Rev { url, rev, commit } => PinJson::Rev {
            kind: "rev",
            url: url.clone(),
            rev: rev.clone(),
            commit: commit.to_string(),
            detail,
        },
        Pin::Path { directory } => PinJson::Path {
            kind: "path",
            path: pin_words::path_pin_shown(root, directory),
            detail,
        },
        Pin::Unrecognised { source } => PinJson::Unrecognised {
            kind: "unrecognised",
            source: source.clone(),
            detail,
        },
    }
}

/// `drift`: `state` always present; `reason` only on the state that carries
/// one at all — never `{"state":"matches","reason":null}`.
#[derive(Serialize)]
#[serde(untagged)]
enum DriftJson {
    Matches {
        state: &'static str,
    },
    Drifted {
        state: &'static str,
        reason: &'static str,
    },
}

const fn drift_json(drift: Drift) -> DriftJson {
    match drift {
        Drift::Matches => DriftJson::Matches { state: "matches" },
        Drift::Drifted(DriftReason::Missing) => DriftJson::Drifted {
            state: "drifted",
            reason: "missing",
        },
        Drift::Drifted(DriftReason::Changed) => DriftJson::Drifted {
            state: "drifted",
            reason: "changed",
        },
    }
}

/// `newer`: an object whose key names what it is. Untagged, so
/// `{"version": "0.2.0"}` carries no other key at all — never `"tag": null`
/// alongside it. Only a [`Pin::DefaultBranch`] pin's own answer ever carries
/// a `branch` key: an explicit `branch =` pin's own name is already known
/// from the pin itself, so its `Newer::Commit` never has one to report, and
/// its variant here declares no such field to hold one in. A default
/// branch's own `branch` is `null` when the server did not name it, which is
/// itself a value worth reporting, not an absent fact: like every listed field
/// with no value in this document, it is `null` and never omitted.
#[derive(Serialize)]
#[serde(untagged)]
enum NewerJson {
    Version {
        version: String,
    },
    Tag {
        tag: String,
    },
    BranchCommit {
        commit: String,
    },
    DefaultBranchCommit {
        commit: String,
        branch: Option<String>,
    },
}

/// [`Newer::Commit`]'s own JSON needs the pin as well as the answer: which of
/// [`NewerJson`]'s two commit variants applies is decided by which pin asked
/// the question, not by anything in the answer alone.
fn newer_json(pin: &Pin, newer: &Newer) -> NewerJson {
    match newer {
        Newer::Version(version) => NewerJson::Version {
            version: version.to_string(),
        },
        Newer::Tag(tag) => NewerJson::Tag { tag: tag.clone() },
        Newer::Commit { sha, branch } => commit_newer_json(pin, sha, branch.as_ref()),
    }
}

/// # Panics
///
/// If `pin` is anything but [`Pin::Branch`] or [`Pin::DefaultBranch`], or if
/// an explicit [`Pin::Branch`]'s own `branch` is `Some` — `behind.rs` only
/// ever builds a [`Newer::Commit`] for one of those two pin kinds, and only
/// ever names a branch on the second of them (its own documented
/// invariant), so this re-asserts both facts again at the JSON boundary
/// rather than trusting the value silently.
fn commit_newer_json(pin: &Pin, sha: &ObjectId, branch: Option<&String>) -> NewerJson {
    match pin {
        Pin::DefaultBranch { .. } => NewerJson::DefaultBranchCommit {
            commit: sha.to_string(),
            branch: branch.cloned(),
        },
        Pin::Branch { .. } => {
            assert!(
                branch.is_none(),
                "an explicit branch pin's own Newer::Commit never carries a branch name"
            );
            NewerJson::BranchCommit {
                commit: sha.to_string(),
            }
        }
        Pin::CratesIo
        | Pin::OtherRegistry { .. }
        | Pin::Tag { .. }
        | Pin::Rev { .. }
        | Pin::Path { .. }
        | Pin::Unrecognised { .. } => {
            unreachable!("only a Branch or DefaultBranch pin's own behind fact is ever a Commit")
        }
    }
}

/// `behind`: `state` always present; `newer`, `reason` and `detail` present
/// only on the states that carry them at all — `{"state": "current"}` alone,
/// with no other key.
#[derive(Serialize)]
#[serde(untagged)]
enum BehindJson {
    Current {
        state: &'static str,
    },
    Pinned {
        state: &'static str,
    },
    Behind {
        state: &'static str,
        newer: NewerJson,
        detail: String,
    },
    Undetermined {
        state: &'static str,
        reason: &'static str,
        detail: String,
    },
}

fn behind_json(pin: &Pin, fact: &Behind) -> BehindJson {
    match fact {
        Behind::Current => BehindJson::Current { state: "current" },
        Behind::Pinned => BehindJson::Pinned { state: "pinned" },
        Behind::Behind(newer) => BehindJson::Behind {
            state: "behind",
            newer: newer_json(pin, newer),
            detail: newer_detail(pin, newer),
        },
        Behind::Undetermined(undetermined) => BehindJson::Undetermined {
            state: "undetermined",
            reason: undetermined_reason_json(undetermined.reason),
            detail: undetermined_detail(undetermined),
        },
    }
}

const fn undetermined_reason_json(reason: UndeterminedReason) -> &'static str {
    match reason {
        UndeterminedReason::OtherRegistry => "other-registry",
        UndeterminedReason::Unreachable => "unreachable",
        UndeterminedReason::NotInIndex => "not-in-index",
        UndeterminedReason::UnexpectedResponse => "unexpected-response",
        UndeterminedReason::TagNotAVersion => "tag-not-a-version",
        UndeterminedReason::BranchMissing => "branch-missing",
        UndeterminedReason::UnrecognisedSource => "unrecognised-source",
        UndeterminedReason::CheckoutUnreadable => "checkout-unreadable",
        UndeterminedReason::LocalFailure => "local-failure",
        UndeterminedReason::DirectoryMissing => "directory-missing",
    }
}

#[derive(Serialize)]
struct PackageIdentityJson {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct ClaimantJson {
    manifest: String,
    dependency: String,
    skeleton: String,
    version: String,
}

/// `refusals[]` (`kind` open): one variant per refusal kind, each declaring
/// exactly the fields `.docs/wearing.md`'s own per-kind list gives it —
/// `kind` first, `message` last, and every field in between present and
/// `null` when its variant lists it but this refusal has no value for it.
/// `kind` and `message` are always [`Refusal::kind`] and
/// [`Refusal::message`] themselves: this crate names a refusal's kind in
/// exactly one place, the same word the human output classifies it by.
#[derive(Serialize)]
#[serde(untagged)]
enum RefusalJson {
    NotATable {
        kind: &'static str,
        manifest: String,
        dependency: Option<String>,
        message: String,
    },
    ReservedKey {
        kind: &'static str,
        manifest: String,
        dependency: String,
        message: String,
    },
    NamesNoDependency {
        kind: &'static str,
        manifest: String,
        dependency: String,
        message: String,
    },
    Unresolved {
        kind: &'static str,
        manifest: String,
        dependency: String,
        message: String,
    },
    Ambiguous {
        kind: &'static str,
        manifest: String,
        dependency: String,
        packages: Vec<PackageIdentityJson>,
        message: String,
    },
    NotASkeleton {
        kind: &'static str,
        manifest: String,
        dependency: String,
        package: PackageIdentityJson,
        message: String,
    },
    OptionRefused {
        kind: &'static str,
        manifest: String,
        dependency: String,
        skeleton: String,
        version: String,
        detail: String,
        message: String,
    },
    SkeletonInvalid {
        kind: &'static str,
        manifest: String,
        dependency: String,
        skeleton: String,
        version: String,
        file: String,
        line: Option<u32>,
        detail: String,
        message: String,
    },
    Overlap {
        kind: &'static str,
        paths: Vec<String>,
        claimants: Vec<ClaimantJson>,
        message: String,
    },
    UnsafePath {
        kind: &'static str,
        manifest: String,
        dependency: String,
        skeleton: String,
        version: String,
        path: String,
        cause: &'static str,
        at: Option<String>,
        on_disk: Option<Vec<String>>,
        message: String,
    },
}

fn refusal_json(refusal: &Refusal) -> RefusalJson {
    let kind = refusal.kind();
    let message = refusal.message();
    match refusal {
        Refusal::Wearing(wearing) => wearing_refusal_json(kind, message, wearing),
        Refusal::OptionShape {
            worn,
            skeleton,
            version,
            refusal,
        } => RefusalJson::OptionRefused {
            kind,
            manifest: worn.manifest.clone(),
            dependency: worn.key.clone(),
            skeleton: skeleton.clone(),
            version: version.to_string(),
            detail: option_shape_detail(refusal),
            message,
        },
        Refusal::Render {
            worn,
            skeleton,
            version,
            error,
        } => render_refusal_json(kind, message, worn, skeleton, version, error),
        Refusal::Overlap(overlap) => overlap_refusal_json(kind, message, overlap),
        Refusal::UnsafePath {
            worn,
            skeleton,
            version,
            path,
            cause,
        } => {
            let (cause_kind, at, on_disk) = unsafe_path_cause_json(cause);
            RefusalJson::UnsafePath {
                kind,
                manifest: worn.manifest.clone(),
                dependency: worn.key.clone(),
                skeleton: skeleton.clone(),
                version: version.to_string(),
                path: path.clone(),
                cause: cause_kind,
                at,
                on_disk,
                message,
            }
        }
    }
}

fn wearing_refusal_json(
    kind: &'static str,
    message: String,
    wearing: &WearingRefusal,
) -> RefusalJson {
    match wearing {
        WearingRefusal::NotATable { manifest, key } => RefusalJson::NotATable {
            kind,
            manifest: manifest.clone(),
            dependency: key.clone(),
            message,
        },
        WearingRefusal::ReservedKey { manifest, key, .. } => RefusalJson::ReservedKey {
            kind,
            manifest: manifest.clone(),
            dependency: key.clone(),
            message,
        },
        WearingRefusal::NamesNoDependency {
            manifest,
            dependency,
        } => RefusalJson::NamesNoDependency {
            kind,
            manifest: manifest.clone(),
            dependency: dependency.clone(),
            message,
        },
        WearingRefusal::Unresolved {
            manifest,
            dependency,
        } => RefusalJson::Unresolved {
            kind,
            manifest: manifest.clone(),
            dependency: dependency.clone(),
            message,
        },
        WearingRefusal::Ambiguous {
            manifest,
            dependency,
            packages,
        } => RefusalJson::Ambiguous {
            kind,
            manifest: manifest.clone(),
            dependency: dependency.clone(),
            packages: packages
                .iter()
                .map(|(name, version)| PackageIdentityJson {
                    name: name.clone(),
                    version: version.clone(),
                })
                .collect(),
            message,
        },
        WearingRefusal::NotASkeleton {
            manifest,
            dependency,
            package: (name, version),
        } => RefusalJson::NotASkeleton {
            kind,
            manifest: manifest.clone(),
            dependency: dependency.clone(),
            package: PackageIdentityJson {
                name: name.clone(),
                version: version.clone(),
            },
            message,
        },
    }
}

/// [`Refusal::Render`]'s own split (survey/refusal.rs's `Refusal::kind`):
/// `error.file()` some names a defect in the skeleton itself
/// (`skeleton-invalid`); `error.file()` none is about the wearer's own
/// recorded choices instead (`option-refused`), the same JSON shape
/// [`Refusal::OptionShape`] builds.
fn render_refusal_json(
    kind: &'static str,
    message: String,
    worn: &WornId,
    skeleton: &str,
    version: &semver::Version,
    error: &RenderError,
) -> RefusalJson {
    match error.file() {
        Some(file) => RefusalJson::SkeletonInvalid {
            kind,
            manifest: worn.manifest.clone(),
            dependency: worn.key.clone(),
            skeleton: skeleton.to_owned(),
            version: version.to_string(),
            file: file.to_owned(),
            line: error.line(),
            detail: error.reason().to_string(),
            message,
        },
        None => RefusalJson::OptionRefused {
            kind,
            manifest: worn.manifest.clone(),
            dependency: worn.key.clone(),
            skeleton: skeleton.to_owned(),
            version: version.to_string(),
            detail: error.reason().to_string(),
            message,
        },
    }
}

fn overlap_refusal_json(kind: &'static str, message: String, overlap: &Overlap) -> RefusalJson {
    RefusalJson::Overlap {
        kind,
        paths: overlap.paths.iter().map(ToString::to_string).collect(),
        claimants: overlap
            .claimants
            .iter()
            .map(|claimant| ClaimantJson {
                manifest: claimant.manifest.clone(),
                dependency: claimant.dependency.clone(),
                skeleton: claimant.skeleton.clone(),
                version: claimant.version.to_string(),
            })
            .collect(),
        message,
    }
}

/// The JSON `cause`, `at` (non-null for `spelled-differently`, the two
/// `…-above` causes, `inside-another-repository`, `untrackable-name` and
/// `name-too-long`) and
/// `on_disk` (non-null only for `spelled-differently`).
fn unsafe_path_cause_json(
    cause: &UnsafePathCause,
) -> (&'static str, Option<String>, Option<Vec<String>>) {
    match cause {
        UnsafePathCause::SpelledDifferently { at, on_disk, .. } => (
            "spelled-differently",
            Some(at.clone()),
            Some(on_disk.clone()),
        ),
        UnsafePathCause::SymbolicLinkAbove { at } => {
            ("symbolic-link-above", Some(at.clone()), None)
        }
        UnsafePathCause::NotADirectoryAbove { at } => {
            ("not-a-directory-above", Some(at.clone()), None)
        }
        UnsafePathCause::Symlink => ("symbolic-link", None, None),
        UnsafePathCause::NotAFile => ("not-a-file", None, None),
        UnsafePathCause::InsideGitDirectory => ("inside-git-directory", None, None),
        UnsafePathCause::InsideAnotherRepository { at } => {
            ("inside-another-repository", Some(at.clone()), None)
        }
        UnsafePathCause::UntrackableName { at } => ("untrackable-name", Some(at.clone()), None),
        UnsafePathCause::NameTooLong { at, .. } => ("name-too-long", Some(at.clone()), None),
        UnsafePathCause::Unreadable { .. } => ("unreadable", None, None),
    }
}

#[derive(Serialize)]
struct AbortedJson {
    kind: &'static str,
    message: String,
}

fn aborted_json(error: &ReadWorkspaceError) -> AbortedJson {
    let (kind, message) = super::abort_message(error, super::AbortingCommand::Check);
    AbortedJson { kind, message }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::path::Path;

    use super::{behind_json, drift_json, newer_json, pin_json, refusal_json};
    use crate::behind::{Behind, Newer, Undetermined, UndeterminedReason};
    use crate::claim::{
        ClaimPath, Claimant, Drift, DriftReason, Overlap, TooLongName, UnsafePathCause,
    };
    use crate::git::ObjectId;
    use crate::skeleton::{Reason, RenderError, SkeletonIdentity};
    use crate::survey::Refusal;
    use crate::survey::poison::{POISON, POISON_ESCAPED, assert_escaped_once};
    use crate::workspace::{CRATES_IO_SOURCE, Pin, ReadWorkspaceError, WearingRefusal, WornId};

    /// `text` parsed as an [`ObjectId`], for a test's own expected value.
    fn oid(text: &str) -> ObjectId {
        ObjectId::parse(text).expect("a well-formed test object id")
    }

    fn unsafe_path_refusal(path: &str, cause: UnsafePathCause) -> Refusal {
        Refusal::UnsafePath {
            worn: WornId {
                manifest: "Cargo.toml".to_owned(),
                key: "a-skeleton".to_owned(),
            },
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
            path: path.to_owned(),
            cause,
        }
    }

    #[test]
    fn current_and_pinned_serialize_with_no_other_field_at_all() {
        assert_eq!(
            serde_json::to_string(&behind_json(&Pin::CratesIo, &Behind::Current))
                .expect("must serialize"),
            r#"{"state":"current"}"#
        );
        let pin = Pin::Path {
            directory: "/anywhere".into(),
        };
        assert_eq!(
            serde_json::to_string(&behind_json(&pin, &Behind::Pinned)).expect("must serialize"),
            r#"{"state":"pinned"}"#
        );
    }

    #[test]
    fn a_registry_newer_version_serializes_as_a_bare_version_object() {
        let json = newer_json(
            &Pin::CratesIo,
            &Newer::Version(semver::Version::new(0, 2, 0)),
        );
        assert_eq!(
            serde_json::to_string(&json).expect("must serialize"),
            r#"{"version":"0.2.0"}"#
        );
    }

    #[test]
    fn an_explicit_branch_pins_own_commit_never_carries_a_branch_key_at_all() {
        let pin = Pin::Branch {
            url: "https://example.invalid/skeleton".to_owned(),
            branch: "main".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        let json = newer_json(
            &pin,
            &Newer::Commit {
                sha: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
                branch: None,
            },
        );
        assert_eq!(
            serde_json::to_string(&json).expect("must serialize"),
            r#"{"commit":"8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"}"#
        );
    }

    #[test]
    fn a_default_branch_pins_own_commit_carries_the_servers_named_branch() {
        let pin = Pin::DefaultBranch {
            url: "https://example.invalid/skeleton".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        let json = newer_json(
            &pin,
            &Newer::Commit {
                sha: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
                branch: Some("main".to_owned()),
            },
        );
        assert_eq!(
            serde_json::to_string(&json).expect("must serialize"),
            r#"{"commit":"8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f","branch":"main"}"#
        );
    }

    // The one-shape rule's headline case: a default branch pin whose server
    // did not name its own branch carries `"branch":null`, not an absent
    // key — an explicit null rather than a missing field.
    #[test]
    fn a_default_branch_pins_own_commit_carries_a_null_branch_when_the_server_named_none() {
        let pin = Pin::DefaultBranch {
            url: "https://example.invalid/skeleton".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        let json = newer_json(
            &pin,
            &Newer::Commit {
                sha: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
                branch: None,
            },
        );
        assert_eq!(
            serde_json::to_string(&json).expect("must serialize"),
            r#"{"commit":"8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f","branch":null}"#
        );
    }

    #[test]
    fn behind_carries_its_own_newer_and_detail() {
        let pin = Pin::CratesIo;
        let fact = Behind::Behind(Newer::Version(semver::Version::new(0, 2, 0)));
        assert_eq!(
            serde_json::to_string(&behind_json(&pin, &fact)).expect("must serialize"),
            r#"{"state":"behind","newer":{"version":"0.2.0"},"detail":"0.2.0 is available"}"#
        );
    }

    #[test]
    fn undetermined_carries_its_own_reason_and_detail() {
        let pin = Pin::OtherRegistry {
            source: "sparse+https://example.invalid/index/".to_owned(),
        };
        let fact = Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::OtherRegistry,
            detail: "`skeletons` asks only crates.io whether a skeleton is behind".to_owned(),
        });
        assert_eq!(
            serde_json::to_string(&behind_json(&pin, &fact)).expect("must serialize"),
            concat!(
                r#"{"state":"undetermined","reason":"other-registry","#,
                r#""detail":"`skeletons` asks only crates.io whether a skeleton is behind"}"#
            )
        );
    }

    // Every assertion below is a full-string comparison of the serialised
    // object, not a field-by-field read: the one-shape rule's failure mode
    // is a variant carrying a field it should not, and a Rust-side
    // destructure of an enum variant would not notice an extra field the way
    // a serialised text comparison does.

    #[test]
    fn spelled_differently_carries_its_own_at_and_on_disk() {
        let refusal = unsafe_path_refusal(
            ".github/dependabot.yml",
            UnsafePathCause::SpelledDifferently {
                at: ".github/dependabot.yml".to_owned(),
                on_disk: vec![".github/DEPENDABOT.YML".to_owned()],
                claimed_spelling_present: false,
            },
        );
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":".github/dependabot.yml","#,
                    r#""cause":"spelled-differently","at":".github/dependabot.yml","#,
                    r#""on_disk":[".github/DEPENDABOT.YML"],"message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn every_other_unsafe_path_cause_carries_a_null_on_disk() {
        let refusal = unsafe_path_refusal("root.yml", UnsafePathCause::NotAFile);
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":"root.yml","#,
                    r#""cause":"not-a-file","at":null,"on_disk":null,"message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn symbolic_link_above_carries_its_at_with_a_null_on_disk() {
        let refusal = unsafe_path_refusal(
            "a/b/deep.yml",
            UnsafePathCause::SymbolicLinkAbove {
                at: "a/b".to_owned(),
            },
        );
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":"a/b/deep.yml","#,
                    r#""cause":"symbolic-link-above","at":"a/b","on_disk":null,"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn inside_another_repository_carries_its_at_with_a_null_on_disk() {
        let refusal = unsafe_path_refusal(
            "sub/x.yml",
            UnsafePathCause::InsideAnotherRepository {
                at: "sub".to_owned(),
            },
        );
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":"sub/x.yml","#,
                    r#""cause":"inside-another-repository","at":"sub","on_disk":null,"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn an_untrackable_name_carries_its_at_with_a_null_on_disk() {
        let refusal = unsafe_path_refusal(
            "sub/.git./x.yml",
            UnsafePathCause::UntrackableName {
                at: "sub/.git.".to_owned(),
            },
        );
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":"sub/.git./x.yml","#,
                    r#""cause":"untrackable-name","at":"sub/.git.","on_disk":null,"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn a_name_that_is_too_long_carries_its_at_with_a_null_on_disk() {
        let refusal = unsafe_path_refusal(
            "a/long.yml",
            UnsafePathCause::NameTooLong {
                at: "a/long.yml".to_owned(),
                bytes: 256,
                name: TooLongName::Staging,
            },
        );
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unsafe-path","manifest":"Cargo.toml","dependency":"a-skeleton","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0","path":"a/long.yml","#,
                    r#""cause":"name-too-long","at":"a/long.yml","on_disk":null,"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn drift_matches_carries_no_reason_field_at_all() {
        assert_eq!(
            serde_json::to_string(&drift_json(Drift::Matches)).expect("must serialize"),
            r#"{"state":"matches"}"#
        );
    }

    #[test]
    fn drift_drifted_carries_its_own_reason() {
        assert_eq!(
            serde_json::to_string(&drift_json(Drift::Drifted(DriftReason::Missing)))
                .expect("must serialize"),
            r#"{"state":"drifted","reason":"missing"}"#
        );
        assert_eq!(
            serde_json::to_string(&drift_json(Drift::Drifted(DriftReason::Changed)))
                .expect("must serialize"),
            r#"{"state":"drifted","reason":"changed"}"#
        );
    }

    #[test]
    fn a_crates_io_pin_serializes_as_registry_with_the_fixed_source() {
        assert_eq!(
            serde_json::to_string(&pin_json(&Pin::CratesIo, Path::new("/w")))
                .expect("must serialize"),
            format!(r#"{{"kind":"registry","source":"{CRATES_IO_SOURCE}","detail":"crates.io"}}"#)
        );
    }

    #[test]
    fn an_other_registry_pin_names_its_own_source_and_detail() {
        let pin = Pin::OtherRegistry {
            source: "sparse+https://example.invalid/index/".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"registry","source":"sparse+https://example.invalid/index/","#,
                r#""detail":"registry sparse+https://example.invalid/index/"}"#
            )
        );
    }

    #[test]
    fn a_tag_pin_carries_url_tag_and_commit() {
        let pin = Pin::Tag {
            url: "https://github.com/acme/toolchain-skeleton".to_owned(),
            tag: "v0.3.0".to_owned(),
            commit: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"tag","url":"https://github.com/acme/toolchain-skeleton","#,
                r#""tag":"v0.3.0","commit":"8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f","#,
                r#""detail":"tag v0.3.0 of https://github.com/acme/toolchain-skeleton"}"#
            )
        );
    }

    #[test]
    fn a_branch_pin_carries_url_branch_and_commit() {
        let pin = Pin::Branch {
            url: "https://github.com/acme/lint-skeleton".to_owned(),
            branch: "main".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"branch","url":"https://github.com/acme/lint-skeleton","#,
                r#""branch":"main","commit":"3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39","#,
                r#""detail":"branch main of https://github.com/acme/lint-skeleton at 3f2a9c1"}"#
            )
        );
    }

    #[test]
    fn a_default_branch_pin_carries_url_and_commit_with_no_branch_field_at_all() {
        let pin = Pin::DefaultBranch {
            url: "https://github.com/acme/lint-skeleton".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"default-branch","url":"https://github.com/acme/lint-skeleton","#,
                r#""commit":"3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39","#,
                r#""detail":"the default branch of https://github.com/acme/lint-skeleton "#,
                r#"at 3f2a9c1"}"#
            )
        );
    }

    #[test]
    fn a_rev_pin_carries_url_rev_and_commit() {
        let pin = Pin::Rev {
            url: "https://github.com/acme/some-skeleton".to_owned(),
            rev: "deadbee".to_owned(),
            commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"rev","url":"https://github.com/acme/some-skeleton","rev":"deadbee","#,
                r#""commit":"3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39","#,
                r#""detail":"rev deadbee of https://github.com/acme/some-skeleton"}"#
            )
        );
    }

    #[test]
    fn a_path_pin_shows_its_directory_relative_to_the_workspace_root() {
        let pin = Pin::Path {
            directory: Path::new("/w/ci-skeleton").to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w/root"))).expect("must serialize"),
            r#"{"kind":"path","path":"../ci-skeleton","detail":"path ../ci-skeleton"}"#
        );
    }

    #[test]
    fn an_unrecognised_pin_repeats_its_source_as_the_detail() {
        let pin = Pin::Unrecognised {
            source: "registry+file:///nowhere".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&pin_json(&pin, Path::new("/w"))).expect("must serialize"),
            concat!(
                r#"{"kind":"unrecognised","source":"registry+file:///nowhere","#,
                r#""detail":"registry+file:///nowhere"}"#
            )
        );
    }

    #[test]
    fn not_a_table_for_the_whole_wearing_table_carries_a_null_dependency() {
        let refusal = Refusal::Wearing(WearingRefusal::NotATable {
            manifest: "Cargo.toml".to_owned(),
            key: None,
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"not-a-table","manifest":"Cargo.toml","dependency":null,"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn not_a_table_for_one_wearing_key_carries_that_key_as_dependency() {
        let refusal = Refusal::Wearing(WearingRefusal::NotATable {
            manifest: "Cargo.toml".to_owned(),
            key: Some("dependabot".to_owned()),
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"not-a-table","manifest":"Cargo.toml","dependency":"dependabot","#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn reserved_key_reports_the_key_it_was_as_the_dependency() {
        // The crate declared under the key is named by the message, never by
        // the `dependency` field, which holds the reserved key itself.
        for key in ["options", "verbatim"] {
            let refusal = Refusal::Wearing(WearingRefusal::ReservedKey {
                manifest: "Cargo.toml".to_owned(),
                key: key.to_owned(),
                crate_name: "some-crate".to_owned(),
            });
            let message = refusal.message();
            let message_json = serde_json::to_string(&message).expect("a string always serializes");
            assert_eq!(
                serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
                format!(
                    concat!(
                        r#"{{"kind":"reserved-key","manifest":"Cargo.toml","dependency":"{key}","#,
                        r#""message":{message_json}}}"#
                    ),
                    key = key,
                    message_json = message_json
                )
            );
        }
    }

    #[test]
    fn names_no_dependency_carries_manifest_and_dependency() {
        let refusal = Refusal::Wearing(WearingRefusal::NamesNoDependency {
            manifest: "Cargo.toml".to_owned(),
            dependency: "dependabto".to_owned(),
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"names-no-dependency","manifest":"Cargo.toml","#,
                    r#""dependency":"dependabto","message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn unresolved_carries_manifest_and_dependency() {
        let refusal = Refusal::Wearing(WearingRefusal::Unresolved {
            manifest: "Cargo.toml".to_owned(),
            dependency: "dependabot".to_owned(),
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"unresolved","manifest":"Cargo.toml","dependency":"dependabot","#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn ambiguous_carries_every_resolved_package() {
        let refusal = Refusal::Wearing(WearingRefusal::Ambiguous {
            manifest: "Cargo.toml".to_owned(),
            dependency: "dependabot".to_owned(),
            packages: vec![
                ("a-dependabot-skeleton".to_owned(), "0.1.0".to_owned()),
                ("a-dependabot-skeleton".to_owned(), "0.2.0".to_owned()),
            ],
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"ambiguous","manifest":"Cargo.toml","dependency":"dependabot","#,
                    r#""packages":[{{"name":"a-dependabot-skeleton","version":"0.1.0"}},"#,
                    r#"{{"name":"a-dependabot-skeleton","version":"0.2.0"}}],"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn not_a_skeleton_carries_the_resolved_package() {
        let refusal = Refusal::Wearing(WearingRefusal::NotASkeleton {
            manifest: "Cargo.toml".to_owned(),
            dependency: "dependabot".to_owned(),
            package: ("some-crate".to_owned(), "1.0.0".to_owned()),
        });
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"not-a-skeleton","manifest":"Cargo.toml","#,
                    r#""dependency":"dependabot","#,
                    r#""package":{{"name":"some-crate","version":"1.0.0"}},"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn skeleton_invalid_names_the_file_and_line_the_render_refused_at() {
        let refusal = Refusal::Render {
            worn: WornId {
                manifest: "Cargo.toml".to_owned(),
                key: "lint".to_owned(),
            },
            skeleton: "lint-skeleton".to_owned(),
            version: semver::Version::new(0, 4, 0),
            error: RenderError::about_line(
                SkeletonIdentity::Named("lint-skeleton".to_owned()),
                "files/clippy.toml",
                NonZeroU32::new(3).expect("3 is nonzero"),
                Reason::PackageNameMissing,
            ),
        };
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"skeleton-invalid","manifest":"Cargo.toml","dependency":"lint","#,
                    r#""skeleton":"lint-skeleton","version":"0.4.0","file":"files/clippy.toml","#,
                    r#""line":3,"detail":"has no string `package.name`","message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn option_refused_names_no_file_at_all() {
        let refusal = Refusal::Render {
            worn: WornId {
                manifest: "Cargo.toml".to_owned(),
                key: "lint".to_owned(),
            },
            skeleton: "lint-skeleton".to_owned(),
            version: semver::Version::new(0, 4, 0),
            error: RenderError::about_choice(
                SkeletonIdentity::Named("lint-skeleton".to_owned()),
                Reason::PackageNameMissing,
            ),
        };
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"option-refused","manifest":"Cargo.toml","dependency":"lint","#,
                    r#""skeleton":"lint-skeleton","version":"0.4.0","#,
                    r#""detail":"has no string `package.name`","message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn overlap_carries_every_colliding_path_and_claimant() {
        let overlap = Overlap {
            paths: vec![ClaimPath::from_rendering_path("dependabot.yml").expect("a valid claim")],
            claimants: vec![
                Claimant {
                    manifest: "Cargo.toml".to_owned(),
                    dependency: "a".to_owned(),
                    skeleton: "a-skeleton".to_owned(),
                    version: semver::Version::new(0, 1, 0),
                },
                Claimant {
                    manifest: "Cargo.toml".to_owned(),
                    dependency: "b".to_owned(),
                    skeleton: "b-skeleton".to_owned(),
                    version: semver::Version::new(0, 1, 0),
                },
            ],
        };
        let refusal = Refusal::Overlap(overlap);
        let message = refusal.message();
        let message_json = serde_json::to_string(&message).expect("a string always serializes");
        assert_eq!(
            serde_json::to_string(&refusal_json(&refusal)).expect("must serialize"),
            format!(
                concat!(
                    r#"{{"kind":"overlap","paths":["dependabot.yml"],"#,
                    r#""claimants":[{{"manifest":"Cargo.toml","dependency":"a","#,
                    r#""skeleton":"a-skeleton","version":"0.1.0"}},{{"manifest":"Cargo.toml","#,
                    r#""dependency":"b","skeleton":"b-skeleton","version":"0.1.0"}}],"#,
                    r#""message":{message_json}}}"#
                ),
                message_json = message_json
            )
        );
    }

    #[test]
    fn a_pin_detail_is_one_line_while_its_own_fields_keep_the_exact_text() {
        // `detail` is prose for a person, so the newline is escaped in it;
        // `url`, `tag` and `commit` are data a program reads, so they carry
        // the exact string.
        let pin = Pin::Tag {
            url: POISON.to_owned(),
            tag: POISON.to_owned(),
            commit: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
        };

        let value = serde_json::to_value(pin_json(&pin, Path::new("/w"))).expect("must serialize");

        assert_eq!(value["url"], POISON);
        assert_eq!(value["tag"], POISON);
        assert_eq!(
            value["detail"],
            format!("tag {POISON_ESCAPED} of {POISON_ESCAPED}")
        );
        assert_escaped_once(
            value["detail"].as_str().expect("a string"),
            "the pin detail",
        );
    }

    #[test]
    fn a_behind_detail_is_one_line_while_a_newer_tag_keeps_the_exact_text() {
        let behind = Behind::Behind(Newer::Tag(POISON.to_owned()));
        let value =
            serde_json::to_value(behind_json(&Pin::CratesIo, &behind)).expect("must serialize");
        assert_eq!(value["newer"]["tag"], POISON);
        assert_eq!(
            value["detail"],
            format!("tag {POISON_ESCAPED} is available")
        );
        assert_escaped_once(
            value["detail"].as_str().expect("a string"),
            "the newer detail",
        );

        let undetermined = Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::Unreachable,
            detail: format!("could not reach {POISON}"),
        });
        let value = serde_json::to_value(behind_json(&Pin::CratesIo, &undetermined))
            .expect("must serialize");
        assert_eq!(value["detail"], format!("could not reach {POISON_ESCAPED}"));
        assert_escaped_once(
            value["detail"].as_str().expect("a string"),
            "the undetermined detail",
        );
    }

    #[test]
    fn an_aborted_document_holds_cargos_words_on_one_line() {
        let error = ReadWorkspaceError::CargoMetadataFailed {
            stderr: POISON.to_owned(),
        };

        let document = super::aborted(&error);

        let value: serde_json::Value = serde_json::from_str(&document).expect("valid JSON");
        assert_eq!(
            value["aborted"]["message"],
            format!("cargo metadata --locked failed: {POISON_ESCAPED}")
        );
        assert_escaped_once(
            value["aborted"]["message"].as_str().expect("a string"),
            "the abort message",
        );
    }
}
