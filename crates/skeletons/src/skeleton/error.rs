//! [`RenderError`]: why a skeleton could not be rendered — which skeleton, where,
//! and what is wrong.

use std::fmt;
use std::num::NonZeroU32;
use std::path::PathBuf;

use super::escaped::{Echoed, Escaped};

/// Which skeleton a refusal is about.
///
/// A render always starts out knowing only the directory it was given; the
/// skeleton's own name becomes known partway through reading its manifest, and
/// every refusal from that point on names the skeleton, not the directory —
/// structural, not merely observed: `manifest::read` is the only place
/// `Directory` is ever built, and once it returns successfully, `load` binds
/// one `Named` value and threads that single binding through every later
/// phase, so nothing downstream can reconstruct a `Directory` again.
/// The "after" half is exercised by
/// `crates/skeletons/src/acceptance/validation.rs` →
/// `a_partial_that_is_not_valid_utf8_is_refused_naming_it_even_when_unselected`,
/// whose refusal happens well past the manifest and still names the
/// skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkeletonIdentity {
    /// The skeleton's `package.name`, once its manifest has been read that far.
    Named(String),
    /// The directory `render` was given, for the refusals that happen before
    /// the skeleton's name can be read: `Cargo.toml` unreadable, not TOML, or
    /// without a `package.name` that is a non-empty string.
    Directory(PathBuf),
}

impl fmt::Display for SkeletonIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(formatter, "{}", Escaped(name)),
            Self::Directory(path) => {
                write!(formatter, "{}", Escaped(&path.to_string_lossy()))
            }
        }
    }
}

/// Which of the three option types a declaration turned out to be — carried on
/// a placeholder or directive refusal so it can say "you named a real
/// option, but the wrong kind of one" rather than just "unknown name".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionKind {
    /// Fills: a closed set of values, one chosen, replacing a placeholder.
    Enum,
    /// Selects: a closed set of values, any chosen, each inserting a
    /// partial at a directive.
    Set,
    /// Fills: the wearer's own text, verbatim, replacing a placeholder; with
    /// no default it is optional, and leaving it unset drops the lines
    /// holding its placeholder.
    Text,
}

impl fmt::Display for OptionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Enum => "enum",
            Self::Set => "set",
            Self::Text => "text",
        })
    }
}

/// Which shape of TOML value was expected at a manifest key, when the skeleton
/// author wrote something else there. Displays with its indefinite article,
/// so "an array" reads as English rather than "a array".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TomlType {
    String,
    Array,
    Table,
}

impl fmt::Display for TomlType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::String => "a string",
            Self::Array => "an array",
            Self::Table => "a table",
        })
    }
}

/// What is wrong with a skeleton, or with the choices given to render it.
///
/// Every variant pairs with the `file()` and `line()` [`RenderError`]
/// reports alongside it, documented on each constructor in this module
/// rather than repeated here.
#[derive(Debug)]
pub(crate) enum Reason {
    /// A path that should hold skeleton content could not be read at all.
    Unreadable { cause: std::io::Error },
    /// `Cargo.toml` does not parse as TOML.
    ManifestNotToml { message: String },
    /// `Cargo.toml` has no string `package.name`.
    PackageNameMissing,
    /// `Cargo.toml`'s `package.name` is an empty string.
    PackageNameEmpty,
    /// `Cargo.toml` has no `[package.metadata.skeletons]` table, so it is not a
    /// skeleton.
    NotASkeleton,
    /// A key the schema does not recognise, named by its dotted path from
    /// the manifest root.
    UnknownKey { key: String },
    /// A required key is absent, named by its dotted path.
    MissingKey { key: String },
    /// A key holds a TOML value of the wrong shape.
    WrongType { key: String, expected: TomlType },
    /// An option's own key does not match the name grammar.
    OptionNameInvalid { option: String },
    /// An option declares a `type` other than `enum`, `set` or `text`.
    OptionTypeUnknown { option: String, given: String },
    /// An option declares an empty `values` array.
    NoValues { option: String },
    /// A declared value, or a `text` option's default, is empty or holds a
    /// control character.
    ValueInvalid { option: String, value: String },
    /// One option's `values` names the same value twice.
    ValueDeclaredTwice { option: String, value: String },
    /// An enum or set default names a value the option does not declare.
    DefaultNotDeclared { option: String, value: String },
    /// A set's `default` lists the same value twice.
    DefaultListsValueTwice { option: String, value: String },
    /// A set value's `partial` names a file the skeleton does not ship.
    PartialNotFound {
        option: String,
        value: String,
        partial: String,
    },
    /// A partial file is named by more than one declared value.
    PartialSelectedTwice { partial: String },
    /// A partial file exists but no declared value names it.
    PartialSelectedByNothing { partial: String },
    /// An option is declared but nothing in the skeleton fills or selects it.
    OptionUnused { option: String },
    /// `verbatim` lists the same path twice.
    VerbatimPathListedTwice { path: String },
    /// A `verbatim` path is the directory holding one or more files under
    /// `files/`, rather than one of them.
    VerbatimPathNamesDirectory { path: String },
    /// A `verbatim` path names no file under `files/` but does name a
    /// partial, spelled relative to `partials/` or as its path in the tree.
    VerbatimPathNamesPartial { path: String },
    /// A `verbatim` path names nothing the skeleton ships under `files/`
    /// and no partial either.
    VerbatimPathNamesNoFile { path: String },
    /// `files/` is absent, or exists but holds no file.
    NoFiles,
    /// `files/` or `partials/` exists but is not a directory.
    NotADirectory,
    /// A symbolic link stands where the skeleton needs a real file or
    /// directory: `Cargo.toml`, the `files/` or `partials/` directory itself,
    /// or anything under either.
    SymbolicLink,
    /// An entry under `files/` or `partials/` is neither a file, a
    /// directory, nor a symbolic link.
    NotAFileOrDirectory,
    /// `Cargo.toml` exists but is not a regular file: a directory, a FIFO, a
    /// socket, or a device.
    NotAFile,
    /// An entry's own name is not valid UTF-8.
    PathNotUtf8,
    /// Two entries in one directory under `files/` or `partials/` are one
    /// name to a filesystem that ignores case or Unicode normalization, so
    /// such a filesystem holds only one of them. `other` is the later of the
    /// two in path order, relative to the skeleton's directory like
    /// [`RenderError::file`], which names the earlier.
    NamesCollide { other: String },
    /// An entry under `files/` or `partials/` is named `Cargo.toml`, or a
    /// name that is one name with it to a filesystem that ignores case or
    /// Unicode normalization: Cargo takes the directory holding it for a
    /// package of its own and leaves it out of the skeleton's package.
    NestedManifest,
    /// A directory's listing held more entries than the render's entry
    /// budget allows.
    TooManyEntries { entries_max: u32 },
    /// Reading a file would take the skeleton past the render's byte budget,
    /// which the manifest, every file and every partial share.
    TooManyBytes { bytes_max: u64 },
    /// The largest render any choice could produce holds more bytes than the
    /// render's rendered-byte budget allows.
    TooManyRenderedBytes { bytes_max: u64 },
    /// A file cannot be decoded as UTF-8 text.
    NotUtf8,
    /// A `{{` was not followed, on the same line, by a well-formed name and
    /// `}}`.
    PlaceholderMalformed,
    /// A `{{` in a partial was not followed, on the same line, by a
    /// well-formed name and `}}`. Its own reason, because the fix
    /// [`Self::PlaceholderMalformed`] names, declaring the file verbatim, is
    /// one a partial can never take.
    PlaceholderMalformedInPartial,
    /// A well-formed placeholder does not name a declared `enum` or `text`
    /// option.
    PlaceholderNotFillOption {
        name: String,
        declared_as: Option<OptionKind>,
    },
    /// A `# skeletons:` line is not exactly `# skeletons:partial <option-name>`.
    DirectiveMalformed,
    /// A well-formed directive does not name a declared `set` option.
    DirectiveNotSetOption {
        name: String,
        declared_as: Option<OptionKind>,
    },
    /// A placeholder for an optional `text` option shares its line with a
    /// placeholder for a different option, which dropping the line would
    /// drop with it. `option` is the first optional placeholder on the line
    /// in reading order and `beside` the first placeholder on it naming
    /// anything else.
    OptionalPlaceholderNotAlone { option: String, beside: String },
    /// A directive line was found inside a partial.
    DirectiveInPartial,
    /// A line of a file or a partial reaches the marker's shape once
    /// whitespace, format characters (Unicode general category Cf) and
    /// control characters (general category Cc) are set aside — before the
    /// marker or inside it — but not once only its leading spaces and tabs
    /// are: a no-break space, a zero-width space, a stray control byte or a
    /// byte-order mark hides it.
    HiddenDirective,
    /// A directive sits on a file's final line, which has no terminator.
    DirectiveUnterminated,
    /// A non-empty partial's last line has no terminator.
    PartialUnterminated,
    /// The wearer set a value for an option the skeleton did not declare.
    UndeclaredOption { option: String },
    /// The wearer gave a choice of the wrong shape for the option's kind.
    ChoiceShapeMismatch {
        option: String,
        declared: OptionKind,
    },
    /// The wearer chose a value outside the option's declared set.
    ValueNotDeclared { option: String, value: String },
    /// The wearer's text for a `text` option is empty or holds a control
    /// character.
    TextChoiceInvalid { option: String, value: String },
    /// The render the wearer's choices produce holds more bytes than the
    /// render's rendered-byte budget allows. `option` is the `text` option
    /// whose value contributes the most bytes to it.
    ChoicesRenderTooLarge { bytes_max: u64, option: String },
    /// The wearer's set choice named the same value twice.
    ValueChosenTwice { option: String, value: String },
}

/// The messages too long for an arm of [`Reason`]'s `Display` to hold on one
/// line. Each is written out as it is, not as a format string, so a `{{` in
/// one is the two braces a placeholder opens with.
const NOT_A_SKELETON: &str = "has no `[package.metadata.skeletons]` table, so it is not a skeleton";
const NESTED_MANIFEST: &str = "is named like a Cargo manifest, so Cargo would leave the \
     directory holding it out of the package";
const PLACEHOLDER_MALFORMED: &str = "opens `{{` without a well-formed name and `}}` on the same \
     line; a `{{` that is not a placeholder can stand only in a file declared verbatim";
const PLACEHOLDER_IN_PARTIAL: &str = "opens `{{` without a well-formed name and `}}` on \
     the same line; a partial cannot hold a `{{` that is not a placeholder, so that text belongs \
     in a file under `files/` declared verbatim";
const DIRECTIVE_MALFORMED: &str = "is not exactly `# skeletons:partial <option-name>`";
const HIDDEN_DIRECTIVE: &str = "reads as a directive only once invisible characters before or \
     inside it are set aside";
const DIRECTIVE_UNTERMINATED: &str =
    "a directive is the file's last line, which has no line terminator";
const PARTIAL_UNTERMINATED: &str = "the partial's last line has no line terminator";

// One arm per variant and no wildcard, so a variant added to `Reason` does
// not compile until it has a message: the compiler, not a test, holds every
// variant to having one. An arm whose message takes more than a line
// delegates to a function below named after its variant, so this stays one
// flat list and each function stays short.
//
// The match is long because `Reason` is a closed enum with a large number of
// variants; it is deliberately not split up. Cutting it further would mean
// grouping the variants into sub-enums, which changes the type rather than
// this function.
impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { cause } => unreadable(formatter, cause),
            Self::ManifestNotToml { message } => manifest_not_toml(formatter, message),
            Self::PackageNameMissing => formatter.write_str("has no string `package.name`"),
            Self::PackageNameEmpty => formatter.write_str("has an empty `package.name`"),
            Self::NotASkeleton => formatter.write_str(NOT_A_SKELETON),
            Self::UnknownKey { key } => unknown_key(formatter, key),
            Self::MissingKey { key } => missing_key(formatter, key),
            Self::WrongType { key, expected } => wrong_type(formatter, key, *expected),
            Self::OptionNameInvalid { option } => option_name_invalid(formatter, option),
            Self::OptionTypeUnknown { option, given } => {
                option_type_unknown(formatter, option, given)
            }
            Self::NoValues { option } => no_values(formatter, option),
            Self::ValueInvalid { option, value } => value_invalid(formatter, option, value),
            Self::ValueDeclaredTwice { option, value } => {
                value_declared_twice(formatter, option, value)
            }
            Self::DefaultNotDeclared { option, value } => {
                default_not_declared(formatter, option, value)
            }
            Self::DefaultListsValueTwice { option, value } => {
                default_lists_value_twice(formatter, option, value)
            }
            Self::PartialNotFound {
                option,
                value,
                partial,
            } => partial_not_found(formatter, option, value, partial),
            Self::PartialSelectedTwice { partial } => partial_selected_twice(formatter, partial),
            Self::PartialSelectedByNothing { partial } => {
                partial_selected_by_nothing(formatter, partial)
            }
            Self::OptionUnused { option } => option_unused(formatter, option),
            Self::VerbatimPathListedTwice { path } => verbatim_path_listed_twice(formatter, path),
            Self::VerbatimPathNamesDirectory { path } => {
                verbatim_path_names_directory(formatter, path)
            }
            Self::VerbatimPathNamesPartial { path } => verbatim_path_names_partial(formatter, path),
            Self::VerbatimPathNamesNoFile { path } => verbatim_path_names_no_file(formatter, path),
            Self::NoFiles => formatter.write_str("has no files under `files/`"),
            Self::NotADirectory => formatter.write_str("is not a directory"),
            Self::SymbolicLink => formatter.write_str("is a symbolic link"),
            Self::NotAFileOrDirectory => formatter.write_str("is neither a file nor a directory"),
            Self::NotAFile => formatter.write_str("is not a regular file"),
            Self::PathNotUtf8 => formatter.write_str("has a name that is not valid utf-8"),
            Self::NamesCollide { other } => names_collide(formatter, other),
            Self::NestedManifest => formatter.write_str(NESTED_MANIFEST),
            Self::TooManyEntries { entries_max } => too_many_entries(formatter, *entries_max),
            Self::TooManyBytes { bytes_max } => too_many_bytes(formatter, *bytes_max),
            Self::TooManyRenderedBytes { bytes_max } => {
                too_many_rendered_bytes(formatter, *bytes_max)
            }
            Self::NotUtf8 => formatter.write_str("is not valid utf-8"),
            Self::PlaceholderMalformed => formatter.write_str(PLACEHOLDER_MALFORMED),
            Self::PlaceholderMalformedInPartial => formatter.write_str(PLACEHOLDER_IN_PARTIAL),
            Self::PlaceholderNotFillOption { name, declared_as } => {
                describe_wrong_kind(formatter, "placeholder", "enum or text", name, *declared_as)
            }
            Self::OptionalPlaceholderNotAlone { option, beside } => {
                optional_placeholder_not_alone(formatter, option, beside)
            }
            Self::DirectiveMalformed => formatter.write_str(DIRECTIVE_MALFORMED),
            Self::DirectiveNotSetOption { name, declared_as } => {
                describe_wrong_kind(formatter, "directive", "set", name, *declared_as)
            }
            Self::DirectiveInPartial => formatter.write_str("a directive appears inside a partial"),
            Self::HiddenDirective => formatter.write_str(HIDDEN_DIRECTIVE),
            Self::DirectiveUnterminated => formatter.write_str(DIRECTIVE_UNTERMINATED),
            Self::PartialUnterminated => formatter.write_str(PARTIAL_UNTERMINATED),
            Self::UndeclaredOption { option } => undeclared_option(formatter, option),
            Self::ChoiceShapeMismatch { option, declared } => {
                choice_shape_mismatch(formatter, option, *declared)
            }
            Self::ValueNotDeclared { option, value } => {
                value_not_declared(formatter, option, value)
            }
            Self::ValueChosenTwice { option, value } => {
                value_chosen_twice(formatter, option, value)
            }
            Self::TextChoiceInvalid { option, value } => {
                text_choice_invalid(formatter, option, value)
            }
            Self::ChoicesRenderTooLarge { bytes_max, option } => {
                choices_render_too_large(formatter, *bytes_max, option)
            }
        }
    }
}

// The messages the arms above delegate, one function to a variant.

fn unreadable(formatter: &mut fmt::Formatter<'_>, cause: &std::io::Error) -> fmt::Result {
    write!(formatter, "cannot be read: {}", Escaped(&cause.to_string()))
}

fn manifest_not_toml(formatter: &mut fmt::Formatter<'_>, message: &str) -> fmt::Result {
    write!(formatter, "is not valid toml: {}", Escaped(message))
}

fn unknown_key(formatter: &mut fmt::Formatter<'_>, key: &str) -> fmt::Result {
    write!(formatter, "declares unknown key {}", Echoed(key))
}

fn missing_key(formatter: &mut fmt::Formatter<'_>, key: &str) -> fmt::Result {
    write!(formatter, "is missing key {}", Echoed(key))
}

fn wrong_type(formatter: &mut fmt::Formatter<'_>, key: &str, expected: TomlType) -> fmt::Result {
    write!(formatter, "expected {} to be {expected}", Echoed(key))
}

fn option_name_invalid(formatter: &mut fmt::Formatter<'_>, option: &str) -> fmt::Result {
    write!(
        formatter,
        "option name {} is not a valid name",
        Echoed(option)
    )
}

fn option_type_unknown(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    given: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {} declares unknown type {}",
        Echoed(option),
        Echoed(given)
    )
}

fn no_values(formatter: &mut fmt::Formatter<'_>, option: &str) -> fmt::Result {
    write!(formatter, "option {} declares no values", Echoed(option))
}

fn value_invalid(formatter: &mut fmt::Formatter<'_>, option: &str, value: &str) -> fmt::Result {
    write!(
        formatter,
        "option {} declares invalid value {}",
        Echoed(option),
        Echoed(value)
    )
}

fn value_declared_twice(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {} declares value {} twice",
        Echoed(option),
        Echoed(value)
    )
}

fn default_not_declared(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {}'s default {} is not one of its declared values",
        Echoed(option),
        Echoed(value)
    )
}

fn default_lists_value_twice(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {}'s default lists value {} twice",
        Echoed(option),
        Echoed(value)
    )
}

fn partial_not_found(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
    partial: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {}'s value {} maps to partial {}, which does not exist",
        Echoed(option),
        Echoed(value),
        Echoed(partial)
    )
}

fn partial_selected_twice(formatter: &mut fmt::Formatter<'_>, partial: &str) -> fmt::Result {
    write!(
        formatter,
        "partial {} is selected by more than one value",
        Echoed(partial)
    )
}

fn partial_selected_by_nothing(formatter: &mut fmt::Formatter<'_>, partial: &str) -> fmt::Result {
    write!(
        formatter,
        "partial {} is selected by no value",
        Echoed(partial)
    )
}

fn option_unused(formatter: &mut fmt::Formatter<'_>, option: &str) -> fmt::Result {
    write!(
        formatter,
        "option {} is declared but used by nothing in the skeleton",
        Echoed(option)
    )
}

fn verbatim_path_listed_twice(formatter: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(formatter, "verbatim path {} is listed twice", Echoed(path))
}

fn verbatim_path_names_directory(formatter: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(
        formatter,
        "verbatim path {} names a directory; list each file under it instead",
        Echoed(path)
    )
}

fn verbatim_path_names_partial(formatter: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(
        formatter,
        "verbatim path {} names a partial, and only a file under `files/` can be verbatim",
        Echoed(path)
    )
}

fn verbatim_path_names_no_file(formatter: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(
        formatter,
        "verbatim path {} names no file; verbatim paths are relative to `files/`",
        Echoed(path)
    )
}

fn names_collide(formatter: &mut fmt::Formatter<'_>, other: &str) -> fmt::Result {
    write!(
        formatter,
        "collides with {} on a filesystem that ignores case or Unicode normalization",
        Echoed(other)
    )
}

fn too_many_entries(formatter: &mut fmt::Formatter<'_>, entries_max: u32) -> fmt::Result {
    write!(
        formatter,
        "the skeleton holds more than {entries_max} entries"
    )
}

fn too_many_rendered_bytes(formatter: &mut fmt::Formatter<'_>, bytes_max: u64) -> fmt::Result {
    write!(formatter, "can render to more than {bytes_max} bytes")
}

fn too_many_bytes(formatter: &mut fmt::Formatter<'_>, bytes_max: u64) -> fmt::Result {
    write!(
        formatter,
        "reading this file takes the skeleton past its {bytes_max}-byte read budget"
    )
}

fn optional_placeholder_not_alone(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    beside: &str,
) -> fmt::Result {
    write!(
        formatter,
        "optional placeholder `{{{{{}}}}}` shares its line with `{{{{{}}}}}`, which \
         would be dropped with it when {} is unset",
        Escaped(option),
        Escaped(beside),
        Echoed(option)
    )
}

fn undeclared_option(formatter: &mut fmt::Formatter<'_>, option: &str) -> fmt::Result {
    write!(
        formatter,
        "option {} is not declared by the skeleton",
        Echoed(option)
    )
}

fn choice_shape_mismatch(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    declared: OptionKind,
) -> fmt::Result {
    write!(
        formatter,
        "option {} is declared as {declared}, and was given the wrong shape of choice",
        Echoed(option)
    )
}

fn value_not_declared(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {} has no declared value {}",
        Echoed(option),
        Echoed(value)
    )
}

fn value_chosen_twice(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    write!(
        formatter,
        "option {}'s value {} was chosen twice",
        Echoed(option),
        Echoed(value)
    )
}

fn text_choice_invalid(
    formatter: &mut fmt::Formatter<'_>,
    option: &str,
    value: &str,
) -> fmt::Result {
    if value.is_empty() {
        write!(
            formatter,
            "option {} was given an empty value",
            Echoed(option)
        )
    } else {
        write!(
            formatter,
            "option {} was given {}, which holds a control character",
            Echoed(option),
            Echoed(value)
        )
    }
}

fn choices_render_too_large(
    formatter: &mut fmt::Formatter<'_>,
    bytes_max: u64,
    option: &str,
) -> fmt::Result {
    write!(
        formatter,
        "the chosen option values render to more than {bytes_max} bytes; {} \
         contributes the most",
        Echoed(option)
    )
}

/// Shared wording for a placeholder or directive that names a real option of
/// the wrong kind, or names nothing declared at all.
fn describe_wrong_kind(
    formatter: &mut fmt::Formatter<'_>,
    shape: &str,
    wanted: &str,
    name: &str,
    declared_as: Option<OptionKind>,
) -> fmt::Result {
    match declared_as {
        None => write!(
            formatter,
            "{shape} {} names no declared option",
            Echoed(name)
        ),
        Some(kind) => write!(
            formatter,
            "{shape} {} names an option declared as {kind}, not {wanted}",
            Echoed(name)
        ),
    }
}

impl std::error::Error for Reason {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { cause } => Some(cause),
            _ => None,
        }
    }
}

/// Why a skeleton could not be rendered: which skeleton, where, and what is wrong.
///
/// Boxed inside, so `Result<Rendering, RenderError>` stays small even though
/// a refusal itself carries a skeleton identity and a location.
pub(crate) struct RenderError(Box<Inner>);

/// The location a refusal names: either a specific file (line included when
/// one is at fault) or, for a refusal about the wearer's choices, no file at
/// all.
#[derive(Debug)]
struct Location {
    file: Option<String>,
    line: Option<NonZeroU32>,
}

struct Inner {
    skeleton: SkeletonIdentity,
    location: Location,
    reason: Reason,
}

impl RenderError {
    /// A refusal about `Cargo.toml` itself, with no single line at fault.
    pub(crate) fn about_manifest(skeleton: SkeletonIdentity, reason: Reason) -> Self {
        Self::new(
            skeleton,
            Some(super::manifest::MANIFEST_NAME.to_owned()),
            None,
            reason,
        )
    }

    /// A refusal about a specific file or directory, with no single line at
    /// fault — a walk refusal, a text-decoding refusal, or a refusal about
    /// something other than one line of one file.
    pub(crate) fn about_file(
        skeleton: SkeletonIdentity,
        file: impl Into<String>,
        reason: Reason,
    ) -> Self {
        Self::new(skeleton, Some(file.into()), None, reason)
    }

    /// A refusal at one specific, 1-based line of one file.
    pub(crate) fn about_line(
        skeleton: SkeletonIdentity,
        file: impl Into<String>,
        line: NonZeroU32,
        reason: Reason,
    ) -> Self {
        Self::new(skeleton, Some(file.into()), Some(line), reason)
    }

    /// A refusal about the wearer's choices, naming no file and no line.
    pub(crate) fn about_choice(skeleton: SkeletonIdentity, reason: Reason) -> Self {
        Self::new(skeleton, None, None, reason)
    }

    fn new(
        skeleton: SkeletonIdentity,
        file: Option<String>,
        line: Option<NonZeroU32>,
        reason: Reason,
    ) -> Self {
        // Postcondition: a location with a line always has a file too — there
        // is no such thing as "line 4 of nothing in particular".
        assert!(
            line.is_none() || file.is_some(),
            "a refusal with a line must also name a file"
        );
        Self(Box::new(Inner {
            skeleton,
            location: Location { file, line },
            reason,
        }))
    }

    /// The skeleton the refusal is about.
    ///
    /// Test-only: `check`/`survey` name a refused skeleton from its own worn
    /// identity (the package name and version `cargo metadata` locked),
    /// never from this accessor — before a skeleton's manifest has been read
    /// this far, `skeleton()` can only answer with the render's own absolute
    /// directory (see [`SkeletonIdentity::Directory`]), a path under the
    /// reader's own `~/.cargo` that nobody recognises. Only tests read it: the
    /// render's acceptance suite (`acceptance/`) and this module's own.
    #[cfg(test)]
    pub(crate) fn skeleton(&self) -> &SkeletonIdentity {
        &self.0.skeleton
    }

    /// The file the refusal is about, relative to the skeleton's directory with
    /// `/` separators, or `None` for a refusal about the wearer's choices.
    pub(crate) fn file(&self) -> Option<&str> {
        self.0.location.file.as_deref()
    }

    /// The 1-based line within [`Self::file`] the refusal is about, or
    /// `None` where no single line is at fault.
    pub(crate) fn line(&self) -> Option<u32> {
        self.0.location.line.map(NonZeroU32::get)
    }

    /// What is wrong.
    pub(crate) fn reason(&self) -> &Reason {
        &self.0.reason
    }
}

impl fmt::Debug for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenderError")
            .field("skeleton", &self.0.skeleton)
            .field("file", &self.0.location.file)
            .field("line", &self.0.location.line)
            .field("reason", &self.0.reason)
            .finish()
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let skeleton = &self.0.skeleton;
        let reason = &self.0.reason;
        match (&self.0.location.file, self.0.location.line) {
            (Some(file), Some(line)) => {
                let file = Escaped(file);
                write!(formatter, "skeleton {skeleton}: {file}:{line}: {reason}")
            }
            (Some(file), None) => {
                let file = Escaped(file);
                write!(formatter, "skeleton {skeleton}: {file}: {reason}")
            }
            (None, _) => write!(formatter, "skeleton {skeleton}: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Delegate to `Reason`'s own `source()` rather than returning
        // `Reason` itself: a caller walking the source chain wants the
        // underlying `io::Error` for `Reason::Unreadable`, not a detour
        // through a type it cannot otherwise do anything with (tested by
        // `source_of_an_unreadable_refusal_is_the_underlying_io_error`,
        // below).
        self.0.reason.source()
    }
}

// Send + Sync: every field of `Inner` is Send + Sync (`SkeletonIdentity` is a
// `String`/`PathBuf`, `Location` is `String`/`NonZeroU32`, `Reason` holds
// only owned data and a `std::io::Error`) — asserted at compile time so a
// future field can never silently take that away.
const _: () = {
    const fn assert_send_and_sync<T: Send + Sync>() {}
    assert_send_and_sync::<RenderError>();
};

/// Converts a zero-based line index (as `Vec<Line>` produces) into the
/// 1-based line number a [`RenderError`] reports.
///
/// # Panics
///
/// If `index` is `u32::MAX` or more, which it never is. `index` counts the
/// lines of a text the byte budget already bounded to at most
/// [`super::limits::BYTES_MAX`] bytes; every line holds at least one byte,
/// so `index` is below `BYTES_MAX`, which `limits` asserts at compile time
/// is below `u32::MAX`. That bound crosses two modules and a `usize` cannot
/// carry it, so it is asserted here, once, rather than expressed in a type.
pub(crate) fn one_based_line(index: usize) -> NonZeroU32 {
    let line = u32::try_from(index)
        .ok()
        .and_then(|index| NonZeroU32::MIN.checked_add(index));
    let Some(line) = line else {
        unreachable!("line index {index} is beyond anything the byte budget lets a render read")
    };
    line
}

#[cfg(test)]
mod tests {
    use super::{OptionKind, Reason, RenderError, SkeletonIdentity, TomlType, one_based_line};

    #[test]
    fn display_names_the_skeleton_file_and_line_when_all_are_known() {
        let error = RenderError::about_line(
            SkeletonIdentity::Named("dependabot".to_owned()),
            "files/settings.yml",
            one_based_line(3),
            Reason::PlaceholderMalformed,
        );
        assert_eq!(
            error.to_string(),
            "skeleton dependabot: files/settings.yml:4: opens `{{` without a well-formed name \
             and `}}` on the same line; a `{{` that is not a placeholder can stand only in a \
             file declared verbatim"
        );
    }

    #[test]
    fn a_malformed_placeholder_in_a_partial_names_the_fix_a_partial_can_take() {
        // A partial can never be verbatim, so its refusal says where the text
        // belongs instead of repeating the file's hint.
        let error = RenderError::about_line(
            SkeletonIdentity::Named("dependabot".to_owned()),
            "partials/cargo.yml",
            one_based_line(0),
            Reason::PlaceholderMalformedInPartial,
        );
        assert_eq!(
            error.to_string(),
            "skeleton dependabot: partials/cargo.yml:1: opens `{{` without a well-formed name \
             and `}}` on the same line; a partial cannot hold a `{{` that is not a placeholder, \
             so that text belongs in a file under `files/` declared verbatim"
        );
    }

    #[test]
    fn display_omits_the_line_when_none_is_at_fault() {
        let error = RenderError::about_manifest(
            SkeletonIdentity::Named("dependabot".to_owned()),
            Reason::PackageNameMissing,
        );
        assert_eq!(
            error.to_string(),
            "skeleton dependabot: Cargo.toml: has no string `package.name`"
        );
    }

    #[test]
    fn display_omits_the_file_for_a_choice_refusal() {
        let error = RenderError::about_choice(
            SkeletonIdentity::Named("dependabot".to_owned()),
            Reason::UndeclaredOption {
                option: "nonexistent".to_owned(),
            },
        );
        assert_eq!(
            error.to_string(),
            "skeleton dependabot: option `nonexistent` is not declared by the skeleton"
        );
    }

    #[test]
    fn accessors_report_the_location_a_refusal_was_built_with() {
        let error = RenderError::about_line(
            SkeletonIdentity::Named("dependabot".to_owned()),
            "files/settings.yml",
            one_based_line(3),
            Reason::PlaceholderMalformed,
        );
        assert_eq!(
            error.skeleton(),
            &SkeletonIdentity::Named("dependabot".to_owned())
        );
        assert_eq!(error.file(), Some("files/settings.yml"));
        assert_eq!(error.line(), Some(4));
        assert!(matches!(error.reason(), Reason::PlaceholderMalformed));
    }

    #[test]
    fn source_of_an_unreadable_refusal_is_the_underlying_io_error() {
        use std::error::Error as _;

        let cause = std::io::Error::other("disk fell over");
        let error = RenderError::about_file(
            SkeletonIdentity::Directory(std::path::PathBuf::from("/some/skeleton")),
            "Cargo.toml",
            Reason::Unreadable { cause },
        );
        assert!(error.source().expect("Unreadable has a source").to_string() == "disk fell over");
    }

    #[test]
    fn one_based_line_turns_a_zero_index_into_line_one() {
        assert_eq!(one_based_line(0).get(), 1);
        assert_eq!(one_based_line(9).get(), 10);
    }

    #[test]
    fn wrong_kind_wording_distinguishes_unknown_from_wrong_kind() {
        let unknown = Reason::PlaceholderNotFillOption {
            name: "cadense".to_owned(),
            declared_as: None,
        };
        let wrong_kind = Reason::PlaceholderNotFillOption {
            name: "workflows".to_owned(),
            declared_as: Some(OptionKind::Set),
        };
        assert_eq!(
            unknown.to_string(),
            "placeholder `cadense` names no declared option"
        );
        assert_eq!(
            wrong_kind.to_string(),
            "placeholder `workflows` names an option declared as set, not enum or text"
        );
    }

    /// Lists every `Reason` variant's pattern once, in `Reason`'s own order, and
    /// builds from that one list everything that identifies a variant: its
    /// name, its index (its position in the list), and how many variants there
    /// are.
    ///
    /// The list is held complete two ways. The `match` in `variant_name` has
    /// no wildcard, so a variant added to `Reason` and missing here fails to
    /// compile. And `REASON_VARIANT_COUNT` is the list's own length, so a
    /// list of values that leaves out the last variant, or any other, is
    /// caught by `assert_one_of_every_variant` rather than passing on a count
    /// taken from the values under test.
    macro_rules! reason_variants {
        ($($pattern:pat),+ $(,)?) => {
            /// The name of every variant, in index order.
            const REASON_VARIANT_NAMES: [&str; [$(stringify!($pattern)),+].len()] =
                [$(stringify!($pattern)),+];

            /// How many variants `Reason` has.
            const REASON_VARIANT_COUNT: usize = REASON_VARIANT_NAMES.len();

            /// The name of `reason`'s variant, as listed.
            fn variant_name(reason: &Reason) -> &'static str {
                match reason {
                    $($pattern => stringify!($pattern)),+
                }
            }
        };
    }

    reason_variants![
        Reason::Unreadable { .. },
        Reason::ManifestNotToml { .. },
        Reason::PackageNameMissing,
        Reason::PackageNameEmpty,
        Reason::NotASkeleton,
        Reason::UnknownKey { .. },
        Reason::MissingKey { .. },
        Reason::WrongType { .. },
        Reason::OptionNameInvalid { .. },
        Reason::OptionTypeUnknown { .. },
        Reason::NoValues { .. },
        Reason::ValueInvalid { .. },
        Reason::ValueDeclaredTwice { .. },
        Reason::DefaultNotDeclared { .. },
        Reason::DefaultListsValueTwice { .. },
        Reason::PartialNotFound { .. },
        Reason::PartialSelectedTwice { .. },
        Reason::PartialSelectedByNothing { .. },
        Reason::OptionUnused { .. },
        Reason::NoFiles,
        Reason::NotADirectory,
        Reason::SymbolicLink,
        Reason::NotAFileOrDirectory,
        Reason::NotAFile,
        Reason::PathNotUtf8,
        Reason::NamesCollide { .. },
        Reason::NestedManifest,
        Reason::TooManyEntries { .. },
        Reason::TooManyBytes { .. },
        Reason::TooManyRenderedBytes { .. },
        Reason::NotUtf8,
        Reason::PlaceholderMalformed,
        Reason::PlaceholderNotFillOption { .. },
        Reason::DirectiveMalformed,
        Reason::DirectiveNotSetOption { .. },
        Reason::OptionalPlaceholderNotAlone { .. },
        Reason::DirectiveInPartial,
        Reason::HiddenDirective,
        Reason::DirectiveUnterminated,
        Reason::PartialUnterminated,
        Reason::UndeclaredOption { .. },
        Reason::ChoiceShapeMismatch { .. },
        Reason::ValueNotDeclared { .. },
        Reason::ValueChosenTwice { .. },
        Reason::TextChoiceInvalid { .. },
        Reason::ChoicesRenderTooLarge { .. },
        Reason::VerbatimPathListedTwice { .. },
        Reason::VerbatimPathNamesDirectory { .. },
        Reason::VerbatimPathNamesPartial { .. },
        Reason::VerbatimPathNamesNoFile { .. },
        Reason::PlaceholderMalformedInPartial,
    ];

    /// `reason`'s position in `reason_variants!`'s list.
    fn variant_index(reason: &Reason) -> usize {
        let name = variant_name(reason);
        REASON_VARIANT_NAMES
            .iter()
            .position(|listed| *listed == name)
            .expect("`variant_name` returns only names the list holds")
    }

    /// Asserts `values` holds exactly one of every `Reason` variant, naming
    /// each that is missing or repeated.
    fn assert_one_of_every_variant(values: &[Reason]) {
        let mut occurrences = [0_usize; REASON_VARIANT_COUNT];
        for value in values {
            occurrences[variant_index(value)] += 1;
        }
        for (name, count) in REASON_VARIANT_NAMES.iter().zip(occurrences) {
            assert_eq!(
                count, 1,
                "every Reason variant must appear exactly once, but `{name}` appears {count} times"
            );
        }
    }

    /// One value of every `Reason` variant, built for
    /// `every_reason_displays_by_the_message_convention` — one function per
    /// kind of thing a refusal is about (the manifest, its `verbatim` list,
    /// the file sets, and lines together with the wearer's choices), in
    /// `Reason`'s own order, so each stays a readable size.
    fn one_of_every_reason() -> Vec<Reason> {
        let mut values = manifest_reasons();
        values.extend(verbatim_reasons());
        values.extend(file_set_reasons());
        values.extend(line_and_choice_reasons());
        values
    }

    fn manifest_reasons() -> Vec<Reason> {
        vec![
            Reason::Unreadable {
                cause: std::io::Error::other("disk fell over"),
            },
            Reason::ManifestNotToml {
                message: "unexpected eof".to_owned(),
            },
            Reason::PackageNameMissing,
            Reason::PackageNameEmpty,
            Reason::NotASkeleton,
            Reason::UnknownKey {
                key: "package.metadata.skeletons.surprising".to_owned(),
            },
            Reason::MissingKey {
                key: "package.metadata.skeletons.options.cadence.default".to_owned(),
            },
            Reason::WrongType {
                key: "package.metadata.skeletons.options".to_owned(),
                expected: TomlType::Table,
            },
            Reason::OptionNameInvalid {
                option: "Cadence".to_owned(),
            },
            Reason::OptionTypeUnknown {
                option: "cadence".to_owned(),
                given: "list".to_owned(),
            },
            Reason::NoValues {
                option: "cadence".to_owned(),
            },
            Reason::ValueInvalid {
                option: "cadence".to_owned(),
                value: String::new(),
            },
            Reason::ValueDeclaredTwice {
                option: "cadence".to_owned(),
                value: "daily".to_owned(),
            },
            Reason::DefaultNotDeclared {
                option: "cadence".to_owned(),
                value: "yearly".to_owned(),
            },
            Reason::DefaultListsValueTwice {
                option: "workflows".to_owned(),
                value: "lint".to_owned(),
            },
            Reason::PartialNotFound {
                option: "workflows".to_owned(),
                value: "lint".to_owned(),
                partial: "lint.yml".to_owned(),
            },
            Reason::PartialSelectedTwice {
                partial: "shared.yml".to_owned(),
            },
            Reason::PartialSelectedByNothing {
                partial: "stray.yml".to_owned(),
            },
            Reason::OptionUnused {
                option: "cadence".to_owned(),
            },
        ]
    }

    fn verbatim_reasons() -> Vec<Reason> {
        vec![
            Reason::VerbatimPathListedTwice {
                path: "ci.yml".to_owned(),
            },
            Reason::VerbatimPathNamesDirectory {
                path: ".github/workflows".to_owned(),
            },
            Reason::VerbatimPathNamesPartial {
                path: "cargo.yml".to_owned(),
            },
            Reason::VerbatimPathNamesNoFile {
                path: "missing.yml".to_owned(),
            },
        ]
    }

    fn file_set_reasons() -> Vec<Reason> {
        vec![
            Reason::NoFiles,
            Reason::NotADirectory,
            Reason::SymbolicLink,
            Reason::NotAFileOrDirectory,
            Reason::NotAFile,
            Reason::PathNotUtf8,
            Reason::NamesCollide {
                other: "files/dependabot.yml".to_owned(),
            },
            Reason::NestedManifest,
            Reason::TooManyEntries { entries_max: 1024 },
            Reason::TooManyBytes {
                bytes_max: 1024 * 1024,
            },
            Reason::TooManyRenderedBytes {
                bytes_max: 8 * 1024 * 1024,
            },
            Reason::NotUtf8,
        ]
    }

    fn line_and_choice_reasons() -> Vec<Reason> {
        vec![
            Reason::PlaceholderMalformed,
            Reason::PlaceholderMalformedInPartial,
            Reason::PlaceholderNotFillOption {
                name: "workflows".to_owned(),
                declared_as: None,
            },
            Reason::DirectiveMalformed,
            Reason::DirectiveNotSetOption {
                name: "cadence".to_owned(),
                declared_as: None,
            },
            Reason::OptionalPlaceholderNotAlone {
                option: "assignee".to_owned(),
                beside: "cadence".to_owned(),
            },
            Reason::DirectiveInPartial,
            Reason::HiddenDirective,
            Reason::DirectiveUnterminated,
            Reason::PartialUnterminated,
            Reason::UndeclaredOption {
                option: "nonexistent".to_owned(),
            },
            Reason::ChoiceShapeMismatch {
                option: "cadence".to_owned(),
                declared: OptionKind::Enum,
            },
            Reason::ValueNotDeclared {
                option: "cadence".to_owned(),
                value: "yearly".to_owned(),
            },
            Reason::ValueChosenTwice {
                option: "workflows".to_owned(),
                value: "lint".to_owned(),
            },
            Reason::TextChoiceInvalid {
                option: "assignee".to_owned(),
                value: String::new(),
            },
            Reason::ChoicesRenderTooLarge {
                bytes_max: 8 * 1024 * 1024,
                option: "note".to_owned(),
            },
        ]
    }

    #[test]
    fn a_control_character_in_an_echoed_value_is_shown_escaped_on_one_line() {
        // Every message that echoes an authored or typed value must keep a
        // newline out of the report line it ends up in: the value is shown as
        // `\n`, never as a line break.
        for reason in [
            Reason::ValueInvalid {
                option: "cadence".to_owned(),
                value: "a\nb".to_owned(),
            },
            Reason::DefaultNotDeclared {
                option: "cadence".to_owned(),
                value: "a\nb".to_owned(),
            },
            Reason::ValueNotDeclared {
                option: "cadence".to_owned(),
                value: "a\nb".to_owned(),
            },
            Reason::TextChoiceInvalid {
                option: "assignee".to_owned(),
                value: "a\nb".to_owned(),
            },
        ] {
            let displayed = reason.to_string();
            assert!(
                displayed.contains("`a\\nb`"),
                "{reason:?} must show the newline escaped: {displayed:?}"
            );
            assert!(
                !displayed.contains('\n'),
                "{reason:?} must stay on one line: {displayed:?}"
            );
        }
    }

    #[test]
    fn a_text_choice_refusal_says_whether_the_value_was_empty_or_held_a_control_character() {
        let empty = Reason::TextChoiceInvalid {
            option: "assignee".to_owned(),
            value: String::new(),
        };
        let control = Reason::TextChoiceInvalid {
            option: "assignee".to_owned(),
            value: "octo\u{85}cat".to_owned(),
        };
        assert_eq!(
            empty.to_string(),
            "option `assignee` was given an empty value"
        );
        assert_eq!(
            control.to_string(),
            "option `assignee` was given `octo\\u0085cat`, which holds a control character"
        );
    }

    #[test]
    fn an_optional_placeholder_refusal_names_both_placeholders() {
        let reason = Reason::OptionalPlaceholderNotAlone {
            option: "assignee".to_owned(),
            beside: "cadence".to_owned(),
        };
        assert_eq!(
            reason.to_string(),
            "optional placeholder `{{assignee}}` shares its line with `{{cadence}}`, which \
             would be dropped with it when `assignee` is unset"
        );
    }

    #[test]
    fn a_render_too_large_refusal_names_the_limit_and_the_option() {
        let reason = Reason::ChoicesRenderTooLarge {
            bytes_max: 8_388_608,
            option: "note".to_owned(),
        };
        assert_eq!(
            reason.to_string(),
            "the chosen option values render to more than 8388608 bytes; `note` contributes \
             the most"
        );
    }

    /// Text holding a newline, a next-line control, a zero-width space and
    /// a quote, every kind of character a one-line report must not carry raw.
    const AWKWARD: &str = "a\nb\u{85}c\u{200b}d\"e";

    /// The characters of [`AWKWARD`] a one-line report must not carry raw. The
    /// zero-width space is a format character, not a control, so
    /// `char::is_control` does not see it.
    const AWKWARD_UNPRINTABLE: [char; 3] = ['\n', '\u{85}', '\u{200b}'];

    /// [`AWKWARD`] as a message prints it: each of [`AWKWARD_UNPRINTABLE`]
    /// escaped, and the quote as typed.
    const AWKWARD_ESCAPED: &str = "a\\nb\\u0085c\\u200Bd\"e";

    /// Every `Reason` variant with each text field set to [`AWKWARD`], in
    /// the groups [`one_of_every_reason`] holds them in: the manifest's, the
    /// file sets', and a line's or the wearer's choices'.
    fn every_reason_with_awkward_text() -> Vec<Reason> {
        let mut values = awkward_manifest_reasons();
        values.extend(awkward_file_set_reasons());
        values.extend(awkward_line_and_choice_reasons());
        values
    }

    /// The manifest's reasons, each with [`AWKWARD`] in every text field.
    fn awkward_manifest_reasons() -> Vec<Reason> {
        let text = || AWKWARD.to_owned();
        vec![
            Reason::Unreadable {
                cause: std::io::Error::other(AWKWARD),
            },
            Reason::ManifestNotToml { message: text() },
            Reason::PackageNameMissing,
            Reason::PackageNameEmpty,
            Reason::NotASkeleton,
            Reason::UnknownKey { key: text() },
            Reason::MissingKey { key: text() },
            Reason::WrongType {
                key: text(),
                expected: TomlType::Table,
            },
            Reason::OptionNameInvalid { option: text() },
            Reason::OptionTypeUnknown {
                option: text(),
                given: text(),
            },
            Reason::NoValues { option: text() },
            Reason::ValueInvalid {
                option: text(),
                value: text(),
            },
            Reason::ValueDeclaredTwice {
                option: text(),
                value: text(),
            },
            Reason::DefaultNotDeclared {
                option: text(),
                value: text(),
            },
            Reason::DefaultListsValueTwice {
                option: text(),
                value: text(),
            },
            Reason::PartialNotFound {
                option: text(),
                value: text(),
                partial: text(),
            },
            Reason::PartialSelectedTwice { partial: text() },
            Reason::PartialSelectedByNothing { partial: text() },
            Reason::OptionUnused { option: text() },
            Reason::VerbatimPathListedTwice { path: text() },
            Reason::VerbatimPathNamesDirectory { path: text() },
            Reason::VerbatimPathNamesPartial { path: text() },
            Reason::VerbatimPathNamesNoFile { path: text() },
        ]
    }

    /// The file sets' reasons, each with [`AWKWARD`] in every text field.
    fn awkward_file_set_reasons() -> Vec<Reason> {
        let text = || AWKWARD.to_owned();
        vec![
            Reason::NoFiles,
            Reason::NotADirectory,
            Reason::SymbolicLink,
            Reason::NotAFileOrDirectory,
            Reason::NotAFile,
            Reason::PathNotUtf8,
            Reason::NamesCollide { other: text() },
            Reason::NestedManifest,
            Reason::TooManyEntries { entries_max: 1024 },
            Reason::TooManyBytes { bytes_max: 1024 },
            Reason::TooManyRenderedBytes { bytes_max: 1024 },
            Reason::NotUtf8,
        ]
    }

    /// A line's and the wearer's choices' reasons, each with [`AWKWARD`] in every text field.
    fn awkward_line_and_choice_reasons() -> Vec<Reason> {
        let text = || AWKWARD.to_owned();
        vec![
            Reason::PlaceholderMalformed,
            Reason::PlaceholderMalformedInPartial,
            Reason::PlaceholderNotFillOption {
                name: text(),
                declared_as: None,
            },
            Reason::DirectiveMalformed,
            Reason::DirectiveNotSetOption {
                name: text(),
                declared_as: Some(OptionKind::Enum),
            },
            Reason::OptionalPlaceholderNotAlone {
                option: text(),
                beside: text(),
            },
            Reason::DirectiveInPartial,
            Reason::HiddenDirective,
            Reason::DirectiveUnterminated,
            Reason::PartialUnterminated,
            Reason::UndeclaredOption { option: text() },
            Reason::ChoiceShapeMismatch {
                option: text(),
                declared: OptionKind::Text,
            },
            Reason::ValueNotDeclared {
                option: text(),
                value: text(),
            },
            Reason::ValueChosenTwice {
                option: text(),
                value: text(),
            },
            Reason::TextChoiceInvalid {
                option: text(),
                value: text(),
            },
            Reason::ChoicesRenderTooLarge {
                bytes_max: 1024,
                option: text(),
            },
        ]
    }

    #[test]
    fn every_reason_prints_on_one_line() {
        // Every variant, with awkward text in every text field, must print
        // without a raw newline, next-line control or zero-width space: a
        // refusal is one line of a report, whichever field the awkward text
        // lands in. `char::is_control` alone would pass the zero-width space,
        // which is a format character and not a control, so the characters
        // are also named. Completeness is held by `reason_variants!`: the list
        // of values must hold each of its variants exactly once, so a variant
        // added to `Reason`, or left out of the list of values, fails here.
        let values = every_reason_with_awkward_text();

        assert_one_of_every_variant(&values);
        for character in AWKWARD_UNPRINTABLE {
            assert!(
                AWKWARD.contains(character),
                "{character:?} is named as awkward but is not in AWKWARD, so this test would \
                 look for text it never supplies"
            );
        }
        for value in &values {
            let displayed = value.to_string();
            assert!(
                !displayed.chars().any(char::is_control),
                "{value:?} must hold no control character: {displayed:?}"
            );
            for character in AWKWARD_UNPRINTABLE {
                assert!(
                    !displayed.contains(character),
                    "{value:?} must not print {character:?} raw: {displayed:?}"
                );
            }
        }
        // A positive control: the escaped form is what the awkward text turns
        // into where a message echoes it, so finding it says the text got as
        // far as the output and the checks above had something to check.
        assert!(
            values
                .iter()
                .any(|value| value.to_string().contains(AWKWARD_ESCAPED)),
            "no variant echoed the awkward text as {AWKWARD_ESCAPED:?}"
        );
    }

    #[test]
    fn a_render_refusal_prints_its_file_and_its_directory_on_one_line() {
        // The file a refusal names, and the directory a render was given
        // before the skeleton's name is known, can both hold a line break.
        let in_a_file = RenderError::about_file(
            SkeletonIdentity::Named("dependabot".to_owned()),
            "files/a\nb.yml",
            Reason::NotUtf8,
        );
        let in_a_directory = RenderError::about_file(
            SkeletonIdentity::Directory(std::path::PathBuf::from("/skeletons/a\nb")),
            "Cargo.toml",
            Reason::NotAFile,
        );

        assert_eq!(
            in_a_file.to_string(),
            "skeleton dependabot: files/a\\nb.yml: is not valid utf-8"
        );
        assert_eq!(
            in_a_directory.to_string(),
            "skeleton /skeletons/a\\nb: Cargo.toml: is not a regular file"
        );
    }

    #[test]
    fn every_reason_displays_by_the_message_convention() {
        // Every `Reason`'s message follows the convention a refusal's
        // `Display` is held to: non-empty, starting with a lowercase
        // letter, with no trailing punctuation, since it is always read
        // after `skeleton <name>: <file>:<line>: `. That every variant has a
        // message at all is the compiler's to check — `Display` matches
        // every variant with no wildcard — so what is left to test is what
        // the messages say. The list is held complete by `reason_variants!`
        // above: a variant added to `Reason` and left out of
        // `one_of_every_reason` is caught by the completeness assertion
        // below, which counts against the variants themselves, not merely
        // trusted to have been remembered.
        let values = one_of_every_reason();

        for value in &values {
            let displayed = value.to_string();
            assert!(!displayed.is_empty(), "{value:?} must display something");
            let first = displayed.chars().next().expect("checked non-empty above");
            assert!(
                first.is_lowercase(),
                "{value:?} must start with a lowercase letter: {displayed:?}"
            );
            let last = displayed.chars().last().expect("checked non-empty above");
            assert!(
                !matches!(last, '.' | '!' | '?'),
                "{value:?} must not end in punctuation: {displayed:?}"
            );
        }

        assert_one_of_every_variant(&values);
    }
}
