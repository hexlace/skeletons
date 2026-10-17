//! A `Cargo.toml`, read as text: the package it names, its targets, every dependency it declares,
//! and, for a workspace root, what the workspace holds.
//!
//! Only what a reader of a workspace without Cargo needs is kept. A manifest that Cargo would
//! refuse is refused here too, and a manifest that uses something this reader does not follow is
//! refused as unsupported, never read differently.

use std::collections::BTreeMap;
use std::path::Path;

use semver::Version;
use serde::Deserialize;

mod declarations;
mod manifest_targets;

pub(crate) use declarations::{Declaration, DeclaredSource, GitReference};
pub(crate) use manifest_targets::{Edition, TargetKind, Targets};

use declarations::{declarations_of, workspace_declarations};
use manifest_targets::{RawTarget, targets_of};

use super::text_error::TextError;

/// The version Cargo gives a package that writes none.
const DEFAULT_VERSION: &str = "0.0.0";

/// A table of dependencies as written: each key with its value, whichever shape that has.
type RawDependencies = BTreeMap<String, toml::Value>;

/// One `Cargo.toml`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PackageManifest {
    package: Option<PackageSection>,
    targets: Targets,
    declarations: Vec<Declaration>,
    workspace: Option<WorkspaceRoot>,
    /// Whether the manifest writes `[patch]`. Cargo applies it from a workspace root, and from a
    /// package that is its own root, so it is read from any manifest.
    has_patch: bool,
    /// Whether the manifest writes `[replace]`, under the same rule as `[patch]`.
    has_replace: bool,
}

/// The `[package]` of a manifest.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PackageSection {
    /// `package.name`.
    pub(crate) name: String,
    /// `package.version`, taken from `[workspace.package]` when it is `version.workspace = true`,
    /// and `0.0.0` when none is written.
    pub(crate) version: String,
    /// Whether `package.workspace` is written, which names a workspace root by path.
    pub(crate) has_workspace_key: bool,
    /// `[package.metadata]` as `cargo metadata` reports it: `null` when there is none, and a
    /// datetime as the object `{"$__toml_private_datetime": "<its text>"}`.
    pub(crate) metadata: serde_json::Value,
}

/// What a manifest with a `[workspace]` table says about its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRoot {
    /// `workspace.members`, as written.
    pub(crate) members: Vec<String>,
    /// `workspace.exclude`, as written.
    pub(crate) exclude: Vec<String>,
    /// `[workspace.dependencies]`, by key, with each path joined onto the root's directory.
    pub(crate) dependencies: BTreeMap<String, Declaration>,
    /// `workspace.package.version`, when written.
    pub(crate) package_version: Option<String>,
    /// `workspace.package.edition`, when written.
    pub(crate) package_edition: Option<Edition>,
}

/// The manifest as written, which `manifest_targets` and `declarations` each read their part of.
#[derive(Deserialize)]
struct RawManifest {
    package: Option<RawPackage>,
    lib: Option<RawLib>,
    bin: Option<Vec<RawTarget>>,
    example: Option<Vec<RawTarget>>,
    test: Option<Vec<RawTarget>>,
    bench: Option<Vec<RawTarget>>,
    #[serde(flatten)]
    tables: RawDependencyTables,
    target: Option<BTreeMap<String, RawDependencyTables>>,
    workspace: Option<RawWorkspace>,
    patch: Option<toml::Value>,
    replace: Option<toml::Value>,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    version: Option<toml::Value>,
    edition: Option<toml::Value>,
    workspace: Option<toml::Value>,
    metadata: Option<toml::Value>,
    autolib: Option<bool>,
    autobins: Option<bool>,
    autoexamples: Option<bool>,
    autotests: Option<bool>,
    autobenches: Option<bool>,
}

#[derive(Deserialize)]
struct RawLib {
    name: Option<String>,
    path: Option<String>,
}

/// The dependency tables of a manifest or of one `[target.<cfg>]`. Cargo reads `dev-dependencies`
/// and `build-dependencies` under their underscore spellings too, and prefers the hyphenated one
/// when both are written.
#[derive(Deserialize, Default)]
struct RawDependencyTables {
    dependencies: Option<RawDependencies>,
    #[serde(rename = "dev-dependencies")]
    dev_hyphenated: Option<RawDependencies>,
    #[serde(rename = "dev_dependencies")]
    dev_underscored: Option<RawDependencies>,
    #[serde(rename = "build-dependencies")]
    build_hyphenated: Option<RawDependencies>,
    #[serde(rename = "build_dependencies")]
    build_underscored: Option<RawDependencies>,
}

#[derive(Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    dependencies: Option<RawDependencies>,
    package: Option<RawWorkspacePackage>,
}

#[derive(Deserialize)]
struct RawWorkspacePackage {
    version: Option<String>,
    edition: Option<String>,
}

/// What a read of a manifest does with a shape Cargo loads and this reader does not follow where it
/// compares sources. Today that is a git URL with a query of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unfollowed {
    /// Refuse the manifest as unsupported, for a reader that compares the dependency's source
    /// with the lock's.
    Refuse,
    /// Read the declaration with the shape as written, for a reader that compares no source of
    /// it. A git URL keeps its query, so it is never the URL of a locked source, which the
    /// lockfile read refuses any query on but one reference: the declaration names no worn
    /// package, as for Cargo, where it is another source.
    ReadPast,
}

impl PackageManifest {
    /// Reads a manifest's text, refusing a shape Cargo loads and this reader does not follow
    /// ([`Unfollowed::Refuse`]).
    ///
    /// `directory` is where the manifest sits, which a `path` dependency is relative to.
    /// `workspace` is the root of the workspace the manifest is a member of, which a
    /// `version.workspace = true` and a `workspace = true` dependency take their values from. A
    /// manifest with a `[workspace]` of its own is its own workspace root and ignores it.
    ///
    /// # Errors
    ///
    /// Unreadable for text that is not a manifest Cargo would read, among it a `git` dependency
    /// of a package that also writes a `path`, which Cargo refuses as ambiguous, and for an
    /// inherited value that no workspace gives. Unsupported for a `git` dependency whose URL has
    /// a query of its own, which Cargo loads, and for a `git` dependency in
    /// `[workspace.dependencies]` that also writes a `path`, which Cargo loads until a member
    /// inherits it.
    pub(crate) fn parse(
        text: &str,
        directory: &Path,
        workspace: Option<&WorkspaceRoot>,
    ) -> Result<Self, TextError> {
        Self::parse_with(text, directory, workspace, Unfollowed::Refuse)
    }

    /// Reads a manifest's text as [`parse`](Self::parse) does, handling a shape Cargo loads and
    /// this reader does not follow as `unfollowed` says: [`Unfollowed::ReadPast`] reads it, for a
    /// read that compares no source of what it finds. [`parse`](Self::parse) is this with
    /// [`Unfollowed::Refuse`]. Every other refusal is as [`parse`](Self::parse) gives it.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse), except that [`Unfollowed::ReadPast`] never gives Unsupported for
    /// a `git` URL's query.
    pub(crate) fn parse_with(
        text: &str,
        directory: &Path,
        workspace: Option<&WorkspaceRoot>,
        unfollowed: Unfollowed,
    ) -> Result<Self, TextError> {
        let raw: RawManifest = toml::from_str(text).map_err(|error| {
            TextError::unreadable(format!("Cargo.toml is not a manifest: {error}"))
        })?;
        if raw.package.is_none() && raw.workspace.is_none() {
            return Err(TextError::unreadable(
                "Cargo.toml has neither a [package] nor a [workspace]",
            ));
        }

        let own_workspace = raw
            .workspace
            .as_ref()
            .map(|section| workspace_root(section, directory, unfollowed))
            .transpose()?;
        let inherited = own_workspace.as_ref().or(workspace);

        let package = raw
            .package
            .as_ref()
            .map(|section| package_section(section, inherited))
            .transpose()?;
        let declarations = declarations_of(&raw, directory, inherited, unfollowed)?;
        let edition = raw
            .package
            .as_ref()
            .map(|section| package_edition(section.edition.as_ref(), inherited))
            .transpose()?
            .unwrap_or(Edition::Edition2015);
        let targets = targets_of(&raw, edition);

        Ok(Self {
            package,
            targets,
            declarations,
            workspace: own_workspace,
            has_patch: raw.patch.is_some(),
            has_replace: raw.replace.is_some(),
        })
    }

    /// Whether the manifest writes `[patch]`.
    #[must_use]
    pub(crate) const fn writes_patch(&self) -> bool {
        self.has_patch
    }

    /// Whether the manifest writes `[replace]`.
    #[must_use]
    pub(crate) const fn writes_replace(&self) -> bool {
        self.has_replace
    }

    /// The manifest's `[package]`, or `None` for a virtual workspace root.
    #[must_use]
    pub(crate) const fn package(&self) -> Option<&PackageSection> {
        self.package.as_ref()
    }

    /// The targets the manifest writes.
    #[must_use]
    pub(crate) const fn targets(&self) -> &Targets {
        &self.targets
    }

    /// Every dependency the manifest declares: `dependencies`, `dev-dependencies` and
    /// `build-dependencies`, then those of each `[target.<cfg>]` in the order of the `cfg` text.
    #[must_use]
    pub(crate) fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    /// What the manifest says about its workspace, when it has a `[workspace]` table.
    #[must_use]
    pub(crate) const fn workspace(&self) -> Option<&WorkspaceRoot> {
        self.workspace.as_ref()
    }
}

/// The `[workspace]` table of the manifest in `directory`.
fn workspace_root(
    section: &RawWorkspace,
    directory: &Path,
    unfollowed: Unfollowed,
) -> Result<WorkspaceRoot, TextError> {
    let dependencies =
        workspace_declarations(section.dependencies.as_ref(), directory, unfollowed)?;

    let package_version = section
        .package
        .as_ref()
        .and_then(|package| package.version.as_deref())
        .map(|version| checked_version(version, "workspace.package.version"))
        .transpose()?;

    let package_edition = section
        .package
        .as_ref()
        .and_then(|package| package.edition.as_deref())
        .map(Edition::named);

    Ok(WorkspaceRoot {
        members: section.members.clone(),
        exclude: section.exclude.clone(),
        dependencies,
        package_version,
        package_edition,
    })
}

/// The `[package]` of a manifest, with its version settled.
fn package_section(
    raw: &RawPackage,
    workspace: Option<&WorkspaceRoot>,
) -> Result<PackageSection, TextError> {
    let metadata = match &raw.metadata {
        Some(value) => serde_json::to_value(value).map_err(|error| {
            TextError::unreadable(format!("package.metadata cannot be read as JSON: {error}"))
        })?,
        None => serde_json::Value::Null,
    };
    Ok(PackageSection {
        name: raw.name.clone(),
        version: package_version(raw.version.as_ref(), workspace)?,
        has_workspace_key: raw.workspace.is_some(),
        metadata,
    })
}

/// A package's version: the one written, the workspace's for `version.workspace = true`, or
/// `0.0.0`.
fn package_version(
    written: Option<&toml::Value>,
    workspace: Option<&WorkspaceRoot>,
) -> Result<String, TextError> {
    match written {
        None => Ok(DEFAULT_VERSION.to_owned()),
        Some(toml::Value::String(version)) => checked_version(version, "package.version"),
        Some(toml::Value::Table(table)) => match table.get("workspace") {
            Some(toml::Value::Boolean(true)) => workspace
                .and_then(|root| root.package_version.clone())
                .ok_or_else(|| {
                    TextError::unreadable(
                        "package.version is `workspace = true`, but no workspace gives a \
                         [workspace.package] version",
                    )
                }),
            Some(
                toml::Value::String(_)
                | toml::Value::Integer(_)
                | toml::Value::Float(_)
                | toml::Value::Boolean(false)
                | toml::Value::Datetime(_)
                | toml::Value::Array(_)
                | toml::Value::Table(_),
            )
            | None => Err(TextError::unreadable(
                "package.version is a table that is not `workspace = true`",
            )),
        },
        Some(
            toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
            | toml::Value::Array(_),
        ) => Err(TextError::unreadable(
            "package.version is neither a string nor `workspace = true`",
        )),
    }
}

/// A package's edition: the one written, the workspace's for `edition.workspace = true`, or 2015,
/// which is what Cargo gives a package that writes none.
fn package_edition(
    written: Option<&toml::Value>,
    workspace: Option<&WorkspaceRoot>,
) -> Result<Edition, TextError> {
    match written {
        None => Ok(Edition::Edition2015),
        Some(toml::Value::String(edition)) => Ok(Edition::named(edition)),
        Some(toml::Value::Table(table)) => match table.get("workspace") {
            Some(toml::Value::Boolean(true)) => workspace
                .and_then(|root| root.package_edition)
                .ok_or_else(|| {
                    TextError::unreadable(
                        "package.edition is `workspace = true`, but no workspace gives a \
                         [workspace.package] edition",
                    )
                }),
            Some(
                toml::Value::String(_)
                | toml::Value::Integer(_)
                | toml::Value::Float(_)
                | toml::Value::Boolean(false)
                | toml::Value::Datetime(_)
                | toml::Value::Array(_)
                | toml::Value::Table(_),
            )
            | None => Err(TextError::unreadable(
                "package.edition is a table that is not `workspace = true`",
            )),
        },
        Some(
            toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
            | toml::Value::Array(_),
        ) => Err(TextError::unreadable(
            "package.edition is neither a string nor `workspace = true`",
        )),
    }
}

/// `version`, when it is a semantic version as Cargo reads one.
fn checked_version(version: &str, key: &str) -> Result<String, TextError> {
    Version::parse(version)
        .map(|_| version.to_owned())
        .map_err(|error| {
            TextError::unreadable(format!("{key} `{version}` is not a version: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use proptest::prelude::*;
    use serde_json::json;

    use super::super::text_error::TextErrorKind;
    use super::{Declaration, DeclaredSource, Edition, PackageManifest, WorkspaceRoot};

    /// Reads `text` as the manifest in `/w/crates/a`, with no workspace above it.
    fn parsed(text: &str) -> PackageManifest {
        PackageManifest::parse(text, Path::new("/w/crates/a"), None)
            .expect("the manifest in this test is well formed")
    }

    /// A registry declaration of crates.io, as a test's expected value.
    fn registry(key: &str, requirement: &str, is_dev: bool) -> Declaration {
        Declaration {
            key: key.to_owned(),
            package: key.to_owned(),
            rename: None,
            is_dev,
            is_optional: false,
            source: DeclaredSource::Registry { name: None },
            version_requirement: Some(requirement.to_owned()),
        }
    }

    #[test]
    fn the_name_and_version_of_a_package_are_read() {
        let manifest = parsed("[package]\nname = \"a\"\nversion = \"1.2.3\"\n");

        let package = manifest.package().expect("a package");
        assert_eq!(package.name, "a");
        assert_eq!(package.version, "1.2.3");
        assert!(!package.has_workspace_key);
        assert_eq!(package.metadata, serde_json::Value::Null);
        assert!(manifest.workspace().is_none());
    }

    #[test]
    fn a_package_with_no_version_is_zero_zero_zero() {
        let manifest = parsed("[package]\nname = \"a\"\n");

        assert_eq!(manifest.package().expect("a package").version, "0.0.0");
    }

    #[test]
    fn a_version_that_is_not_semver_is_unreadable() {
        let error = PackageManifest::parse(
            "[package]\nname = \"a\"\nversion = \"one\"\n",
            Path::new("/w"),
            None,
        )
        .expect_err("`one` is no version");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
        assert!(
            error
                .detail()
                .starts_with("package.version `one` is not a version")
        );
    }

    #[test]
    fn an_inherited_version_is_taken_from_the_workspace_package() {
        let root = parsed("[workspace]\n[workspace.package]\nversion = \"4.5.6\"\n");
        let root = root.workspace().expect("a workspace");

        let member = PackageManifest::parse(
            "[package]\nname = \"m\"\nversion.workspace = true\n",
            Path::new("/w/crates/m"),
            Some(root),
        )
        .expect("a member that inherits its version");

        assert_eq!(member.package().expect("a package").version, "4.5.6");
    }

    #[test]
    fn an_inherited_version_with_no_workspace_is_unreadable() {
        let error = PackageManifest::parse(
            "[package]\nname = \"m\"\nversion.workspace = true\n",
            Path::new("/w/crates/m"),
            None,
        )
        .expect_err("no workspace gives a version");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
    }

    #[test]
    fn a_root_package_inherits_its_version_from_its_own_workspace() {
        let manifest = PackageManifest::parse(
            "[package]\nname = \"r\"\nversion.workspace = true\n\
             [workspace.package]\nversion = \"7.0.0\"\n",
            Path::new("/w"),
            None,
        )
        .expect("a root that inherits from itself");

        assert_eq!(manifest.package().expect("a package").version, "7.0.0");
    }

    #[test]
    fn a_package_workspace_key_is_noticed() {
        let manifest = parsed("[package]\nname = \"a\"\nworkspace = \"../..\"\n");

        assert!(manifest.package().expect("a package").has_workspace_key);
    }

    #[test]
    fn a_manifest_with_neither_package_nor_workspace_is_unreadable() {
        let error = PackageManifest::parse("[dependencies]\n", Path::new("/w"), None)
            .expect_err("nothing to read");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
    }

    #[test]
    fn text_that_is_not_toml_is_unreadable() {
        let error =
            PackageManifest::parse("[package", Path::new("/w"), None).expect_err("not TOML");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
    }

    #[test]
    fn package_metadata_is_reported_as_json() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[package.metadata.skeletons.ci]
options = { name = "x", count = 3, ratio = 0.5, on = true, list = ["a", "b"] }
"#,
        );

        assert_eq!(
            manifest.package().expect("a package").metadata,
            json!({"skeletons": {"ci": {"options": {
                "name": "x", "count": 3, "ratio": 0.5, "on": true, "list": ["a", "b"]
            }}}})
        );
    }

    #[test]
    fn a_datetime_in_package_metadata_is_reported_as_cargo_reports_it() {
        // Cargo reports a TOML datetime as the object `{"$__toml_private_datetime": "<text>"}`,
        // which `schema.rs` reads back as a datetime.
        let manifest = parsed(
            r#"
[package]
name = "a"

[package.metadata]
released = 1979-05-27
at = 1979-05-27T07:32:00Z
"#,
        );

        assert_eq!(
            manifest.package().expect("a package").metadata,
            json!({
                "released": {"$__toml_private_datetime": "1979-05-27"},
                "at": {"$__toml_private_datetime": "1979-05-27T07:32:00Z"},
            })
        );
    }

    #[test]
    fn a_root_reports_its_members_exclusions_dependencies_and_package() {
        let manifest = PackageManifest::parse(
            r#"
[workspace]
members = ["crates/*", "tools/x"]
exclude = ["crates/old"]

[workspace.package]
version = "1.0.0"

[workspace.dependencies]
serde = "1"
"#,
            Path::new("/w"),
            None,
        )
        .expect("a virtual root");

        assert!(manifest.package().is_none());
        assert_eq!(
            manifest.workspace(),
            Some(&WorkspaceRoot {
                members: vec!["crates/*".to_owned(), "tools/x".to_owned()],
                exclude: vec!["crates/old".to_owned()],
                dependencies: [("serde".to_owned(), registry("serde", "1", false))].into(),
                package_version: Some("1.0.0".to_owned()),
                package_edition: None,
            })
        );
    }

    #[test]
    fn a_package_with_no_edition_is_in_2015_and_a_written_edition_is_read() {
        let none = parsed("[package]\nname = \"a\"\n");
        let old = parsed("[package]\nname = \"a\"\nedition = \"2015\"\n");
        let newer = parsed("[package]\nname = \"a\"\nedition = \"2021\"\n");

        assert_eq!(none.targets().edition(), Edition::Edition2015);
        assert_eq!(old.targets().edition(), Edition::Edition2015);
        assert_eq!(newer.targets().edition(), Edition::Edition2018OrLater);
    }

    #[test]
    fn an_inherited_edition_is_taken_from_the_workspace_package() {
        let root = parsed("[workspace]\n[workspace.package]\nedition = \"2021\"\n");
        let root = root.workspace().expect("a workspace");

        let member = PackageManifest::parse(
            "[package]\nname = \"m\"\nedition.workspace = true\n",
            Path::new("/w/crates/m"),
            Some(root),
        )
        .expect("a member that inherits its edition");

        assert_eq!(member.targets().edition(), Edition::Edition2018OrLater);
    }

    #[test]
    fn a_root_package_inherits_its_edition_from_its_own_workspace() {
        let manifest = PackageManifest::parse(
            "[package]\nname = \"r\"\nedition.workspace = true\n\
             [workspace.package]\nedition = \"2015\"\n",
            Path::new("/w"),
            None,
        )
        .expect("a root that inherits from itself");

        assert_eq!(manifest.targets().edition(), Edition::Edition2015);
    }

    #[test]
    fn an_inherited_edition_that_no_workspace_gives_is_unreadable() {
        let manifest = "[package]\nname = \"m\"\nedition.workspace = true\n";
        let root = parsed("[workspace]\n[workspace.package]\nversion = \"1.0.0\"\n");

        for workspace in [None, root.workspace()] {
            let error = PackageManifest::parse(manifest, Path::new("/w/crates/m"), workspace)
                .expect_err("no workspace gives an edition");

            assert_eq!(error.kind(), TextErrorKind::Unreadable);
        }
    }

    #[test]
    fn an_edition_that_is_not_a_string_or_workspace_true_is_unreadable() {
        for edition in [
            "edition = 2021",
            "edition = { workspace = false }",
            "edition = []",
        ] {
            let error = PackageManifest::parse(
                &format!("[package]\nname = \"a\"\n{edition}\n"),
                Path::new("/w"),
                None,
            )
            .expect_err(edition);

            assert_eq!(error.kind(), TextErrorKind::Unreadable, "{edition}");
        }
    }

    #[test]
    fn a_target_table_that_is_not_a_table_is_unreadable() {
        // Cargo refuses `bin = [1]` and `name = 3`, so a target table is read as a table whose
        // `name` and `path` are strings.
        for text in [
            "bin = [1]\n[package]\nname = \"a\"\n",
            "test = 3\n[package]\nname = \"a\"\n",
            "[package]\nname = \"a\"\n[[bin]]\nname = 3\n",
            "[package]\nname = \"a\"\n[[example]]\nname = \"x\"\npath = 3\n",
        ] {
            let error = PackageManifest::parse(text, Path::new("/w"), None).expect_err(text);

            assert_eq!(error.kind(), TextErrorKind::Unreadable, "{text}");
        }
    }

    #[test]
    fn a_manifest_notices_patch_and_replace() {
        // A package that is its own root writes them without a `[workspace]`, so they are read
        // from the manifest, not from the workspace table.
        let patched = PackageManifest::parse(
            "[package]\nname = \"a\"\n[patch.crates-io]\nfoo = { path = \"foo\" }\n",
            Path::new("/w"),
            None,
        )
        .expect("a package");
        let replaced = PackageManifest::parse(
            "[workspace]\n[replace]\n\"foo:1.0.0\" = { path = \"foo\" }\n",
            Path::new("/w"),
            None,
        )
        .expect("a root");
        let plain = PackageManifest::parse("[workspace]\n", Path::new("/w"), None).expect("a root");

        assert!(patched.writes_patch());
        assert!(!patched.writes_replace());
        assert!(replaced.writes_replace());
        assert!(!replaced.writes_patch());
        assert!(!plain.writes_patch());
        assert!(!plain.writes_replace());
    }

    #[test]
    fn a_workspace_with_members_and_nothing_else_has_empty_lists() {
        let manifest =
            PackageManifest::parse("[workspace]\n", Path::new("/w"), None).expect("a root");

        let root = manifest.workspace().expect("a workspace");
        assert!(root.members.is_empty());
        assert!(root.exclude.is_empty());
        assert!(root.dependencies.is_empty());
        assert_eq!(root.package_version, None);
        assert_eq!(root.package_edition, None);
    }

    /// Text standing for a part of a rendered TOML value: written as it is, or still a value to
    /// write. The renderer works through a stack of these, so a deep value takes no call depth.
    enum Piece<'value> {
        Text(String),
        Value(&'value toml::Value),
    }

    /// `value` as TOML text that stands for it after `key =`: tables as inline tables, with every
    /// key quoted.
    fn rendered(value: &toml::Value) -> String {
        let mut text = String::new();
        let mut stack = vec![Piece::Value(value)];
        while let Some(piece) = stack.pop() {
            match piece {
                Piece::Text(literal) => text.push_str(&literal),
                Piece::Value(toml::Value::String(string)) => {
                    stack.push(Piece::Text(format!("{string:?}")));
                }
                Piece::Value(toml::Value::Integer(integer)) => text.push_str(&integer.to_string()),
                Piece::Value(toml::Value::Float(float)) => {
                    stack.push(Piece::Text(format!("{float:?}")));
                }
                Piece::Value(toml::Value::Boolean(flag)) => text.push_str(&flag.to_string()),
                Piece::Value(toml::Value::Datetime(datetime)) => {
                    text.push_str(&datetime.to_string());
                }
                Piece::Value(toml::Value::Array(items)) => {
                    stack.push(Piece::Text("]".to_owned()));
                    for (position, item) in items.iter().enumerate().rev() {
                        stack.push(Piece::Value(item));
                        if position > 0 {
                            stack.push(Piece::Text(", ".to_owned()));
                        }
                    }
                    stack.push(Piece::Text("[".to_owned()));
                }
                Piece::Value(toml::Value::Table(table)) => {
                    stack.push(Piece::Text(" }".to_owned()));
                    let entries: Vec<_> = table.iter().collect();
                    for (position, (key, item)) in entries.into_iter().enumerate().rev() {
                        stack.push(Piece::Value(item));
                        stack.push(Piece::Text(format!("{key:?} = ")));
                        if position > 0 {
                            stack.push(Piece::Text(", ".to_owned()));
                        }
                    }
                    stack.push(Piece::Text("{ ".to_owned()));
                }
            }
        }
        text
    }

    /// Any TOML value, a few levels deep, from the keys and leaves a manifest's tables meet.
    fn arbitrary_value() -> impl Strategy<Value = toml::Value> {
        let key = proptest::sample::select(vec![
            "name",
            "version",
            "workspace",
            "path",
            "git",
            "branch",
            "tag",
            "rev",
            "package",
            "optional",
            "registry",
            "members",
            "exclude",
            "dependencies",
            "metadata",
            "x",
        ]);
        let leaf = prop_oneof![
            "[a-zA-Z0-9 ._/:+<>=,^~*-]{0,12}".prop_map(toml::Value::String),
            (-1_000_000_i64..1_000_000).prop_map(toml::Value::Integer),
            (-1_000_000.0_f64..1_000_000.0).prop_map(toml::Value::Float),
            proptest::bool::ANY.prop_map(toml::Value::Boolean),
            proptest::sample::select(vec!["1979-05-27", "1979-05-27T07:32:00Z", "07:32:00"])
                .prop_map(|text| toml::Value::Datetime(
                    text.parse().expect("each of these is a TOML date or time")
                )),
        ];
        leaf.prop_recursive(3, 24, 4, move |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..4).prop_map(toml::Value::Array),
                proptest::collection::vec((key.clone(), inner), 0..4).prop_map(|entries| {
                    toml::Value::Table(
                        entries
                            .into_iter()
                            .map(|(key, value)| (key.to_owned(), value))
                            .collect(),
                    )
                }),
            ]
        })
    }

    proptest! {
        /// Places arbitrary TOML values at the manifest's known keys (`package`, `dependencies`,
        /// `dev-dependencies`, `target.<cfg>.dependencies`, `lib` and `workspace`), most of them
        /// the wrong shape for their key, and reads the text with and without a workspace above.
        /// Reading must not panic, and must give a manifest that has a `[package]` or a
        /// `[workspace]`, or a text error that says what is wrong.
        #[test]
        fn arbitrary_values_at_the_known_keys_give_a_manifest_or_a_text_error(
            package in proptest::option::of(arbitrary_value()),
            dependencies in proptest::option::of(arbitrary_value()),
            dev_dependencies in proptest::option::of(arbitrary_value()),
            target_dependencies in proptest::option::of(arbitrary_value()),
            lib in proptest::option::of(arbitrary_value()),
            workspace in proptest::option::of(arbitrary_value()),
        ) {
            let mut lines = Vec::new();
            for (key, value) in [
                ("package", &package),
                ("dependencies", &dependencies),
                ("dev-dependencies", &dev_dependencies),
                ("lib", &lib),
                ("workspace", &workspace),
            ] {
                if let Some(value) = value {
                    lines.push(format!("{key} = {}", rendered(value)));
                }
            }
            if let Some(value) = &target_dependencies {
                lines.push(format!(
                    "target = {{ \"cfg(unix)\" = {{ dependencies = {} }} }}",
                    rendered(value)
                ));
            }
            let text = lines.join("\n");
            let root = WorkspaceRoot {
                members: Vec::new(),
                exclude: Vec::new(),
                dependencies: BTreeMap::new(),
                package_version: Some("1.0.0".to_owned()),
                package_edition: None,
            };

            for workspace in [None, Some(&root)] {
                match PackageManifest::parse(&text, Path::new("/w/crates/a"), workspace) {
                    Ok(manifest) => {
                        prop_assert!(
                            manifest.package().is_some() || manifest.workspace().is_some()
                        );
                    }
                    Err(error) => prop_assert!(!error.detail().is_empty()),
                }
            }
        }
    }
}
