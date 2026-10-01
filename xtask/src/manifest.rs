//! The release version as the root manifest records it.
//!
//! It is written in two kinds of place: `[workspace.package] version`, which
//! every member inherits, and the `version` requirement on every
//! `[workspace.dependencies]` entry that has a `path` — this workspace's own
//! crates, which a published manifest can only find by version. Here that is
//! `skeletons` alone; `rituals` and `rituals-core` come from crates.io at a
//! version of their own and are never touched. The tests below check that the
//! two kinds of site agree; this module is what keeps them agreeing when the
//! version moves.

use std::error::Error;
use std::fmt;

use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::version::{ParseVersionError, Version};

/// The version in `[workspace.package]`.
pub(crate) fn workspace_version(document: &DocumentMut) -> Result<Version, ManifestError> {
    let text = document
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .ok_or(ManifestError::MissingWorkspaceVersion)?
        .as_str()
        .ok_or(ManifestError::WorkspaceVersionNotAString)?;
    Version::parse(text).map_err(ManifestError::WorkspaceVersionNotARelease)
}

/// Writes `version` to `[workspace.package]` and to every internal requirement
/// in `[workspace.dependencies]`, and returns the names of the internal
/// dependencies it rewrote.
///
/// Only the version strings change: comments, key order and spacing around
/// each value stay as they were. Nothing is written unless every site can be:
/// an internal dependency without a `version` is refused rather than given
/// one, since its absence means the manifest has stopped looking the way this
/// code assumes.
pub(crate) fn set_release_version(
    document: &mut DocumentMut,
    version: Version,
) -> Result<Vec<String>, ManifestError> {
    let internal = internal_dependency_names(document)?;
    if internal.is_empty() {
        return Err(ManifestError::NoInternalDependencies);
    }
    let text = version.to_string();

    let package_version = document
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("package"))
        .and_then(|package| package.get_mut("version"))
        .ok_or(ManifestError::MissingWorkspaceVersion)?;
    replace_string(package_version, &text).ok_or(ManifestError::WorkspaceVersionNotAString)?;

    let dependencies = workspace_dependencies_mut(document)?;
    for name in &internal {
        let requirement = dependencies
            .get_mut(name)
            .and_then(|entry| entry.as_table_like_mut())
            .and_then(|entry| entry.get_mut("version"))
            .ok_or_else(|| ManifestError::InternalWithoutVersion(name.clone()))?;
        replace_string(requirement, &text)
            .ok_or_else(|| ManifestError::InternalWithoutVersion(name.clone()))?;
    }
    Ok(internal)
}

/// The keys of every `[workspace.dependencies]` entry with a `path`, each
/// checked to carry a string `version` before anything is edited.
fn internal_dependency_names(document: &DocumentMut) -> Result<Vec<String>, ManifestError> {
    let dependencies = document
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Item::as_table_like)
        .ok_or(ManifestError::MissingWorkspaceDependencies)?;
    let mut names = Vec::new();
    for (name, entry) in dependencies.iter() {
        let Some(entry) = entry.as_table_like() else {
            continue;
        };
        if entry.get("path").is_none() {
            continue;
        }
        if entry.get("version").and_then(Item::as_str).is_none() {
            return Err(ManifestError::InternalWithoutVersion(name.to_string()));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

fn workspace_dependencies_mut(
    document: &mut DocumentMut,
) -> Result<&mut dyn TableLike, ManifestError> {
    document
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("dependencies"))
        .and_then(Item::as_table_like_mut)
        .ok_or(ManifestError::MissingWorkspaceDependencies)
}

/// Replaces the string at `item` with `text`, keeping the whitespace and
/// comments around it. `None` if `item` is not a string.
fn replace_string(item: &mut Item, text: &str) -> Option<()> {
    let value = item.as_value_mut()?;
    value.as_str()?;
    let decor = value.decor().clone();
    *value = Value::from(text);
    *value.decor_mut() = decor;
    Some(())
}

/// The root manifest does not have the shape a release version needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManifestError {
    MissingWorkspaceVersion,
    WorkspaceVersionNotAString,
    WorkspaceVersionNotARelease(ParseVersionError),
    MissingWorkspaceDependencies,
    NoInternalDependencies,
    InternalWithoutVersion(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingWorkspaceVersion => {
                formatter.write_str("the root manifest declares no [workspace.package] version")
            }
            Self::WorkspaceVersionNotAString => {
                formatter.write_str("[workspace.package] version is not a string")
            }
            Self::WorkspaceVersionNotARelease(error) => {
                write!(formatter, "[workspace.package] version: {error}")
            }
            Self::MissingWorkspaceDependencies => {
                formatter.write_str("the root manifest declares no [workspace.dependencies]")
            }
            Self::NoInternalDependencies => formatter.write_str(
                "[workspace.dependencies] declares none of this workspace's own crates by path",
            ),
            Self::InternalWithoutVersion(name) => write!(
                formatter,
                "[workspace.dependencies] {name} has a path but no version string"
            ),
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WorkspaceVersionNotARelease(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::process::{self, Program};

    /// This repository's own root manifest, so the tests edit the file the
    /// release workflow edits rather than a sketch of it.
    const ROOT_MANIFEST: &str = include_str!("../../Cargo.toml");

    fn parse(text: &str) -> DocumentMut {
        text.parse().expect("the fixture is valid TOML")
    }

    fn release(text: &str) -> Version {
        Version::parse(text).expect("the fixture version is a release")
    }

    #[test]
    fn bumping_the_root_manifest_moves_every_site_and_only_those_lines() {
        let mut document = parse(ROOT_MANIFEST);
        let before = workspace_version(&document).expect("the root manifest has a version");
        let after = release("98.76.54");
        assert_ne!(before, after);

        let rewritten = set_release_version(&mut document, after).expect("the bump applies");
        assert!(
            !rewritten.is_empty(),
            "the root manifest names its own crates"
        );
        assert_eq!(workspace_version(&document), Ok(after));

        let edited = document.to_string();
        let old = format!("\"{before}\"");
        let new = format!("\"{after}\"");
        let original_lines: Vec<&str> = ROOT_MANIFEST.lines().collect();
        let edited_lines: Vec<&str> = edited.lines().collect();
        assert_eq!(original_lines.len(), edited_lines.len());
        let mut changed = 0;
        for (original, edited) in original_lines.iter().zip(&edited_lines) {
            if original != edited {
                changed += 1;
                assert_eq!(
                    original.replacen(&old, &new, 1),
                    *edited,
                    "a changed line changed anything but its version"
                );
            }
        }
        // The package version plus one line per internal dependency.
        assert_eq!(changed, 1 + rewritten.len());
    }

    /// The one line in the root manifest that declares `key`, as written.
    fn declaration<'text>(manifest: &'text str, key: &str) -> &'text str {
        let prefix = format!("{key} = ");
        let mut lines = manifest.lines().filter(|line| line.starts_with(&prefix));
        let line = lines.next().expect("the manifest declares the key");
        assert_eq!(lines.next(), None, "the manifest declares {key} once");
        line
    }

    #[test]
    fn a_bump_moves_skeletons_with_the_workspace_and_leaves_rituals_and_rituals_core_alone() {
        // The workspace first moves to the version `rituals` is pinned at, so
        // a bump that matched on the old version string, rather than on
        // which dependency carries a `path`, would move `rituals` too.
        let mut document = parse(ROOT_MANIFEST);
        let registry = ["rituals", "ritual"].map(|key| declaration(ROOT_MANIFEST, key));
        let dependencies = &document["workspace"]["dependencies"];
        let pinned = dependencies["rituals"]
            .as_str()
            .expect("rituals is required by version alone");
        assert_eq!(
            dependencies["ritual"]["version"].as_str(),
            Some(pinned),
            "rituals and rituals-core are asked for at one version"
        );
        let pinned = release(pinned);
        set_release_version(&mut document, pinned).expect("the first bump applies");
        let mut reparsed = parse(&document.to_string());
        let after = release("98.76.54");
        assert_ne!(pinned, after);

        let rewritten = set_release_version(&mut reparsed, after).expect("the second bump applies");
        assert_eq!(rewritten, ["skeletons"]);
        let bumped = reparsed.to_string();
        assert_eq!(workspace_version(&reparsed), Ok(after));
        assert_eq!(
            declaration(&bumped, "skeletons"),
            format!("skeletons = {{ path = \"crates/skeletons\", version = \"{after}\" }}")
        );
        assert_eq!(
            ["rituals", "ritual"].map(|key| declaration(&bumped, key)),
            registry,
            "a crate from crates.io keeps its own version"
        );
    }

    #[test]
    fn a_bumped_manifest_reads_back_and_bumps_again() {
        let mut document = parse(ROOT_MANIFEST);
        set_release_version(&mut document, release("0.2.0")).expect("the first bump applies");
        let mut reparsed = parse(&document.to_string());
        assert_eq!(workspace_version(&reparsed), Ok(release("0.2.0")));
        set_release_version(&mut reparsed, release("0.3.0")).expect("the second bump applies");
        assert_eq!(workspace_version(&reparsed), Ok(release("0.3.0")));
    }

    #[test]
    fn a_dependency_table_written_as_a_table_is_rewritten_too() {
        let mut document = parse(
            "[workspace.package]\nversion = \"0.1.0\"\n\n\
             [workspace.dependencies.skeletons]\npath = \"crates/skeletons\"\n\
             version = \"0.1.0\" # kept\n",
        );
        set_release_version(&mut document, release("0.1.1")).expect("the bump applies");
        assert_eq!(
            document.to_string(),
            "[workspace.package]\nversion = \"0.1.1\"\n\n\
             [workspace.dependencies.skeletons]\npath = \"crates/skeletons\"\n\
             version = \"0.1.1\" # kept\n",
        );
    }

    #[test]
    fn an_internal_dependency_without_a_version_is_refused_and_nothing_changes() {
        let text = "[workspace.package]\nversion = \"0.1.0\"\n\n[workspace.dependencies]\n\
                    skeletons = { path = \"crates/skeletons\", version = \"0.1.0\" }\n\
                    command-line = { package = \"skeletons-ritual\", path = \"ritual\" }\n";
        let mut document = parse(text);
        assert_eq!(
            set_release_version(&mut document, release("0.1.1")),
            Err(ManifestError::InternalWithoutVersion(
                "command-line".to_string()
            ))
        );
        assert_eq!(document.to_string(), text);
    }

    #[test]
    fn registry_dependencies_are_left_alone() {
        let text = "[workspace.package]\nversion = \"0.1.0\"\n\n[workspace.dependencies]\n\
                    skeletons = { path = \"crates/skeletons\", version = \"0.1.0\" }\n\
                    ritual = { package = \"rituals-core\", version = \"0.1.0\" }\n\
                    rituals = \"0.1.0\"\n";
        let mut document = parse(text);
        let rewritten =
            set_release_version(&mut document, release("0.1.1")).expect("the bump applies");
        assert_eq!(rewritten, ["skeletons"]);
        assert!(document.to_string().contains(
            "ritual = { package = \"rituals-core\", version = \"0.1.0\" }\nrituals = \"0.1.0\"\n"
        ));
    }

    #[test]
    fn a_manifest_without_the_expected_shape_is_refused() {
        let cases = [
            ("", ManifestError::MissingWorkspaceVersion),
            (
                "[workspace.package]\nversion = 1\n",
                ManifestError::WorkspaceVersionNotAString,
            ),
            (
                "[workspace.package]\nversion = \"0.1.0\"\n",
                ManifestError::MissingWorkspaceDependencies,
            ),
            (
                "[workspace.package]\nversion = \"0.1.0\"\n[workspace.dependencies]\nx = \"1\"\n",
                ManifestError::NoInternalDependencies,
            ),
        ];
        for (text, error) in cases {
            let mut document = parse(text);
            let outcome = workspace_version(&document)
                .and_then(|_| set_release_version(&mut document, release("0.1.1")));
            assert_eq!(outcome.map(|_| ()), Err(error), "for {text:?}");
        }
    }

    #[test]
    fn a_workspace_version_that_is_not_a_release_is_refused() {
        let document = parse("[workspace.package]\nversion = \"0.1.0-rc.1\"\n");
        assert!(matches!(
            workspace_version(&document),
            Err(ManifestError::WorkspaceVersionNotARelease(_))
        ));
    }

    /// Every workspace member's manifest, as Cargo lists the members.
    fn member_manifests() -> Vec<String> {
        let metadata = process::query(
            Program::Cargo,
            &crate::workspace::root(),
            &["metadata", "--no-deps", "--format-version", "1"],
        )
        .expect("cargo metadata runs on this workspace");
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata).expect("cargo metadata prints JSON");
        metadata["packages"]
            .as_array()
            .expect("cargo metadata lists packages")
            .iter()
            .map(|package| {
                package["manifest_path"]
                    .as_str()
                    .expect("every package has a manifest path")
                    .to_string()
            })
            .collect()
    }

    /// What in one member's manifest would let its version, or the version of
    /// a crate of this workspace it depends on, drift from what a bump
    /// writes: a version of its own, or a dependency declared by `path`
    /// outside `[workspace.dependencies]`, where the bump never looks.
    fn member_drift(manifest_path: &Path) -> Vec<String> {
        let text = std::fs::read_to_string(manifest_path).expect("the member manifest reads");
        let document = parse(&text);
        let name = document["package"]["name"]
            .as_str()
            .expect("the member has a name");
        let mut drift = Vec::new();
        let inherits = document
            .get("package")
            .and_then(|package| package.get("version"))
            .and_then(|version| version.get("workspace"))
            .and_then(Item::as_bool);
        if inherits != Some(true) {
            drift.push(format!("{name} does not declare version.workspace = true"));
        }
        for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let Some(entries) = document.get(table).and_then(Item::as_table_like) else {
                continue;
            };
            for (key, entry) in entries.iter() {
                if entry.get("path").is_some() {
                    drift.push(format!(
                        "{name} declares [{table}] {key} by path itself, not through \
                         [workspace.dependencies]"
                    ));
                }
            }
        }
        drift
    }

    #[test]
    fn every_member_and_every_internal_requirement_is_at_the_workspace_version() {
        let document = parse(ROOT_MANIFEST);
        let version = workspace_version(&document)
            .expect("the root manifest has a version")
            .to_string();
        let internal = internal_dependency_names(&document).expect("the shape is valid");
        assert_eq!(
            internal,
            ["skeletons"],
            "the one crate this workspace releases"
        );
        let mut drift: Vec<String> = internal
            .iter()
            .filter_map(|name| {
                let requirement = document["workspace"]["dependencies"][name.as_str()]["version"]
                    .as_str()
                    .unwrap_or_default();
                (requirement != version).then(|| {
                    format!(
                        "[workspace.dependencies] {name} requires {requirement:?}, not {version:?}"
                    )
                })
            })
            .collect();

        let members = member_manifests();
        assert_eq!(
            members.len(),
            4,
            "skeletons, skeletons-ritual, skeletons-wearer and xtask: {members:?}"
        );
        for manifest_path in &members {
            drift.extend(member_drift(Path::new(manifest_path)));
        }
        assert!(
            drift.is_empty(),
            "every crate must release at the workspace version {version}:\n{}",
            drift.join("\n")
        );
    }
}
