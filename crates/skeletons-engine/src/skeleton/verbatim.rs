//! `verbatim`: the paths under `files/` a manifest declares are shipped byte
//! for byte, checked on their own terms by [`parse`] and then matched against
//! what the skeleton ships by [`VerbatimFiles::link`].
//!
//! A declaration is stated, never inferred, and each entry is compared by
//! exact string equality with the walked paths: no `./`, no case folding, no
//! Unicode folding. The walk already refuses two siblings that fold to one
//! name, so a path differing from a file's only in case names nothing, rather
//! than guessing which file the author meant.
//!
//! An entry that is not a file under `files/` is refused as the one thing it
//! is instead, so the message can say what to write: a directory (which is
//! never taken as a subtree, since that would quietly make a file added there
//! later verbatim), a partial (which is only ever text), or nothing at all.

use std::collections::BTreeSet;
use std::ops::Bound;

use super::declarations::ROOT;
use super::error::{Reason, RenderError, SkeletonIdentity, TomlType};
use super::keyed::Keyed;
use super::template::Partial;
use super::walk::TreePath;

/// The prefix that spells a partial by its path in the skeleton's tree,
/// rather than relative to `partials/`.
const PARTIALS_PREFIX: &str = "partials/";

/// The paths under `files/` a skeleton ships verbatim.
///
/// The only way to make one outside a test is [`Self::link`], which takes
/// each member from the walked file set itself and never from the manifest's
/// text, so every path here names a file the skeleton ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerbatimFiles(BTreeSet<TreePath>);

/// What one declared `verbatim` entry turned out to name.
#[derive(Debug, PartialEq, Eq)]
enum Classified<'files> {
    /// A file under `files/`, as the walk spelled its path.
    File(&'files TreePath),
    /// The directory holding at least one file under `files/`.
    Directory,
    /// A partial, spelled relative to `partials/` or by its tree path.
    Partial,
    /// Nothing the skeleton ships.
    NoFile,
}

impl VerbatimFiles {
    /// Matches every declared `verbatim` entry, in declared order, against
    /// `file_paths` (everything `files/` holds) and `partial_paths`
    /// (everything `partials/` holds), refusing the first entry that is not
    /// one of the files.
    pub(crate) fn link(
        skeleton: &SkeletonIdentity,
        declared: &[String],
        file_paths: &BTreeSet<TreePath>,
        partial_paths: &Keyed<Partial, TreePath>,
    ) -> Result<Self, RenderError> {
        let partial_names: BTreeSet<&str> = partial_paths
            .iter()
            .map(|(_key, path)| path.as_str())
            .collect();

        let mut linked = BTreeSet::new();
        for entry in declared {
            let refusal = |reason: fn(String) -> Reason| {
                RenderError::about_manifest(skeleton.clone(), reason(entry.clone()))
            };
            match classify(entry, file_paths, &partial_names) {
                Classified::File(path) => {
                    linked.insert(path.clone());
                }
                Classified::Directory => {
                    return Err(refusal(|path| Reason::VerbatimPathNamesDirectory { path }));
                }
                Classified::Partial => {
                    return Err(refusal(|path| Reason::VerbatimPathNamesPartial { path }));
                }
                Classified::NoFile => {
                    return Err(refusal(|path| Reason::VerbatimPathNamesNoFile { path }));
                }
            }
        }

        // Postconditions: `parse` refused a repeated entry, so each entry
        // linked a distinct file, and every member came out of the walked
        // set. These are what `is_verbatim` relies on to mean "a file the
        // skeleton ships".
        assert_eq!(
            linked.len(),
            declared.len(),
            "every declared entry linked a distinct file"
        );
        assert!(
            linked.iter().all(|path| file_paths.contains(path)),
            "a verbatim path is always one of the walked files"
        );
        Ok(Self(linked))
    }

    /// Whether the file at `path` under `files/` is shipped verbatim.
    pub(crate) fn contains(&self, path: &TreePath) -> bool {
        self.0.contains(path)
    }

    /// The verbatim set made directly from `paths`, for a test that needs
    /// some without a manifest to declare them or a `files/` tree to walk.
    #[cfg(test)]
    pub(crate) fn for_test(paths: &[&str]) -> Self {
        Self(
            paths
                .iter()
                .map(|path| TreePath::for_test(&path.split('/').collect::<Vec<_>>()))
                .collect(),
        )
    }
}

/// Names what `entry` is, checking a file first, then a directory, then a
/// partial: a file is the only thing a declaration may name, and a directory
/// is the mistake a skeleton's author is most likely to have meant a list of
/// files by.
fn classify<'files>(
    entry: &str,
    file_paths: &'files BTreeSet<TreePath>,
    partial_names: &BTreeSet<&str>,
) -> Classified<'files> {
    if let Some(path) = file_paths.get(entry) {
        return Classified::File(path);
    }

    // A trailing `/` is forgiven here alone, to name the directory the
    // author plainly meant. It never makes an entry a file.
    let directory = entry.strip_suffix('/').unwrap_or(entry);
    if names_a_directory(directory, file_paths) {
        return Classified::Directory;
    }

    let by_tree_path = entry.strip_prefix(PARTIALS_PREFIX);
    let names_a_partial = partial_names.contains(entry)
        || by_tree_path.is_some_and(|relative| partial_names.contains(relative));
    if names_a_partial {
        return Classified::Partial;
    }
    Classified::NoFile
}

/// Whether some walked file lies below `directory`: the first path at or
/// after `directory/` in path order starts with it.
///
/// An empty `directory` names no directory, and `"/"` cannot start a walked
/// path, so neither is taken for one.
fn names_a_directory(directory: &str, file_paths: &BTreeSet<TreePath>) -> bool {
    if directory.is_empty() {
        return false;
    }
    let prefix = format!("{directory}/");
    file_paths
        .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
        .next()
        .is_some_and(|path| path.as_str().starts_with(&prefix))
}

/// Reads the `verbatim` key of `metadata` — the `[package.metadata.skeletons]`
/// table itself — into its entries in declared order, or none when it is
/// absent. Checks only what the manifest alone can: that it is an array of
/// strings, and that none is listed twice.
pub(super) fn parse(
    skeleton: &SkeletonIdentity,
    metadata: &toml::Table,
) -> Result<Vec<String>, RenderError> {
    let key = format!("{ROOT}.verbatim");
    let wrong_type = |expected| {
        RenderError::about_manifest(
            skeleton.clone(),
            Reason::WrongType {
                key: key.clone(),
                expected,
            },
        )
    };

    let entries = match metadata.get("verbatim") {
        None => return Ok(Vec::new()),
        Some(toml::Value::Array(entries)) => entries,
        Some(_not_an_array) => return Err(wrong_type(TomlType::Array)),
    };

    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let toml::Value::String(path) = entry else {
            return Err(wrong_type(TomlType::String));
        };
        paths.push(path.clone());
    }

    let mut seen = BTreeSet::new();
    for path in &paths {
        if !seen.insert(path.as_str()) {
            return Err(RenderError::about_manifest(
                skeleton.clone(),
                Reason::VerbatimPathListedTwice { path: path.clone() },
            ));
        }
    }

    assert_eq!(
        seen.len(),
        paths.len(),
        "a parsed verbatim list holds each path once"
    );
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::{Classified, VerbatimFiles, classify, parse};
    use crate::skeleton::declarations::Schema;
    use crate::skeleton::error::{Reason, RenderError, SkeletonIdentity, TomlType};
    use crate::skeleton::keyed::{Keyed, KeyedBuilder};
    use crate::skeleton::template::Partial;
    use crate::skeleton::walk::TreePath;

    fn skeleton() -> SkeletonIdentity {
        SkeletonIdentity::Named("test".to_owned())
    }

    fn table(toml_text: &str) -> toml::Table {
        toml_text.parse().expect("test fixture must be valid TOML")
    }

    /// A walked file set holding exactly `paths`, each `/`-separated.
    fn files(paths: &[&str]) -> BTreeSet<TreePath> {
        paths
            .iter()
            .map(|path| TreePath::for_test(&path.split('/').collect::<Vec<_>>()))
            .collect()
    }

    /// A walked partial list holding exactly `names`, in path order.
    fn partials(names: &[&str]) -> Keyed<Partial, TreePath> {
        let sorted: BTreeSet<TreePath> = names
            .iter()
            .map(|name| TreePath::for_test(&[name]))
            .collect();
        let mut builder = KeyedBuilder::new();
        for path in sorted {
            builder.push(path);
        }
        builder.finish()
    }

    fn link_declared(
        declared: &[&str],
        file_paths: &BTreeSet<TreePath>,
        partial_paths: &Keyed<Partial, TreePath>,
    ) -> Result<VerbatimFiles, RenderError> {
        let declared: Vec<String> = declared.iter().map(|entry| (*entry).to_owned()).collect();
        VerbatimFiles::link(&skeleton(), &declared, file_paths, partial_paths)
    }

    fn assert_refused(result: Result<VerbatimFiles, RenderError>, wording: &str) {
        let error = result.expect_err("the declaration must be refused");
        assert_eq!(error.file(), Some("Cargo.toml"));
        assert_eq!(error.line(), None);
        assert_eq!(error.reason().to_string(), wording);
    }

    fn names_of(partial_names: &[&'static str]) -> BTreeSet<&'static str> {
        partial_names.iter().copied().collect()
    }

    #[test]
    fn an_absent_key_and_an_empty_array_declare_the_same_nothing() {
        assert_eq!(
            parse(&skeleton(), &table("")).expect("absent"),
            Vec::<String>::new()
        );
        assert_eq!(
            parse(&skeleton(), &table("verbatim = []")).expect("empty"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn entries_are_read_in_declared_order() {
        let paths = parse(&skeleton(), &table(r#"verbatim = ["b.yml", "a.yml"]"#))
            .expect("two strings are well-formed");
        assert_eq!(paths, vec!["b.yml".to_owned(), "a.yml".to_owned()]);
    }

    #[test]
    fn a_value_that_is_not_an_array_is_refused_as_the_wrong_type() {
        let error = parse(&skeleton(), &table(r#"verbatim = "a.yml""#)).expect_err("a string");
        assert!(matches!(
            error.reason(),
            Reason::WrongType { key, expected: TomlType::Array }
                if key == "package.metadata.skeletons.verbatim"
        ));
    }

    #[test]
    fn an_entry_that_is_not_a_string_is_refused_as_the_wrong_type() {
        let error = parse(&skeleton(), &table(r#"verbatim = ["a.yml", 1]"#)).expect_err("a number");
        assert!(matches!(
            error.reason(),
            Reason::WrongType { key, expected: TomlType::String }
                if key == "package.metadata.skeletons.verbatim"
        ));
    }

    #[test]
    fn a_path_listed_twice_is_refused_naming_it() {
        let error = parse(
            &skeleton(),
            &table(r#"verbatim = ["a.yml", "b.yml", "a.yml"]"#),
        )
        .expect_err("a repeat");
        assert!(matches!(
            error.reason(),
            Reason::VerbatimPathListedTwice { path } if path == "a.yml"
        ));
    }

    #[test]
    fn spellings_that_differ_are_two_paths_not_a_repeat() {
        // Comparison is by exact string equality, so `./a.yml` and `a.yml`
        // are two entries here; linking then refuses the one that names no
        // file, and never folds it into the other.
        parse(
            &skeleton(),
            &table(r#"verbatim = ["a.yml", "./a.yml", "A.yml"]"#),
        )
        .expect("three distinct spellings");
    }

    #[test]
    fn a_wrong_shape_is_reported_before_a_repeat() {
        let error = parse(&skeleton(), &table(r#"verbatim = ["a.yml", "a.yml", 1]"#))
            .expect_err("a number and a repeat");
        assert!(matches!(error.reason(), Reason::WrongType { .. }));
    }

    #[test]
    fn a_file_is_linked_by_its_exact_path() {
        let file_paths = files(&["ci.yml", ".github/workflows/ci.yml"]);
        let linked = link_declared(&[".github/workflows/ci.yml"], &file_paths, &partials(&[]))
            .expect("a walked file links");

        assert!(linked.contains(&TreePath::for_test(&[".github", "workflows", "ci.yml"])));
        assert!(!linked.contains(&TreePath::for_test(&["ci.yml"])));
    }

    #[test]
    fn nothing_declared_links_the_empty_set() {
        let linked =
            link_declared(&[], &files(&["ci.yml"]), &partials(&[])).expect("nothing to link");
        assert!(!linked.contains(&TreePath::for_test(&["ci.yml"])));
    }

    #[test]
    fn a_file_classifies_as_a_file() {
        let file_paths = files(&["a/b.yml", "c.yml"]);
        assert_eq!(
            classify("a/b.yml", &file_paths, &names_of(&[])),
            Classified::File(&TreePath::for_test(&["a", "b.yml"]))
        );
    }

    #[test]
    fn a_directory_classifies_as_a_directory_with_or_without_a_trailing_slash() {
        let file_paths = files(&[".github/workflows/ci.yml", "z.yml"]);
        for entry in [
            ".github",
            ".github/",
            ".github/workflows",
            ".github/workflows/",
        ] {
            assert_eq!(
                classify(entry, &file_paths, &names_of(&[])),
                Classified::Directory,
                "{entry:?} names a directory"
            );
        }
    }

    #[test]
    fn a_name_that_only_begins_like_a_directory_is_not_one() {
        // `.git` is a prefix of `.github` as text but not as a path.
        let file_paths = files(&[".github/ci.yml"]);
        assert_eq!(
            classify(".git", &file_paths, &names_of(&[])),
            Classified::NoFile
        );
    }

    #[test]
    fn a_trailing_slash_never_makes_a_file_of_a_path() {
        let file_paths = files(&["a.yml"]);
        assert_eq!(
            classify("a.yml/", &file_paths, &names_of(&[])),
            Classified::NoFile
        );
    }

    #[test]
    fn a_partial_classifies_as_a_partial_in_either_spelling() {
        let file_paths = files(&["ci.yml"]);
        let partial_names = names_of(&["cargo.yml"]);
        for entry in ["cargo.yml", "partials/cargo.yml"] {
            assert_eq!(
                classify(entry, &file_paths, &partial_names),
                Classified::Partial,
                "{entry:?} names a partial"
            );
        }
    }

    #[test]
    fn a_file_of_the_same_name_as_a_partial_is_a_file() {
        // `files/cargo.yml` and `partials/cargo.yml` can both exist; the
        // entry names the file, and is not refused for the partial.
        let file_paths = files(&["cargo.yml"]);
        assert_eq!(
            classify("cargo.yml", &file_paths, &names_of(&["cargo.yml"])),
            Classified::File(&TreePath::for_test(&["cargo.yml"]))
        );
    }

    #[test]
    fn everything_else_classifies_as_no_file() {
        let file_paths = files(&["a.yml", "dir/b.yml"]);
        let partial_names = names_of(&["cargo.yml"]);
        for entry in [
            "",
            "/",
            "//",
            "missing.yml",
            "files/a.yml",
            "A.yml",
            "./a.yml",
            "a.yml ",
            "dir//b.yml",
            "partials/missing.yml",
            "partials/",
            "partials/partials/cargo.yml",
        ] {
            assert_eq!(
                classify(entry, &file_paths, &partial_names),
                Classified::NoFile,
                "{entry:?} names nothing"
            );
        }
    }

    #[test]
    fn each_refusal_has_its_own_wording() {
        let file_paths = files(&[".github/workflows/ci.yml", "present.yml"]);
        let partial_paths = partials(&["cargo.yml"]);

        assert_refused(
            link_declared(&[".github/workflows"], &file_paths, &partial_paths),
            "verbatim path `.github/workflows` names a directory; list each file under it instead",
        );
        assert_refused(
            link_declared(&["cargo.yml"], &file_paths, &partial_paths),
            "verbatim path `cargo.yml` names a partial, and only a file under `files/` can be \
             verbatim",
        );
        assert_refused(
            link_declared(&["partials/cargo.yml"], &file_paths, &partial_paths),
            "verbatim path `partials/cargo.yml` names a partial, and only a file under `files/` \
             can be verbatim",
        );
        assert_refused(
            link_declared(&["files/present.yml"], &file_paths, &partial_paths),
            "verbatim path `files/present.yml` names no file; verbatim paths are relative to \
             `files/`",
        );
        assert_refused(
            link_declared(&[""], &file_paths, &partial_paths),
            "verbatim path `` names no file; verbatim paths are relative to `files/`",
        );
    }

    #[test]
    fn the_first_bad_entry_in_declared_order_is_the_one_refused() {
        let file_paths = files(&["good.yml"]);
        let error = link_declared(
            &["good.yml", "second", "first"],
            &file_paths,
            &partials(&[]),
        )
        .expect_err("two entries name nothing");
        assert!(matches!(
            error.reason(),
            Reason::VerbatimPathNamesNoFile { path } if path == "second"
        ));
    }

    /// Links `manifest_text` through the schema the way a load does: parsed,
    /// then linked against `files/` holding `file_names` and `partials/`
    /// holding `partial_names`.
    fn load_manifest(
        manifest_text: &str,
        file_names: &[&str],
        partial_names: &[&str],
    ) -> Result<(), RenderError> {
        let schema = Schema::parse(&skeleton(), &table(manifest_text))?;
        schema
            .link(&skeleton(), &files(file_names), &partials(partial_names))
            .map(|_declarations| ())
    }

    #[test]
    fn a_partial_mapping_defect_is_reported_before_a_verbatim_path_defect() {
        // The manifest has both: a `set` value mapped to a partial that does
        // not exist, and a verbatim path naming no file. The mapping is the
        // earlier phase, so it is the refusal that comes back.
        let manifest = r#"
            verbatim = ["missing.yml"]

            [options.tools]
            type = "set"
            default = []

            [[options.tools.values]]
            value = "cargo"
            partial = "absent.yml"
        "#;
        let error = load_manifest(manifest, &["present.yml"], &[]).expect_err("both defects");
        assert!(
            matches!(error.reason(), Reason::PartialNotFound { .. }),
            "expected the partial mapping refused first, got {error:?}"
        );
    }

    #[test]
    fn a_partial_no_value_names_is_reported_before_a_verbatim_path_defect() {
        let error = load_manifest(
            r#"verbatim = ["missing.yml"]"#,
            &["present.yml"],
            &["stray.yml"],
        )
        .expect_err("both defects");
        assert!(
            matches!(error.reason(), Reason::PartialSelectedByNothing { .. }),
            "expected the unmapped partial refused first, got {error:?}"
        );
    }

    #[test]
    fn an_options_defect_is_reported_before_a_verbatim_shape_defect() {
        let error =
            load_manifest("verbatim = 1\noptions = 2", &["a.yml"], &[]).expect_err("both defects");
        assert!(
            matches!(
                error.reason(),
                Reason::WrongType { key, .. } if key == "package.metadata.skeletons.options"
            ),
            "expected `options` refused first, got {error:?}"
        );
    }

    #[test]
    fn verbatim_is_a_known_key_and_any_other_is_still_unknown() {
        load_manifest(r#"verbatim = ["a.yml"]"#, &["a.yml"], &[]).expect("`verbatim` is known");
        let error = load_manifest("verbatims = []", &["a.yml"], &[]).expect_err("not a key");
        assert!(matches!(
            error.reason(),
            Reason::UnknownKey { key } if key == "package.metadata.skeletons.verbatims"
        ));
    }

    #[test]
    fn a_verbatim_declaration_needs_no_options_table() {
        load_manifest(r#"verbatim = ["a.yml"]"#, &["a.yml"], &[]).expect("declares no options");
    }

    proptest! {
        /// The property: whatever the entry and whatever the skeleton
        /// ships, a linked set holds only paths from the walked file set,
        /// and an entry is linked exactly when it is one of them. The input
        /// space is short entries over an alphabet of `a`, `.` and `/`,
        /// which reaches empty entries, doubled and trailing slashes and
        /// dot segments, against a small fixed skeleton.
        #[test]
        fn a_linked_path_is_always_a_walked_file(entry in "[a./]{0,6}") {
            let file_paths = files(&["a", "a.a/a", ".a/a.a", "aa/a/a"]);
            let partial_paths = partials(&["a.a", "aa"]);
            let linked = link_declared(&[&entry], &file_paths, &partial_paths);
            let names_a_file = file_paths.iter().any(|path| path.as_str() == entry);
            match linked {
                Ok(linked) => {
                    prop_assert!(names_a_file, "{entry:?} linked but names no walked file");
                    let path = file_paths.get(entry.as_str()).expect("a linked entry is a file");
                    prop_assert!(linked.contains(path));
                }
                Err(_refusal) => {
                    prop_assert!(!names_a_file, "{entry:?} names a file but was refused");
                }
            }
        }
    }
}
