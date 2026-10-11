//! Taking a wearing out of the command line's manifest, in place.
//!
//! The dependency under the key and the `[package.metadata.skeletons.<key>]`
//! table are removed from the manifest's text with `toml_edit`, which keeps
//! every comment, blank line and ordering the wearer wrote, and the result is
//! checked against an independent parse of the manifest before it is kept: it
//! must be the manifest as it was, less those entries.
//!
//! `cargo remove` is not used for the dependency. It also deletes unused
//! `[workspace.dependencies]` entries, profile tables for the crate and
//! comments above tables it empties, in the workspace root manifest as well as
//! the member's, so it changes things that are not this wearing and a manifest
//! that is not this one. The part of it rebuilt here is the one that finds the
//! tables a dependency can be declared in. Its clean-up of `[features]` is not:
//! a feature that still names the key makes Cargo refuse what is left, and that
//! is reported with Cargo's own words.
//!
//! The dependency is removed from every dependency table of the manifest that
//! declares the key, those under `[target.<name>]` too, and a dependency table
//! it leaves empty goes with its header. Nothing else is touched: an inherited
//! `[workspace.dependencies]` entry stays.

use toml_edit::{DocumentMut, Item, TableLike};

/// The names a dependency table has in a manifest: the three Cargo documents
/// and the two underscore spellings it still reads.
const DEPENDENCY_TABLES: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "dev_dependencies",
    "build_dependencies",
];

/// The tables on the way from the manifest to a wearing table, from the
/// outermost in.
const WEARING_PATH: [&str; 3] = ["package", "metadata", "skeletons"];

/// Why a wearing could not be taken out of a manifest's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManifestRefusal {
    /// The manifest is not TOML this module can read.
    Unreadable { detail: String },
    /// The manifest has no wearing table to take out, or the edited manifest is
    /// not the original less the wearing, for the reason `detail` gives.
    Defect { detail: String },
}

/// `manifest` with the dependency under `key` and the table
/// `[package.metadata.skeletons.<key>]` taken out.
///
/// The dependency is taken out of every dependency table that declares the key,
/// and a table that leaves empty is taken out with its header, as is a
/// `skeletons` or `metadata` table that only held the wearing. On a manifest
/// `wear` wrote this is its inverse: the manifest as it was before.
///
/// # Errors
///
/// Returns [`ManifestRefusal`] when the manifest cannot be read, when it has
/// no wearing table under `key`, or when what was written is not the manifest
/// as it was less the wearing.
pub(crate) fn without_wearing(manifest: &str, key: &str) -> Result<String, ManifestRefusal> {
    assert!(!key.is_empty(), "a wearing sits under a key");
    let mut document: DocumentMut =
        manifest
            .parse()
            .map_err(|error: toml_edit::TomlError| ManifestRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    take_out_wearing_table(&mut document, key)?;
    take_out_dependencies(document.as_table_mut(), key);
    let rewritten = document.to_string();
    verify(manifest, &rewritten, key)?;
    Ok(rewritten)
}

/// Removes `package.metadata.skeletons.<key>` from `document`, and the
/// `skeletons` and `metadata` tables above it when that leaves them empty.
fn take_out_wearing_table(document: &mut DocumentMut, key: &str) -> Result<(), ManifestRefusal> {
    let absent = || ManifestRefusal::Defect {
        detail: "the manifest has no wearing table to take out".to_owned(),
    };
    let [package, metadata, skeletons] = WEARING_PATH;
    let package = document
        .get_mut(package)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(absent)?;
    let metadata_table = package
        .get_mut(metadata)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(absent)?;
    let skeletons_table = metadata_table
        .get_mut(skeletons)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(absent)?;
    skeletons_table.remove(key).ok_or_else(absent)?;
    if skeletons_table.is_empty() {
        metadata_table.remove(skeletons);
    }
    if metadata_table.is_empty() {
        package.remove(metadata);
    }
    Ok(())
}

/// Removes `key` from every dependency table directly in `root`, and from
/// every dependency table in a `[target.<name>]` table of it.
fn take_out_dependencies(root: &mut dyn TableLike, key: &str) {
    take_out_of_dependency_tables(root, key);
    let Some(targets) = root.get_mut("target").and_then(Item::as_table_like_mut) else {
        return;
    };
    for (_name, target) in targets.iter_mut() {
        if let Some(target) = target.as_table_like_mut() {
            take_out_of_dependency_tables(target, key);
        }
    }
}

/// Removes `key` from each of `parent`'s dependency tables, and a dependency
/// table that is left empty.
fn take_out_of_dependency_tables(parent: &mut dyn TableLike, key: &str) {
    for name in DEPENDENCY_TABLES {
        let emptied = parent
            .get_mut(name)
            .and_then(Item::as_table_like_mut)
            .is_some_and(|table| {
                table.remove(key);
                table.is_empty()
            });
        if emptied {
            parent.remove(name);
        }
    }
}

/// Checks `rewritten` against `original`, parsed by `toml` rather than by the
/// library that wrote it: equal in value to the original less the dependency
/// under `key` and the wearing table, once the tables the removal emptied are
/// taken out of both.
fn verify(original: &str, rewritten: &str, key: &str) -> Result<(), ManifestRefusal> {
    let mut expected: toml::Table =
        original
            .parse()
            .map_err(|error: toml::de::Error| ManifestRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    take_out_of_value(&mut expected, key).ok_or_else(|| ManifestRefusal::Defect {
        detail: "the manifest has no wearing table to take out".to_owned(),
    })?;
    let mut actual: toml::Table =
        rewritten
            .parse()
            .map_err(|error: toml::de::Error| ManifestRefusal::Defect {
                detail: format!("the edited manifest is not TOML: {error}"),
            })?;
    prune_emptied(&mut expected);
    prune_emptied(&mut actual);
    if expected == actual {
        Ok(())
    } else {
        Err(ManifestRefusal::Defect {
            detail: "the edited manifest differs from the original by more than the wearing"
                .to_owned(),
        })
    }
}

/// Takes the wearing table and the dependency under `key` out of `table`, or
/// returns `None` when there is no wearing table to take out.
fn take_out_of_value(table: &mut toml::Table, key: &str) -> Option<()> {
    let [package, metadata, skeletons] = WEARING_PATH;
    table
        .get_mut(package)?
        .get_mut(metadata)?
        .get_mut(skeletons)?
        .as_table_mut()?
        .remove(key)?;
    take_out_of_dependency_values(table, key);
    if let Some(targets) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_name, target) in targets.iter_mut() {
            if let Some(target) = target.as_table_mut() {
                take_out_of_dependency_values(target, key);
            }
        }
    }
    Some(())
}

/// Removes `key` from each dependency table directly in `table`.
fn take_out_of_dependency_values(table: &mut toml::Table, key: &str) {
    for name in DEPENDENCY_TABLES {
        if let Some(dependencies) = table.get_mut(name).and_then(toml::Value::as_table_mut) {
            dependencies.remove(key);
        }
    }
}

/// Takes out of `table` every dependency table that is empty, every
/// `[target.<name>]` table that is then empty and `target` itself, and the
/// `skeletons` and `metadata` tables under `package` that are empty.
fn prune_emptied(table: &mut toml::Table) {
    prune_empty_dependency_values(table);
    if let Some(targets) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_name, target) in targets.iter_mut() {
            if let Some(target) = target.as_table_mut() {
                prune_empty_dependency_values(target);
            }
        }
        targets.retain(|_, target| target.as_table().is_none_or(|target| !target.is_empty()));
    }
    if table
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(toml::Table::is_empty)
    {
        table.remove("target");
    }
    let [package, metadata, skeletons] = WEARING_PATH;
    let Some(package) = table.get_mut(package).and_then(toml::Value::as_table_mut) else {
        return;
    };
    let Some(metadata) = package
        .get_mut(metadata)
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    if metadata
        .get(skeletons)
        .and_then(toml::Value::as_table)
        .is_some_and(toml::Table::is_empty)
    {
        metadata.remove(skeletons);
    }
    if metadata.is_empty() {
        package.remove("metadata");
    }
}

/// Takes out of `table` each dependency table directly in it that is empty.
fn prune_empty_dependency_values(table: &mut toml::Table) {
    for name in DEPENDENCY_TABLES {
        if table
            .get(name)
            .and_then(toml::Value::as_table)
            .is_some_and(toml::Table::is_empty)
        {
            table.remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{ManifestRefusal, verify, without_wearing};
    use crate::wear::{Key, with_empty_wearing_table};

    const PACKAGE: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n";

    fn removed(manifest: &str) -> Result<String, ManifestRefusal> {
        without_wearing(manifest, "tidy")
    }

    /// `manifest` as `wear` leaves it once it has added the wearing table for
    /// `key`, on top of a dependency that is already declared.
    fn worn(manifest: &str, key: &str) -> String {
        with_empty_wearing_table(
            manifest,
            &Key::new(key).expect("a test passes only keys it knows are valid"),
        )
        .expect("a test passes only manifests that can take the table")
    }

    // Each case is a manifest as it was before `wear`, and the same manifest as
    // `cargo add --dev --rename tidy --path ../dep` leaves it (run against Cargo
    // itself, so the shapes are Cargo's own: a new table at the end, or a line
    // in the table that is there, sorted by key, with a comment after the last
    // table staying last). Wearing then removing must give the original back
    // byte for byte.
    const BYTE_CASES: [(&str, &str); 8] = [
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\n",
        ),
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dependencies]\nx = \"1\"\n",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dependencies]\nx = \"1\"\n\n\
             [dev-dependencies]\ntidy = { path = \"../dep\", package = \"dep\" }\n",
        ),
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\nx = \"1\"\n",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\nx = \"1\"\n",
        ),
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
             [package.metadata.ritual]\ntasks = [\"a\"]\n\n[dev-dependencies]\nx = \"1\"\n",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
             [package.metadata.ritual]\ntasks = [\"a\"]\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\nx = \"1\"\n",
        ),
        (
            "# header\n[package]\nname = \"cli\"\nversion = \"0.1.0\"\n# trailing comment\n",
            "# header\n[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\n# trailing comment\n",
        ),
        (
            "package = { name = \"cli\", version = \"0.1.0\" }\n",
            "package = { name = \"cli\", version = \"0.1.0\" }\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\n",
        ),
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\
             metadata = { ritual = { tasks = [] } }\n",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\
             metadata = { ritual = { tasks = [] } }\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\n",
        ),
        (
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n# last word, no newline",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dev-dependencies]\n\
             tidy = { path = \"../dep\", package = \"dep\" }\n# last word, no newline",
        ),
    ];

    #[test]
    fn removing_what_wear_wrote_gives_back_the_manifest_byte_for_byte() {
        for (before, after_cargo_add) in BYTE_CASES {
            let wearing = worn(after_cargo_add, "tidy");

            assert_eq!(
                removed(&wearing).as_deref(),
                Ok(before),
                "from the manifest:\n{wearing}"
            );
        }
    }

    #[test]
    fn removing_what_wear_wrote_next_to_another_wearing_leaves_that_wearing_alone() {
        let before = format!(
            "{PACKAGE}\n[package.metadata.skeletons.other]\n\n[dev-dependencies]\n\
             other = \"1\"\n"
        );
        // `other` sorts before `tidy`, so Cargo appends the new line after it.
        let wearing = worn(&format!("{before}tidy = \"1\"\n"), "tidy");

        assert_eq!(removed(&wearing).as_deref(), Ok(before.as_str()));
    }

    #[test]
    fn a_dependency_in_each_shape_of_dependency_table_is_removed_and_nothing_else() {
        // The manifest shape, the key in it, and what is left. Every other
        // entry, table and comment stays exactly as written.
        let cases = [
            (
                "inline table in the root",
                "dev-dependencies = { tidy = \"1\", other = \"1\" }\n[package]\nname = \"c\"\n\
                 [package.metadata.skeletons.tidy]\n",
                "dev-dependencies = { other = \"1\" }\n[package]\nname = \"c\"\n",
            ),
            (
                "dotted keys in the root",
                "dev-dependencies.other = \"1\"\ndev-dependencies.tidy = \"1\"\n[package]\n\
                 name = \"c\"\n[package.metadata.skeletons.tidy]\n",
                "dev-dependencies.other = \"1\"\n[package]\nname = \"c\"\n",
            ),
            (
                "a table of its own",
                "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                 [dev-dependencies.other]\nversion = \"1\"\n[dev-dependencies.tidy]\n\
                 version = \"1\"\n",
                "[package]\nname = \"c\"\n[dev-dependencies.other]\nversion = \"1\"\n",
            ),
            (
                "inherited from the workspace",
                "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                 [dev-dependencies]\ntidy = { workspace = true }\n\
                 [workspace.dependencies]\ntidy = \"1\"\n",
                "[package]\nname = \"c\"\n[workspace.dependencies]\ntidy = \"1\"\n",
            ),
            (
                "under a target",
                "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                 [target.'cfg(unix)'.dev-dependencies]\ntidy = \"1\"\nother = \"1\"\n",
                "[package]\nname = \"c\"\n[target.'cfg(unix)'.dev-dependencies]\nother = \"1\"\n",
            ),
            (
                "in several tables",
                "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                 [dependencies]\ntidy = \"1\"\nother = \"1\"\n\
                 [build-dependencies]\ntidy = \"1\"\n\
                 [dev_dependencies]\ntidy = \"1\"\nx = \"1\"\n",
                "[package]\nname = \"c\"\n[dependencies]\nother = \"1\"\n\
                 [dev_dependencies]\nx = \"1\"\n",
            ),
        ];

        for (shape, manifest, left) in cases {
            assert_eq!(removed(manifest).as_deref(), Ok(left), "{shape}");
        }
    }

    #[test]
    fn a_dependency_table_left_empty_goes_with_its_header_under_a_target_too() {
        let manifest = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [target.'cfg(unix)'.dev-dependencies]\ntidy = \"1\"\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok("[package]\nname = \"c\"\n")
        );
    }

    #[test]
    fn a_comment_on_the_removed_entry_goes_with_it_and_other_comments_stay() {
        let manifest = "# the package\n[package]\nname = \"c\"\n\n\
                        [package.metadata.skeletons.tidy]\n\n\
                        # the tools\n[dev-dependencies]\n# keeps the code tidy\ntidy = \"1\"\n\
                        # the other\nother = \"1\"\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok("# the package\n[package]\nname = \"c\"\n\n\
                # the tools\n[dev-dependencies]\n# the other\nother = \"1\"\n")
        );
    }

    #[test]
    fn a_wearing_table_is_removed_whatever_it_holds() {
        let manifest = "[package]\nname = \"c\"\n\n[package.metadata.skeletons.tidy]\n\
                        some = \"value\"\n\n[package.metadata.skeletons.tidy.options]\n\
                        a = 1\n\n[dev-dependencies]\ntidy = \"1\"\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok("[package]\nname = \"c\"\n")
        );
    }

    #[test]
    fn a_sibling_wearing_and_the_tables_above_it_stay() {
        let manifest = "[package]\nname = \"c\"\n\n[package.metadata.skeletons.other]\n\n\
                        [package.metadata.skeletons.tidy]\n\n[dev-dependencies]\n\
                        other = \"1\"\ntidy = \"1\"\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok(
                "[package]\nname = \"c\"\n\n[package.metadata.skeletons.other]\n\n\
                [dev-dependencies]\nother = \"1\"\n"
            )
        );
    }

    #[test]
    fn metadata_that_held_more_than_the_wearing_keeps_what_else_it_held() {
        let manifest = "[package]\nname = \"c\"\n\n[package.metadata.ritual]\ntasks = []\n\n\
                        [package.metadata.skeletons.tidy]\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok("[package]\nname = \"c\"\n\n[package.metadata.ritual]\ntasks = []\n")
        );
    }

    #[test]
    fn a_wearing_written_inside_inline_tables_is_taken_out_of_them() {
        let manifest = "package = { name = \"c\", metadata = { skeletons = { tidy = {} } } }\n\
                        [dev-dependencies]\ntidy = \"1\"\n";

        let rewritten = removed(manifest).expect("an inline wearing can be taken out");

        let parsed: toml::Table = rewritten.parse().expect("the result is TOML");
        assert_eq!(
            parsed,
            "package = { name = \"c\" }\n"
                .parse::<toml::Table>()
                .expect("TOML")
        );
    }

    #[test]
    fn a_manifest_with_no_wearing_table_for_the_key_is_a_defect_whatever_else_it_holds() {
        for manifest in [
            PACKAGE.to_owned(),
            format!("{PACKAGE}\n[dev-dependencies]\ntidy = \"1\"\n"),
            format!("{PACKAGE}\n[package.metadata.skeletons.other]\n"),
            format!("{PACKAGE}\n[package.metadata]\nskeletons = 5\n"),
            "package = 5\n".to_owned(),
            String::new(),
        ] {
            assert_eq!(
                removed(&manifest),
                Err(ManifestRefusal::Defect {
                    detail: "the manifest has no wearing table to take out".to_owned()
                }),
                "{manifest:?}"
            );
        }
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_unreadable() {
        let Err(ManifestRefusal::Unreadable { detail }) = removed("[package\nname =") else {
            panic!("a manifest that is not TOML must be refused as unreadable");
        };

        assert!(!detail.is_empty(), "the refusal must say what was wrong");
    }

    #[test]
    fn the_check_accepts_the_original_less_exactly_the_wearing() {
        let original = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [dev-dependencies]\ntidy = \"1\"\nother = \"1\"\n";

        assert_eq!(
            verify(
                original,
                "[package]\nname = \"c\"\n[dev-dependencies]\nother = \"1\"\n",
                "tidy"
            ),
            Ok(())
        );
    }

    #[test]
    fn the_check_refuses_an_edit_that_leaves_something_out_puts_something_back_or_changes_a_value()
    {
        // Each is the right edit with one thing wrong, and each is a defect
        // because it is not the original less the wearing: an unrelated
        // dependency lost, the dependency kept, the wearing table kept, a
        // value changed, everything lost, and an edit that is not TOML.
        let original = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [dev-dependencies]\ntidy = \"1\"\nother = \"1\"\n";
        let right = "[package]\nname = \"c\"\n[dev-dependencies]\nother = \"1\"\n";

        for (what, rewritten) in [
            ("an unrelated dependency lost", "[package]\nname = \"c\"\n"),
            (
                "the dependency kept",
                "[package]\nname = \"c\"\n[dev-dependencies]\ntidy = \"1\"\nother = \"1\"\n",
            ),
            (
                "the wearing table kept",
                "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                 [dev-dependencies]\nother = \"1\"\n",
            ),
            (
                "a value changed",
                "[package]\nname = \"d\"\n[dev-dependencies]\nother = \"1\"\n",
            ),
            ("everything lost", ""),
            ("not TOML", "[package\n"),
        ] {
            assert!(
                matches!(
                    verify(original, rewritten, "tidy"),
                    Err(ManifestRefusal::Defect { .. })
                ),
                "{what}"
            );
        }
        assert_eq!(verify(original, right, "tidy"), Ok(()));
    }

    /// The text `cargo add` leaves a manifest in, for the dependency `key`
    /// declared in the shape numbered `shape`, on top of `base`.
    fn declaring(base: &str, key: &str, shape: usize) -> String {
        match shape {
            0 => format!("{base}\n[dev-dependencies]\n{key} = \"1\"\n"),
            1 => format!("{base}\n[dev-dependencies]\nother = \"1\"\n{key} = \"1\"\n"),
            2 => format!("dev-dependencies.{key} = \"1\"\n{base}"),
            3 => format!("{base}\n[target.'cfg(unix)'.dev-dependencies]\n{key} = \"1\"\n"),
            4 => format!(
                "{base}\n[dependencies]\n{key} = \"1\"\n\n[build-dependencies]\n{key} = \"1\"\n"
            ),
            5 => format!("{base}\n[dev-dependencies.{key}]\nworkspace = true\n"),
            6 => format!(
                "{base}\n[dependencies]\n{key} = {{ workspace = true }}\n\n\
                 [workspace.dependencies]\n{key} = \"1\"\n"
            ),
            _ => unreachable!("a test passes only the shapes there are"),
        }
    }

    /// How many shapes [`declaring`] has.
    const DECLARING_SHAPES: usize = 7;

    /// Whether `table` declares a dependency under `key` in any dependency
    /// table of the package, or holds a wearing table for it.
    fn mentions(table: &toml::Table, key: &str) -> bool {
        let names = super::DEPENDENCY_TABLES;
        let declares = |table: &toml::Table| {
            names.iter().any(|name| {
                table
                    .get(*name)
                    .and_then(toml::Value::as_table)
                    .is_some_and(|dependencies| dependencies.contains_key(key))
            })
        };
        let in_target = table
            .get("target")
            .and_then(toml::Value::as_table)
            .is_some_and(|targets| {
                targets
                    .values()
                    .filter_map(toml::Value::as_table)
                    .any(declares)
            });
        let wearing = table
            .get("package")
            .and_then(|package| package.get("metadata"))
            .and_then(|metadata| metadata.get("skeletons"))
            .is_some_and(|skeletons| skeletons.get(key).is_some());
        declares(table) || in_target || wearing
    }

    const BASES: [&str; 4] = [
        "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n",
        "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[package.metadata.ritual]\n\
         tasks = [\"a\"]\n",
        "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[package.metadata.skeletons.other]\n",
        "# leading comment\n[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[profile.release]\n\
         lto = true\n# trailing comment",
    ];

    proptest! {
        // Property: for any key in the grammar, any base manifest and any shape
        // the dependency is declared in, wearing the key and then removing it
        // gives a manifest that parses, mentions the key nowhere, and equals
        // the manifest before the dependency was declared in value, apart from
        // tables that exist only to hold the dependency or the wearing. The
        // input space is keys across the whole grammar up to its 64-byte
        // limit, with `other` left out because the bases already use it.
        #[test]
        fn wearing_a_key_and_removing_it_changes_nothing_but_those_entries(
            text in "[a-zA-Z_][a-zA-Z0-9_-]{0,63}",
            base in 0..BASES.len(),
            shape in 0..DECLARING_SHAPES,
        ) {
            prop_assume!(text != "other");
            let declared = declaring(BASES[base], &text, shape);
            let wearing = worn(&declared, &text);
            // The positive control: the key is there to be taken out, so a
            // removal that finds nothing cannot pass for one that did.
            let present: toml::Table = wearing
                .parse()
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert!(mentions(&present, &text), "the key is not in {wearing}");

            let rewritten = without_wearing(&wearing, &text)
                .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;

            let mut after: toml::Table = rewritten
                .parse()
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert!(!mentions(&after, &text), "the key is still there in {rewritten}");
            let mut before: toml::Table = BASES[base]
                .parse()
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            // `declaring` puts a table of the key's own at `dev-dependencies`
            // in shape 1, with a second entry, and a workspace table in shape
            // 6; both are less than the bases, so they are taken out of the
            // result and the base alike.
            for table in [&mut after, &mut before] {
                table.remove("workspace");
                if let Some(dependencies) = table
                    .get_mut("dev-dependencies")
                    .and_then(toml::Value::as_table_mut)
                {
                    dependencies.remove("other");
                    if dependencies.is_empty() {
                        table.remove("dev-dependencies");
                    }
                }
            }
            prop_assert_eq!(after, before);
        }
    }
}
