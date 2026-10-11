//! Golden tests: `check`'s human output and `--json` document, for the
//! four-bone example of record, must be exactly what `.docs/wearing.md`
//! shows — read from that file itself, never from a second copy kept here.
//!
//! The doc and this crate's own output can drift apart in exactly one
//! direction unnoticed: a change to `table.rs` or `json.rs` that the doc is
//! never updated to match. Reading the expected text out of the doc, rather
//! than writing it out again by hand in this file, means that drift is a
//! failing test naming the paragraph a reader actually sees, not a second
//! copy for someone to remember to keep in step.
//!
//! `.docs/` sits above this crate's directory, so the published package does
//! not carry it: this module is empty unless the build is from a checkout
//! (`skeletons_checkout`, set by `build.rs`). The condition is an inner
//! attribute rather than one on the `mod` declaration in `check.rs` so the
//! module stays a `#[cfg(test)]` one, which is what the workspace's
//! clippy configuration recognises as test code.

#![cfg(skeletons_checkout)]

use std::num::NonZeroU32;
use std::path::PathBuf;

use super::report::{Filter, Report};
use crate::behind::{Behind, Newer, Undetermined, UndeterminedReason};
use crate::claim::{ClaimPath, Claimant, Drift, DriftReason};
use crate::git::ObjectId;
use crate::skeleton::{Choices, Reason, RenderError, SkeletonIdentity};
use crate::survey::{Refusal, Row, Survey};
use crate::workspace::{Pin, WearingRefusal, WornDependency};

/// `text` parsed as an [`ObjectId`], for this fixture's own pins.
fn oid(text: &str) -> ObjectId {
    ObjectId::parse(text).expect("a well-formed fixture object id")
}

/// `.docs/wearing.md`, read once at compile time — the one copy of the
/// worked examples every test in this file reads its expected text from.
const WEARING_DOC: &str = include_str!("../../../../.docs/wearing.md");

/// The fenced block that immediately follows the line
/// `<!-- example: <marker> -->` in `.docs/wearing.md`: the text between the
/// fence's opening and closing lines, with neither fence line nor a trailing
/// newline.
///
/// # Panics
///
/// If `marker` does not appear in the doc, or is not followed by a fenced
/// block — a marker that moved or was renamed must fail loudly, naming
/// itself, rather than silently comparing against the wrong paragraph or an
/// empty string.
fn example_block(marker: &str) -> &'static str {
    let marker_line = format!("<!-- example: {marker} -->");
    let Some(marker_at) = WEARING_DOC.find(&marker_line) else {
        panic!("`.docs/wearing.md` has no `{marker_line}` marker");
    };
    let after_marker = &WEARING_DOC[marker_at + marker_line.len()..];

    let Some(fence_at) = after_marker.find("```") else {
        panic!("`{marker_line}` is not followed by a fenced block");
    };
    let fence_line = &after_marker[fence_at..];
    let Some(first_newline) = fence_line.find('\n') else {
        panic!("`{marker_line}`'s own fenced block has no content");
    };
    let content = &fence_line[first_newline + 1..];

    let Some(closing_at) = content.find("\n```") else {
        panic!("`{marker_line}`'s own fenced block is never closed");
    };
    &content[..closing_at]
}

/// One worn skeleton of the fixture the golden tests share: a worn
/// dependency and the behind fact it reads.
struct FixtureSkeleton {
    worn: WornDependency,
    behind: Behind,
}

/// The workspace root the fixture's own path pin is shown relative to:
/// `ci`'s directory, `/w/ci-skeleton`, sits beside it, so
/// `pin_words::pin_detail` shows it relative rather than falling back to the
/// absolute form.
fn fixture_root() -> PathBuf {
    "/w/repo".into()
}

fn worn(
    manifest: &str,
    key: &str,
    package: &str,
    version: (u64, u64, u64),
    pin: Pin,
) -> WornDependency {
    WornDependency {
        manifest: manifest.to_owned(),
        key: key.to_owned(),
        package: package.to_owned(),
        version: semver::Version::new(version.0, version.1, version.2),
        skeleton_directory: "/skeleton".into(),
        pin,
        choices: Ok(Choices::new()),
    }
}

fn claim_path(path: &str) -> ClaimPath {
    ClaimPath::from_rendering_path(path).expect("every fixture path is a valid claim")
}

/// The four worn skeletons of the full example in `.docs/wearing.md`, in
/// the order it lists them: a
/// registry-pinned skeleton that is behind and has a drifted bone, a
/// branch-pinned skeleton whose own render is refused and whose behind
/// answer is undetermined, a tag-pinned skeleton that is current and whose
/// bones match, and a path-pinned skeleton that is pinned and whose bone is
/// missing.
fn fixture_skeletons() -> Vec<FixtureSkeleton> {
    vec![
        FixtureSkeleton {
            worn: worn(
                "Cargo.toml",
                "dependabot",
                "a-dependabot-skeleton",
                (0, 1, 2),
                Pin::CratesIo,
            ),
            behind: Behind::Behind(Newer::Version(semver::Version::new(0, 2, 0))),
        },
        FixtureSkeleton {
            worn: worn(
                "Cargo.toml",
                "lint",
                "lint-skeleton",
                (0, 4, 0),
                Pin::Branch {
                    url: "https://github.com/acme/lint-skeleton".to_owned(),
                    branch: "main".to_owned(),
                    commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
                },
            ),
            behind: Behind::Undetermined(Undetermined {
                reason: UndeterminedReason::Unreachable,
                detail: "could not reach https://github.com/acme/lint-skeleton: unable to \
                         access 'https://github.com/acme/lint-skeleton/': Could not resolve \
                         host: github.com"
                    .to_owned(),
            }),
        },
        FixtureSkeleton {
            worn: worn(
                "Cargo.toml",
                "toolchain",
                "toolchain-skeleton",
                (0, 3, 0),
                Pin::Tag {
                    url: "https://github.com/acme/toolchain-skeleton".to_owned(),
                    tag: "v0.3.0".to_owned(),
                    commit: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
                },
            ),
            behind: Behind::Current,
        },
        FixtureSkeleton {
            worn: worn(
                "tools/Cargo.toml",
                "ci",
                "ci-skeleton",
                (0, 1, 0),
                Pin::Path {
                    directory: "/w/ci-skeleton".into(),
                },
            ),
            behind: Behind::Pinned,
        },
    ]
}

/// The bones every skeleton but `lint` contributes (`lint`'s own render is
/// refused, so it claims nothing): `dependabot`'s file has changed,
/// `toolchain`'s two files both match, and `ci`'s file is missing.
fn fixture_rows(skeletons: &[FixtureSkeleton]) -> Vec<Row> {
    let claimant = |skeleton: &WornDependency| Claimant {
        manifest: skeleton.manifest.clone(),
        dependency: skeleton.key.clone(),
        skeleton: skeleton.package.clone(),
        version: skeleton.version.clone(),
    };
    let mut rows = vec![
        Row {
            path: claim_path(".github/dependabot.yml"),
            claimant: claimant(&skeletons[0].worn),
            drift: Drift::Drifted(DriftReason::Changed),
            rendered: b"placeholder render".to_vec(),
        },
        Row {
            path: claim_path("rust-toolchain.toml"),
            claimant: claimant(&skeletons[2].worn),
            drift: Drift::Matches,
            rendered: b"placeholder render".to_vec(),
        },
        Row {
            path: claim_path("rustfmt.toml"),
            claimant: claimant(&skeletons[2].worn),
            drift: Drift::Matches,
            rendered: b"placeholder render".to_vec(),
        },
        Row {
            path: claim_path(".github/workflows/ci.yml"),
            claimant: claimant(&skeletons[3].worn),
            drift: Drift::Drifted(DriftReason::Missing),
            rendered: b"placeholder render".to_vec(),
        },
    ];
    // The survey itself always hands `check`/`sync` rows in path order
    // (`survey::survey`'s own postcondition); this fixture reproduces that
    // ordering rather than relying on it, so a golden test failure is never
    // actually about this file's own row order.
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows
}

/// `lint`'s own render refusal (`skeleton-invalid`: a defect named at
/// `files/clippy.toml:3`), and the wearing table refusal about no single
/// skeleton (`tools/Cargo.toml`'s `dependabto` key names no dependency) —
/// the two refusals of that example.
fn fixture_refusals(skeletons: &[FixtureSkeleton]) -> Vec<Refusal> {
    vec![
        Refusal::Render {
            worn: skeletons[1].worn.id(),
            skeleton: skeletons[1].worn.package.clone(),
            version: skeletons[1].worn.version.clone(),
            error: RenderError::about_line(
                SkeletonIdentity::Named("lint-skeleton".to_owned()),
                "files/clippy.toml",
                NonZeroU32::new(3).expect("3 is nonzero"),
                Reason::PlaceholderNotFillOption {
                    name: "cadense".to_owned(),
                    declared_as: None,
                },
            ),
        },
        Refusal::Wearing(WearingRefusal::NamesNoDependency {
            manifest: "tools/Cargo.toml".to_owned(),
            dependency: "dependabto".to_owned(),
        }),
    ]
}

/// Builds the full four-skeleton example of `.docs/wearing.md` as a [`Report`], and
/// hands it to `read` — every golden test's own repository, described
/// exactly once here, so `table::lines` and `json::document` are always
/// compared against the same fixture rather than two that could quietly
/// drift apart.
///
/// A closure, not a returned `Report`, because [`Survey`]'s own `worn` field
/// borrows the [`WornDependency`] values this function builds: handing the
/// report to `read` while those values are still in scope avoids leaking
/// them, or a self-referential struct, just to give it a `'static` lifetime
/// it does not need.
fn with_fixture_report<Result>(read: impl FnOnce(&Report<'_>) -> Result) -> Result {
    let skeletons = fixture_skeletons();
    let worn: Vec<&WornDependency> = skeletons.iter().map(|skeleton| &skeleton.worn).collect();
    let rows = fixture_rows(&skeletons);
    let refusals = fixture_refusals(&skeletons);

    let survey = Survey {
        root: fixture_root(),
        worn,
        rows,
        refusals,
    };
    let behind = skeletons
        .iter()
        .map(|skeleton| (skeleton.worn.id(), skeleton.behind.clone()))
        .collect();
    let report = Report::new(survey, behind);
    read(&report)
}

#[cfg(test)]
mod tests {
    use super::{Filter, example_block, with_fixture_report};
    use crate::check::{json, table};
    use crate::workspace::ReadWorkspaceError;

    /// Verifies `check`'s plain human output, for the four-skeleton example
    /// of `.docs/wearing.md`, is exactly what that file shows under
    /// `<!-- example: check-human -->` — word for word, including the
    /// column alignment `table.rs` computes.
    #[test]
    fn the_documented_human_example_is_what_check_prints() {
        let printed =
            with_fixture_report(|report| table::lines(report, Filter::default(), false).join("\n"));
        assert_eq!(printed, example_block("check-human"));
    }

    /// Verifies `check --json`'s document, for the same example, matches
    /// `.docs/wearing.md`'s own JSON block under
    /// `<!-- example: check-json -->` — parsed to [`serde_json::Value`] on
    /// both sides, since object key order is explicitly not promised, and a
    /// `Value` comparison (unlike a field-by-field read)
    /// still fails on a stray extra key the one-shape rule (see
    /// `check/json.rs`) would otherwise let through unnoticed.
    #[test]
    fn the_documented_json_example_is_what_check_json_prints() {
        let printed =
            with_fixture_report(|report| json::document(report, Filter::default(), false));
        let printed_value: serde_json::Value =
            serde_json::from_str(&printed).expect("check --json always produces valid JSON");
        let documented_value: serde_json::Value = serde_json::from_str(example_block("check-json"))
            .expect("the documented example must itself be valid JSON");
        assert_eq!(printed_value, documented_value);
    }

    /// Verifies the aborted document `check --json` prints when the
    /// workspace itself could not be read matches `.docs/wearing.md`'s own
    /// block under `<!-- example: check-json-aborted -->`.
    #[test]
    fn the_documented_aborted_example_is_what_an_abort_prints() {
        let printed = json::aborted(&ReadWorkspaceError::Lockfile);
        let printed_value: serde_json::Value =
            serde_json::from_str(&printed).expect("check --json always produces valid JSON");
        let documented_value: serde_json::Value =
            serde_json::from_str(example_block("check-json-aborted"))
                .expect("the documented example must itself be valid JSON");
        assert_eq!(printed_value, documented_value);
    }
}
