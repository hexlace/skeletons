//! Cargo's configuration files, read so that what only Cargo follows in them is refused.
//!
//! Cargo reads a configuration file from the directory it starts in, from each directory above
//! it and from the Cargo home, and three things in those files change which packages and which
//! files a workspace has: `paths`, which puts a package at another directory and gets past
//! `--locked` while doing so, `[patch]`, which swaps a dependency for another one, and `include`,
//! which pulls in more files. Reproducing any of them is Cargo's resolution, so the reader
//! refuses them by name. Everything else in a configuration file leaves the report as it is, and
//! is read past.

mod cargo_home;
mod files;

use std::path::{Path, PathBuf};

use super::error::ReadWithoutCargoError;
use super::text_error::TextError;

pub(super) use cargo_home::{HomeVariables, cargo_home_directory};

/// Every configuration file Cargo reads for a directory, parsed, nearest first.
#[derive(Debug)]
pub(super) struct CargoConfiguration {
    files: Vec<ConfigurationFile>,
}

/// One configuration file, as the table its text parses to.
#[derive(Debug)]
struct ConfigurationFile {
    path: PathBuf,
    table: toml::Table,
}

/// Reads the configuration files Cargo reads when it starts in `start`, with the Cargo home
/// `cargo_home`.
///
/// `start` is a real directory, as `std::fs::canonicalize` gives one.
///
/// # Errors
///
/// Unreadable, naming the file, for a file that cannot be read or is not TOML, which Cargo
/// refuses too.
pub(super) fn read(
    start: &Path,
    cargo_home: Option<&Path>,
) -> Result<CargoConfiguration, ReadWithoutCargoError> {
    let listed = files::configuration_files(start, cargo_home);
    let files = listed
        .iter()
        .map(|path| read_file(path))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        files.len(),
        listed.len(),
        "a file is read for each one listed, and a failure to read one returned above"
    );
    Ok(CargoConfiguration { files })
}

/// The file at `path`, read and parsed as TOML.
fn read_file(path: &Path) -> Result<ConfigurationFile, ReadWithoutCargoError> {
    let text =
        std::fs::read_to_string(path).map_err(|error| ReadWithoutCargoError::file(path, &error))?;
    let table = toml::from_str::<toml::Table>(&text).map_err(|error| {
        ReadWithoutCargoError::text(path, &TextError::unreadable(error.to_string()))
    })?;
    Ok(ConfigurationFile {
        path: path.to_path_buf(),
        table,
    })
}

impl CargoConfiguration {
    /// Refuses a file that uses `include`, `paths` or `[patch]`, the first one found going from
    /// the nearest file out, and within a file in that order.
    ///
    /// # Errors
    ///
    /// Unsupported for an `include`, for a `paths` with an entry and for a `[patch.<name>]` with
    /// an entry, naming the file. Unreadable for a `paths` that is not an array and a `patch` or
    /// `patch.<name>` that is not a table, which Cargo refuses too.
    pub(super) fn refuse_what_only_cargo_follows(&self) -> Result<(), ReadWithoutCargoError> {
        self.files
            .iter()
            .try_for_each(ConfigurationFile::refuse_what_only_cargo_follows)
    }
}

impl ConfigurationFile {
    /// Refuses this file for the first of `include`, `paths` and `[patch]` it uses.
    fn refuse_what_only_cargo_follows(&self) -> Result<(), ReadWithoutCargoError> {
        self.refuse_include()?;
        self.refuse_paths()?;
        self.refuse_patch()
    }

    /// Refuses an `include` of any shape. Cargo 1.95 follows it and Cargo 1.85 ignores it, so
    /// which files it adds depends on the Cargo that runs, and the reader follows none.
    fn refuse_include(&self) -> Result<(), ReadWithoutCargoError> {
        if self.table.contains_key("include") {
            Err(self.unsupported("`include`"))
        } else {
            Ok(())
        }
    }

    /// Refuses a `paths` with an entry, which puts a package at another directory and gets past
    /// `--locked`. An empty array changes nothing.
    fn refuse_paths(&self) -> Result<(), ReadWithoutCargoError> {
        let Some(paths) = self.table.get("paths") else {
            return Ok(());
        };
        let Some(entries) = paths.as_array() else {
            return Err(self.unreadable("`paths` must be an array"));
        };
        if entries.is_empty() {
            Ok(())
        } else {
            Err(self.unsupported("`paths`"))
        }
    }

    /// Refuses a `[patch.<name>]` with an entry, which swaps a dependency for another package.
    /// A table with no entry swaps nothing.
    fn refuse_patch(&self) -> Result<(), ReadWithoutCargoError> {
        let Some(patch) = self.table.get("patch") else {
            return Ok(());
        };
        let Some(registries) = patch.as_table() else {
            return Err(self.unreadable("`patch` must be a table"));
        };
        for (name, entries) in registries {
            let Some(entries) = entries.as_table() else {
                return Err(self.unreadable(&format!("`patch.{name}` must be a table")));
            };
            if !entries.is_empty() {
                return Err(self.unsupported(&format!("`[patch.{name}]`")));
            }
        }
        Ok(())
    }

    /// The refusal of `what` in this file, which Cargo follows and the reader does not.
    fn unsupported(&self, what: &str) -> ReadWithoutCargoError {
        ReadWithoutCargoError::unsupported(format!("{what} in {}", self.path.display()))
    }

    /// The refusal of this file for `what`, which Cargo refuses too.
    fn unreadable(&self, what: &str) -> ReadWithoutCargoError {
        ReadWithoutCargoError::unreadable(format!("{}: {what}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use proptest::prelude::*;

    use super::{ConfigurationFile, read};
    use crate::workspace::without_cargo::error::ReadWithoutCargoError;

    /// A temporary directory with a real path, and the start `w` in it.
    struct Tree {
        _directory: tempfile::TempDir,
        top: PathBuf,
    }

    impl Tree {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("a temporary directory");
            let top = directory.path().canonicalize().expect("a real path");
            std::fs::create_dir(top.join("w")).expect("the start");
            Self {
                _directory: directory,
                top,
            }
        }

        fn start(&self) -> PathBuf {
            self.top.join("w")
        }

        /// Writes `text` to `relative` under the top and says where.
        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.top.join(relative);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("a directory");
            std::fs::write(&path, text).expect("a file");
            path
        }

        /// What reading the configuration from the start, and refusing what only Cargo follows,
        /// says.
        fn verdict(&self, cargo_home: Option<&Path>) -> Result<(), ReadWithoutCargoError> {
            read(&self.start(), cargo_home)?.refuse_what_only_cargo_follows()
        }
    }

    /// The verdict on a single file `w/.cargo/config.toml` holding `text`, and the file.
    fn verdict_on(text: &str) -> (Result<(), ReadWithoutCargoError>, PathBuf) {
        let tree = Tree::new();
        let file = tree.write("w/.cargo/config.toml", text);
        (tree.verdict(None), file)
    }

    fn unsupported(detail: String) -> Result<(), ReadWithoutCargoError> {
        Err(ReadWithoutCargoError::unsupported(detail))
    }

    fn unreadable(detail: String) -> Result<(), ReadWithoutCargoError> {
        Err(ReadWithoutCargoError::unreadable(detail))
    }

    #[test]
    fn an_include_in_any_form_is_refused_naming_the_file() {
        // Cargo 1.95 follows `include` and 1.85 ignores it, so it is refused whatever it holds:
        // a string, an array of strings, and the table form.
        for text in [
            "include = \"more.toml\"\n",
            "include = [\"a.toml\", \"b.toml\"]\n",
            "include = [{ path = \"more.toml\", optional = true }]\n",
            "[[include]]\npath = \"more.toml\"\n",
        ] {
            let (verdict, file) = verdict_on(text);

            assert_eq!(
                verdict,
                unsupported(format!("`include` in {}", file.display())),
                "{text}"
            );
        }
    }

    #[test]
    fn a_paths_array_with_an_entry_is_refused_naming_the_file() {
        let (verdict, file) = verdict_on("paths = [\"../override/house-rules\"]\n");

        assert_eq!(
            verdict,
            unsupported(format!("`paths` in {}", file.display()))
        );
    }

    #[test]
    fn a_patch_table_with_an_entry_is_refused_naming_the_table_and_the_file() {
        for (text, name) in [
            (
                "[patch.crates-io]\nhouse-rules = { path = \"../patched\" }\n",
                "crates-io",
            ),
            (
                "[patch.'https://example.test/index']\nx = { git = \"https://example.test/x\" }\n",
                "https://example.test/index",
            ),
        ] {
            let (verdict, file) = verdict_on(text);

            assert_eq!(
                verdict,
                unsupported(format!("`[patch.{name}]` in {}", file.display())),
                "{text}"
            );
        }
    }

    #[test]
    fn an_empty_patch_table_does_not_hide_a_later_one_with_an_entry() {
        // An empty `[patch.a]` changes nothing, and the entry in `[patch.b]` is still found.
        let (verdict, file) = verdict_on("[patch.a]\n[patch.b]\nx = { path = \"x\" }\n");

        assert_eq!(
            verdict,
            unsupported(format!("`[patch.b]` in {}", file.display()))
        );
    }

    #[test]
    fn what_changes_nothing_in_the_report_is_read_past() {
        // Over-refusing is a failure too: a source replacement, registries, aliases, build
        // settings, an empty `paths`, an empty patch table, and keys no one knows are all
        // things Cargo reads that leave the packages and their files where they were.
        for text in [
            "",
            "[source.crates-io]\nreplace-with = \"vendored\"\n\
             [source.vendored]\ndirectory = \"v\"\n",
            "[registries.alt]\nindex = \"https://example.test/index\"\n",
            "[alias]\nb = \"build\"\n",
            "[build]\ntarget-dir = \"out\"\n",
            "paths = []\n",
            "[patch.crates-io]\n",
            "patch = {}\n",
            "[nobody-knows]\nthis = 1\n",
        ] {
            let (verdict, _) = verdict_on(text);

            assert_eq!(verdict, Ok(()), "{text}");
        }
    }

    #[test]
    fn text_that_is_not_toml_is_unreadable_naming_the_file() {
        let tree = Tree::new();
        let file = tree.write("w/.cargo/config.toml", "paths = [\n");

        let verdict = tree.verdict(None);

        let Err(ReadWithoutCargoError::ManifestUnreadable { detail }) = verdict else {
            panic!("expected an unreadable file, got {verdict:?}");
        };
        assert!(
            detail.starts_with(&format!("{}: ", file.display())),
            "{detail}"
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_unreadable_naming_the_file() {
        // `config` is a directory: Cargo takes it as the file and fails to read it.
        let tree = Tree::new();
        std::fs::create_dir_all(tree.top.join("w/.cargo/config")).expect("a directory");

        let verdict = tree.verdict(None);

        let Err(ReadWithoutCargoError::ManifestUnreadable { detail }) = verdict else {
            panic!("expected an unreadable file, got {verdict:?}");
        };
        assert!(
            detail.starts_with(&format!(
                "{} could not be read",
                tree.top.join("w/.cargo/config").display()
            )),
            "{detail}"
        );
    }

    #[test]
    fn a_paths_or_patch_of_the_wrong_type_is_unreadable() {
        for (text, what) in [
            ("paths = \"../x\"\n", "`paths` must be an array"),
            ("paths = { a = 1 }\n", "`paths` must be an array"),
            ("patch = 1\n", "`patch` must be a table"),
            ("patch = [\"x\"]\n", "`patch` must be a table"),
            (
                "[patch]\ncrates-io = \"x\"\n",
                "`patch.crates-io` must be a table",
            ),
        ] {
            let (verdict, file) = verdict_on(text);

            assert_eq!(
                verdict,
                unreadable(format!("{}: {what}", file.display())),
                "{text}"
            );
        }
    }

    #[test]
    fn within_a_file_include_comes_before_paths_and_paths_before_patch() {
        // One file, written twice: with all three keys, and then with the first one gone.
        let tree = Tree::new();
        let file = tree.write(
            "w/.cargo/config.toml",
            "include = \"x.toml\"\npaths = [\"p\"]\n[patch.crates-io]\nx = { path = \"x\" }\n",
        );
        let all_three = tree.verdict(None);
        tree.write(
            "w/.cargo/config.toml",
            "paths = [\"p\"]\n[patch.crates-io]\nx = { path = \"x\" }\n",
        );
        let paths_and_patch = tree.verdict(None);

        assert_eq!(
            all_three,
            unsupported(format!("`include` in {}", file.display()))
        );
        assert_eq!(
            paths_and_patch,
            unsupported(format!("`paths` in {}", file.display()))
        );
    }

    #[test]
    fn the_nearest_file_is_refused_before_one_further_out_and_the_home_comes_last() {
        let tree = Tree::new();
        let near = tree.write("w/.cargo/config.toml", "paths = [\"near\"]\n");
        tree.write(".cargo/config.toml", "include = \"far.toml\"\n");
        let home = tree.write("cargo-home/config.toml", "paths = [\"home\"]\n");

        assert_eq!(
            tree.verdict(Some(&tree.top.join("cargo-home"))),
            unsupported(format!("`paths` in {}", near.display()))
        );
        std::fs::remove_file(&near).expect("remove the nearest");
        assert_eq!(
            tree.verdict(Some(&tree.top.join("cargo-home"))),
            unsupported(format!(
                "`include` in {}",
                tree.top.join(".cargo/config.toml").display()
            ))
        );
        std::fs::remove_file(tree.top.join(".cargo/config.toml")).expect("remove the next");
        assert_eq!(
            tree.verdict(Some(&tree.top.join("cargo-home"))),
            unsupported(format!("`paths` in {}", home.display()))
        );
    }

    proptest! {
        /// Over any set of tables nobody knows, with an empty `paths` or none: they are read
        /// past, and the same text with one `paths` entry put in front of them is refused for
        /// `paths`. A table named for a key that is not `include`, `paths` or `patch` can hold
        /// anything, so the property is that none of it is mistaken for one of the three.
        #[test]
        fn tables_nobody_knows_are_read_past_and_a_paths_entry_among_them_is_found(
            names in proptest::collection::btree_set("x-[a-z]{1,8}", 0..6),
            entry in "[a-z./]{1,12}",
            writes_empty_paths in proptest::bool::ANY,
        ) {
            let tables: String = names
                .iter()
                .map(|name| ["[", name, "]\nvalue = 1\nlist = [\"paths\"]\n"].concat())
                .collect();
            let empty = if writes_empty_paths { "paths = []\n" } else { "" };
            let file = |text: String| ConfigurationFile {
                path: PathBuf::from("/c/config.toml"),
                table: toml::from_str(&text).expect("the generated text is TOML"),
            };

            let read_past = file(format!("{empty}{tables}"));
            let refused = file(format!("paths = [\"{entry}\"]\n{tables}"));

            prop_assert_eq!(read_past.refuse_what_only_cargo_follows(), Ok(()));
            prop_assert_eq!(
                refused.refuse_what_only_cargo_follows(),
                unsupported("`paths` in /c/config.toml".to_owned())
            );
        }
    }

    #[test]
    fn the_file_without_the_extension_is_the_one_read_when_both_exist() {
        // Cargo ignores `config.toml` beside a `config`, so a `paths` only in `config.toml` is
        // not Cargo's, and one in `config` is, whatever `config.toml` says.
        let hidden = Tree::new();
        hidden.write("w/.cargo/config", "");
        hidden.write("w/.cargo/config.toml", "paths = [\"x\"]\n");
        let read = Tree::new();
        let legacy = read.write("w/.cargo/config", "paths = [\"x\"]\n");
        read.write("w/.cargo/config.toml", "");

        assert_eq!(hidden.verdict(None), Ok(()));
        assert_eq!(
            read.verdict(None),
            unsupported(format!("`paths` in {}", legacy.display()))
        );
    }
}
