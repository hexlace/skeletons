//! The targets a manifest writes: its `[lib]` table, which arrays of targets it has, and which
//! kinds it leaves Cargo to discover from the files beside it.
//!
//! This is the manifest's text and nothing else. What the package's files make of it is decided
//! in `targets`, which reads the file system.

use std::collections::BTreeSet;

use super::RawManifest;

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

/// The targets a manifest writes.
pub(super) fn targets_of(raw: &RawManifest) -> Targets {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::PackageManifest;
    use super::{LibTarget, TargetKind};

    /// Reads `text` as the manifest in `/w/crates/a`, with no workspace above it.
    fn parsed(text: &str) -> PackageManifest {
        PackageManifest::parse(text, Path::new("/w/crates/a"), None)
            .expect("the manifest in this test is well formed")
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
}
