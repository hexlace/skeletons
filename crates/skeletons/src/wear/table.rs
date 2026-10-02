//! Writing the empty `[package.metadata.skeletons.<key>]` table into the
//! command line's manifest, in place.
//!
//! The manifest is edited with `toml_edit`, which keeps every comment, blank
//! line and ordering the wearer wrote, and the result is checked against an
//! independent parse of the manifest before it is kept: it must hold the one
//! new empty table and nothing else different.
//!
//! Where the table lands is `toml_edit`'s own placement, not something this
//! module decides: after the last `[package…]` table, with one blank line
//! around it. That is right after `[package]`, or after
//! `[package.metadata.ritual]` or a sibling wearing table.

use std::path::Path;

use rituals::Outcome;
use rituals_compose::rollback::Changes;
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

use super::refusal::{Parent, WearRefusal};
use super::request::Key;

/// The tables a wearing table sits under, from the outermost in.
const PARENTS: [Parent; 3] = [Parent::Package, Parent::Metadata, Parent::Skeletons];

/// How the tables on the way to the wearing table are written in the
/// manifest: each under a header of its own, or inside an inline table, which
/// a header cannot reach into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableStyle {
    Headers,
    Inline,
}

impl TableStyle {
    /// The style a table made inside `item` has to be written in.
    fn inside(item: &Item) -> Self {
        if item.is_inline_table() {
            Self::Inline
        } else {
            Self::Headers
        }
    }
}

/// Why the wearing table could not be added to a manifest's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TableRefusal {
    /// A table the wearing table goes under is present and not a table.
    ParentNotATable(Parent),
    /// A wearing table for the key is already there.
    KeyPresent,
    /// The manifest is not TOML this module can read.
    Unreadable { detail: String },
    /// The edited manifest is not the original plus one empty table, for the
    /// reason `detail` gives.
    Defect { detail: String },
}

impl TableRefusal {
    /// This refusal as the [`WearRefusal`] that words it, for the manifest
    /// shown as `manifest` and the key `key`.
    pub(crate) fn into_refusal(self, manifest: &str, key: &Key) -> WearRefusal {
        let manifest = manifest.to_owned();
        let key = key.as_str().to_owned();
        match self {
            Self::ParentNotATable(parent) => WearRefusal::ParentNotATable { manifest, parent },
            Self::KeyPresent => WearRefusal::TableWithoutDependency { manifest, key },
            Self::Unreadable { detail } => WearRefusal::ManifestUnreadable { manifest, detail },
            Self::Defect { detail } => WearRefusal::TableDefect {
                manifest,
                key,
                detail,
            },
        }
    }
}

/// `manifest` with an empty `[package.metadata.skeletons.<key>]` table added.
///
/// A `package`, `metadata` or `skeletons` that is missing is created as an
/// implicit table, so no header is written for it. One that is an inline table
/// gets the new table as an empty inline table inside it, since a header
/// cannot reach into one.
///
/// # Errors
///
/// Returns [`TableRefusal`] when a table on the way is not a table, when the
/// key already has an entry, when the manifest cannot be read, or when what
/// was written is not the manifest as it was plus the one empty table.
pub(crate) fn with_empty_wearing_table(manifest: &str, key: &Key) -> Result<String, TableRefusal> {
    let mut document: DocumentMut =
        manifest
            .parse()
            .map_err(|error: toml_edit::TomlError| TableRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    insert_empty_table(&mut document, key)?;
    let rewritten = document.to_string();
    verify(manifest, &rewritten, key)?;
    Ok(rewritten)
}

/// Descends to `package.metadata.skeletons`, creating what is missing, and
/// inserts the empty table under `key` there.
fn insert_empty_table(document: &mut DocumentMut, key: &Key) -> Result<(), TableRefusal> {
    let mut table: &mut dyn TableLike = document.as_table_mut();
    let mut style = TableStyle::Headers;
    for parent in PARENTS {
        let item = table
            .entry(parent.key())
            .or_insert(empty_parent_table(style));
        style = TableStyle::inside(item);
        table = item
            .as_table_like_mut()
            .ok_or(TableRefusal::ParentNotATable(parent))?;
    }
    if table.contains_key(key.as_str()) {
        return Err(TableRefusal::KeyPresent);
    }
    table.insert(key.as_str(), empty_wearing_table(style));
    Ok(())
}

/// A table created only to hold what goes under it, which prints no header of
/// its own, or an empty inline table when it is made inside one.
fn empty_parent_table(style: TableStyle) -> Item {
    match style {
        TableStyle::Inline => Item::Value(Value::InlineTable(InlineTable::new())),
        TableStyle::Headers => {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        }
    }
}

/// The wearing table itself: empty, with its own header, or `{}` inside an
/// inline table.
fn empty_wearing_table(style: TableStyle) -> Item {
    match style {
        TableStyle::Inline => Item::Value(Value::InlineTable(InlineTable::new())),
        TableStyle::Headers => Item::Table(Table::new()),
    }
}

/// Checks `rewritten` against `original`, parsed by `toml` rather than by the
/// library that wrote it: equal in value, except for the one empty table at
/// `package.metadata.skeletons.<key>`.
fn verify(original: &str, rewritten: &str, key: &Key) -> Result<(), TableRefusal> {
    let mut expected: toml::Table =
        original
            .parse()
            .map_err(|error: toml::de::Error| TableRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    put_empty_table(&mut expected, key).ok_or_else(|| TableRefusal::Defect {
        detail: "a table the new one goes under is not a table".to_owned(),
    })?;
    let actual: toml::Table =
        rewritten
            .parse()
            .map_err(|error: toml::de::Error| TableRefusal::Defect {
                detail: format!("the edited manifest is not TOML: {error}"),
            })?;
    if expected == actual {
        Ok(())
    } else {
        Err(TableRefusal::Defect {
            detail: "the edited manifest differs from the original by more than the new table"
                .to_owned(),
        })
    }
}

/// Puts an empty table at `package.metadata.skeletons.<key>` in `table`, or
/// returns `None` when a table on the way is not a table.
fn put_empty_table(table: &mut toml::Table, key: &Key) -> Option<()> {
    let mut current = table;
    for parent in PARENTS {
        current = current
            .entry(parent.key())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()?;
    }
    current.insert(
        key.as_str().to_owned(),
        toml::Value::Table(toml::Table::new()),
    );
    Some(())
}

/// Adds the empty wearing table for `key` to the manifest at `manifest_path`,
/// through `changes`.
///
/// The manifest is read as `cargo add` left it, which `changes` already has
/// the original of, since `cargo add` ran through it. `manifest` is the path
/// as a message shows it.
///
/// # Errors
///
/// Returns a failure saying why the manifest could not be read, why the
/// table could not be added, or why the file could not be written.
pub(crate) fn write(
    changes: &mut Changes,
    manifest_path: &Path,
    manifest: &str,
    key: &Key,
) -> Outcome {
    let before = std::fs::read_to_string(manifest_path).map_err(|error| {
        WearRefusal::ManifestUnreadable {
            manifest: manifest.to_owned(),
            detail: error.to_string(),
        }
        .into_failure()
    })?;
    let after = with_empty_wearing_table(&before, key)
        .map_err(|refusal| refusal.into_refusal(manifest, key).into_failure())?;
    changes.write(manifest_path, after)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rituals_compose::rollback;

    use super::{TableRefusal, with_empty_wearing_table, write};
    use crate::wear::refusal::{Parent, WearRefusal};
    use crate::wear::request::Key;

    fn key(text: &str) -> Key {
        Key::new(text).expect("a test passes only keys it knows are valid")
    }

    fn with_table(manifest: &str) -> Result<String, TableRefusal> {
        with_empty_wearing_table(manifest, &key("tidy"))
    }

    const PACKAGE: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n";

    // The four shapes a manifest's package metadata takes, and the exact bytes
    // each comes out as. Placement is `toml_edit`'s: after the last
    // `[package...]` table, one blank line either side. Nothing documents it,
    // so these pin it, and a red here after a dependency bump is that
    // dependency changing, not `wear`.

    #[test]
    fn with_no_metadata_the_table_goes_right_after_package() {
        let manifest = format!("{PACKAGE}\n[dependencies]\nx = \"1\"\n");

        assert_eq!(
            with_table(&manifest).as_deref(),
            Ok("[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                [package.metadata.skeletons.tidy]\n\n\
                [dependencies]\nx = \"1\"\n")
        );
    }

    #[test]
    fn beside_ritual_metadata_the_table_goes_after_it() {
        let manifest =
            format!("{PACKAGE}\n[package.metadata.ritual]\ntasks = [\"a\"]\n\n[dependencies]\n");

        assert_eq!(
            with_table(&manifest).as_deref(),
            Ok("[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                [package.metadata.ritual]\ntasks = [\"a\"]\n\n\
                [package.metadata.skeletons.tidy]\n\n\
                [dependencies]\n")
        );
    }

    #[test]
    fn beside_a_sibling_wearing_table_the_table_goes_after_it() {
        let manifest =
            format!("{PACKAGE}\n[package.metadata.skeletons.other]\n\n[dev-dependencies]\n");

        assert_eq!(
            with_table(&manifest).as_deref(),
            Ok("[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                [package.metadata.skeletons.other]\n\n\
                [package.metadata.skeletons.tidy]\n\n\
                [dev-dependencies]\n")
        );
    }

    #[test]
    fn beside_a_dotted_sibling_under_package_the_table_goes_after_package() {
        let manifest =
            "[package]\nname = \"cli\"\nmetadata.skeletons.other = {}\n\n[dev-dependencies]\n";

        assert_eq!(
            with_table(manifest).as_deref(),
            Ok(
                "[package]\nname = \"cli\"\nmetadata.skeletons.other = {}\n\n\
                [package.metadata.skeletons.tidy]\n\n\
                [dev-dependencies]\n"
            )
        );
    }

    #[test]
    fn a_manifest_with_nothing_after_package_gains_the_table_at_the_end() {
        assert_eq!(
            with_table(PACKAGE).as_deref(),
            Ok("[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                [package.metadata.skeletons.tidy]\n")
        );
    }

    #[test]
    fn a_trailing_comment_and_a_missing_final_newline_are_kept() {
        let manifest = "[package]\nname = \"cli\"\n# last word, no newline";

        assert_eq!(
            with_table(manifest).as_deref(),
            Ok("[package]\nname = \"cli\"\n\n\
                [package.metadata.skeletons.tidy]\n# last word, no newline")
        );
    }

    #[test]
    fn an_inline_metadata_parent_gains_an_empty_inline_table_inside_it() {
        // A header cannot reach into an inline table, so the table is written
        // as `tidy = {}` inside `skeletons`, inside `metadata`. `toml_edit`
        // spaces the new key after the existing entry's own trailing space.
        let manifest = format!("{PACKAGE}metadata = {{ ritual = {{ tasks = [] }} }}\n");

        assert_eq!(
            with_table(&manifest).as_deref(),
            Ok("[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\
                metadata = { ritual = { tasks = [] } , skeletons = { tidy = {} } }\n")
        );
    }

    #[test]
    fn an_inline_package_gains_metadata_inside_it() {
        let manifest = "package = { name = \"cli\", version = \"0.1.0\" }\n";

        assert_eq!(
            with_table(manifest).as_deref(),
            Ok("package = { name = \"cli\", version = \"0.1.0\" , \
                metadata = { skeletons = { tidy = {} } } }\n")
        );
    }

    #[test]
    fn an_inline_skeletons_table_gains_the_key_inside_it() {
        let manifest = format!("{PACKAGE}metadata.skeletons = {{ other = {{}} }}\n");

        let rewritten = with_table(&manifest).expect("an inline skeletons table takes a key");

        assert!(
            rewritten.contains("skeletons = { other = {} , tidy = {} }"),
            "the key must be written inside the inline table; it was:\n{rewritten}"
        );
    }

    #[test]
    fn a_skeletons_entry_that_is_not_a_table_is_refused() {
        let manifest = format!("{PACKAGE}metadata.skeletons = 5\n");

        assert_eq!(
            with_table(&manifest),
            Err(TableRefusal::ParentNotATable(Parent::Skeletons))
        );
    }

    #[test]
    fn a_metadata_entry_that_is_not_a_table_is_refused() {
        let manifest = format!("{PACKAGE}metadata = \"notes\"\n");

        assert_eq!(
            with_table(&manifest),
            Err(TableRefusal::ParentNotATable(Parent::Metadata))
        );
    }

    #[test]
    fn a_package_entry_that_is_not_a_table_is_refused() {
        assert_eq!(
            with_table("package = 5\n"),
            Err(TableRefusal::ParentNotATable(Parent::Package))
        );
    }

    #[test]
    fn a_key_that_already_has_an_entry_is_refused_whatever_its_shape() {
        for existing in [
            "[package.metadata.skeletons.tidy]\n",
            "[package.metadata.skeletons]\ntidy = \"text\"\n",
            "[package.metadata.skeletons]\ntidy = {}\n",
        ] {
            let manifest = format!("{PACKAGE}\n{existing}");
            assert_eq!(
                with_table(&manifest),
                Err(TableRefusal::KeyPresent),
                "{existing:?}"
            );
        }
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_unreadable() {
        let Err(TableRefusal::Unreadable { detail }) = with_table("[package\nname =") else {
            panic!("a manifest that is not TOML must be refused as unreadable");
        };

        assert!(!detail.is_empty(), "the refusal must say what was wrong");
    }

    #[test]
    fn each_refusal_reads_as_the_message_that_words_it() {
        let shown = "crates/cli/Cargo.toml";
        let tidy = key("tidy");
        let expected = [
            (
                TableRefusal::ParentNotATable(Parent::Metadata),
                WearRefusal::ParentNotATable {
                    manifest: shown.to_owned(),
                    parent: Parent::Metadata,
                },
            ),
            (
                TableRefusal::KeyPresent,
                WearRefusal::TableWithoutDependency {
                    manifest: shown.to_owned(),
                    key: "tidy".to_owned(),
                },
            ),
            (
                TableRefusal::Unreadable {
                    detail: "bad".to_owned(),
                },
                WearRefusal::ManifestUnreadable {
                    manifest: shown.to_owned(),
                    detail: "bad".to_owned(),
                },
            ),
            (
                TableRefusal::Defect {
                    detail: "differs".to_owned(),
                },
                WearRefusal::TableDefect {
                    manifest: shown.to_owned(),
                    key: "tidy".to_owned(),
                    detail: "differs".to_owned(),
                },
            ),
        ];

        for (refusal, wording) in expected {
            assert_eq!(refusal.into_refusal(shown, &tidy), wording);
        }
    }

    #[test]
    fn write_puts_the_table_into_the_file_through_changes() {
        // The table is added to the manifest as it stands on disk, and a run
        // that then fails gets the original bytes back, which shows the write
        // went through the recorder and not around it.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let manifest_path = directory.path().join("Cargo.toml");
        std::fs::write(&manifest_path, PACKAGE).expect("write the manifest");
        let tidy = key("tidy");

        let outcome = rollback::attempt("running again", |changes| {
            write(changes, &manifest_path, "Cargo.toml", &tidy)?;
            let written = std::fs::read_to_string(&manifest_path).expect("read it back");
            assert!(written.contains("[package.metadata.skeletons.tidy]"));
            Err::<(), _>(rituals::Failure::new("a later step failed"))
        });

        assert!(outcome.is_err());
        assert_eq!(
            std::fs::read_to_string(&manifest_path).expect("read it back"),
            PACKAGE,
            "a failed run must put the original manifest back"
        );
    }

    #[test]
    fn write_refuses_a_missing_manifest_as_unreadable_and_writes_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let manifest_path = directory.path().join("Cargo.toml");

        let outcome = rollback::attempt("running again", |changes| {
            write(changes, &manifest_path, "Cargo.toml", &key("tidy"))
        });

        let message = outcome
            .expect_err("a missing manifest cannot take a table")
            .to_string();
        assert!(
            message.starts_with("wear could not read Cargo.toml to add the wearing table: "),
            "{message}"
        );
        assert!(!manifest_path.exists(), "nothing may be written");
    }

    /// Manifests with every shape the package metadata can take, each with a
    /// sibling wearing table called `other` where it has one.
    fn manifests() -> Vec<String> {
        vec![
            format!("{PACKAGE}\n[dependencies]\nx = \"1\"\n"),
            format!("{PACKAGE}\n[package.metadata.ritual]\ntasks = [\"a\"]\n"),
            format!("{PACKAGE}\n[package.metadata.skeletons.other]\n"),
            format!("{PACKAGE}metadata.skeletons.other = {{}}\n"),
            format!("{PACKAGE}metadata = {{ ritual = {{ tasks = [] }} }}\n"),
            format!("{PACKAGE}metadata.skeletons = {{ other = {{}} }}\n"),
            "package = { name = \"cli\", version = \"0.1.0\" }\n".to_owned(),
            format!("# leading comment\n{PACKAGE}# trailing comment"),
        ]
    }

    /// Removes the empty table at `package.metadata.skeletons.<key>` from
    /// `table`, then every table the removal left empty and that the original
    /// did not have, and returns whether the empty table was there.
    fn without_the_wearing_table(table: &mut toml::Table, key: &str) -> bool {
        let Some(skeletons) = table
            .get_mut("package")
            .and_then(|package| package.get_mut("metadata"))
            .and_then(|metadata| metadata.get_mut("skeletons"))
            .and_then(toml::Value::as_table_mut)
        else {
            return false;
        };
        skeletons.remove(key) == Some(toml::Value::Table(toml::Table::new()))
    }

    proptest! {
        // Property: for any key in the grammar and every shape of package
        // metadata, the output parses, holds exactly one new empty table at
        // the key, and with that table removed equals the manifest it came
        // from in value, apart from `skeletons` and `metadata` tables that
        // exist only to hold it. The input space is keys across the whole
        // grammar, up to its 64-byte limit, and a manifest of every shape.
        #[test]
        fn the_output_gains_exactly_one_empty_table_and_nothing_else_changes(
            text in "[a-zA-Z_][a-zA-Z0-9_-]{0,63}",
            shape in 0..manifests().len(),
        ) {
            prop_assume!(text != "other");
            gains_exactly_the_empty_table(&text, &manifests()[shape])?;
        }
    }

    /// Adds the wearing table for `text` to `manifest` and checks the result
    /// as the property above says.
    fn gains_exactly_the_empty_table(text: &str, manifest: &str) -> Result<(), TestCaseError> {
        let fail = |what: &dyn std::fmt::Debug| TestCaseError::fail(format!("{what:?}"));
        let rewritten = with_empty_wearing_table(manifest, &key(text)).map_err(|r| fail(&r))?;
        let mut after: toml::Table = rewritten.parse().map_err(|e| fail(&e))?;
        let before: toml::Table = manifest.parse().map_err(|e| fail(&e))?;

        prop_assert!(
            without_the_wearing_table(&mut after, text),
            "no empty table at the key in {rewritten}"
        );
        prune_empty(&mut after, &before);
        prop_assert_eq!(after, before);
        Ok(())
    }

    /// Removes `package.metadata.skeletons`, then `package.metadata`, from
    /// `after` when each is empty and `before` has no such table.
    fn prune_empty(after: &mut toml::Table, before: &toml::Table) {
        let had = |path: &[&str]| {
            let mut current = before;
            for name in path {
                match current.get(*name).and_then(toml::Value::as_table) {
                    Some(next) => current = next,
                    None => return false,
                }
            }
            true
        };
        let Some(package) = after.get_mut("package").and_then(toml::Value::as_table_mut) else {
            return;
        };
        let Some(metadata) = package
            .get_mut("metadata")
            .and_then(toml::Value::as_table_mut)
        else {
            return;
        };
        let skeletons_empty = metadata
            .get("skeletons")
            .and_then(toml::Value::as_table)
            .is_some_and(toml::Table::is_empty);
        if skeletons_empty && !had(&["package", "metadata", "skeletons"]) {
            metadata.remove("skeletons");
        }
        if metadata.is_empty() && !had(&["package", "metadata"]) {
            package.remove("metadata");
        }
    }
}
