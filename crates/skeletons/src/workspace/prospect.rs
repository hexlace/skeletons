//! What `wear` has to know about the workspace before it writes anything: the
//! workspace as `check` and `sync` read it, and the one member it writes into,
//! with every dependency that member already declares.
//!
//! Both come from the one `cargo metadata` document, so a refusal `wear` makes
//! before `cargo add` runs is made on exactly the workspace the later read
//! back will see.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::schema::{Document, Package};
use super::{Network, ReadWorkspaceError, Workspace, cargo_metadata, from_document, locate};

/// The workspace, and the member `wear` writes into, if the workspace has it.
pub(crate) struct Prospect {
    pub(crate) workspace: Workspace,
    /// `None` when no workspace member has the package's name, which is a
    /// command line run from a project it was not built from.
    pub(crate) member: Option<Member>,
}

/// The workspace member `wear` writes into.
pub(crate) struct Member {
    /// The member's manifest as Cargo reports it, which is absolute.
    pub(crate) manifest_path: PathBuf,
    /// `manifest_path` as a message shows it: relative to the workspace root.
    pub(crate) manifest: String,
    /// Every dependency the member declares, in any table of its manifest.
    pub(crate) declared: Vec<Declared>,
    /// What the member's `[package.metadata.skeletons]` is.
    pub(crate) skeletons: SkeletonsTable,
}

/// One dependency a member declares, as the manifest spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declared {
    /// What the dependency is declared under: its rename, or its crate's name
    /// when it has none.
    pub(crate) key: String,
    /// The crate the dependency is on.
    pub(crate) crate_name: String,
}

/// What a member's `[package.metadata.skeletons]` is, as far as the tables
/// above it allow it to be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkeletonsTable {
    /// There is no such table.
    Absent,
    /// It is a table, holding these keys.
    Keys(BTreeSet<String>),
    /// `[package.metadata]` is there and is not a table, so nothing can be
    /// under it: there is no `skeletons` to read, and none can be written.
    MetadataNotATable,
    /// `[package.metadata.skeletons]` is there and is not a table.
    SkeletonsNotATable,
}

/// Reads `directory`'s workspace `--locked`, letting cargo reach the network as
/// `check` does, and finds `package_name` among its members.
///
/// # Errors
///
/// Returns [`ReadWorkspaceError`] when `cargo metadata` could not be run or
/// refused.
pub(crate) fn read_prospect(
    directory: &Path,
    package_name: &str,
) -> Result<Prospect, ReadWorkspaceError> {
    let document = cargo_metadata::fetch(directory, Network::Allowed)?;
    Ok(Prospect {
        workspace: from_document(&document),
        member: member_of(&document, package_name),
    })
}

/// The workspace member named `package_name`, or `None` when no member is.
///
/// A dependency that happens to carry the name is not a member, so it is not
/// found: `wear` writes only into a package of the workspace.
fn member_of(document: &Document, package_name: &str) -> Option<Member> {
    let package = document
        .packages
        .iter()
        // The running command line's own package name and the members' names
        // both come from Cargo, spelled as their manifests spell them, so this
        // is identity, not a name a person typed: compared exactly.
        .filter(|package| package.name == package_name)
        .find(|package| document.workspace_members.contains(&package.id))?;
    Some(Member {
        manifest_path: package.manifest_path.clone(),
        manifest: super::relative_to_root(&document.workspace_root, &package.manifest_path),
        declared: declared_by(package),
        skeletons: skeletons_table(&package.metadata),
    })
}

/// Every dependency `package` declares.
///
/// `cargo metadata` lists them all, normal, dev and build, and the ones under
/// a `[target…]` table too, so nothing here is filtered by platform: a key a
/// dependency holds on one platform is taken on every platform.
fn declared_by(package: &Package) -> Vec<Declared> {
    package
        .dependencies
        .iter()
        .map(|dependency| Declared {
            key: locate::declared_key(dependency).to_owned(),
            crate_name: dependency.name.clone(),
        })
        .collect()
}

/// What `metadata`, a package's `[package.metadata]` as raw JSON, holds at
/// `skeletons`.
///
/// Cargo reports a `metadata` the manifest does not have as `null`, so that
/// is absent. Any other `metadata` that is not a table has no `skeletons` in
/// it and none can be written under it, which is its own state.
fn skeletons_table(metadata: &serde_json::Value) -> SkeletonsTable {
    match metadata {
        serde_json::Value::Null => SkeletonsTable::Absent,
        serde_json::Value::Object(_) => match metadata.get("skeletons") {
            None => SkeletonsTable::Absent,
            Some(serde_json::Value::Object(entries)) => {
                SkeletonsTable::Keys(entries.keys().cloned().collect())
            }
            Some(_) => SkeletonsTable::SkeletonsNotATable,
        },
        serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_)
        | serde_json::Value::Array(_) => SkeletonsTable::MetadataNotATable,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::{Declared, SkeletonsTable, member_of};
    use crate::workspace::schema::Document;

    /// A `cargo metadata --format-version 1` document captured from a scratch
    /// workspace of five members, to settle what `cargo metadata` reports of
    /// target-specific dependencies: `wearer` depends on `plain`, on `other`
    /// renamed `neat`, on `tooling` as a dev-dependency, and on `never` under
    /// `[target.'cfg(any())'.dependencies]`, a table no platform ever
    /// satisfies, and wears `plain`. Captured 2026-10-02 with cargo 1.95.0
    /// as `cargo metadata --format-version 1 --locked --all-features`, the
    /// capturing machine's absolute paths replaced with `/WORKSPACE`.
    #[cfg(skeletons_checkout)]
    const TARGET_SPECIFIC_WORKSPACE: &str = include_str!("fixtures/target-specific-workspace.json");

    /// The workspace `schema.rs`'s tests describe, captured the same way.
    #[cfg(skeletons_checkout)]
    const DEMO_WORKSPACE: &str = include_str!("fixtures/demo-workspace.json");

    fn document(source: &str) -> Document {
        serde_json::from_str(source).expect("a test document must parse")
    }

    fn declared(key: &str, crate_name: &str) -> Declared {
        Declared {
            key: key.to_owned(),
            crate_name: crate_name.to_owned(),
        }
    }

    /// A document with one package `name` in the workspace, whose
    /// `[package.metadata]` is `metadata` and whose dependencies are the given
    /// JSON objects, and one more package outside it.
    fn one_member(name: &str, metadata: &str, dependencies: &str) -> Document {
        document(&format!(
            r#"{{
                "version": 1,
                "workspace_root": "/WORKSPACE",
                "workspace_members": ["member-id"],
                "packages": [
                    {{
                        "id": "member-id", "name": "{name}", "version": "0.1.0",
                        "source": null, "manifest_path": "/WORKSPACE/{name}/Cargo.toml",
                        "dependencies": [{dependencies}], "metadata": {metadata}
                    }},
                    {{
                        "id": "outside-id", "name": "outside", "version": "1.0.0",
                        "source": null, "manifest_path": "/ELSEWHERE/outside/Cargo.toml",
                        "dependencies": [], "metadata": null
                    }}
                ],
                "resolve": {{ "nodes": [] }}
            }}"#
        ))
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_target_specific_dependency_is_declared_like_any_other() {
        // Settles what `cargo metadata` does without `--filter-platform`: the
        // dependency under `[target.'cfg(any())'.dependencies]`, which no
        // platform satisfies, is listed all the same, as are the dev and the
        // renamed ones. A key held only on some platform is taken on all.
        let member = member_of(&document(TARGET_SPECIFIC_WORKSPACE), "wearer")
            .expect("wearer is a member of the captured workspace");

        assert_eq!(
            member.declared,
            vec![
                declared("neat", "other"),
                declared("plain", "plain"),
                declared("tooling", "tooling"),
                declared("never", "never"),
            ]
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_member_of_the_captured_workspace_is_read_with_its_manifest_and_wearing_keys() {
        let member = member_of(&document(TARGET_SPECIFIC_WORKSPACE), "wearer")
            .expect("wearer is a member of the captured workspace");

        assert_eq!(member.manifest, "wearer/Cargo.toml");
        assert_eq!(
            member.manifest_path,
            Path::new("/WORKSPACE/wearer/Cargo.toml")
        );
        assert_eq!(
            member.skeletons,
            SkeletonsTable::Keys(BTreeSet::from(["plain".to_owned()]))
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_member_with_no_dependencies_and_no_metadata_declares_nothing_and_wears_nothing() {
        let member = member_of(&document(TARGET_SPECIFIC_WORKSPACE), "plain")
            .expect("plain is a member of the captured workspace");

        assert_eq!(member.declared, Vec::new());
        assert_eq!(member.skeletons, SkeletonsTable::Absent);
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_renamed_dependency_is_declared_under_its_rename() {
        let member = member_of(&document(DEMO_WORKSPACE), "wearer")
            .expect("wearer is a member of the demo workspace");

        assert_eq!(
            member.declared,
            vec![declared("a-skeleton", "skeleton-crate")]
        );
        assert_eq!(
            member.skeletons,
            SkeletonsTable::Keys(BTreeSet::from(["a-skeleton".to_owned()]))
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_name_no_member_has_is_not_found() {
        assert!(member_of(&document(TARGET_SPECIFIC_WORKSPACE), "absent").is_none());
    }

    #[test]
    fn a_package_outside_the_workspace_is_not_a_member() {
        // `outside` is in `packages`, as every dependency is, but is not one of
        // the workspace's own, so `wear` never writes into it.
        let document = one_member("inside", "null", "");

        assert!(member_of(&document, "outside").is_none());
        assert!(member_of(&document, "inside").is_some());
    }

    #[test]
    fn a_member_with_no_metadata_has_no_skeletons_table() {
        let member = member_of(&one_member("m", "null", ""), "m").expect("m is a member");

        assert_eq!(member.skeletons, SkeletonsTable::Absent);
    }

    #[test]
    fn a_metadata_table_without_skeletons_has_no_skeletons_table() {
        let member =
            member_of(&one_member("m", r#"{"ritual": {}}"#, ""), "m").expect("m is a member");

        assert_eq!(member.skeletons, SkeletonsTable::Absent);
    }

    #[test]
    fn a_metadata_that_is_not_a_table_is_not_a_table() {
        // Cargo takes a free-form `metadata` of any shape, and reports a
        // missing one as `null`, which is the one non-table that is absent.
        for metadata in ["1", "1.5", r#""notes""#, "true", r#"["skeletons"]"#] {
            let member = member_of(&one_member("m", metadata, ""), "m").expect("m is a member");

            assert_eq!(
                member.skeletons,
                SkeletonsTable::MetadataNotATable,
                "{metadata}"
            );
        }
    }

    #[test]
    fn an_empty_skeletons_table_is_a_table_with_no_keys() {
        let member =
            member_of(&one_member("m", r#"{"skeletons": {}}"#, ""), "m").expect("m is a member");

        assert_eq!(member.skeletons, SkeletonsTable::Keys(BTreeSet::new()));
    }

    #[test]
    fn every_key_of_the_skeletons_table_is_kept_whatever_it_holds() {
        // A key holding a table, one holding a string and a reserved word are
        // all keys: `wear` only asks whether its own key is among them.
        let metadata = r#"{"skeletons": {"a": {}, "b": "text", "options": {}}}"#;
        let member = member_of(&one_member("m", metadata, ""), "m").expect("m is a member");

        assert_eq!(
            member.skeletons,
            SkeletonsTable::Keys(BTreeSet::from([
                "a".to_owned(),
                "b".to_owned(),
                "options".to_owned()
            ]))
        );
    }

    #[test]
    fn a_skeletons_entry_that_is_not_a_table_is_not_a_table() {
        for metadata in [
            r#"{"skeletons": 5}"#,
            r#"{"skeletons": "text"}"#,
            r#"{"skeletons": ["a"]}"#,
            r#"{"skeletons": true}"#,
        ] {
            let member = member_of(&one_member("m", metadata, ""), "m").expect("m is a member");

            assert_eq!(
                member.skeletons,
                SkeletonsTable::SkeletonsNotATable,
                "{metadata}"
            );
        }
    }

    #[test]
    fn a_dependency_with_a_target_is_declared_whichever_platform_it_names() {
        // The same reading as the captured document, with the dependency
        // written out: the `target` of a dependency changes nothing here.
        let dependencies = r#"
            {"name": "never", "rename": null, "target": "cfg(any())"},
            {"name": "windy", "rename": "gale", "target": "x86_64-unknown-none"}
        "#;
        let member = member_of(&one_member("m", "null", dependencies), "m").expect("m is a member");

        assert_eq!(
            member.declared,
            vec![declared("never", "never"), declared("gale", "windy")]
        );
    }
}
