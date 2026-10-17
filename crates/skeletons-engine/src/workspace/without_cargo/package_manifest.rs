//! A `Cargo.toml`, read as text: the package it names, its targets, every dependency it declares,
//! and, for a workspace root, what the workspace holds.
//!
//! Only what a reader of a workspace without Cargo needs is kept. A manifest that Cargo would
//! refuse is refused here too, and a manifest that uses something this reader does not follow is
//! refused as unsupported, never read differently.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use semver::Version;
use serde::Deserialize;

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

/// A kind of target a package can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TargetKind {
    Library,
    Binary,
    Example,
    Test,
    Benchmark,
}

/// The `[lib]` table of a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibTarget {
    /// `lib.name`, when written.
    pub(crate) name: Option<String>,
    /// `lib.path`, when written, as written.
    pub(crate) path: Option<String>,
}

/// The targets a manifest writes, and which kinds it leaves Cargo to discover from the files
/// beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Targets {
    /// The `[lib]` table, when there is one.
    pub(crate) library: Option<LibTarget>,
    declared: BTreeSet<TargetKind>,
    undiscovered: BTreeSet<TargetKind>,
}

impl Targets {
    /// Whether the manifest writes a table for a target of `kind`: `[lib]`, or at least one
    /// `[[bin]]`, `[[example]]`, `[[test]]` or `[[bench]]`.
    #[must_use]
    pub(crate) fn has_declared(&self, kind: TargetKind) -> bool {
        match kind {
            TargetKind::Library => self.library.is_some(),
            TargetKind::Binary | TargetKind::Example | TargetKind::Test | TargetKind::Benchmark => {
                self.declared.contains(&kind)
            }
        }
    }

    /// Whether Cargo discovers targets of `kind` from the files, which every kind does unless its
    /// `autolib`, `autobins`, `autoexamples`, `autotests` or `autobenches` key is `false`.
    #[must_use]
    pub(crate) fn discovers(&self, kind: TargetKind) -> bool {
        !self.undiscovered.contains(&kind)
    }
}

/// One dependency a manifest declares, in any of its dependency tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    /// The key it is written under.
    pub(crate) key: String,
    /// The package it names: `package = "…"` when written, the key otherwise.
    pub(crate) package: String,
    /// `Some(key)` whenever `package =` is written, as `cargo metadata` reports a rename.
    pub(crate) rename: Option<String>,
    /// Whether it is in a `dev-dependencies` table.
    pub(crate) is_dev: bool,
    /// Whether it is written `optional = true`.
    pub(crate) is_optional: bool,
    /// Where the package comes from.
    pub(crate) source: DeclaredSource,
    /// The `version` requirement as written, when there is one.
    pub(crate) version_requirement: Option<String>,
}

/// Where a declaration says its package comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclaredSource {
    /// A `path` dependency. The directory is the path joined onto the directory of the manifest
    /// that wrote it, or of the workspace root for a dependency inherited from it, and is not
    /// normalized.
    Path { directory: PathBuf },
    /// A `git` dependency, with the one reference it names.
    Git {
        url: String,
        reference: GitReference,
    },
    /// A registry dependency, from the named registry or from crates.io when there is none.
    Registry { name: Option<String> },
}

/// The one reference a git dependency names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitReference {
    /// None of `branch`, `tag` and `rev`: the remote's default branch.
    DefaultBranch,
    Branch(String),
    Tag(String),
    Rev(String),
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
    /// Whether the manifest writes `[patch]`.
    pub(crate) has_patch: bool,
    /// Whether the manifest writes `[replace]`.
    pub(crate) has_replace: bool,
}

#[derive(Deserialize)]
struct RawManifest {
    package: Option<RawPackage>,
    lib: Option<RawLib>,
    bin: Option<Vec<toml::Value>>,
    example: Option<Vec<toml::Value>>,
    test: Option<Vec<toml::Value>>,
    bench: Option<Vec<toml::Value>>,
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
}

/// Where a table of dependencies sits, which every declaration in it needs to be read.
struct Site<'site> {
    /// The table as a reader of the manifest names it, such as `[dependencies]`.
    table: &'site str,
    is_dev: bool,
    /// The directory of the manifest the table is in.
    directory: &'site Path,
    /// The workspace a `workspace = true` declaration takes its value from.
    workspace: Option<&'site WorkspaceRoot>,
}

/// One declaration's table, with the key it is written under, read by key name.
struct Written<'written> {
    table: &'written toml::Table,
    key: &'written str,
    site: &'written Site<'written>,
}

impl PackageManifest {
    /// Reads a manifest's text.
    ///
    /// `directory` is where the manifest sits, which a `path` dependency is relative to.
    /// `workspace` is the root of the workspace the manifest is a member of, which a
    /// `version.workspace = true` and a `workspace = true` dependency take their values from. A
    /// manifest with a `[workspace]` of its own is its own workspace root and ignores it.
    ///
    /// # Errors
    ///
    /// Unreadable for text that is not a manifest Cargo would read, and for an inherited value
    /// that no workspace gives. Unsupported for a `git` dependency that also writes a `path`.
    pub(crate) fn parse(
        text: &str,
        directory: &Path,
        workspace: Option<&WorkspaceRoot>,
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
            .map(|section| {
                workspace_root(
                    section,
                    directory,
                    raw.patch.is_some(),
                    raw.replace.is_some(),
                )
            })
            .transpose()?;
        let inherited = own_workspace.as_ref().or(workspace);

        let package = raw
            .package
            .as_ref()
            .map(|section| package_section(section, inherited))
            .transpose()?;
        let declarations = declarations_of(&raw, directory, inherited)?;
        let targets = targets_of(&raw);

        Ok(Self {
            package,
            targets,
            declarations,
            workspace: own_workspace,
        })
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
    has_patch: bool,
    has_replace: bool,
) -> Result<WorkspaceRoot, TextError> {
    let site = Site {
        table: "[workspace.dependencies]",
        is_dev: false,
        directory,
        workspace: None,
    };
    let mut dependencies = BTreeMap::new();
    for (key, value) in section.dependencies.iter().flatten() {
        dependencies.insert(key.clone(), declaration(key, value, &site)?);
    }

    let package_version = section
        .package
        .as_ref()
        .and_then(|package| package.version.as_deref())
        .map(|version| checked_version(version, "workspace.package.version"))
        .transpose()?;

    Ok(WorkspaceRoot {
        members: section.members.clone(),
        exclude: section.exclude.clone(),
        dependencies,
        package_version,
        has_patch,
        has_replace,
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

/// `version`, when it is a semantic version as Cargo reads one.
fn checked_version(version: &str, key: &str) -> Result<String, TextError> {
    Version::parse(version)
        .map(|_| version.to_owned())
        .map_err(|error| {
            TextError::unreadable(format!("{key} `{version}` is not a version: {error}"))
        })
}

/// The targets a manifest writes.
fn targets_of(raw: &RawManifest) -> Targets {
    let written =
        |tables: &Option<Vec<toml::Value>>| tables.as_ref().is_some_and(|t| !t.is_empty());
    let declared = [
        (TargetKind::Binary, written(&raw.bin)),
        (TargetKind::Example, written(&raw.example)),
        (TargetKind::Test, written(&raw.test)),
        (TargetKind::Benchmark, written(&raw.bench)),
    ]
    .into_iter()
    .filter_map(|(kind, present)| present.then_some(kind))
    .collect();

    let undiscovered = raw
        .package
        .as_ref()
        .map(|package| {
            [
                (TargetKind::Library, package.autolib),
                (TargetKind::Binary, package.autobins),
                (TargetKind::Example, package.autoexamples),
                (TargetKind::Test, package.autotests),
                (TargetKind::Benchmark, package.autobenches),
            ]
            .into_iter()
            .filter_map(|(kind, flag)| (flag == Some(false)).then_some(kind))
            .collect()
        })
        .unwrap_or_default();

    Targets {
        library: raw.lib.as_ref().map(|lib| LibTarget {
            name: lib.name.clone(),
            path: lib.path.clone(),
        }),
        declared,
        undiscovered,
    }
}

/// Every declaration of a manifest: its own tables, then those of each `[target.<cfg>]`.
fn declarations_of(
    raw: &RawManifest,
    directory: &Path,
    workspace: Option<&WorkspaceRoot>,
) -> Result<Vec<Declaration>, TextError> {
    let mut declarations = Vec::new();
    collect_declarations(&raw.tables, "", directory, workspace, &mut declarations)?;
    for (cfg, tables) in raw.target.iter().flatten() {
        let prefix = format!("target.{cfg}.");
        collect_declarations(tables, &prefix, directory, workspace, &mut declarations)?;
    }
    Ok(declarations)
}

/// Appends the declarations of `tables` to `declarations`. `prefix` is what a reader names the
/// tables by in front of `dependencies`, such as `target.cfg(unix).`.
fn collect_declarations(
    tables: &RawDependencyTables,
    prefix: &str,
    directory: &Path,
    workspace: Option<&WorkspaceRoot>,
    declarations: &mut Vec<Declaration>,
) -> Result<(), TextError> {
    let kinds = [
        ("dependencies", false, tables.dependencies.as_ref()),
        (
            "dev-dependencies",
            true,
            tables
                .dev_hyphenated
                .as_ref()
                .or(tables.dev_underscored.as_ref()),
        ),
        (
            "build-dependencies",
            false,
            tables
                .build_hyphenated
                .as_ref()
                .or(tables.build_underscored.as_ref()),
        ),
    ];
    for (name, is_dev, table) in kinds {
        let label = format!("[{prefix}{name}]");
        let site = Site {
            table: &label,
            is_dev,
            directory,
            workspace,
        };
        for (key, value) in table.into_iter().flatten() {
            declarations.push(declaration(key, value, &site)?);
        }
    }
    Ok(())
}

/// One entry of a dependency table.
fn declaration(key: &str, value: &toml::Value, site: &Site<'_>) -> Result<Declaration, TextError> {
    match value {
        toml::Value::String(requirement) => Ok(Declaration {
            key: key.to_owned(),
            package: key.to_owned(),
            rename: None,
            is_dev: site.is_dev,
            is_optional: false,
            source: DeclaredSource::Registry { name: None },
            version_requirement: Some(requirement.clone()),
        }),
        toml::Value::Table(table) => detailed_declaration(&Written { table, key, site }),
        toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_)
        | toml::Value::Array(_) => Err(TextError::unreadable(format!(
            "{} `{key}` is neither a version nor a table",
            site.table
        ))),
    }
}

/// An entry that is a table: inherited from the workspace, or written out.
fn detailed_declaration(written: &Written<'_>) -> Result<Declaration, TextError> {
    let is_optional = written.flag("optional")?.unwrap_or(false);
    if written.table.contains_key("workspace") {
        return inherited_declaration(written, is_optional);
    }

    let package = written.string("package")?;
    Ok(Declaration {
        key: written.key.to_owned(),
        package: package.clone().unwrap_or_else(|| written.key.to_owned()),
        rename: package.map(|_| written.key.to_owned()),
        is_dev: written.site.is_dev,
        is_optional,
        source: written.source()?,
        version_requirement: written.string("version")?,
    })
}

/// An entry written `workspace = true`: the workspace's declaration of the same key, in this
/// table's kind and with this entry's `optional`.
fn inherited_declaration(
    written: &Written<'_>,
    is_optional: bool,
) -> Result<Declaration, TextError> {
    let (site, key) = (written.site, written.key);
    if written.table.get("workspace") != Some(&toml::Value::Boolean(true)) {
        return Err(TextError::unreadable(format!(
            "{} `{key}`: `workspace` must be `true`",
            site.table
        )));
    }
    let inherited = site
        .workspace
        .and_then(|root| root.dependencies.get(key))
        .ok_or_else(|| {
            TextError::unreadable(format!(
                "{} `{key}` is `workspace = true`, but no [workspace.dependencies] declares `{key}`",
                site.table
            ))
        })?;
    Ok(Declaration {
        is_dev: site.is_dev,
        is_optional,
        ..inherited.clone()
    })
}

impl Written<'_> {
    /// The string written under `name`, when one is.
    fn string(&self, name: &str) -> Result<Option<String>, TextError> {
        match self.table.get(name) {
            None => Ok(None),
            Some(toml::Value::String(text)) => Ok(Some(text.clone())),
            Some(
                toml::Value::Integer(_)
                | toml::Value::Float(_)
                | toml::Value::Boolean(_)
                | toml::Value::Datetime(_)
                | toml::Value::Array(_)
                | toml::Value::Table(_),
            ) => Err(self.not_a(name, "a string")),
        }
    }

    /// The boolean written under `name`, when one is.
    fn flag(&self, name: &str) -> Result<Option<bool>, TextError> {
        match self.table.get(name) {
            None => Ok(None),
            Some(toml::Value::Boolean(flag)) => Ok(Some(*flag)),
            Some(
                toml::Value::String(_)
                | toml::Value::Integer(_)
                | toml::Value::Float(_)
                | toml::Value::Datetime(_)
                | toml::Value::Array(_)
                | toml::Value::Table(_),
            ) => Err(self.not_a(name, "a boolean")),
        }
    }

    fn not_a(&self, name: &str, what: &str) -> TextError {
        TextError::unreadable(format!(
            "{} `{}`: `{name}` must be {what}",
            self.site.table, self.key
        ))
    }

    /// Where the entry says its package comes from.
    fn source(&self) -> Result<DeclaredSource, TextError> {
        match (self.string("git")?, self.string("path")?) {
            (Some(url), None) => Ok(DeclaredSource::Git {
                url,
                reference: self.git_reference()?,
            }),
            (Some(_), Some(_)) => Err(TextError::unsupported(format!(
                "{} `{}` writes both `git` and `path`, which Cargo only warns about",
                self.site.table, self.key
            ))),
            (None, Some(path)) => Ok(DeclaredSource::Path {
                directory: self.site.directory.join(path),
            }),
            (None, None) => Ok(DeclaredSource::Registry {
                name: self.string("registry")?,
            }),
        }
    }

    /// The one of `branch`, `tag` and `rev` the entry writes.
    fn git_reference(&self) -> Result<GitReference, TextError> {
        match (
            self.string("branch")?,
            self.string("tag")?,
            self.string("rev")?,
        ) {
            (None, None, None) => Ok(GitReference::DefaultBranch),
            (Some(branch), None, None) => Ok(GitReference::Branch(branch)),
            (None, Some(tag), None) => Ok(GitReference::Tag(tag)),
            (None, None, Some(rev)) => Ok(GitReference::Rev(rev)),
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                Err(TextError::unreadable(format!(
                    "{} `{}` writes more than one of `branch`, `tag` and `rev`",
                    self.site.table, self.key
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{
        Declaration, DeclaredSource, GitReference, LibTarget, PackageManifest, TargetKind,
        WorkspaceRoot,
    };

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

        assert!(error.is_unreadable());
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

        assert!(error.is_unreadable());
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

        assert!(error.is_unreadable());
    }

    #[test]
    fn text_that_is_not_toml_is_unreadable() {
        let error =
            PackageManifest::parse("[package", Path::new("/w"), None).expect_err("not TOML");

        assert!(error.is_unreadable());
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
    fn a_lib_table_reports_its_name_and_path() {
        let manifest =
            parsed("[package]\nname = \"a\"\n[lib]\nname = \"zed\"\npath = \"src/zed.rs\"\n");

        assert_eq!(
            manifest.targets().library,
            Some(LibTarget {
                name: Some("zed".to_owned()),
                path: Some("src/zed.rs".to_owned()),
            })
        );
        assert!(manifest.targets().has_declared(TargetKind::Library));
    }

    #[test]
    fn a_lib_table_with_only_a_path_has_no_name() {
        let manifest = parsed("[package]\nname = \"a\"\n[lib]\npath = \"x.rs\"\n");

        assert_eq!(
            manifest.targets().library,
            Some(LibTarget {
                name: None,
                path: Some("x.rs".to_owned()),
            })
        );
    }

    #[test]
    fn a_manifest_with_no_target_tables_declares_none_and_discovers_all() {
        let manifest = parsed("[package]\nname = \"a\"\n");

        assert_eq!(manifest.targets().library, None);
        for kind in [
            TargetKind::Library,
            TargetKind::Binary,
            TargetKind::Example,
            TargetKind::Test,
            TargetKind::Benchmark,
        ] {
            assert!(
                !manifest.targets().has_declared(kind),
                "{kind:?} is not declared"
            );
            assert!(manifest.targets().discovers(kind), "{kind:?} is discovered");
        }
    }

    #[test]
    fn each_array_of_targets_is_noticed_on_its_own() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[[bin]]
name = "tool"

[[bench]]
name = "speed"
"#,
        );

        let targets = manifest.targets();
        assert!(targets.has_declared(TargetKind::Binary));
        assert!(targets.has_declared(TargetKind::Benchmark));
        assert!(!targets.has_declared(TargetKind::Example));
        assert!(!targets.has_declared(TargetKind::Test));
        assert!(!targets.has_declared(TargetKind::Library));
    }

    #[test]
    fn an_empty_array_of_targets_declares_none() {
        let manifest = parsed("[package]\nname = \"a\"\nexample = []\n");

        assert!(!manifest.targets().has_declared(TargetKind::Example));
    }

    #[test]
    fn each_auto_key_set_to_false_turns_off_its_own_discovery() {
        let manifest = parsed(
            r#"
[package]
name = "a"
autolib = false
autobins = false
autoexamples = false
autotests = false
autobenches = false
"#,
        );

        for kind in [
            TargetKind::Library,
            TargetKind::Binary,
            TargetKind::Example,
            TargetKind::Test,
            TargetKind::Benchmark,
        ] {
            assert!(
                !manifest.targets().discovers(kind),
                "{kind:?} is not discovered"
            );
        }
    }

    #[test]
    fn an_auto_key_set_to_true_or_left_out_keeps_discovery_on() {
        let manifest = parsed("[package]\nname = \"a\"\nautobins = false\nautotests = true\n");

        let targets = manifest.targets();
        assert!(!targets.discovers(TargetKind::Binary));
        assert!(targets.discovers(TargetKind::Test));
        assert!(targets.discovers(TargetKind::Library));
        assert!(targets.discovers(TargetKind::Example));
        assert!(targets.discovers(TargetKind::Benchmark));
    }

    #[test]
    fn every_dependency_table_and_its_underscore_spelling_is_read() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[dependencies]
normal = "1"

[dev-dependencies]
hyphen_dev = "2"

[build-dependencies]
hyphen_build = "3"

[dev_dependencies]
underscore_dev = "4"

[build_dependencies]
underscore_build = "5"
"#,
        );

        // The hyphenated spelling wins when both are written, as it does for Cargo, so the
        // underscore dev and build tables here are the ones Cargo ignores.
        assert_eq!(
            manifest.declarations(),
            &[
                registry("normal", "1", false),
                registry("hyphen_dev", "2", true),
                registry("hyphen_build", "3", false),
            ]
        );
    }

    #[test]
    fn the_underscore_spellings_are_read_when_they_are_the_only_ones() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[dev_dependencies]
tool = "2"

[build_dependencies]
generator = "3"
"#,
        );

        assert_eq!(
            manifest.declarations(),
            &[
                registry("tool", "2", true),
                registry("generator", "3", false)
            ]
        );
    }

    #[test]
    fn target_tables_are_read_under_every_cfg() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[target.'cfg(unix)'.dependencies]
unix_only = "1"

[target.'cfg(unix)'.dev-dependencies]
unix_dev = "2"

[target.x86_64-pc-windows-msvc.build-dependencies]
triple_build = "3"

[target.'cfg(target_os = "macos")'.dev_dependencies]
mac_dev = "4"
"#,
        );

        // Targets come in the order of their `cfg` text: `cfg(target_os = "macos")`,
        // `cfg(unix)`, then the triple.
        assert_eq!(
            manifest.declarations(),
            &[
                registry("mac_dev", "4", true),
                registry("unix_only", "1", false),
                registry("unix_dev", "2", true),
                registry("triple_build", "3", false),
            ]
        );
    }

    #[test]
    fn a_package_rename_records_the_key_as_the_rename() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[dependencies]
serde_alias = { package = "serde", version = "1" }
same = { package = "same", version = "2" }
"#,
        );

        assert_eq!(
            manifest.declarations(),
            &[
                Declaration {
                    key: "same".to_owned(),
                    package: "same".to_owned(),
                    rename: Some("same".to_owned()),
                    is_dev: false,
                    is_optional: false,
                    source: DeclaredSource::Registry { name: None },
                    version_requirement: Some("2".to_owned()),
                },
                Declaration {
                    key: "serde_alias".to_owned(),
                    package: "serde".to_owned(),
                    rename: Some("serde_alias".to_owned()),
                    is_dev: false,
                    is_optional: false,
                    source: DeclaredSource::Registry { name: None },
                    version_requirement: Some("1".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn an_optional_dependency_is_marked_optional() {
        let manifest = parsed(
            "[package]\nname = \"a\"\n[dependencies]\nextra = { version = \"1\", optional = true }\n",
        );

        let declarations = manifest.declarations();
        assert_eq!(declarations.len(), 1);
        assert!(declarations[0].is_optional);
        assert!(!declarations[0].is_dev);
    }

    #[test]
    fn a_path_dependency_is_relative_to_its_manifest() {
        let manifest = parsed(
            "[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\", version = \"0.1\" }\n",
        );

        assert_eq!(
            manifest.declarations(),
            &[Declaration {
                key: "b".to_owned(),
                package: "b".to_owned(),
                rename: None,
                is_dev: false,
                is_optional: false,
                source: DeclaredSource::Path {
                    directory: "/w/crates/a/../b".into()
                },
                version_requirement: Some("0.1".to_owned()),
            }]
        );
    }

    #[test]
    fn a_git_dependency_names_at_most_one_reference() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[dependencies]
by_branch = { git = "https://example.invalid/g", branch = "feat/x" }
by_default = { git = "https://example.invalid/g" }
by_rev = { git = "https://example.invalid/g", rev = "abc123" }
by_tag = { git = "https://example.invalid/g", tag = "v1" }
"#,
        );

        let references: Vec<_> = manifest
            .declarations()
            .iter()
            .map(|declaration| match &declaration.source {
                DeclaredSource::Git { url, reference } => (url.as_str(), reference.clone()),
                DeclaredSource::Path { .. } | DeclaredSource::Registry { .. } => {
                    panic!("`{}` should be a git dependency", declaration.key)
                }
            })
            .collect();
        assert_eq!(
            references,
            [
                (
                    "https://example.invalid/g",
                    GitReference::Branch("feat/x".to_owned())
                ),
                ("https://example.invalid/g", GitReference::DefaultBranch),
                (
                    "https://example.invalid/g",
                    GitReference::Rev("abc123".to_owned())
                ),
                (
                    "https://example.invalid/g",
                    GitReference::Tag("v1".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn a_git_dependency_with_two_references_is_unreadable() {
        let error = PackageManifest::parse(
            "[package]\nname = \"a\"\n[dependencies]\n\
             g = { git = \"https://example.invalid/g\", branch = \"x\", tag = \"y\" }\n",
            Path::new("/w"),
            None,
        )
        .expect_err("a branch and a tag at once");

        assert!(error.is_unreadable());
        assert_eq!(
            error.detail(),
            "[dependencies] `g` writes more than one of `branch`, `tag` and `rev`"
        );
    }

    #[test]
    fn a_dependency_with_both_git_and_path_is_unsupported() {
        let error = PackageManifest::parse(
            "[package]\nname = \"a\"\n[dependencies]\n\
             g = { git = \"https://example.invalid/g\", path = \"../g\" }\n",
            Path::new("/w"),
            None,
        )
        .expect_err("git and path at once");

        assert!(error.is_unsupported());
    }

    #[test]
    fn a_registry_dependency_keeps_the_registry_name_it_writes() {
        let manifest = parsed(
            "[package]\nname = \"a\"\n[dependencies]\n\
             private = { version = \"1\", registry = \"corp\" }\n",
        );

        assert_eq!(
            manifest.declarations()[0].source,
            DeclaredSource::Registry {
                name: Some("corp".to_owned())
            }
        );
    }

    #[test]
    fn a_dependency_with_no_source_and_no_version_is_a_registry_dependency_with_no_requirement() {
        let manifest = parsed("[package]\nname = \"a\"\n[dependencies]\nbare = {}\n");

        assert_eq!(
            manifest.declarations()[0].source,
            DeclaredSource::Registry { name: None }
        );
        assert_eq!(manifest.declarations()[0].version_requirement, None);
    }

    #[test]
    fn a_dependency_key_of_the_wrong_type_is_unreadable() {
        for body in [
            "d = { optional = \"yes\" }",
            "d = { package = 3 }",
            "d = { version = 1 }",
            "d = { path = true }",
            "d = 7",
        ] {
            let text = format!("[package]\nname = \"a\"\n[dependencies]\n{body}\n");
            let error = PackageManifest::parse(&text, Path::new("/w"), None)
                .expect_err("a value of the wrong type");
            assert!(error.is_unreadable(), "`{body}` is unreadable");
        }
    }

    #[test]
    fn a_workspace_dependency_is_merged_with_its_path_relative_to_the_root() {
        // The root is `/w`, so the shared path dependency is `/w/crates/shared`, not relative to
        // the member at `/w/crates/a`. The member adds `optional`.
        let root = PackageManifest::parse(
            r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
shared = { path = "crates/shared", version = "0.2.0" }
serde = "1"
renamed = { package = "real", version = "3" }
"#,
            Path::new("/w"),
            None,
        )
        .expect("a root");
        let root = root.workspace().expect("a workspace");

        let member = PackageManifest::parse(
            r#"
[package]
name = "a"

[dependencies]
shared = { workspace = true, optional = true }
renamed.workspace = true

[dev-dependencies]
serde.workspace = true
"#,
            Path::new("/w/crates/a"),
            Some(root),
        )
        .expect("a member");

        assert_eq!(
            member.declarations(),
            &[
                Declaration {
                    key: "renamed".to_owned(),
                    package: "real".to_owned(),
                    rename: Some("renamed".to_owned()),
                    is_dev: false,
                    is_optional: false,
                    source: DeclaredSource::Registry { name: None },
                    version_requirement: Some("3".to_owned()),
                },
                Declaration {
                    key: "shared".to_owned(),
                    package: "shared".to_owned(),
                    rename: None,
                    is_dev: false,
                    is_optional: true,
                    source: DeclaredSource::Path {
                        directory: "/w/crates/shared".into()
                    },
                    version_requirement: Some("0.2.0".to_owned()),
                },
                registry("serde", "1", true),
            ]
        );
    }

    #[test]
    fn a_workspace_dependency_the_root_does_not_declare_is_unreadable() {
        let root = PackageManifest::parse("[workspace]\n", Path::new("/w"), None).expect("a root");

        let error = PackageManifest::parse(
            "[package]\nname = \"a\"\n[dependencies]\nmissing.workspace = true\n",
            Path::new("/w/a"),
            root.workspace(),
        )
        .expect_err("the root declares nothing");

        assert!(error.is_unreadable());
        assert_eq!(
            error.detail(),
            "[dependencies] `missing` is `workspace = true`, but no [workspace.dependencies] \
             declares `missing`"
        );
    }

    #[test]
    fn a_workspace_dependency_with_no_workspace_is_unreadable() {
        let error = PackageManifest::parse(
            "[package]\nname = \"a\"\n[dependencies]\nx.workspace = true\n",
            Path::new("/w"),
            None,
        )
        .expect_err("no workspace");

        assert!(error.is_unreadable());
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
                has_patch: false,
                has_replace: false,
            })
        );
    }

    #[test]
    fn a_root_notices_patch_and_replace() {
        let patched = PackageManifest::parse(
            "[workspace]\n[patch.crates-io]\nfoo = { path = \"foo\" }\n",
            Path::new("/w"),
            None,
        )
        .expect("a root");
        let replaced = PackageManifest::parse(
            "[workspace]\n[replace]\n\"foo:1.0.0\" = { path = \"foo\" }\n",
            Path::new("/w"),
            None,
        )
        .expect("a root");

        let patched = patched.workspace().expect("a workspace");
        assert!(patched.has_patch);
        assert!(!patched.has_replace);
        let replaced = replaced.workspace().expect("a workspace");
        assert!(replaced.has_replace);
        assert!(!replaced.has_patch);
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
    }
}
