use std::fmt::Write as _;
use std::path::Path;

use tempfile::TempDir;

use super::declarations::ROOT;
use super::limits::RENDERED_BYTES_MAX;
use super::{Choice, Choices, Reason, RenderError, render};

/// The length of the one value of the `flavor` enum: what each line of
/// `files/large.txt` grows by, so enough lines make the largest render
/// too large.
const FLAVOR_LENGTH: usize = 4096;

/// The length of the `note` text the wearer gives when that choice is
/// the defect: what each line of `files/notes.txt` grows by, so its
/// many lines make the chosen render too large.
const NOTE_LENGTH: usize = 4096;

/// A file's bytes that are not UTF-8 and hold a `{{` that would be refused
/// as a template: what `files/blob.bin` holds, which only `verbatim`
/// lets a skeleton ship.
const NOT_UTF_8: [u8; 3] = [0xFF, b'{', b'{'];

/// One thing wrong with the test skeleton, each the single reason
/// [`render`] refuses at one stage of its documented order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Defect {
    /// `Cargo.toml` has no `package.name`.
    PackageName,
    /// An option declares a `type` that does not exist.
    OptionType,
    /// Two `set` values map the same partial.
    PartialMappedTwice,
    /// The `verbatim` list holds a value that is not a string.
    VerbatimShape,
    /// The `verbatim` list names one file twice.
    VerbatimRepeat,
    /// A file under `files/` is named like a Cargo manifest, which the
    /// directory walk refuses.
    WalkedManifestName,
    /// A `set` value maps a partial that does not exist.
    MappedPartialMissing,
    /// A partial exists that no `set` value maps.
    PartialUnmapped,
    /// The `verbatim` list names a path that is no file under `files/`.
    VerbatimNamesNoFile,
    /// A file under `files/` holds a `{{` that is not a placeholder.
    FileTemplate,
    /// A partial holds a `{{` that is not a placeholder.
    PartialTemplate,
    /// An option is declared that nothing uses.
    OptionUnused,
    /// The largest render any choice could produce is past the size
    /// limit.
    LargestRender,
    /// The wearer chooses an option nothing declares.
    UndeclaredChoice,
    /// The wearer's own text makes the chosen render past the size
    /// limit.
    ChosenRender,
}

/// Every [`Defect`], in the order [`render`]'s documentation says the
/// refusals come: the order this test holds the code to. Each stage is
/// the one the defect at the same position makes.
const DOCUMENTED_ORDER: &[Defect] = &[
    Defect::PackageName,
    Defect::OptionType,
    Defect::PartialMappedTwice,
    Defect::VerbatimShape,
    Defect::VerbatimRepeat,
    Defect::WalkedManifestName,
    Defect::MappedPartialMissing,
    Defect::PartialUnmapped,
    Defect::VerbatimNamesNoFile,
    Defect::FileTemplate,
    Defect::PartialTemplate,
    Defect::OptionUnused,
    Defect::LargestRender,
    Defect::UndeclaredChoice,
    Defect::ChosenRender,
];

impl Defect {
    /// Whether `reason` is the refusal this defect causes, recognised by
    /// the reason's variant and never by its message.
    fn is_refused_by(self, reason: &Reason) -> bool {
        match self {
            Self::PackageName => matches!(reason, Reason::PackageNameMissing),
            Self::OptionType => matches!(reason, Reason::OptionTypeUnknown { .. }),
            Self::PartialMappedTwice => {
                matches!(reason, Reason::PartialSelectedTwice { .. })
            }
            Self::VerbatimShape => matches!(
                reason,
                Reason::WrongType { key, .. } if *key == format!("{ROOT}.verbatim")
            ),
            Self::VerbatimRepeat => {
                matches!(reason, Reason::VerbatimPathListedTwice { .. })
            }
            Self::WalkedManifestName => matches!(reason, Reason::NestedManifest),
            Self::MappedPartialMissing => matches!(reason, Reason::PartialNotFound { .. }),
            Self::PartialUnmapped => {
                matches!(reason, Reason::PartialSelectedByNothing { .. })
            }
            Self::VerbatimNamesNoFile => {
                matches!(reason, Reason::VerbatimPathNamesNoFile { .. })
            }
            Self::FileTemplate => matches!(reason, Reason::PlaceholderMalformed),
            Self::PartialTemplate => {
                matches!(reason, Reason::PlaceholderMalformedInPartial)
            }
            Self::OptionUnused => matches!(reason, Reason::OptionUnused { .. }),
            Self::LargestRender => matches!(
                reason,
                Reason::TooManyRenderedBytes { bytes_max } if *bytes_max == RENDERED_BYTES_MAX
            ),
            Self::UndeclaredChoice => matches!(reason, Reason::UndeclaredOption { .. }),
            Self::ChosenRender => matches!(
                reason,
                Reason::ChoicesRenderTooLarge { option, .. } if option == "note"
            ),
        }
    }
}

/// How many lines, each holding one value of `value_length` bytes and its
/// newline, make a render one line past the size limit.
fn lines_past_the_limit(value_length: usize) -> usize {
    let line_length = u64::try_from(value_length + 1).expect("a line's length fits a u64");
    usize::try_from(RENDERED_BYTES_MAX / line_length + 1).expect("a line count fits a usize")
}

/// The `Cargo.toml`'s `[package]` table, without its `name` when that is
/// the defect.
fn package_section(planted: &[Defect]) -> String {
    let name = if planted.contains(&Defect::PackageName) {
        ""
    } else {
        "name = \"demo\"\n"
    };
    format!("[package]\n{name}version = \"0.0.0\"\n")
}

/// The skeleton's own table, holding its `verbatim` list: one real file,
/// then one entry for each `verbatim` defect planted.
fn skeletons_section(planted: &[Defect]) -> String {
    let mut entries = vec!["\"blob.bin\""];
    if planted.contains(&Defect::VerbatimRepeat) {
        entries.push("\"blob.bin\"");
    }
    if planted.contains(&Defect::VerbatimShape) {
        entries.push("7");
    }
    if planted.contains(&Defect::VerbatimNamesNoFile) {
        entries.push("\"nowhere.bin\"");
    }
    format!("[{ROOT}]\nverbatim = [{}]\n", entries.join(", "))
}

/// Every option section but `workflows`: the `enum` and `text` options, each
/// used by a file, and the `broken` and `spare` options that are defects when
/// planted.
fn non_workflow_option_sections(planted: &[Defect]) -> Vec<String> {
    let flavor = "x".repeat(FLAVOR_LENGTH);
    let mut sections = vec![
        format!(
            "[{ROOT}.options.flavor]\ntype = \"enum\"\nvalues = [\"{flavor}\"]\n\
             default = \"{flavor}\"\n"
        ),
        format!("[{ROOT}.options.note]\ntype = \"text\"\n"),
    ];
    if planted.contains(&Defect::OptionType) {
        sections.push(format!("[{ROOT}.options.broken]\ntype = \"bogus\"\n"));
    }
    if planted.contains(&Defect::OptionUnused) {
        sections.push(format!(
            "[{ROOT}.options.spare]\ntype = \"text\"\ndefault = \"x\"\n"
        ));
    }
    sections
}

/// The `workflows` `set` option: two values mapping the two partials that
/// exist, bent by each partial-mapping defect planted.
fn workflows_section(planted: &[Defect]) -> String {
    let second_partial = if planted.contains(&Defect::PartialMappedTwice) {
        "lint.yml"
    } else {
        "test.yml"
    };
    let mut values = vec![("lint", "lint.yml"), ("test", second_partial)];
    if planted.contains(&Defect::MappedPartialMissing) {
        values.push(("deploy", "missing.yml"));
    }

    let mut section = format!("[{ROOT}.options.workflows]\ntype = \"set\"\ndefault = []\n");
    for (value, partial) in values {
        write!(
            section,
            "\n[[{ROOT}.options.workflows.values]]\nvalue = \"{value}\"\n\
             partial = \"{partial}\"\n"
        )
        .expect("writing to a String cannot fail");
    }
    section
}

/// The whole `Cargo.toml` with every defect in `planted` in it.
fn manifest_text(planted: &[Defect]) -> String {
    let mut sections = vec![package_section(planted), skeletons_section(planted)];
    sections.extend(non_workflow_option_sections(planted));
    sections.push(workflows_section(planted));
    sections.join("\n")
}

/// Every file under `files/` and `partials/`, as its path in the skeleton
/// and its bytes, with every defect in `planted` in it.
fn tree_files(planted: &[Defect]) -> Vec<(&'static str, Vec<u8>)> {
    let large_lines = if planted.contains(&Defect::LargestRender) {
        lines_past_the_limit(FLAVOR_LENGTH)
    } else {
        1
    };
    let lint: &[u8] = if planted.contains(&Defect::PartialTemplate) {
        b"{{\n"
    } else {
        b"lint: true\n"
    };
    let mut files = vec![
        (
            "files/ci.yml",
            b"jobs:\n# skeletons:partial workflows\ndone: true\n".to_vec(),
        ),
        (
            "files/notes.txt",
            "{{note}}\n"
                .repeat(lines_past_the_limit(NOTE_LENGTH))
                .into_bytes(),
        ),
        (
            "files/large.txt",
            "{{flavor}}\n".repeat(large_lines).into_bytes(),
        ),
        ("files/blob.bin", NOT_UTF_8.to_vec()),
        ("partials/lint.yml", lint.to_vec()),
        ("partials/test.yml", b"test: true\n".to_vec()),
    ];
    if planted.contains(&Defect::WalkedManifestName) {
        files.push(("files/Cargo.toml", b"nested\n".to_vec()));
    }
    if planted.contains(&Defect::FileTemplate) {
        files.push(("files/bad.txt", b"{{\n".to_vec()));
    }
    if planted.contains(&Defect::PartialUnmapped) {
        files.push(("partials/orphan.yml", b"orphan: true\n".to_vec()));
    }
    files
}

/// The wearer's choices, with every defect in `planted` among them.
fn choices_for(planted: &[Defect]) -> Choices {
    let mut choices = Choices::new();
    if planted.contains(&Defect::UndeclaredChoice) {
        choices.insert("undeclared", Choice::One("x".to_owned()));
    }
    if planted.contains(&Defect::ChosenRender) {
        choices.insert("note", Choice::One("x".repeat(NOTE_LENGTH)));
    }
    choices
}

/// Writes `manifest_text` and `files` (each a path in the skeleton and
/// its bytes) into a fresh scratch directory, in the order given, and
/// renders it with `choices`.
fn render_tree(
    manifest_text: &str,
    files: Vec<(&str, Vec<u8>)>,
    choices: &Choices,
) -> Result<(), RenderError> {
    let directory = TempDir::new().expect("create scratch directory");
    let root: &Path = directory.path();
    std::fs::write(root.join("Cargo.toml"), manifest_text).expect("write Cargo.toml");
    for (relative, bytes) in files {
        let path = root.join(relative);
        let parent = path.parent().expect("a skeleton file has a directory");
        std::fs::create_dir_all(parent).expect("create the file's directory");
        std::fs::write(&path, bytes).expect("write skeleton file");
    }
    render(root, choices).map(|_rendering| ())
}

/// A `Cargo.toml` that declares a skeleton and nothing else: no options
/// and no `verbatim` files.
fn bare_manifest_text() -> String {
    format!("{}\n[{ROOT}]\n", package_section(&[]))
}

/// Renders the skeleton with exactly the defects in `planted` in it.
fn render_planted(planted: &[Defect]) -> Result<(), RenderError> {
    render_tree(
        &manifest_text(planted),
        tree_files(planted),
        &choices_for(planted),
    )
}

/// Pins the order of refusals stated in [`render`]'s documentation.
#[test]
fn a_skeleton_is_refused_in_the_order_render_documents() {
    // Plants one defect at every stage of the documented order and
    // renders: the refusal must be the first stage's. Then removes that
    // stage's defect and renders again: the refusal must be the second
    // stage's, and so on down the order. A code order that runs any two
    // stages the other way round, adjacent or not, meets a later stage's
    // refusal on the earlier stage's turn. A refusal is recognised by its
    // `Reason` variant, never its message. With every defect removed the
    // skeleton must render.
    for (stage_index, expected) in DOCUMENTED_ORDER.iter().enumerate() {
        let planted = &DOCUMENTED_ORDER[stage_index..];

        let Err(error) = render_planted(planted) else {
            panic!("with {planted:?} planted, the skeleton rendered; expected {expected:?}");
        };

        assert!(
            expected.is_refused_by(error.reason()),
            "with {planted:?} planted, expected the refusal of {expected:?}, got {error:?}"
        );
    }

    render_planted(&[]).expect("the skeleton with every defect removed must render");
}

/// Pins the clause "then files in path order" of [`render`]'s
/// documentation.
#[test]
fn files_are_refused_in_path_order() {
    // Two files hold the same defect, a `{{` that is not a placeholder.
    // `files/b.txt` sorts after `files/a.txt`, so the refusal that names
    // `files/a.txt` comes from reading the files in path order, and a read
    // in the reverse of path order would name `files/b.txt`. The directory
    // walk sorts what it lists, so the order the files were written in never
    // reaches the code under test; the files are listed `b` first only so
    // the test does not lean on it. A refusal is recognised by its `Reason`
    // variant and the file it names, never its message.
    let files = vec![
        ("files/b.txt", b"{{\n".to_vec()),
        ("files/a.txt", b"{{\n".to_vec()),
    ];

    let error = render_tree(&bare_manifest_text(), files, &Choices::new())
        .expect_err("two malformed files must be refused");

    assert!(
        matches!(error.reason(), Reason::PlaceholderMalformed),
        "expected a malformed placeholder, got {error:?}"
    );
    assert_eq!(error.file(), Some("files/a.txt"));
}

/// Pins the clause "then partials in path order" of [`render`]'s
/// documentation.
#[test]
fn partials_are_refused_in_path_order_whatever_order_they_are_mapped() {
    // Two partials hold the same defect. The `set` option maps `b.yml`
    // by its first value and `a.yml` by its second, so the refusal that
    // names `partials/a.yml` can only come from reading the partials in
    // path order, not in the order the mapping lists them or the reverse of
    // path order. The order the files were written in never reaches the
    // code under test, so the test does not claim it.
    let manifest = format!(
        "{}\n[{ROOT}.options.tools]\ntype = \"set\"\ndefault = []\n\n\
         [[{ROOT}.options.tools.values]]\nvalue = \"one\"\npartial = \"b.yml\"\n\n\
         [[{ROOT}.options.tools.values]]\nvalue = \"two\"\npartial = \"a.yml\"\n",
        package_section(&[])
    );
    let files = vec![
        ("files/ci.yml", b"# skeletons:partial tools\n".to_vec()),
        ("partials/b.yml", b"{{\n".to_vec()),
        ("partials/a.yml", b"{{\n".to_vec()),
    ];

    let error = render_tree(&manifest, files, &Choices::new())
        .expect_err("two malformed partials must be refused");

    assert!(
        matches!(error.reason(), Reason::PlaceholderMalformedInPartial),
        "expected a malformed placeholder in a partial, got {error:?}"
    );
    assert_eq!(error.file(), Some("partials/a.yml"));
}

/// Pins the clause "the wearer's own choices, each in option-name order"
/// of [`render`]'s documentation.
#[test]
fn choices_are_refused_in_option_name_order() {
    // The wearer chooses two options nothing declares. `resolve` walks the
    // choices in option-name order, so the refusal names `alpha`; a walk in
    // any other order, such as the reverse, would refuse `beta` first.
    // `Choices` keeps its entries sorted by name, so the order they are
    // inserted in is erased and is not what this test distinguishes. The
    // refusal is recognised by its `Reason` variant and the option it names,
    // never its message.
    let files = vec![("files/plain.txt", b"plain\n".to_vec())];
    let mut choices = Choices::new();
    choices.insert("beta", Choice::One("x".to_owned()));
    choices.insert("alpha", Choice::One("x".to_owned()));

    let error = render_tree(&bare_manifest_text(), files, &choices)
        .expect_err("two undeclared options must be refused");

    assert!(
        matches!(
            error.reason(),
            Reason::UndeclaredOption { option } if option == "alpha"
        ),
        "expected the undeclared option `alpha`, got {error:?}"
    );
}
