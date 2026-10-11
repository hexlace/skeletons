//! The slice of `cargo metadata --format-version 1`'s JSON this crate reads,
//! following `rituals-compose`'s own precedent for the same format: derive,
//! a checked `version`, and every field named for what `cargo metadata`
//! actually calls it, so this file can be held next to Cargo's own
//! documentation and read the same words. Unknown fields are ignored —
//! serde's own default — because cargo adds fields over time and this crate
//! only ever reads a small, fixed slice of them.

use std::path::PathBuf;

use serde::Deserialize;

/// The whole of `cargo metadata --format-version 1`'s output this crate
/// reads.
#[derive(Debug, Deserialize)]
pub(crate) struct Document {
    pub(crate) version: u64,
    /// Never printed as such. Read only to relativise the paths this crate
    /// reports against it (`workspace::relative_to_root`).
    pub(crate) workspace_root: PathBuf,
    /// The package ids of every workspace member, as opposed to a dependency
    /// pulled in from outside the workspace.
    pub(crate) workspace_members: Vec<String>,
    pub(crate) packages: Vec<Package>,
    pub(crate) resolve: Resolve,
}

/// One package in the resolved graph — a workspace member or a dependency,
/// at any depth.
#[derive(Debug, Deserialize)]
pub(crate) struct Package {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    /// `null` for a path dependency; `registry+…`/`sparse+…` for a registry
    /// dependency; `git+…` for a git dependency. See `workspace/pin.rs`.
    pub(crate) source: Option<String>,
    pub(crate) manifest_path: PathBuf,
    pub(crate) dependencies: Vec<Dependency>,
    /// `[package.metadata]`, as a raw JSON value: a wearer's own
    /// `[package.metadata.skeletons.<key>]` tables, or a skeleton's own
    /// `[package.metadata.skeletons]` schema, in whatever shape they were
    /// written — validating that shape is `workspace/wearing_table.rs`'s
    /// job, not this module's. Absent in the source JSON, and so defaulted
    /// to `null`, when a package declares no `[package.metadata]` at all.
    #[serde(default)]
    pub(crate) metadata: serde_json::Value,
}

/// The key under which `cargo metadata --format-version 1` spells a TOML
/// datetime: a datetime is reported as the object `{KEY: "<its text>"}`.
const TOML_DATETIME_KEY: &str = "$__toml_private_datetime";

/// Reads `value`, a node of a package's `metadata` as `cargo metadata` reports
/// it, as a TOML table: the entries it holds, or `None` when it is not one.
///
/// A package's `[package.metadata]` is free-form, so Cargo accepts a TOML
/// datetime anywhere in it, and `--format-version 1` reports one on every
/// toolchain this crate supports as a JSON object with the single key
/// `$__toml_private_datetime`, whose value is the datetime's text as a string:
/// `metadata.skeletons = 1979-05-27` arrives as
/// `{"skeletons": {"$__toml_private_datetime": "1979-05-27"}}`. An object of
/// exactly that shape is read as a datetime, so it is not a table here.
///
/// Cargo's manifest parser reserves that key for datetimes: a manifest that
/// writes it as a quoted key holding a datetime string produces the same
/// metadata as one that writes the datetime itself, so the report gives no
/// means to tell the two apart, and the key is Cargo's own spelling of a
/// datetime. Cargo refuses the key when its value is not a datetime string,
/// and drops any sibling keys written beside it.
///
/// The key is matched exactly: an object whose only key is that one, holding a
/// string. An object whose single value is not a string, or that holds the key
/// among others, cannot come from Cargo; it is read as a table.
pub(crate) fn as_toml_table(
    value: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let serde_json::Value::Object(entries) = value else {
        return None;
    };
    match (entries.len(), entries.get(TOML_DATETIME_KEY)) {
        (1, Some(serde_json::Value::String(_))) => None,
        (_, _) => Some(entries),
    }
}

/// Whether `value` is a TOML datetime, as `cargo metadata` spells one: an
/// object that [`as_toml_table`] declines for being exactly that.
pub(crate) fn is_toml_datetime(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(_) => as_toml_table(value).is_none(),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_)
        | serde_json::Value::Array(_) => false,
    }
}

/// One entry in a package's own `[dependencies]` (or `[dev-dependencies]`,
/// `[build-dependencies]`, a target-specific table) — as declared, not yet
/// resolved to the package it points at.
#[derive(Debug, Deserialize)]
pub(crate) struct Dependency {
    pub(crate) name: String,
    /// The name the dependency is declared under, when the manifest renames
    /// its package: `y = { package = "x" }` gives `name` `x` and `rename`
    /// `y`.
    pub(crate) rename: Option<String>,
}

/// The resolved dependency graph.
#[derive(Debug, Deserialize)]
pub(crate) struct Resolve {
    pub(crate) nodes: Vec<Node>,
}

/// One package's position in the resolved graph: which packages it depends
/// on, and what extern-crate name rustc gives each one.
#[derive(Debug, Deserialize)]
pub(crate) struct Node {
    pub(crate) id: String,
    pub(crate) deps: Vec<NodeDependency>,
}

/// One resolved dependency of a [`Node`].
#[derive(Debug, Deserialize)]
pub(crate) struct NodeDependency {
    /// The extern-crate identifier rustc is given for this dependency: the
    /// dependency key or its rename, with any `-` already turned into `_` by
    /// Cargo.
    pub(crate) name: String,
    /// The package id this dependency resolves to.
    pub(crate) pkg: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Document, as_toml_table, is_toml_datetime};

    /// A `cargo metadata --format-version 1` document captured from a
    /// scratch workspace built to exercise every shape this module reads: a
    /// two-member workspace where `wearer` depends on `skeleton-crate`
    /// renamed `a-skeleton`, and `skeleton-crate` declares
    /// `[package.metadata.skeletons]` with one enum option, and `wearer` wears it
    /// with `[package.metadata.skeletons.a-skeleton] cadence = "daily"`. The
    /// capturing
    /// machine's absolute paths are replaced with `/WORKSPACE` throughout.
    #[cfg(skeletons_checkout)]
    const DEMO_WORKSPACE: &str = include_str!("fixtures/demo-workspace.json");

    // The three shapes below are what `cargo metadata --format-version 1 --no-deps`
    // reported, identically on cargo 1.95.0 and 1.85.0, for a manifest whose
    // `[package]` has `metadata = 1979-05-27T07:32:00Z`, whose `[package]` has
    // `metadata.skeletons = 1979-05-27`, and whose `[package.metadata.skeletons]`
    // has `x = 07:32:00`.

    #[test]
    fn a_metadata_that_is_a_datetime_is_not_a_table() {
        let metadata = json!({"$__toml_private_datetime": "1979-05-27T07:32:00Z"});

        assert!(as_toml_table(&metadata).is_none());
        assert!(is_toml_datetime(&metadata));
    }

    #[test]
    fn a_skeletons_that_is_a_datetime_is_not_a_table() {
        let metadata = json!({"skeletons": {"$__toml_private_datetime": "1979-05-27"}});

        assert!(as_toml_table(&metadata).is_some(), "the metadata holds it");
        assert!(as_toml_table(&metadata["skeletons"]).is_none());
        assert!(is_toml_datetime(&metadata["skeletons"]));
    }

    #[test]
    fn an_option_value_that_is_a_datetime_is_not_a_table() {
        let metadata = json!({"skeletons": {"x": {"$__toml_private_datetime": "07:32:00"}}});

        assert!(as_toml_table(&metadata["skeletons"]).is_some());
        assert!(as_toml_table(&metadata["skeletons"]["x"]).is_none());
        assert!(is_toml_datetime(&metadata["skeletons"]["x"]));
    }

    #[test]
    fn a_table_holding_the_datetime_key_among_others_is_a_table() {
        let value = json!({"$__toml_private_datetime": "1979-05-27", "other": 1});

        assert_eq!(as_toml_table(&value).map(serde_json::Map::len), Some(2));
        assert!(!is_toml_datetime(&value));
    }

    #[test]
    fn a_one_key_object_whose_value_is_not_a_string_is_a_table() {
        // Cargo spells a datetime's text as a string and never anything else,
        // so this is a table a manifest wrote with that key.
        for value in [
            json!({"$__toml_private_datetime": 5}),
            json!({"$__toml_private_datetime": null}),
            json!({"$__toml_private_datetime": ["1979-05-27"]}),
            json!({"$__toml_private_datetime": {}}),
        ] {
            assert!(as_toml_table(&value).is_some(), "{value} must be a table");
            assert!(!is_toml_datetime(&value), "{value} must not be a datetime");
        }
    }

    #[test]
    fn an_ordinary_table_is_a_table_whatever_it_holds() {
        for value in [json!({}), json!({"a": 1}), json!({"a": {"b": "c"}})] {
            assert!(as_toml_table(&value).is_some(), "{value} must be a table");
            assert!(!is_toml_datetime(&value), "{value} must not be a datetime");
        }
    }

    #[test]
    fn a_value_that_is_not_an_object_is_not_a_table() {
        for value in [
            json!(null),
            json!(true),
            json!(1),
            json!("1979-05-27"),
            json!(["a"]),
        ] {
            assert!(as_toml_table(&value).is_none(), "{value} is no table");
            assert!(!is_toml_datetime(&value), "{value} is no datetime");
        }
    }

    fn parse(document: &str) -> serde_json::Result<Document> {
        serde_json::from_str(document)
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn the_fixture_parses_and_reports_format_version_one() {
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        assert_eq!(document.version, 1);
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn both_workspace_members_are_listed() {
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        assert_eq!(document.workspace_members.len(), 2);
        assert!(
            document
                .workspace_members
                .iter()
                .any(|id| id.contains("wearer"))
        );
        assert!(
            document
                .workspace_members
                .iter()
                .any(|id| id.contains("skeleton-crate"))
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_workspace_members_source_is_null_and_its_dependency_carries_its_rename() {
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        let wearer = document
            .packages
            .iter()
            .find(|package| package.name == "wearer")
            .expect("wearer must be in the fixture");
        assert_eq!(wearer.source, None);
        let dependency = wearer
            .dependencies
            .first()
            .expect("wearer must declare one dependency");
        assert_eq!(dependency.name, "skeleton-crate");
        assert_eq!(dependency.rename.as_deref(), Some("a-skeleton"));
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_path_dependencys_target_package_has_a_null_source_and_an_absolute_manifest_path() {
        // A dependency's own directory is never read from its declared
        // `[dependencies]` entry (cargo metadata's `path` key there is not
        // part of this schema at all): it comes from the resolved target
        // package's own `manifest_path`, which `workspace.rs` reads via
        // `manifest_path.parent()`. This is what that lookup actually
        // reads: `skeleton-crate`'s own `source` is null (the mark of a path
        // dependency, as `workspace/pin.rs` documents) and its
        // `manifest_path` is absolute.
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        let skeleton_crate = document
            .packages
            .iter()
            .find(|package| package.name == "skeleton-crate")
            .expect("skeleton-crate must be in the fixture");
        assert_eq!(skeleton_crate.source, None);
        assert!(skeleton_crate.manifest_path.is_absolute());
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_renamed_dependencys_extern_crate_name_turns_the_dash_into_an_underscore() {
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        let wearer_node = document
            .resolve
            .nodes
            .iter()
            .find(|node| node.id.contains("wearer"))
            .expect("wearer must have a resolve node");
        let resolved = wearer_node
            .deps
            .first()
            .expect("wearer must resolve one dependency");
        assert_eq!(resolved.name, "a_skeleton");
        assert!(resolved.pkg.contains("skeleton-crate"));
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_skeletons_own_schema_and_a_wearers_choices_are_both_read_as_raw_json() {
        let document = parse(DEMO_WORKSPACE).expect("the fixture must parse");
        let skeleton = document
            .packages
            .iter()
            .find(|package| package.name == "skeleton-crate")
            .expect("skeleton-crate must be in the fixture");
        assert!(skeleton.metadata["skeletons"]["options"]["cadence"].is_object());

        let wearer = document
            .packages
            .iter()
            .find(|package| package.name == "wearer")
            .expect("wearer must be in the fixture");
        assert_eq!(
            wearer.metadata["skeletons"]["a-skeleton"]["cadence"],
            "daily"
        );
    }

    #[test]
    fn a_package_with_no_metadata_table_reports_null() {
        // `resolve.nodes` and `dependencies` lists are read the same way,
        // but `metadata` is the one field this module defaults when the
        // source JSON omits it entirely — proved directly against a minimal
        // document rather than the fixture, which always carries one.
        let minimal = r#"{
            "version": 1,
            "workspace_root": "/WORKSPACE",
            "workspace_members": [],
            "packages": [{
                "id": "x",
                "name": "x",
                "version": "0.0.0",
                "source": null,
                "manifest_path": "/WORKSPACE/x/Cargo.toml",
                "targets": [],
                "dependencies": []
            }],
            "resolve": { "nodes": [] }
        }"#;
        let document = parse(minimal).expect("a document with no metadata table must parse");
        assert!(document.packages[0].metadata.is_null());
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_future_format_version_still_parses_here_and_is_refused_by_the_reader() {
        // `workspace/cargo_metadata.rs::parse` is what refuses an
        // unsupported version, by name; this module's own job is only to
        // report the number faithfully, so a document version 2 still
        // deserialises fine at this layer.
        let future = DEMO_WORKSPACE.replacen("\"version\": 1,", "\"version\": 2,", 1);
        let document = parse(&future).expect("a future version must still parse at this layer");
        assert_eq!(document.version, 2);
    }
}
