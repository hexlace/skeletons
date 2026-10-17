//! The dependencies a manifest declares, in any of its dependency tables: each one's key, the
//! package it names, where that package comes from, and what the manifest says about it.
//!
//! A `workspace = true` entry is merged with the root's declaration of the same key, so a reader of
//! a member's declarations sees what Cargo would resolve.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::canonical_git_url::CanonicalGitUrl;
use super::super::text_error::TextError;
use super::{RawDependencies, RawDependencyTables, RawManifest, Unfollowed, WorkspaceRoot};

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
    /// A `git` dependency, with the one reference it names. The URL is held canonicalised, as
    /// Cargo compares it, because the lockfile may spell it another way.
    Git {
        url: CanonicalGitUrl,
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

/// Where a table of dependencies sits, which every declaration in it needs to be read.
struct Site<'site> {
    /// The table as a reader of the manifest names it, such as `[dependencies]`.
    table: &'site str,
    is_dev: bool,
    /// The directory of the manifest the table is in.
    directory: &'site Path,
    /// The workspace a `workspace = true` declaration takes its value from.
    workspace: Option<&'site WorkspaceRoot>,
    /// What to do with a shape Cargo loads and this reader does not follow.
    unfollowed: Unfollowed,
}

/// One declaration's table, with the key it is written under, read by key name.
struct Written<'written> {
    table: &'written toml::Table,
    key: &'written str,
    site: &'written Site<'written>,
}

/// The declarations of a `[workspace.dependencies]` table, by key, with each path joined onto
/// `directory`, the root's.
pub(super) fn workspace_declarations(
    table: Option<&RawDependencies>,
    directory: &Path,
    unfollowed: Unfollowed,
) -> Result<BTreeMap<String, Declaration>, TextError> {
    let site = Site {
        table: "[workspace.dependencies]",
        is_dev: false,
        directory,
        workspace: None,
        unfollowed,
    };
    let mut declarations = BTreeMap::new();
    for (key, value) in table.into_iter().flatten() {
        declarations.insert(key.clone(), declaration(key, value, &site)?);
    }
    Ok(declarations)
}

/// Every declaration of a manifest: its own tables, then those of each `[target.<cfg>]`.
pub(super) fn declarations_of(
    raw: &RawManifest,
    directory: &Path,
    workspace: Option<&WorkspaceRoot>,
    unfollowed: Unfollowed,
) -> Result<Vec<Declaration>, TextError> {
    let mut declarations = Vec::new();
    let reading = Reading {
        directory,
        workspace,
        unfollowed,
    };
    collect_declarations(&raw.tables, "", &reading, &mut declarations)?;
    for (cfg, tables) in raw.target.iter().flatten() {
        let prefix = format!("target.{cfg}.");
        collect_declarations(tables, &prefix, &reading, &mut declarations)?;
    }
    Ok(declarations)
}

/// What every table of one manifest shares as its dependencies are read: where the manifest is,
/// the workspace it inherits from, and what to do with a shape this reader does not follow.
struct Reading<'reading> {
    directory: &'reading Path,
    workspace: Option<&'reading WorkspaceRoot>,
    unfollowed: Unfollowed,
}

/// Appends the declarations of `tables` to `declarations`. `prefix` is what a reader names the
/// tables by in front of `dependencies`, such as `target.cfg(unix).`.
fn collect_declarations(
    tables: &RawDependencyTables,
    prefix: &str,
    reading: &Reading<'_>,
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
            directory: reading.directory,
            workspace: reading.workspace,
            unfollowed: reading.unfollowed,
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
                "{} `{key}` is `workspace = true`, but no [workspace.dependencies] declares \
                 `{key}`",
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
            (Some(text), None) => self.git_source(&text),
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

    /// The git source of the entry whose `git` is `text`, with the one reference it writes.
    ///
    /// A URL with a query of its own, an empty query included, is refused when the site refuses
    /// what it does not follow. Cargo keeps that query in the lockfile's source, writing the
    /// reference after it, and reads the source back as the same one; this reader reads a
    /// source's query only as one reference, so the two could not be compared, and
    /// `?branch=dev` written with no `branch` key would lock as a branch.
    ///
    /// When the site reads past it, the URL is kept with its query. Such a declaration is never
    /// the source of a lock entry this reader holds: the lockfile read refuses every git source
    /// whose query is not exactly one reference, so no entry has a URL with a query to agree
    /// with ([`locked_source::agrees`](super::super::locked_source::agrees)). It names no worn
    /// package, as for Cargo, where it is another source.
    fn git_source(&self, text: &str) -> Result<DeclaredSource, TextError> {
        let url = CanonicalGitUrl::parse(text).map_err(|reason| {
            TextError::unreadable(format!(
                "{} `{}`: `git = \"{text}\"` is not a URL cargo reads: {reason}",
                self.site.table, self.key
            ))
        })?;
        match (url.has_query(), self.site.unfollowed) {
            (true, Unfollowed::Refuse) => {
                return Err(TextError::unsupported(format!(
                    "{} `{}`: `git = \"{text}\"` has a query of its own, which Cargo keeps in \
                     the lockfile's source and this reader does not follow",
                    self.site.table, self.key
                )));
            }
            (true, Unfollowed::ReadPast) | (false, Unfollowed::Refuse | Unfollowed::ReadPast) => {}
        }
        Ok(DeclaredSource::Git {
            url,
            reference: self.git_reference()?,
        })
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
    use std::path::{Path, PathBuf};

    use proptest::prelude::*;

    use super::super::{PackageManifest, Unfollowed};
    use super::{CanonicalGitUrl, Declaration, DeclaredSource, GitReference};
    use crate::workspace::without_cargo::text_error::TextErrorKind;

    /// Reads `text` as the manifest in `/w/crates/a`, with no workspace above it.
    fn parsed(text: &str) -> PackageManifest {
        PackageManifest::parse(text, Path::new("/w/crates/a"), None)
            .expect("the manifest in this test is well formed")
    }

    /// The URL `text` after Cargo's canonicalisation, as a test's expected value.
    fn git_url(text: &str) -> CanonicalGitUrl {
        CanonicalGitUrl::parse(text).expect("a URL cargo reads")
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
            "[package]\nname = \"a\"\n[dependencies]\n\
             extra = { version = \"1\", optional = true }\n",
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
                DeclaredSource::Git { url, reference } => (url.clone(), reference.clone()),
                DeclaredSource::Path { .. } | DeclaredSource::Registry { .. } => {
                    panic!("`{}` should be a git dependency", declaration.key)
                }
            })
            .collect();
        let url = git_url("https://example.invalid/g");
        assert_eq!(
            references,
            [
                (url.clone(), GitReference::Branch("feat/x".to_owned())),
                (url.clone(), GitReference::DefaultBranch),
                (url.clone(), GitReference::Rev("abc123".to_owned())),
                (url, GitReference::Tag("v1".to_owned())),
            ]
        );
    }

    #[test]
    fn a_git_url_is_held_as_cargo_canonicalises_it() {
        let manifest = parsed(
            "[package]\nname = \"a\"\n[dependencies]\n\
             g = { git = \"https://GitHub.com/Foo/Bar.git/\" }\n",
        );

        let [declaration] = manifest.declarations() else {
            panic!("one declaration");
        };
        let DeclaredSource::Git { url, .. } = &declaration.source else {
            panic!("a git dependency");
        };
        assert_eq!(url.as_str(), "https://github.com/foo/bar");
    }

    #[test]
    fn a_git_url_cargo_refuses_makes_the_manifest_unreadable() {
        // An scp-style address does not parse, and a host and path with no scheme parses as a
        // URL that cannot be a base: Cargo refuses the manifest for either, and the error
        // names the table, the key and the URL as written.
        for (url, reason) in [
            ("git@github.com:foo/bar.git", "relative URL without a base"),
            (
                "github.com:foo/bar",
                "cannot-be-a-base-URLs are not supported",
            ),
        ] {
            let error = PackageManifest::parse(
                &format!("[package]\nname = \"a\"\n[dependencies]\ng = {{ git = \"{url}\" }}\n"),
                Path::new("/w"),
                None,
            )
            .expect_err("a git URL cargo refuses");

            assert_eq!(error.kind(), TextErrorKind::Unreadable);
            assert_eq!(
                error.detail(),
                format!("[dependencies] `g`: `git = \"{url}\"` is not a URL cargo reads: {reason}")
            );
        }
    }

    #[test]
    fn a_git_url_with_a_query_of_its_own_is_unsupported_naming_the_url() {
        // Cargo keeps a URL's query in the lock and reads it back as the same source; this
        // reader compares none, so any query is refused by name, an empty one included, and so
        // is one that spells `branch=` with no `branch` key, which would lock as a branch.
        for url in [
            "https://example.invalid/g?x=1",
            "https://example.invalid/g?",
            "https://example.invalid/g?branch=dev",
        ] {
            let error = PackageManifest::parse(
                &format!("[package]\nname = \"a\"\n[dependencies]\ng = {{ git = \"{url}\" }}\n"),
                Path::new("/w"),
                None,
            )
            .expect_err("a git URL with a query");

            assert_eq!(error.kind(), TextErrorKind::Unsupported, "{url}");
            assert_eq!(
                error.detail(),
                format!(
                    "[dependencies] `g`: `git = \"{url}\"` has a query of its own, which Cargo \
                     keeps in the lockfile's source and this reader does not follow"
                )
            );
        }
    }

    #[test]
    fn a_git_url_with_a_query_is_refused_in_the_workspace_dependencies_too() {
        // A `workspace = true` member takes the root's declaration, which is read through the
        // same path, so the root's table is where the refusal is named.
        let error = PackageManifest::parse(
            "[workspace]\nmembers = []\n\
             [workspace.dependencies]\ng = { git = \"https://example.invalid/g?x=1\" }\n",
            Path::new("/w"),
            None,
        )
        .expect_err("a git URL with a query");

        assert_eq!(error.kind(), TextErrorKind::Unsupported);
        assert_eq!(
            error.detail(),
            "[workspace.dependencies] `g`: `git = \"https://example.invalid/g?x=1\"` has a query \
             of its own, which Cargo keeps in the lockfile's source and this reader does not \
             follow"
        );
    }

    #[test]
    fn a_git_url_with_a_query_is_read_with_its_query_when_reading_past_it() {
        // Reading past is for a reader that compares no source: the declaration is kept, under
        // its key and the package it names, with the URL as written, query included.
        let manifest = PackageManifest::parse_with(
            "[package]\nname = \"a\"\n[dependencies]\n\
             renamed = { package = \"g\", git = \"https://example.invalid/g?x=1\" }\n",
            Path::new("/w"),
            None,
            Unfollowed::ReadPast,
        )
        .expect("a manifest Cargo reads");

        let [declaration] = manifest.declarations() else {
            panic!(
                "expected one declaration, got {:?}",
                manifest.declarations()
            );
        };
        assert_eq!(declaration.key, "renamed");
        assert_eq!(declaration.package, "g");
        let DeclaredSource::Git { url, reference } = &declaration.source else {
            panic!("expected a git source, got {:?}", declaration.source);
        };
        assert!(url.has_query(), "{url:?}");
        assert_eq!(url.as_str(), "https://example.invalid/g?x=1");
        assert_eq!(*reference, GitReference::DefaultBranch);
    }

    #[test]
    fn reading_past_a_query_refuses_every_other_shape_cargo_refuses() {
        // Only the query is read past. A URL Cargo does not read, and a query beside two
        // references, are refused as when not reading past.
        for (declaration, kind) in [
            (
                "g = { git = \"git@github.com:foo/bar.git\" }",
                TextErrorKind::Unreadable,
            ),
            (
                "g = { git = \"https://example.invalid/g?x=1\", branch = \"x\", tag = \"y\" }",
                TextErrorKind::Unreadable,
            ),
        ] {
            let error = PackageManifest::parse_with(
                &format!("[package]\nname = \"a\"\n[dependencies]\n{declaration}\n"),
                Path::new("/w"),
                None,
                Unfollowed::ReadPast,
            )
            .expect_err(declaration);

            assert_eq!(error.kind(), kind, "{declaration}");
        }
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unsupported);
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
            assert_eq!(
                error.kind(),
                TextErrorKind::Unreadable,
                "`{body}` is unreadable"
            );
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
    }

    #[test]
    fn a_git_dependency_on_a_revision_is_read_with_no_requirement() {
        // The case the generated declarations once shrank to: a git dependency on a revision,
        // under a key that names its own package, with no version requirement.
        let manifest = parsed(
            r#"
[package]
name = "a"

[dependencies]
a = { git = "https://aaa.invalid/a", rev = "aaaa0aa" }
"#,
        );

        assert_eq!(
            manifest.declarations(),
            [Declaration {
                key: "a".to_owned(),
                package: "a".to_owned(),
                rename: None,
                is_dev: false,
                is_optional: false,
                source: DeclaredSource::Git {
                    url: git_url("https://aaa.invalid/a"),
                    reference: GitReference::Rev("aaaa0aa".to_owned()),
                },
                version_requirement: None,
            }]
        );
    }

    /// A declaration as the generator holds it: what the manifest will write, in the parts
    /// Cargo's tables are made of, before it is rendered.
    #[derive(Debug, Clone)]
    struct Generated {
        key: String,
        renamed_from: Option<String>,
        is_dev: bool,
        is_optional: bool,
        source: GeneratedSource,
        version_requirement: Option<String>,
    }

    #[derive(Debug, Clone)]
    enum GeneratedSource {
        Path(String),
        Git(String, GitReference),
        Registry(Option<String>),
    }

    fn generated_source() -> impl Strategy<Value = GeneratedSource> {
        prop_oneof![
            proptest::collection::vec(proptest::sample::select(vec!["..", "x", "y.z"]), 1..4)
                .prop_map(|segments| GeneratedSource::Path(segments.join("/"))),
            (
                "https://[a-z]{3,8}\\.invalid/[a-z]{1,8}",
                prop_oneof![
                    Just(GitReference::DefaultBranch),
                    "[a-zA-Z0-9_/-]{1,8}".prop_map(GitReference::Branch),
                    "[a-zA-Z0-9_/.-]{1,8}".prop_map(GitReference::Tag),
                    "[0-9a-f]{7,40}".prop_map(GitReference::Rev),
                ],
            )
                .prop_map(|(url, reference)| GeneratedSource::Git(url, reference)),
            proptest::option::of("[a-z][a-z0-9-]{0,7}").prop_map(GeneratedSource::Registry),
        ]
    }

    fn generated_declaration() -> impl Strategy<Value = Generated> {
        (
            "[a-zA-Z][a-zA-Z0-9_-]{0,8}",
            proptest::option::of("[a-zA-Z][a-zA-Z0-9_-]{0,8}"),
            proptest::bool::ANY,
            proptest::bool::ANY,
            generated_source(),
            proptest::option::of("(\\^|=|~|>=)?[0-9]{1,2}(\\.[0-9]{1,2}){0,2}(, <[0-9]{1,2})?"),
        )
            .prop_map(
                |(key, renamed_from, is_dev, is_optional, source, version_requirement)| Generated {
                    key,
                    renamed_from,
                    is_dev,
                    is_optional,
                    source,
                    version_requirement,
                },
            )
    }

    impl Generated {
        /// The `key = …` line the manifest writes: the bare version string when nothing but a
        /// registry version is written, an inline table otherwise.
        fn line(&self) -> String {
            let shorthand = self.renamed_from.is_none()
                && !self.is_optional
                && matches!(self.source, GeneratedSource::Registry(None));
            if let (true, Some(requirement)) = (shorthand, &self.version_requirement) {
                return format!("{} = {requirement:?}", self.key);
            }
            let mut fields = Vec::new();
            if let Some(package) = &self.renamed_from {
                fields.push(format!("package = {package:?}"));
            }
            if let Some(requirement) = &self.version_requirement {
                fields.push(format!("version = {requirement:?}"));
            }
            if self.is_optional {
                fields.push("optional = true".to_owned());
            }
            match &self.source {
                GeneratedSource::Path(path) => fields.push(format!("path = {path:?}")),
                GeneratedSource::Git(url, reference) => {
                    fields.push(format!("git = {url:?}"));
                    match reference {
                        GitReference::DefaultBranch => {}
                        GitReference::Branch(name) => fields.push(format!("branch = {name:?}")),
                        GitReference::Tag(name) => fields.push(format!("tag = {name:?}")),
                        GitReference::Rev(name) => fields.push(format!("rev = {name:?}")),
                    }
                }
                GeneratedSource::Registry(registry) => {
                    if let Some(name) = registry {
                        fields.push(format!("registry = {name:?}"));
                    }
                }
            }
            format!("{} = {{ {} }}", self.key, fields.join(", "))
        }

        /// The declaration the reader must give back for the line, written out from the parts and
        /// the manifest's directory `/w/crates/a` rather than computed by the reader.
        fn expected(&self) -> Declaration {
            Declaration {
                key: self.key.clone(),
                package: self
                    .renamed_from
                    .clone()
                    .unwrap_or_else(|| self.key.clone()),
                rename: self.renamed_from.as_ref().map(|_| self.key.clone()),
                is_dev: self.is_dev,
                is_optional: self.is_optional,
                source: match &self.source {
                    GeneratedSource::Path(path) => DeclaredSource::Path {
                        directory: PathBuf::from(format!("/w/crates/a/{path}")),
                    },
                    GeneratedSource::Git(url, reference) => DeclaredSource::Git {
                        url: git_url(url),
                        reference: reference.clone(),
                    },
                    GeneratedSource::Registry(name) => {
                        DeclaredSource::Registry { name: name.clone() }
                    }
                },
                version_requirement: self.version_requirement.clone(),
            }
        }
    }

    proptest! {
        /// Writes a generated declaration (its key, a `package =` rename, a path, git or registry
        /// source, a version requirement, dev and optional) into a manifest, reads the manifest,
        /// and requires the one declaration it holds to equal the generated one.
        #[test]
        fn a_generated_declaration_is_read_back_as_generated(
            declaration in generated_declaration(),
        ) {
            let table = if declaration.is_dev { "dev-dependencies" } else { "dependencies" };
            let text = format!("[package]\nname = \"a\"\n\n[{table}]\n{}\n", declaration.line());

            let manifest = PackageManifest::parse(&text, Path::new("/w/crates/a"), None);

            prop_assert_eq!(
                manifest.map(|manifest| manifest.declarations().to_vec()),
                Ok(vec![declaration.expected()])
            );
        }
    }
}
