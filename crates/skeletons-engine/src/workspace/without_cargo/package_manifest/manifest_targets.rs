//! The targets a manifest writes: its `[lib]` table, each `[[bin]]`, `[[example]]`, `[[test]]`
//! and `[[bench]]` table, which kinds it leaves Cargo to discover from the files beside it, and
//! the edition those paths are read under.
//!
//! This is the manifest's text and nothing else. What the package's files make of it is decided
//! in `targets`, which reads the file system.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

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

/// One `[[bin]]`, `[[example]]`, `[[test]]` or `[[bench]]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaredTarget {
    /// The kind of the array the table is in.
    pub(crate) kind: TargetKind,
    /// `name`, when written.
    pub(crate) name: Option<String>,
    /// `path`, when written, as written.
    pub(crate) path: Option<String>,
}

/// The edition of a package, as far as the places Cargo looks for its targets depend on it.
///
/// Which editions exist depends on the toolchain, and this reader takes no position on that: it
/// keeps only whether the package is in the one edition Cargo gives the old fallbacks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edition {
    /// `2015`, which a package that writes no edition is in.
    Edition2015,
    /// Any other edition.
    Edition2018OrLater,
}

impl Edition {
    /// The edition Cargo reads from the text `written`.
    #[must_use]
    pub(crate) fn named(written: &str) -> Self {
        if written == "2015" {
            Self::Edition2015
        } else {
            Self::Edition2018OrLater
        }
    }
}

/// The targets a manifest writes, and which kinds it leaves Cargo to discover from the files
/// beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Targets {
    /// The `[lib]` table, when there is one.
    pub(crate) library: Option<LibTarget>,
    /// Every table of `[[bin]]`, `[[example]]`, `[[test]]` and `[[bench]]`, in manifest order.
    declared: Vec<DeclaredTarget>,
    /// The kinds whose array key the manifest writes, even as an empty array.
    written: BTreeSet<TargetKind>,
    /// The `autolib`, `autobins`, `autoexamples`, `autotests` and `autobenches` keys written.
    automatic: BTreeMap<TargetKind, bool>,
    edition: Edition,
}

impl Targets {
    /// The tables the manifest writes for `kind`, in manifest order. `Library` has none here: its
    /// table is [`Targets::library`].
    pub(crate) fn declared(&self, kind: TargetKind) -> impl Iterator<Item = &DeclaredTarget> {
        self.declared
            .iter()
            .filter(move |target| target.kind == kind)
    }

    /// Whether Cargo adds the targets of `kind` it finds in the files to the ones written.
    ///
    /// A kind's `auto` key decides it when written. Otherwise a kind with no table is discovered,
    /// and a kind with a table (an empty array included) is discovered in every edition but 2015.
    #[must_use]
    pub(crate) fn discovers(&self, kind: TargetKind) -> bool {
        match (self.automatic.get(&kind), self.written.contains(&kind)) {
            (Some(flag), _) => *flag,
            (None, false) => true,
            (None, true) => self.edition == Edition::Edition2018OrLater,
        }
    }

    /// The edition the package's targets are looked for under.
    #[must_use]
    pub(crate) const fn edition(&self) -> Edition {
        self.edition
    }
}

/// A `[[bin]]`, `[[example]]`, `[[test]]` or `[[bench]]` table as written.
#[derive(Deserialize)]
pub(super) struct RawTarget {
    name: Option<String>,
    path: Option<String>,
}

/// The targets a manifest writes, under `edition`.
pub(super) fn targets_of(raw: &RawManifest, edition: Edition) -> Targets {
    let arrays = [
        (TargetKind::Binary, &raw.bin),
        (TargetKind::Example, &raw.example),
        (TargetKind::Test, &raw.test),
        (TargetKind::Benchmark, &raw.bench),
    ];
    let written = arrays
        .iter()
        .filter_map(|(kind, tables)| tables.as_ref().map(|_| *kind))
        .collect();
    let declared = arrays
        .iter()
        .flat_map(|(kind, tables)| {
            tables.iter().flatten().map(|table| DeclaredTarget {
                kind: *kind,
                name: table.name.clone(),
                path: table.path.clone(),
            })
        })
        .collect();

    let automatic = raw
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
            .filter_map(|(kind, flag)| flag.map(|flag| (kind, flag)))
            .collect()
        })
        .unwrap_or_default();

    Targets {
        library: raw.lib.as_ref().map(|lib| LibTarget {
            name: lib.name.clone(),
            path: lib.path.clone(),
        }),
        declared,
        written,
        automatic,
        edition,
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
            assert_eq!(
                manifest.targets().declared(kind).count(),
                0,
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
        let declared = |kind| {
            targets
                .declared(kind)
                .map(|target| (target.name.clone(), target.path.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            declared(TargetKind::Binary),
            [(Some("tool".to_owned()), None)]
        );
        assert_eq!(
            declared(TargetKind::Benchmark),
            [(Some("speed".to_owned()), None)]
        );
        assert_eq!(declared(TargetKind::Example), []);
        assert_eq!(declared(TargetKind::Test), []);
    }

    #[test]
    fn tables_are_kept_in_manifest_order_with_their_names_and_paths() {
        let manifest = parsed(
            r#"
[package]
name = "a"

[[bin]]
name = "second"
path = "b.rs"

[[bin]]
path = "nameless.rs"

[[bin]]
name = "first"
"#,
        );

        let tables: Vec<_> = manifest
            .targets()
            .declared(TargetKind::Binary)
            .map(|target| (target.name.as_deref(), target.path.as_deref()))
            .collect();
        assert_eq!(
            tables,
            [
                (Some("second"), Some("b.rs")),
                (None, Some("nameless.rs")),
                (Some("first"), None),
            ]
        );
    }

    #[test]
    fn an_empty_array_of_targets_declares_none_but_still_counts_as_written() {
        // Cargo reads `example = []` as a written array: in 2015 it stops discovering examples,
        // and in any later edition it does not.
        let old = parsed("example = []\n[package]\nname = \"a\"\n");
        let newer = parsed("example = []\n[package]\nname = \"a\"\nedition = \"2021\"\n");

        assert_eq!(old.targets().declared(TargetKind::Example).count(), 0);
        assert!(!old.targets().discovers(TargetKind::Example));
        assert!(old.targets().discovers(TargetKind::Test));
        assert!(newer.targets().discovers(TargetKind::Example));
    }

    #[test]
    fn a_written_table_stops_discovery_in_2015_unless_the_auto_key_says_otherwise() {
        let none = "[package]\nname = \"a\"\n[[test]]\nname = \"x\"\n";
        let on = "[package]\nname = \"a\"\nautotests = true\n[[test]]\nname = \"x\"\n";
        let newer = "[package]\nname = \"a\"\nedition = \"2018\"\n[[test]]\nname = \"x\"\n";
        let off = "[package]\nname = \"a\"\nedition = \"2018\"\nautotests = false\n\
                   [[test]]\nname = \"x\"\n";

        assert!(!parsed(none).targets().discovers(TargetKind::Test));
        assert!(parsed(on).targets().discovers(TargetKind::Test));
        assert!(parsed(newer).targets().discovers(TargetKind::Test));
        assert!(!parsed(off).targets().discovers(TargetKind::Test));
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
