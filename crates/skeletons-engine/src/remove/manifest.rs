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
//! declares the key, those under `[target.<name>]` too, and a table the removal
//! leaves empty goes with its header. A table that was empty before stays, with
//! its comments, when the removal took nothing out of it. A table `wear` put the
//! key into that was empty before `wear` is empty again only because of the
//! removal, and nothing tells it from a table `wear` created, so it goes. Nothing
//! else is touched: an inherited `[workspace.dependencies]` entry stays.
//!
//! No comment the wearer wrote is lost, with one exception: a comment attached
//! to a key line that is removed. That is the comment on the lines directly
//! above the `key = value` line, the one after the value on its line, and the
//! same for every key line inside a table that goes, comments inside a
//! multi-line value included. A comment on a table's header goes nowhere with
//! the header: the comments above it and the one beside it are moved onto
//! whatever follows it in the text, the next header that stays or, when none
//! follows, the end of the file. That holds for every header that goes, those
//! of tables and arrays of tables inside the wearing table and a dependency
//! written as a table of its own (`[dev-dependencies.tidy]`) among them. A
//! comment moved off a header can end up standing alone, which the diff shows.
//! A textual check beside the one on values refuses the edit, naming the line,
//! when a comment line of the manifest is gone and is not under the exception.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, Key, RawString, Table, TableLike};

use super::names::WearingKey;

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

/// A manifest's text before an edit and after it.
#[derive(Debug, Clone, Copy)]
struct Edit<'text> {
    original: &'text str,
    rewritten: &'text str,
}

/// What an edit took out of a document besides the entries themselves: the
/// header comments that have to be moved onto what follows, and the comment
/// lines that go with removed key lines.
#[derive(Debug, Default)]
struct Removals {
    /// For each header that went: the position it had among the document's
    /// headers, and the comment text to move onto what follows it.
    carried: Vec<(usize, String)>,
    /// The comment lines that went with a removed key line, as they were
    /// written. The text check allows exactly these to be missing.
    excepted: Vec<String>,
}

impl Removals {
    /// Takes `name` out of `parent`, and records what that does to comments.
    ///
    /// Every header in the item has its comments lifted off into
    /// [`Removals::carried`] first, so what is left of the item, rendered, holds
    /// only the comments that go with its key lines, and those are recorded as
    /// [`Removals::excepted`]. Returns `None`, changing nothing, when `parent`
    /// has no `name`.
    fn take(&mut self, parent: &mut dyn TableLike, name: &str) -> Option<Item> {
        for table in tables_in(parent.get_mut(name)?) {
            visit_tables(vec![table], |table| self.lift_header(table));
        }
        let (key, item) = parent.get_key_value(name)?;
        self.excepted.extend(comment_lines(&rendered(key, item)));
        self.excepted.extend(comment_lines(&key_decor_text(key)));
        let taken = parent.remove(name);
        assert!(taken.is_some(), "the item was found a moment ago");
        taken
    }

    /// Moves the comments of `table`'s header, when it has one, into
    /// [`Removals::carried`] and leaves the header without any.
    fn lift_header(&mut self, table: &mut Table) {
        let Some(position) = table.position() else {
            return;
        };
        let decor = table.decor_mut();
        let carried = carried_text(raw_text(decor.prefix()), raw_text(decor.suffix()));
        decor.set_prefix("");
        decor.set_suffix("");
        if !carried.is_empty() {
            self.carried.push((position, carried));
        }
    }

    /// Puts each carried text onto the first header that stays after the one
    /// it came from, ahead of that header's own comments, or onto the end of
    /// the file when no header follows. Texts that land together keep the order
    /// of the headers they came from. Returns the excepted lines.
    fn rehome(self, document: &mut DocumentMut) -> Vec<String> {
        let Self {
            mut carried,
            excepted,
        } = self;
        carried.sort_by_key(|&(position, _)| position);
        let stays = header_positions(document.as_table());
        let mut landings: BTreeMap<usize, String> = BTreeMap::new();
        let mut at_the_end = String::new();
        for (position, text) in carried {
            let landing = stays.get(stays.partition_point(|&stay| stay <= position));
            match landing {
                Some(&landing) => landings.entry(landing).or_default().push_str(&text),
                None => at_the_end.push_str(&text),
            }
        }
        visit_tables(vec![document.as_table_mut()], |table| {
            let Some(carried) = table
                .position()
                .and_then(|position| landings.get(&position))
            else {
                return;
            };
            let own = raw_text(table.decor().prefix()).to_owned();
            table.decor_mut().set_prefix(format!("{carried}{own}"));
        });
        if !at_the_end.is_empty() {
            let own = raw_text(Some(document.trailing())).to_owned();
            document.set_trailing(format!("{at_the_end}{own}"));
        }
        excepted
    }
}

/// `manifest` with the dependency under `key` and the table
/// `[package.metadata.skeletons.<key>]` taken out.
///
/// The key is whatever the manifest holds, the empty key included: it is the
/// wearer's own, and an empty one is taken out like any other.
///
/// The dependency is taken out of every dependency table that declares the key,
/// and a table that the removal leaves empty is taken out with its header, as is a
/// `skeletons` or `metadata` table that only held the wearing. On a manifest
/// `wear` wrote this is its inverse: the manifest as it was before, except when
/// the dependency table `wear` put the key into was there and empty before.
/// Taking the key out empties it again, and nothing in the manifest tells it
/// from a table `wear` created, so it goes with its header. The comments on a
/// header that goes are kept, moved onto what follows it; only the comments of
/// key lines that go are lost (see the module documentation).
///
/// # Errors
///
/// Returns [`ManifestRefusal`] when the manifest cannot be read, when it has
/// no wearing table under `key`, or when what was written is not the manifest
/// as it was less the wearing, or has lost a comment it should have kept.
pub(crate) fn without_wearing(manifest: &str, key: &WearingKey) -> Result<String, ManifestRefusal> {
    let key = key.as_str();
    let mut document: DocumentMut =
        manifest
            .parse()
            .map_err(|error: toml_edit::TomlError| ManifestRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    let mut removals = Removals::default();
    take_out_wearing_table(&mut document, key, &mut removals)?;
    take_out_dependencies(document.as_table_mut(), key, &mut removals);
    let excepted = removals.rehome(&mut document);
    let rewritten = document.to_string();
    let edit = Edit {
        original: manifest,
        rewritten: &rewritten,
    };
    verify(&edit, key)?;
    comments_kept(&edit, &excepted)?;
    Ok(rewritten)
}

/// Removes `package.metadata.skeletons.<key>` from `document`, and the
/// `skeletons` and `metadata` tables above it when that leaves them empty.
fn take_out_wearing_table(
    document: &mut DocumentMut,
    key: &str,
    removals: &mut Removals,
) -> Result<(), ManifestRefusal> {
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
    removals.take(skeletons_table, key).ok_or_else(absent)?;
    if skeletons_table.is_empty() {
        removals.take(metadata_table, skeletons);
    }
    if metadata_table.is_empty() {
        removals.take(package, metadata);
    }
    Ok(())
}

/// Removes `key` from every dependency table directly in `root`, and from
/// every dependency table in a `[target.<name>]` table of it. A table goes
/// only when taking the key out is what emptied it, so one that was empty
/// before stays with its header and comments.
fn take_out_dependencies(root: &mut dyn TableLike, key: &str, removals: &mut Removals) {
    take_out_of_dependency_tables(root, key, removals);
    let Some(targets) = root.get_mut("target").and_then(Item::as_table_like_mut) else {
        return;
    };
    let was_empty = targets.is_empty();
    let mut emptied_names = Vec::new();
    for (name, target) in targets.iter_mut() {
        if let Some(target) = target.as_table_like_mut() {
            if take_out_of_dependency_tables(target, key, removals) {
                emptied_names.push(name.to_owned());
            }
        }
    }
    for name in emptied_names {
        removals.take(targets, &name);
    }
    if !was_empty && targets.is_empty() {
        removals.take(root, "target");
    }
}

/// Removes `key` from each of `parent`'s dependency tables, and each one that
/// holding the key made non-empty and removing it left empty. Returns whether
/// `parent` itself went from holding something to holding nothing.
fn take_out_of_dependency_tables(
    parent: &mut dyn TableLike,
    key: &str,
    removals: &mut Removals,
) -> bool {
    let was_empty = parent.is_empty();
    for name in DEPENDENCY_TABLES {
        let emptied = parent
            .get_mut(name)
            .and_then(Item::as_table_like_mut)
            .is_some_and(|table| removals.take(table, key).is_some() && table.is_empty());
        if emptied {
            removals.take(parent, name);
        }
    }
    !was_empty && parent.is_empty()
}

/// The tables `item` is: itself when it is a table, each of its entries when it
/// is an array of tables, and none otherwise.
fn tables_in(item: &mut Item) -> Vec<&mut Table> {
    match item {
        Item::Table(table) => vec![table],
        Item::ArrayOfTables(tables) => tables.iter_mut().collect(),
        Item::Value(_) | Item::None => Vec::new(),
    }
}

/// Calls `visit` on each of `pending` and on every table and array-of-tables
/// entry nested in them, parents before children, with an explicit stack.
fn visit_tables(mut pending: Vec<&mut Table>, mut visit: impl FnMut(&mut Table)) {
    while let Some(table) = pending.pop() {
        visit(&mut *table);
        for (_name, child) in table.iter_mut() {
            pending.extend(tables_in(child));
        }
    }
}

/// The positions, in ascending order, of every table in `root` that has a
/// header: implicit and dotted tables have none.
fn header_positions(root: &Table) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut pending = vec![root];
    while let Some(table) = pending.pop() {
        positions.extend(table.position());
        for (_name, child) in table {
            match child {
                Item::Table(table) => pending.push(table),
                Item::ArrayOfTables(tables) => pending.extend(tables.iter()),
                Item::Value(_) | Item::None => {}
            }
        }
    }
    positions.sort_unstable();
    positions
}

/// The text of `raw`, empty when there is none.
fn raw_text(raw: Option<&RawString>) -> &str {
    raw.and_then(RawString::as_str).unwrap_or_default()
}

/// The comment text a header carries: its prefix up to and including the last
/// comment line it holds, blank lines before that kept and whatever follows
/// dropped, then the comment after the header on its line as a line of its own.
fn carried_text(prefix: &str, suffix: &str) -> String {
    let mut end = 0;
    let mut offset = 0;
    for line in prefix.split_inclusive('\n') {
        offset += line.len();
        if line.trim_start().starts_with('#') {
            end = offset;
        }
    }
    let mut text = prefix[..end].to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let beside = suffix.trim();
    if beside.starts_with('#') {
        text.push_str(beside);
        text.push('\n');
    }
    text
}

/// The text `item` prints as under `key`, in a document of its own.
fn rendered(key: &Key, item: &Item) -> String {
    let mut document = DocumentMut::new();
    document.as_table_mut().insert_formatted(key, item.clone());
    document.to_string()
}

/// The text of every decor `key` holds, one after another.
fn key_decor_text(key: &Key) -> String {
    let mut text = String::new();
    for decor in [key.leaf_decor(), key.dotted_decor()] {
        text.push_str(raw_text(decor.prefix()));
        text.push('\n');
        text.push_str(raw_text(decor.suffix()));
        text.push('\n');
    }
    text
}

/// The lines of `text` that are comments: those whose trimmed text starts
/// with `#`.
fn comment_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(str::to_owned)
}

/// Checks that every comment line of the edit's `original` is still a line of
/// its `rewritten`, counted as a multiset, except the lines in `excepted`, which
/// went with removed key lines. A line is a comment when its trimmed text starts
/// with `#`. This is a check on the text, beside [`verify`]'s on the values,
/// because the values cannot see a comment.
fn comments_kept(edit: &Edit<'_>, excepted: &[String]) -> Result<(), ManifestRefusal> {
    let mut allowed_gone: BTreeMap<&str, usize> = BTreeMap::new();
    for line in excepted {
        *allowed_gone.entry(line).or_default() += 1;
    }
    let mut present: BTreeMap<&str, usize> = BTreeMap::new();
    for line in edit.rewritten.lines() {
        *present.entry(line).or_default() += 1;
    }
    for line in edit.original.lines() {
        if !line.trim_start().starts_with('#') {
            continue;
        }
        let allowance = allowed_gone.get_mut(line).filter(|count| **count > 0);
        if let Some(count) = allowance {
            *count -= 1;
            continue;
        }
        match present.get_mut(line).filter(|count| **count > 0) {
            Some(count) => *count -= 1,
            None => {
                return Err(ManifestRefusal::Defect {
                    detail: format!("the edited manifest lost the comment line {line:?}"),
                });
            }
        }
    }
    Ok(())
}

/// Checks the edit's `rewritten` against its `original`, parsed by `toml`
/// rather than by the library that wrote it: equal in value to the original
/// less the dependency under `key`, the wearing table, and exactly the tables
/// that removing those emptied. Nothing is pruned from the rewritten side, so
/// an empty table the original held that is missing from it, or one the
/// removal emptied that is still in it, is a difference.
fn verify(edit: &Edit<'_>, key: &str) -> Result<(), ManifestRefusal> {
    let mut expected: toml::Table =
        edit.original
            .parse()
            .map_err(|error: toml::de::Error| ManifestRefusal::Unreadable {
                detail: error.to_string(),
            })?;
    take_out_of_value(&mut expected, key).ok_or_else(|| ManifestRefusal::Defect {
        detail: "the manifest has no wearing table to take out".to_owned(),
    })?;
    let actual: toml::Table =
        edit.rewritten
            .parse()
            .map_err(|error: toml::de::Error| ManifestRefusal::Defect {
                detail: format!("the edited manifest is not TOML: {error}"),
            })?;
    if expected == actual {
        Ok(())
    } else {
        Err(ManifestRefusal::Defect {
            detail: "the edited manifest differs from the original by more than the wearing"
                .to_owned(),
        })
    }
}

/// Takes the wearing table and the dependency under `key` out of `table`, and
/// each table that held something before and holds nothing after, or returns
/// `None` when there is no wearing table to take out.
fn take_out_of_value(table: &mut toml::Table, key: &str) -> Option<()> {
    let [package, metadata, skeletons] = WEARING_PATH;
    let package = table.get_mut(package)?.as_table_mut()?;
    let metadata_table = package.get_mut(metadata)?.as_table_mut()?;
    let skeletons_table = metadata_table.get_mut(skeletons)?.as_table_mut()?;
    skeletons_table.remove(key)?;
    if skeletons_table.is_empty() {
        metadata_table.remove(skeletons);
    }
    if metadata_table.is_empty() {
        package.remove(metadata);
    }
    take_out_of_dependency_values(table, key);
    if let Some(targets) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        let held_something = !targets.is_empty();
        targets.retain(|_name, target| {
            target
                .as_table_mut()
                .is_none_or(|target| !take_out_of_dependency_values(target, key))
        });
        if held_something && targets.is_empty() {
            table.remove("target");
        }
    }
    Some(())
}

/// Removes `key` from each dependency table directly in `table`, and each of
/// those the removal left empty. Returns whether `table` held something before
/// and holds nothing after.
fn take_out_of_dependency_values(table: &mut toml::Table, key: &str) -> bool {
    let held_something = !table.is_empty();
    for name in DEPENDENCY_TABLES {
        let emptied = table
            .get_mut(name)
            .and_then(toml::Value::as_table_mut)
            .is_some_and(|dependencies| {
                dependencies.remove(key).is_some() && dependencies.is_empty()
            });
        if emptied {
            table.remove(name);
        }
    }
    held_something && table.is_empty()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Edit, ManifestRefusal, comments_kept, verify, without_wearing};
    use crate::remove::names::WearingKey;
    use crate::wear::{Key, with_empty_wearing_table};

    const PACKAGE: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n";

    /// [`verify`] over an edit of `original` into `rewritten`, for the key
    /// `tidy`.
    fn verified(original: &str, rewritten: &str) -> Result<(), ManifestRefusal> {
        verify(
            &Edit {
                original,
                rewritten,
            },
            "tidy",
        )
    }

    fn removed(manifest: &str) -> Result<String, ManifestRefusal> {
        without_wearing(manifest, &WearingKey::new("tidy"))
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
    // byte for byte. No case has a dependency table that was empty before
    // `wear`; that one does not come back, and has its own test below.
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
    fn a_dependency_table_that_was_empty_before_wear_goes_with_its_header_when_remove_empties_it() {
        // Wearing then removing is not byte for byte here. The manifest had an
        // empty `[dev-dependencies]`; `cargo add --dev --rename tidy --path
        // ../dep`, run against Cargo itself, puts the key in it. Removing the
        // key leaves the table empty, and a table the removal empties goes
        // with its header, because nothing in the manifest tells it from a
        // table `wear` created. So the manifest comes back without the table, and
        // without the blank line above its header.
        let before = format!("{PACKAGE}\n[dev-dependencies]\n");
        let after_cargo_add = format!(
            "{PACKAGE}\n[dev-dependencies]\ntidy = {{ path = \"../dep\", package = \"dep\" }}\n"
        );
        let wearing = worn(&after_cargo_add, "tidy");

        let rewritten = removed(&wearing);

        assert_eq!(rewritten.as_deref(), Ok(PACKAGE));
        assert_ne!(rewritten.as_deref(), Ok(before.as_str()));
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
    fn an_empty_table_that_was_empty_before_the_removal_stays_with_its_comments() {
        // Each manifest declares the dependency in `[dev-dependencies]` and
        // also holds an empty table the key was never in, with a comment above
        // its header and one on or after it. Removing the wearing empties only
        // `[dev-dependencies]`, so the other table, its header and its
        // comments must come through byte for byte.
        let wearing = "[package]\nname = \"c\"\n\n[package.metadata.skeletons.tidy]\n\n\
                       [dev-dependencies]\ntidy = \"1\"\n\n";
        for (what, kept) in [
            (
                "an empty dependency table",
                "# above\n[dependencies] # beside\n# after\n",
            ),
            (
                "an empty dependency table under a target",
                "# above\n[target.'cfg(unix)'.dependencies] # beside\n# after\n",
            ),
            (
                "an empty target table",
                "# above\n[target.'cfg(unix)'] # beside\n# after\n",
            ),
        ] {
            let manifest = format!("{wearing}{kept}");

            let rewritten = removed(&manifest).expect("the wearing can be taken out");

            assert!(rewritten.contains(kept), "{what}:\n{rewritten}");
        }
    }

    #[test]
    fn an_empty_metadata_neighbour_stays_when_the_wearing_goes() {
        // The wearing table is the only thing removed from `package.metadata`,
        // so an empty neighbour keeps `metadata` and itself.
        let manifest = "[package]\nname = \"c\"\n\n# the neighbour\n[package.metadata.ritual]\n\n\
                        [package.metadata.skeletons.tidy]\n";

        assert_eq!(
            removed(manifest).as_deref(),
            Ok("[package]\nname = \"c\"\n\n# the neighbour\n[package.metadata.ritual]\n")
        );
    }

    #[test]
    fn a_target_table_the_removal_emptied_goes_with_the_target_above_it() {
        let manifest = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [target.'cfg(unix)']\n[target.'cfg(unix)'.dev-dependencies]\n\
                        tidy = \"1\"\n";

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
    fn a_wearing_under_an_empty_key_is_taken_out_like_any_other() {
        let manifest = "[package]\nname = \"c\"\n\n[package.metadata.skeletons.\"\"]\n\n\
                        [dev-dependencies]\n\"\" = \"1\"\n";

        assert_eq!(
            without_wearing(manifest, &WearingKey::new("")).as_deref(),
            Ok("[package]\nname = \"c\"\n")
        );
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
            verified(
                original,
                "[package]\nname = \"c\"\n[dev-dependencies]\nother = \"1\"\n"
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
                    verified(original, rewritten),
                    Err(ManifestRefusal::Defect { .. })
                ),
                "{what}"
            );
        }
        assert_eq!(verified(original, right), Ok(()));
    }

    #[test]
    fn the_check_sees_an_empty_table_dropped_or_left_behind() {
        // The original holds an empty `[dependencies]` the key was never in,
        // and the key alone in `[dev-dependencies]`. The right result keeps the
        // first and drops the second. Each wrong result is the right one with
        // one empty table dropped that this run did not empty, or kept that it
        // did, and each must be refused.
        let original = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [dependencies]\n[dev-dependencies]\ntidy = \"1\"\n\
                        [target.a]\n[target.b.dependencies]\n";
        let right = "[package]\nname = \"c\"\n[dependencies]\n[target.a]\n\
                     [target.b.dependencies]\n";

        for (what, rewritten) in [
            (
                "an unrelated empty dependency table dropped",
                "[package]\nname = \"c\"\n[target.a]\n[target.b.dependencies]\n",
            ),
            (
                "an unrelated empty target table dropped",
                "[package]\nname = \"c\"\n[dependencies]\n[target.b.dependencies]\n",
            ),
            (
                "an unrelated empty table under a target dropped",
                "[package]\nname = \"c\"\n[dependencies]\n[target.a]\n",
            ),
            (
                "the table the removal emptied kept",
                "[package]\nname = \"c\"\n[dependencies]\n[dev-dependencies]\n[target.a]\n\
                 [target.b.dependencies]\n",
            ),
        ] {
            assert!(
                matches!(
                    verified(original, rewritten),
                    Err(ManifestRefusal::Defect { .. })
                ),
                "{what}"
            );
        }
        assert_eq!(verified(original, right), Ok(()));
    }

    #[test]
    fn the_check_expects_the_target_tables_the_removal_emptied_to_be_gone() {
        let original = "[package]\nname = \"c\"\n[package.metadata.skeletons.tidy]\n\
                        [target.a.dev-dependencies]\ntidy = \"1\"\n";

        assert_eq!(verified(original, "[package]\nname = \"c\"\n"), Ok(()));
        assert!(matches!(
            verified(original, "[package]\nname = \"c\"\n[target.a]\n"),
            Err(ManifestRefusal::Defect { .. })
        ));
        assert!(matches!(
            verified(original, "[package]\nname = \"c\"\n[target]\n"),
            Err(ManifestRefusal::Defect { .. })
        ));
    }

    /// The manifest head every comment case builds on.
    const HEAD: &str = "[package]\nname = \"c\"\n";

    /// The wearing table for `tidy`, header only.
    const WEARING: &str = "[package.metadata.skeletons.tidy]\n";

    /// Runs `without_wearing` for `tidy` over each case (a label, a manifest and
    /// the text it must come out as) and compares the whole text.
    fn assert_removals(cases: &[(&str, String, String)]) {
        for (what, manifest, left) in cases {
            assert_eq!(
                removed(manifest).as_deref(),
                Ok(left.as_str()),
                "{what}, from the manifest:\n{manifest}"
            );
        }
    }

    #[test]
    fn the_comments_around_the_wearing_tables_header_move_to_what_follows() {
        // The header is the one thing removed, so a comment above it, one on
        // its line and one directly below it (the next header's, since the
        // table is empty) all stay; a comment above the table's own key line
        // goes with that line. When nothing follows, the text moves to the end
        // of the file, whether or not the file ended in a newline.
        assert_removals(&[
            (
                "above",
                format!(
                    "{HEAD}\n# above\n{WEARING}\n[dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}\n# above\n\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "beside",
                format!(
                    "{HEAD}\n[package.metadata.skeletons.tidy] # beside\n\n[dev-dependencies]\n\
                    other = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}# beside\n\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "under, on an empty table",
                format!(
                    "{HEAD}\n{WEARING}# under\n\n[dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}# under\n\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "under, on a key line the table holds",
                format!(
                    "{HEAD}\n{WEARING}# the key's\nsome = 1\n\n[dev-dependencies]\n\
                    other = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "last in the file",
                format!("{HEAD}\n# above\n[package.metadata.skeletons.tidy] # beside\n"),
                format!("{HEAD}\n# above\n# beside\n"),
            ),
            (
                "last in a file with no final newline",
                format!("{HEAD}\n# above\n[package.metadata.skeletons.tidy] # beside"),
                format!("{HEAD}\n# above\n# beside\n"),
            ),
        ]);
    }

    #[test]
    fn the_comments_around_each_header_inside_the_wearing_table_move_to_what_follows() {
        // The wearing table holds a sub-table and two entries of an array of
        // tables, each with a comment above and one beside its header. Every
        // one of them survives, in file order, ahead of the next header; the
        // comment below a sub-table's header is that table's key line comment,
        // which goes.
        let manifest = format!(
            "{HEAD}\n{WEARING}some = 1\n\n# about options\n\
             [package.metadata.skeletons.tidy.options] # beside options\n# its key\na = 1\n\n\
             # about items\n[[package.metadata.skeletons.tidy.items]] # beside items\nb = 2\n\n\
             # about second\n[[package.metadata.skeletons.tidy.items]]\nb = 3\n\n\
             [dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
        );
        let left = format!(
            "{HEAD}\n# about options\n# beside options\n\n# about items\n# beside items\n\n\
             # about second\n\n[dev-dependencies]\nother = \"1\"\n"
        );

        assert_removals(&[("sub-tables and arrays of tables", manifest, left)]);
    }

    #[test]
    fn the_comments_around_the_header_of_a_skeletons_or_metadata_table_that_goes_move_on() {
        // A `[package.metadata.skeletons]` or `[package.metadata]` header the
        // wearing leaves empty goes too, and its comments follow the same
        // rule: above and beside it move to what follows.
        assert_removals(&[
            (
                "skeletons, above and beside",
                format!(
                    "{HEAD}\n# above skeletons\n[package.metadata.skeletons] # beside skeletons\n\
                     {WEARING}\n[dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!(
                    "{HEAD}\n# above skeletons\n# beside skeletons\n\n[dev-dependencies]\n\
                     other = \"1\"\n"
                ),
            ),
            (
                "skeletons, under",
                format!(
                    "{HEAD}\n[package.metadata.skeletons]\n# under skeletons\n{WEARING}\n\
                     [dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}# under skeletons\n\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "metadata, above and beside",
                format!(
                    "{HEAD}\n# above metadata\n[package.metadata] # beside metadata\n\
                     {WEARING}\n[dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!(
                    "{HEAD}\n# above metadata\n# beside metadata\n\n[dev-dependencies]\n\
                     other = \"1\"\n"
                ),
            ),
            (
                "metadata, under",
                format!(
                    "{HEAD}\n[package.metadata]\n# under metadata\n{WEARING}\n\
                     [dev-dependencies]\nother = \"1\"\ntidy = \"1\"\n"
                ),
                format!("{HEAD}# under metadata\n\n[dev-dependencies]\nother = \"1\"\n"),
            ),
        ]);
    }

    #[test]
    fn the_comments_around_a_dependency_written_as_a_table_of_its_own_move_on() {
        // `[dev-dependencies.tidy]` is a header that goes while its parent
        // stays: a comment above and one beside it move to the end of the file,
        // and the comment below it is its key line's, which goes.
        let manifest = format!(
            "{HEAD}\n{WEARING}\n[dev-dependencies]\nother = \"1\"\n\n# above tidy\n\
             [dev-dependencies.tidy] # beside tidy\n# under tidy\nversion = \"1\"\n"
        );
        let left =
            format!("{HEAD}\n[dev-dependencies]\nother = \"1\"\n\n# above tidy\n# beside tidy\n");

        assert_removals(&[("a table of its own", manifest, left)]);
    }

    #[test]
    fn the_comments_around_a_dependency_table_the_removal_emptied_move_on() {
        // `[dev-dependencies]` held only the key, so its header goes. A
        // comment above it and one beside it move to what follows; the one
        // below is the removed key line's, which goes. The same holds under a
        // `[target.…]` table, which goes with it.
        let tail = "\n[dependencies]\nx = \"1\"\n";
        assert_removals(&[
            (
                "above",
                format!("{HEAD}\n{WEARING}\n# above\n[dev-dependencies]\ntidy = \"1\"\n{tail}"),
                format!("{HEAD}\n# above\n\n[dependencies]\nx = \"1\"\n"),
            ),
            (
                "beside",
                format!("{HEAD}\n{WEARING}\n[dev-dependencies] # beside\ntidy = \"1\"\n{tail}"),
                format!("{HEAD}# beside\n\n[dependencies]\nx = \"1\"\n"),
            ),
            (
                "under",
                format!("{HEAD}\n{WEARING}\n[dev-dependencies]\n# under\ntidy = \"1\"\n{tail}"),
                format!("{HEAD}\n[dependencies]\nx = \"1\"\n"),
            ),
            (
                "above, under a target",
                format!(
                    "{HEAD}\n{WEARING}\n# above\n[target.'cfg(unix)'.dev-dependencies]\n\
                     tidy = \"1\"\n{tail}"
                ),
                format!("{HEAD}\n# above\n\n[dependencies]\nx = \"1\"\n"),
            ),
            (
                "beside, under a target",
                format!(
                    "{HEAD}\n{WEARING}\n[target.'cfg(unix)'.dev-dependencies] # beside\n\
                     tidy = \"1\"\n{tail}"
                ),
                format!("{HEAD}# beside\n\n[dependencies]\nx = \"1\"\n"),
            ),
            (
                "under, under a target",
                format!(
                    "{HEAD}\n{WEARING}\n[target.'cfg(unix)'.dev-dependencies]\n# under\n\
                     tidy = \"1\"\n{tail}"
                ),
                format!("{HEAD}\n[dependencies]\nx = \"1\"\n"),
            ),
        ]);
    }

    #[test]
    fn the_comments_around_a_target_table_that_goes_with_its_dependencies_move_on_in_order() {
        // `[target.'cfg(unix)']` and the dependency table below it both go,
        // each with a comment above and one beside. They are adjacent headers,
        // so their text lands together, the target's first, as in the file.
        let manifest = format!(
            "{HEAD}\n{WEARING}\n# above target\n[target.'cfg(unix)'] # beside target\n\
             # above dependencies\n[target.'cfg(unix)'.dev-dependencies] # beside dependencies\n\
             tidy = \"1\"\n\n[dependencies]\nx = \"1\"\n"
        );
        let left = format!(
            "{HEAD}\n# above target\n# beside target\n# above dependencies\n\
             # beside dependencies\n\n[dependencies]\nx = \"1\"\n"
        );

        assert_removals(&[("two adjacent removed headers", manifest, left)]);
    }

    #[test]
    fn the_comments_around_the_target_table_that_goes_with_its_children_move_on() {
        // `[target]` itself has a header here, and a comment above it, beside
        // it and below it. The table under it goes, so `target` goes too, and
        // all three stay: the one below is the next header's, not a key line's.
        let manifest = format!(
            "{HEAD}\n{WEARING}\n# above target\n[target] # beside target\n# under target\n\
             [target.'cfg(unix)'.dev-dependencies]\ntidy = \"1\"\n\n[dependencies]\nx = \"1\"\n"
        );
        let left = format!(
            "{HEAD}\n# above target\n# beside target\n# under target\n\n\
             [dependencies]\nx = \"1\"\n"
        );

        assert_removals(&[("target", manifest, left)]);
    }

    #[test]
    fn a_comment_under_an_empty_table_that_stays_is_kept_when_the_next_header_goes() {
        // `[build-dependencies]` was empty already and stays; the comment under
        // its header is the header after it, which the removal empties and
        // takes out. The comment must outlive that header.
        let manifest = format!(
            "{HEAD}\n{WEARING}\n[build-dependencies]\n# was empty before\n\n\
             [target.\"cfg(unix)\".dev-dependencies]\ntidy = \"1\"\n"
        );
        let left = format!("{HEAD}\n[build-dependencies]\n# was empty before\n");

        assert_removals(&[("first shape", manifest, left)]);
    }

    #[test]
    fn a_comment_that_reads_as_the_end_of_package_is_kept_when_the_wearing_after_it_goes() {
        let manifest =
            format!("{HEAD}publish = false\n# publish stays false until the audit\n{WEARING}");
        let left = format!("{HEAD}publish = false\n# publish stays false until the audit\n");

        assert_removals(&[("second shape", manifest, left)]);
    }

    #[test]
    fn the_comments_above_and_beside_an_emptied_table_are_kept_and_the_one_inside_goes() {
        // The comment above `[dev-dependencies]` and the one beside it stay;
        // the one inside is the removed key line's and goes.
        let manifest = format!(
            "{HEAD}\n{WEARING}\n# My dev deps, keep this note\n[dev-dependencies] # trailing\n\
             # inside note\ntidy = \"1\"\n"
        );
        let left = format!("{HEAD}\n# My dev deps, keep this note\n# trailing\n");

        assert_removals(&[("third shape", manifest, left)]);
    }

    #[test]
    fn a_comment_above_a_removed_key_line_goes_in_every_form_a_key_line_takes() {
        // Where no header goes there is nothing to carry. The comment above the
        // removed `tidy` line goes with it and the comment above the line that
        // stays, `other`, stays, in a dependency table, an inline table, and
        // dotted keys at the root.
        assert_removals(&[
            (
                "a table",
                format!(
                    "{HEAD}\n{WEARING}\n[dev-dependencies]\n# about other\nother = \"1\"\n\
                    # about tidy\ntidy = \"1\"\n"
                ),
                format!("{HEAD}\n[dev-dependencies]\n# about other\nother = \"1\"\n"),
            ),
            (
                "an inline table",
                format!(
                    "dev-dependencies = {{ tidy = \"1\", other = \"1\" }} # about both\n{HEAD}\n\
                    {WEARING}"
                ),
                format!("dev-dependencies = {{ other = \"1\" }} # about both\n{HEAD}"),
            ),
            (
                "dotted keys",
                format!(
                    "# about other\ndev-dependencies.other = \"1\"\n# about tidy\n\
                    dev-dependencies.tidy = \"1\"\n{HEAD}\n{WEARING}"
                ),
                format!("# about other\ndev-dependencies.other = \"1\"\n{HEAD}"),
            ),
        ]);
    }

    #[test]
    fn the_comments_that_go_with_a_removed_key_line_include_those_inside_its_value() {
        // A comment inside a multi-line array, and a line that starts with `#`
        // inside a multi-line string, are part of the removed key line, so the
        // check must not count them as lost; and the only dotted key of its
        // table takes the comment above it with it.
        assert_removals(&[
            (
                "a multi-line array",
                format!(
                    "{HEAD}\n{WEARING}\n[dev-dependencies]\nother = \"1\"\ntidy = [\n\
                      # inside\n  \"1\",\n]\n"
                ),
                format!("{HEAD}\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "a multi-line string",
                format!(
                    "{HEAD}\n{WEARING}\n[dev-dependencies]\nother = \"1\"\ntidy = \"\"\"\n\
                    # not a comment\n\"\"\"\n"
                ),
                format!("{HEAD}\n[dev-dependencies]\nother = \"1\"\n"),
            ),
            (
                "the only dotted key",
                format!("# about tidy\ndev-dependencies.tidy = \"1\"\n{HEAD}\n{WEARING}"),
                HEAD.to_owned(),
            ),
        ]);
    }

    #[test]
    fn the_comments_of_a_header_that_goes_land_on_an_array_of_tables_entry_that_stays() {
        // The next header that stays may be an entry of an array of tables;
        // the carried comment goes ahead of that entry's own.
        let manifest =
            format!("{HEAD}\n# about wearing\n{WEARING}\n# about bin\n[[bin]]\nname = \"x\"\n");
        let left = format!("{HEAD}\n# about wearing\n\n# about bin\n[[bin]]\nname = \"x\"\n");

        assert_removals(&[("an array of tables entry", manifest, left)]);
    }

    /// [`comments_kept`] over an edit of `original` into `rewritten`, where
    /// the lines `excepted` go with removed key lines.
    fn comments_checked(
        original: &str,
        rewritten: &str,
        excepted: &[&str],
    ) -> Result<(), ManifestRefusal> {
        let excepted: Vec<String> = excepted.iter().map(|line| (*line).to_owned()).collect();
        comments_kept(
            &Edit {
                original,
                rewritten,
            },
            &excepted,
        )
    }

    #[test]
    fn the_comment_check_refuses_an_edit_that_loses_a_comment_and_names_the_line() {
        // The original holds three comment lines. Each wrong rewrite loses one
        // that is not excepted (or one of two equal lines), and the refusal
        // must name it; the right rewrite loses only the excepted one.
        let original = "# first\n[a]\n# second\nx = 1\n  # third\n[b]\n# first\n";

        for (what, rewritten, lost) in [
            (
                "a first line lost",
                "# first\n[a]\nx = 1\n  # third\n[b]\n# first\n",
                "# second",
            ),
            (
                "an indented line lost",
                "# first\n[a]\n# second\nx = 1\n[b]\n# first\n",
                "  # third",
            ),
            (
                "one of two equal lines lost",
                "# first\n[a]\n# second\nx = 1\n  # third\n[b]\n",
                "# first",
            ),
        ] {
            let Err(ManifestRefusal::Defect { detail }) =
                comments_checked(original, rewritten, &[])
            else {
                panic!("{what}: an edit that loses a comment must be refused");
            };
            assert!(detail.contains(lost), "{what}: the detail was {detail:?}");
        }
        assert_eq!(
            comments_checked(
                original,
                "# first\n[a]\nx = 1\n  # third\n[b]\n# first\n",
                &["# second"]
            ),
            Ok(())
        );
    }

    #[test]
    fn the_comment_check_accepts_an_edit_that_loses_only_excepted_lines() {
        // Positive control for the check above: nothing lost, comments moved
        // within the file, and the excepted lines lost, are all accepted.
        let original = "# a\n[x]\n# b\nk = 1\n# c\n";

        assert_eq!(comments_checked(original, original, &[]), Ok(()));
        assert_eq!(comments_checked(original, "# c\n# a\n# b\n", &[]), Ok(()));
        assert_eq!(comments_checked(original, "# a\n", &["# b", "# c"]), Ok(()));
    }

    #[test]
    fn the_comment_check_counts_an_excepted_line_once_for_each_time_it_was_removed() {
        // `# same` is in the original twice. Excepting it once leaves one to
        // keep, so losing both is refused and losing one is accepted.
        let original = "# same\n[x]\n# same\nk = 1\n";

        assert_eq!(
            comments_checked(original, "# same\n[x]\n", &["# same"]),
            Ok(())
        );
        assert!(matches!(
            comments_checked(original, "[x]\n", &["# same"]),
            Err(ManifestRefusal::Defect { .. })
        ));
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
        // input space is keys across the whole grammar `wear` holds a key to,
        // up to the longest it accepts, with `other` left out because the
        // bases already use it.
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

            let rewritten = without_wearing(&wearing, &WearingKey::new(&text))
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
