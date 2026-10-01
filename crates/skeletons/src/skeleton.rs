//! A skeleton: a crate of data — metadata, whole files and partials — read from
//! its directory and rendered into the bytes a wearing repository should
//! hold.
//!
//! [`render`] is the whole of this module's reason to exist. It reads a skeleton
//! directory in three phases: **load** (manifest → option schema; directory
//! walk → file and partial paths; schema linked against the partial and file
//! paths → declarations; every file and partial parsed into a template, checked
//! against the declarations as it goes; every option used, and the largest
//! render any choice could produce within the render's size limit → a
//! validated skeleton), **resolve** (the wearer's choices against the
//! declarations, and the render they produce within the size limit), and
//! **assemble** (the validated skeleton and resolved choices → bytes). A
//! skeleton is validated whole before any of it is assembled, so a defect in a
//! partial nobody selected is refused exactly like one in a partial everybody
//! sees — the render is the contract, and a contract
//! cannot be sound for some option values and broken for others (tested by
//! `crates/skeletons/src/acceptance/validation.rs` →
//! `a_defect_in_a_partial_the_choice_does_not_select_is_still_refused`).
//!
//! Validation keeps what it proves. A placeholder holds the key of the
//! `enum` or `text` option it fills from, a directive the key of its `set`
//! option, a `set` value the key of its partial, and the resolved choices hold
//! one entry for every key there is — so resolving, sizing and assembly follow
//! keys and never look a name up again.
//!
//! Everything below `load` is one concept per file (lines, names, values,
//! placeholders and directives are the text a skeleton is written in; the
//! manifest and its declarations are the schema, and `verbatim` is the part of
//! it naming the files shipped as bytes; keyed lists are how one
//! validated part refers to another; the walk and its limits are the file
//! sets; folding and siblings decide when two entry names are one name;
//! templates are parsed text and a shipped file is one file under `files/`,
//! parsed or held as bytes; the validated skeleton is all of it, checked;
//! measure is how many bytes a render can produce; choices are the wearer's
//! input; rendering is assembly).

mod choices;
mod declarations;
mod directive;
mod error;
mod escaped;
mod folding;
mod keyed;
mod limits;
mod lines;
mod manifest;
mod measure;
mod name;
mod placeholder;
// Builds its skeleton in a scratch directory rather than reading
// `test-skeletons/`, so unlike the `acceptance` tests that read it, it runs in
// the published package too.
#[cfg(test)]
mod refusal_order_tests;
mod rendering;
mod shipped_file;
mod siblings;
mod template;
mod validated;
mod value;
mod verbatim;
mod walk;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) use choices::{Choice, Choices};
pub(crate) use error::{RenderError, SkeletonIdentity};
pub(crate) use escaped::Escaped;
// Re-exported so every other module that decides whether two names are one
// name folds them the same way this module's own walk folds two entry names
// into one: the two must never disagree about which two names are one name.
pub(crate) use folding::{FoldedName, ascii_names_collide};
#[cfg(test)]
pub(crate) use folding::{slow_fold, tricky_name};
pub(crate) use rendering::Rendering;

// `OptionKind` and `Reason` name the render's own internal vocabulary — a
// `RenderError`'s `reason()` is read by `check` only through its `Display`
// text and its `file()`/`line()` locations, never by matching a `Reason`
// variant, so only test code outside `skeleton` names
// either type: the render's acceptance suite (`acceptance/`) and the tests of
// `check` and `survey` that build a `RenderError`. `#[cfg(test)]` says so
// outright, rather than a module-wide dead-code expectation hiding it.
// `OptionKind` is named only by the acceptance submodules that read
// `test-skeletons/`, so it is re-exported only where those are compiled.
#[cfg(all(test, skeletons_checkout))]
pub(crate) use error::OptionKind;
#[cfg(test)]
pub(crate) use error::Reason;

use declarations::{Declarations, Schema};
use keyed::{Keyed, KeyedBuilder};
use shipped_file::ShippedFile;
use template::Partial;
use validated::ValidatedSkeleton;
use walk::TreePath;

/// Reads the skeleton at `skeleton_directory`, validates it whole, resolves
/// `choices` against its declared options, and returns every file under
/// `files/` rendered — or the first refusal, in a fixed order: the
/// manifest (its `options`, among them that no partial is mapped twice, then
/// the shape and repeats of its `verbatim` list), then the directory walk,
/// then the partial mapping (every mapped partial exists, every partial is
/// mapped), then the `verbatim` paths (each names a file under `files/`, in
/// declared order), then files in path order, then partials in path order,
/// then whether every declared option is used by something, then whether the
/// largest render any choice could produce fits the render's size limit, then
/// the wearer's own choices, each in option-name order, then whether the
/// render they produce fits the size limit.
///
/// `skeleton_directory` is the skeleton crate's root: the directory holding its
/// `Cargo.toml`, `files/` and `partials/`.
pub(crate) fn render(
    skeleton_directory: impl AsRef<Path>,
    choices: &Choices,
) -> Result<Rendering, RenderError> {
    let (skeleton, validated) = load(skeleton_directory.as_ref())?;
    let resolved = choices::resolve(&skeleton, validated.declarations(), choices)?;
    measure::check_chosen(&skeleton, &validated, &resolved)?;

    Ok(rendering::assemble(&validated, &resolved))
}

/// Reads the skeleton at `skeleton_directory` and validates it whole: the
/// load phase of [`render`], up to and including the check that the largest
/// render any choice could produce fits the render's size limit.
fn load(skeleton_directory: &Path) -> Result<(SkeletonIdentity, ValidatedSkeleton), RenderError> {
    let mut byte_budget = limits::ByteBudget::new();

    let manifest = manifest::read(skeleton_directory, &mut byte_budget)?;
    let skeleton = SkeletonIdentity::Named(manifest.name);
    let schema = Schema::parse(&skeleton, &manifest.metadata)?;

    let mut entry_budget = limits::EntryBudget::new();
    let file_paths = walk::walk_files(skeleton_directory, &skeleton, &mut entry_budget)?;
    let partial_paths = in_path_order(walk::walk_partials(
        skeleton_directory,
        &skeleton,
        &mut entry_budget,
    )?);

    let declarations = schema.link(&skeleton, &file_paths, &partial_paths)?;

    let files = parse_files(
        &skeleton,
        skeleton_directory,
        &file_paths,
        &declarations,
        &mut byte_budget,
    )?;
    let partials = parse_partials(
        &skeleton,
        skeleton_directory,
        &partial_paths,
        &declarations,
        &mut byte_budget,
    )?;

    let validated = ValidatedSkeleton::validate(&skeleton, declarations, files, partials)?;
    Ok((skeleton, validated))
}

/// The walked `partials/` paths as the list that hands out every
/// partial's key: the walk's own path order, position for position.
fn in_path_order(partial_paths: BTreeSet<TreePath>) -> Keyed<Partial, TreePath> {
    let mut keyed = KeyedBuilder::new();
    for path in partial_paths {
        keyed.push(path);
    }
    keyed.finish()
}

/// Reads every file under `files/`, in path order, stopping at the first
/// refusal: a file the declarations name verbatim as the bytes it holds, and
/// every other as text parsed into a [`template::Template`].
fn parse_files(
    skeleton: &SkeletonIdentity,
    skeleton_directory: &Path,
    file_paths: &BTreeSet<TreePath>,
    declarations: &Declarations,
    byte_budget: &mut limits::ByteBudget,
) -> Result<BTreeMap<TreePath, ShippedFile>, RenderError> {
    let mut files = BTreeMap::new();
    for path in file_paths {
        let relative = format!("files/{path}");
        let full_path = skeleton_directory.join("files").join(path.as_str());
        let about_file =
            |reason| RenderError::about_file(skeleton.clone(), relative.clone(), reason);
        let file = if declarations.is_verbatim(path) {
            ShippedFile::Verbatim(limits::read_bytes(&full_path, byte_budget).map_err(about_file)?)
        } else {
            let text = limits::read_utf8(&full_path, byte_budget).map_err(about_file)?;
            let template = template::parse_file(&text, declarations).map_err(|failure| {
                RenderError::about_line(
                    skeleton.clone(),
                    relative.clone(),
                    failure.line,
                    failure.reason,
                )
            })?;
            ShippedFile::Templated(template)
        };
        files.insert(path.clone(), file);
    }
    Ok(files)
}

/// Reads and parses every file under `partials/`, in path order, into a
/// [`template::Partial`], stopping at the first refusal. Made by mapping
/// `partial_paths`, so each partial has the key every `set` value naming it
/// was linked to.
fn parse_partials(
    skeleton: &SkeletonIdentity,
    skeleton_directory: &Path,
    partial_paths: &Keyed<Partial, TreePath>,
    declarations: &Declarations,
    byte_budget: &mut limits::ByteBudget,
) -> Result<Keyed<Partial, Partial>, RenderError> {
    partial_paths.try_map(|_key, path| {
        let relative = format!("partials/{path}");
        let full_path = skeleton_directory.join("partials").join(path.as_str());
        let text = limits::read_utf8(&full_path, byte_budget).map_err(|reason| {
            RenderError::about_file(skeleton.clone(), relative.clone(), reason)
        })?;
        template::parse_partial(&text, declarations).map_err(|failure| {
            RenderError::about_line(
                skeleton.clone(),
                relative.clone(),
                failure.line,
                failure.reason,
            )
        })
    })
}

// Every test here renders a skeleton read from `test-skeletons/`, which the
// published package leaves out, so they run only in a checkout.
#[cfg(all(test, skeletons_checkout))]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Choice, Choices, Declarations, Reason, ShippedFile, SkeletonIdentity, TreePath, render,
    };

    /// One mebibyte, the unit the render size limit is counted in.
    const MEBIBYTE: usize = 1024 * 1024;

    /// The directory of the test skeleton at `relative` under
    /// `crates/skeletons/test-skeletons`. A copy of `test_skeleton` in
    /// `crate::acceptance`, which is private to that module; duplicating
    /// three lines here is cheaper than widening its visibility for one
    /// test.
    fn test_skeleton(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-skeletons")
            .join(relative)
    }

    #[test]
    fn rendered_too_large_is_refused_whatever_the_choices() {
        // The largest render this skeleton's one set option could ever produce
        // (every value selected) is 1025 directives x 8192 bytes, past the
        // rendered-byte budget; refused before choices are looked at, so it
        // is refused the same way whether nothing is chosen or the wearer
        // explicitly selects nothing — a skeleton's validity does not depend on
        // who wears it.
        for choices in [Choices::new(), {
            let mut choices = Choices::new();
            choices.insert("blocks", Choice::Many(Vec::new()));
            choices
        }] {
            let error = render(test_skeleton("refused/rendered-too-large"), &choices).expect_err(
                "a skeleton whose largest render exceeds the byte budget must be refused",
            );
            assert_eq!(error.file(), Some("files/big.txt"));
            assert!(
                matches!(
                    error.reason(),
                    Reason::TooManyRenderedBytes { bytes_max }
                        if *bytes_max == super::limits::RENDERED_BYTES_MAX
                ),
                "expected a rendered-byte-budget refusal, got {error:?}"
            );
        }
    }

    /// A choice set giving each `(option, length)` a value of that many `x`
    /// bytes.
    fn text_choices(values: &[(&str, usize)]) -> Choices {
        let mut choices = Choices::new();
        for (option, length) in values {
            choices.insert(*option, Choice::One("x".repeat(*length)));
        }
        choices
    }

    /// Asserts `error` refuses a render past the size limit as the wearer's
    /// choice, naming no file and naming `option`.
    fn assert_refused_naming(error: &super::RenderError, option: &str) {
        assert_eq!(error.file(), None, "a wearer's choice names no file");
        assert!(
            matches!(
                error.reason(),
                Reason::ChoicesRenderTooLarge { option: named, .. } if named == option
            ),
            "expected a render-too-large refusal naming `{option}`, got {error:?}"
        );
    }

    #[test]
    fn a_value_filling_the_render_to_exactly_the_limit_renders() {
        // 1,024 lines of `{{note}}`, each ending in one newline, so a note of
        // 8,191 bytes makes 1,024 x 8,192 = 8 MiB, which is the limit itself
        // and so allowed.
        let rendering = render(
            test_skeleton("renders/text-many-placeholders"),
            &text_choices(&[("note", 8_191)]),
        )
        .expect("a render of exactly the limit must be allowed");

        assert_eq!(
            rendering.get("notes.txt").map(<[u8]>::len),
            Some(8 * 1024 * 1024)
        );
    }

    #[test]
    fn a_value_one_byte_past_the_limit_is_refused_naming_the_option() {
        // One byte more in the note is one byte more on each of the 1,024
        // lines: 1,024 x 8,193 = 8 MiB + 1,024 bytes.
        let error = render(
            test_skeleton("renders/text-many-placeholders"),
            &text_choices(&[("note", 8_192)]),
        )
        .expect_err("a render past the limit must be refused");

        assert_refused_naming(&error, "note");
    }

    #[test]
    fn a_skeleton_whose_optional_lines_all_drop_validates_and_renders_empty() {
        // Unset, `note` drops all 1,024 lines, so the skeleton validates and
        // its one file is empty.
        let rendering = render(
            test_skeleton("renders/text-many-placeholders"),
            &Choices::new(),
        )
        .expect("the skeleton with `note` unset must render");

        assert_eq!(rendering.get("notes.txt"), Some(b"".as_slice()));
    }

    #[test]
    fn an_undeclared_option_is_refused_before_the_render_is_sized() {
        // The value alone would make the render too large, so the refusal
        // that comes back is the earlier one, about the option nothing
        // declares.
        let error = render(
            test_skeleton("renders/text-many-placeholders"),
            &text_choices(&[("note", 8_192), ("undeclared", 1)]),
        )
        .expect_err("an undeclared option must be refused");

        assert!(
            matches!(
                error.reason(),
                Reason::UndeclaredOption { option } if option == "undeclared"
            ),
            "expected the undeclared option to be refused first, got {error:?}"
        );
    }

    #[test]
    fn a_heavy_default_does_not_take_the_blame_for_the_wearers_text() {
        // `padding` has a 6,000-byte default filling 1,000 lines: 1,000 x
        // 6,001 = 6,001,000 bytes, under the limit alone and the largest
        // contributor. The wearer's `extra` fills 100 lines; at 30,000 bytes
        // that is 100 x 30,001 = 3,000,100 more, 9,001,100 in all and past
        // 8,388,608. The wearer's option is named, not the default's.
        let skeleton = test_skeleton("renders/text-heavy-default");
        render(&skeleton, &text_choices(&[("extra", 1_000)]))
            .expect("the same skeleton with a short value must render");

        let error = render(&skeleton, &text_choices(&[("extra", 30_000)]))
            .expect_err("a render past the limit must be refused");

        assert_refused_naming(&error, "extra");
    }

    #[test]
    fn a_placeholder_in_a_partial_two_directives_insert_counts_twice() {
        // `note` fills the one line of a partial that two directives insert,
        // so 2 placeholders; `other` fills 3 lines of the file. With 2.2 MiB
        // (2,306,867 bytes) of `note` and 1.3 MiB (1,363,148 bytes) of
        // `other`: `note` contributes 2 x 2,306,867 = 4,613,734 and `other`
        // 3 x 1,363,148 = 4,089,444, so `note` is named. Counted once it
        // would be 2,306,867, and `other` would be. Together they make 8.7
        // million bytes, past the limit.
        let skeleton = test_skeleton("renders/text-partial-in-two-directives");
        render(&skeleton, &text_choices(&[("note", 4), ("other", 4)]))
            .expect("the same skeleton with short values must render");

        let error = render(
            &skeleton,
            &text_choices(&[("note", 22 * MEBIBYTE / 10), ("other", 13 * MEBIBYTE / 10)]),
        )
        .expect_err("a render past the limit must be refused");

        assert_refused_naming(&error, "note");
    }

    #[test]
    fn removing_any_one_wearer_value_from_a_refused_set_of_choices_renders() {
        // The refusal names an option whose value the wearer can shorten, and
        // it is only ever useful if doing so helps. Here each value alone is
        // well within the limit (2.5 MiB, and 4 x 1.5 MiB = 6 MiB).
        let skeleton = test_skeleton("renders/text-render-size-by-placeholders");
        let refused = [("narrow", 5 * MEBIBYTE / 2), ("wide", 3 * MEBIBYTE / 2)];
        render(&skeleton, &text_choices(&refused))
            .expect_err("the two values together are past the limit");

        for removed in 0..refused.len() {
            let remaining: Vec<(&str, usize)> = refused
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != removed)
                .map(|(_, pair)| *pair)
                .collect();
            render(&skeleton, &text_choices(&remaining)).unwrap_or_else(|error| {
                panic!("choices {remaining:?} must render once a value is removed: {error}")
            });
        }
    }

    #[test]
    fn a_verbatim_file_is_read_as_bytes_and_never_parsed() {
        // Verifies that reading `files/` follows the declarations: a path
        // they name verbatim is held as the exact bytes it holds, though
        // those are not UTF-8 and hold a `{{` that would be refused as a
        // template, and every other path is parsed as before. Exercised
        // against a real directory holding one file of each kind.
        let directory =
            std::env::temp_dir().join(format!("skeletons-parse-files-test-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("files")).expect("create fixture directory");
        let bytes: &[u8] = &[b'{', b'{', 0xFF, 0x00, 0xFE];
        std::fs::write(directory.join("files/blob.bin"), bytes).expect("write verbatim file");
        std::fs::write(directory.join("files/plain.txt"), "plain\n").expect("write plain file");

        let declarations = Declarations::for_test_verbatim("", &[], &["blob.bin"]);
        let file_paths = BTreeSet::from([
            TreePath::for_test(&["blob.bin"]),
            TreePath::for_test(&["plain.txt"]),
        ]);
        let files = super::parse_files(
            &SkeletonIdentity::Named("test".to_owned()),
            &directory,
            &file_paths,
            &declarations,
            &mut super::limits::ByteBudget::new(),
        )
        .expect("both files read");
        std::fs::remove_dir_all(&directory).expect("clean up fixture directory");

        assert_eq!(
            files.get(&TreePath::for_test(&["blob.bin"])),
            Some(&ShippedFile::Verbatim(bytes.to_vec()))
        );
        assert!(
            matches!(
                files.get(&TreePath::for_test(&["plain.txt"])),
                Some(ShippedFile::Templated(_))
            ),
            "a path the declarations do not name verbatim must be parsed"
        );
    }
}
