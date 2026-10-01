//! Why something could not be determined — every kind of refusal `check` and
//! `sync` report, and the messages of `.docs/wearing.md`'s *Refusals* table
//! (lowercase, names in backticks, `<what is wrong>; <what to do>`).

use super::spelling::told_apart;
use crate::claim::{
    ClaimPath, FoldedRelation, NAME_BYTES_MAX, Overlap, TooLongName, UnsafePathCause,
    folded_relation, staging_name,
};
use crate::skeleton::{Escaped, RenderError};
use crate::workspace::{OptionShapeRefusal, WearingRefusal, WornId};

/// One refusal: something `check`/`sync` could not determine, either about
/// one worn skeleton or about no single one.
pub(crate) enum Refusal {
    /// A manifest's own wearing table could not name a worn skeleton at all.
    Wearing(WearingRefusal),
    /// A recorded option value's TOML shape a `Choice` cannot represent.
    OptionShape {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        refusal: OptionShapeRefusal,
    },
    /// The skeleton's own render refused — a defect in the skeleton itself
    /// (`error.file()` is `Some`) or an undeclared/malformed choice the
    /// render itself caught (`error.file()` is `None`).
    Render {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        error: RenderError,
    },
    /// Two or more claims collide.
    Overlap(Overlap),
    /// A claimed path could not be safely resolved on disk, would sit inside
    /// `.git`, holds a name git will not track, or holds a name too long.
    UnsafePath {
        worn: WornId,
        skeleton: String,
        version: semver::Version,
        path: String,
        cause: UnsafePathCause,
    },
}

impl Refusal {
    /// The `kind` this refusal reports in `--json`, and the word `check`'s
    /// human output classifies it by.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Wearing(wearing) => match wearing {
                WearingRefusal::NotATable { .. } => "not-a-table",
                WearingRefusal::ReservedKey { .. } => "reserved-key",
                WearingRefusal::NamesNoDependency { .. } => "names-no-dependency",
                WearingRefusal::Unresolved { .. } => "unresolved",
                WearingRefusal::Ambiguous { .. } => "ambiguous",
                WearingRefusal::NotASkeleton { .. } => "not-a-skeleton",
            },
            Self::OptionShape { .. } => "option-refused",
            // A render refusal about a specific file is a defect in the
            // skeleton itself; one about no file at all (`error.file()` is
            // `None`) is about the wearer's own choices instead — the
            // render's own documented invariant (`skeleton-format.md`,
            // §Refusals) that a choice refusal never names a file.
            Self::Render { error, .. } => {
                if error.file().is_some() {
                    "skeleton-invalid"
                } else {
                    "option-refused"
                }
            }
            Self::Overlap(_) => "overlap",
            Self::UnsafePath { .. } => "unsafe-path",
        }
    }

    /// The worn skeleton this refusal is about, when it is about exactly
    /// one — decides whether `check`'s human output places it inside that
    /// skeleton's own block or in the closing `refused:` list.
    pub(crate) const fn about_one_skeleton(&self) -> Option<&WornId> {
        match self {
            Self::Wearing(_) | Self::Overlap(_) => None,
            Self::OptionShape { worn, .. }
            | Self::Render { worn, .. }
            | Self::UnsafePath { worn, .. } => Some(worn),
        }
    }

    /// The claimed path an `unsafe-path` refusal is about — the order more
    /// than one of a skeleton's own refusals are shown in (`check`'s own
    /// `refusals_for`). `None` for every other kind, which a skeleton never
    /// carries more than one of at once (see [`Self::about_one_skeleton`]).
    pub(crate) fn unsafe_path(&self) -> Option<&str> {
        match self {
            Self::UnsafePath { path, .. } => Some(path.as_str()),
            _ => None,
        }
    }

    /// The self-contained message this refusal reports — the same text
    /// `sync` prints after `refused: ` and the closing `refused:` list in
    /// `check`'s human output carries in full.
    ///
    /// For the three refusals about one worn skeleton, this is
    /// `<dependency> in <manifest> (<skeleton> <version>): <text>`; `check`
    /// prints `<text>` alone inside that skeleton's own block, given by
    /// [`Self::text_in_block`].
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Wearing(wearing) => wearing_message(wearing),
            Self::Overlap(overlap) => overlap_message(overlap),
            Self::OptionShape {
                worn,
                skeleton,
                version,
                ..
            }
            | Self::Render {
                worn,
                skeleton,
                version,
                ..
            }
            | Self::UnsafePath {
                worn,
                skeleton,
                version,
                ..
            } => {
                let (key, manifest, skeleton) = (
                    Escaped(&worn.key),
                    Escaped(&worn.manifest),
                    Escaped(skeleton),
                );
                format!(
                    "{key} in {manifest} ({skeleton} {version}): {}",
                    self.text_in_block()
                )
            }
        }
    }

    /// The text `check`'s human output shows inside a refused skeleton's own
    /// block, after `refused: ` — everything [`Self::message`] carries for
    /// that skeleton, minus the identity its block header already gives.
    ///
    /// # Panics
    ///
    /// If called on a refusal [`Self::about_one_skeleton`] returns `None`
    /// for — there is no such block to place it in.
    pub(crate) fn text_in_block(&self) -> String {
        match self {
            Self::OptionShape { worn, refusal, .. } => {
                let (key, manifest) = (Escaped(&worn.key), Escaped(&worn.manifest));
                format!(
                    "[package.metadata.skeletons.{key}] in {manifest}: {}",
                    option_shape_detail(refusal)
                )
            }
            Self::Render {
                worn,
                skeleton,
                version,
                error,
            } => render_text(worn, skeleton, version, error),
            Self::UnsafePath {
                skeleton,
                version,
                path,
                cause,
                ..
            } => unsafe_path_text(path, cause, skeleton, version),
            Self::Wearing(_) | Self::Overlap(_) => {
                unreachable!("text_in_block is only called for a refusal about one worn skeleton")
            }
        }
    }
}

/// The `text_in_block` for a `Render` refusal: a skeleton defect
/// (`error.file()` is `Some`) names the file, line and reason, and closes by
/// naming the defect as the skeleton's own; a choice refusal (`error.file()`
/// is `None`) instead prefixes the render's own reason with the wearing
/// table's own identity, since the render itself never learns it.
fn render_text(
    worn: &WornId,
    skeleton: &str,
    version: &semver::Version,
    error: &RenderError,
) -> String {
    error.file().map_or_else(
        || choice_refusal_text(worn, error),
        |file| skeleton_defect_text(file, skeleton, version, error),
    )
}

/// The `text_in_block` for a render refusal about `file`: where it is, why,
/// and whose defect it is.
fn skeleton_defect_text(
    file: &str,
    skeleton: &str,
    version: &semver::Version,
    error: &RenderError,
) -> String {
    let (file, skeleton) = (Escaped(file), Escaped(skeleton));
    let line = error
        .line()
        .map_or_else(String::new, |line| format!(":{line}"));
    format!(
        "{file}{line}: {}; this is a defect in {skeleton} {version}, not in this repository",
        error.reason()
    )
}

/// The `text_in_block` for a render refusal that names no file: the wearing
/// table's own identity, then the render's own reason.
fn choice_refusal_text(worn: &WornId, error: &RenderError) -> String {
    let (key, manifest) = (Escaped(&worn.key), Escaped(&worn.manifest));
    format!(
        "[package.metadata.skeletons.{key}] in {manifest}: {}",
        error.reason()
    )
}

/// An illustrative recorded value used only to show the two accepted
/// spellings of a wearing table entry (`cadence = "…"` or `cadence =
/// […]`) — not a value this crate ever validates against a skeleton's own
/// declarations, since a shape refusal is caught before any skeleton is
/// consulted.
const SHAPE_EXAMPLE_VALUE: &str = "weekly";

/// The text naming what is wrong with one recorded option value's TOML
/// shape, and the two accepted spellings — shared between the message shown
/// inside a skeleton's own block ([`Refusal::text_in_block`]) and the JSON
/// `detail` field a `--json` reader gets on its own (`check/json.rs`).
pub(crate) fn option_shape_detail(refusal: &OptionShapeRefusal) -> String {
    option_shape_words(&refusal.option, refusal.found)
}

/// [`option_shape_detail`]'s words for a recorded option named `option` whose
/// value is `found`, a description of its TOML shape.
fn option_shape_words(option: &str, found: impl std::fmt::Display) -> String {
    let shown = Escaped(option);
    format!(
        "`{shown}` is {found}, not a string or an array of strings; it reads `{}` or `{}`",
        shape_example_string(option),
        shape_example_array(option),
    )
}

fn shape_example_string(option: &str) -> String {
    let option = Escaped(option);
    format!("{option} = \"{SHAPE_EXAMPLE_VALUE}\"")
}

fn shape_example_array(option: &str) -> String {
    let option = Escaped(option);
    format!("{option} = [\"{SHAPE_EXAMPLE_VALUE}\"]")
}

/// The message for an `unsafe-path` refusal's own `cause`.
fn unsafe_path_text(
    path: &str,
    cause: &UnsafePathCause,
    skeleton: &str,
    version: &semver::Version,
) -> String {
    let (path, skeleton) = (Escaped(path), Escaped(skeleton));
    match cause {
        // `spelled_differently_text` shows every name it prints, the path
        // included, through `told_apart`, which escapes it; it is handed the
        // path as it came.
        UnsafePathCause::SpelledDifferently {
            at,
            on_disk,
            claimed_spelling_present,
        } => spelled_differently_text(path.0, at, on_disk, *claimed_spelling_present),
        UnsafePathCause::SymbolicLinkAbove { at } => {
            let at = Escaped(at);
            format!(
                "{path} is under a symbolic link, {at}; `skeletons` reads and writes a claimed \
                 file only through real directories, so replace the link with a directory"
            )
        }
        UnsafePathCause::Symlink => format!(
            "{path} is a symbolic link; `skeletons` reads and writes a claimed file only as a regular \
             file, so replace the link with the file"
        ),
        UnsafePathCause::NotADirectoryAbove { at } => {
            let at = Escaped(at);
            format!("{path} cannot exist, because {at} is a file; move {at} away")
        }
        UnsafePathCause::NotAFile => format!("{path} is not a regular file; move it away"),
        UnsafePathCause::InsideGitDirectory => format!(
            "{path} is inside .git, which `skeletons` never writes; this is a defect in {skeleton} \
             {version}, not in this repository"
        ),
        UnsafePathCause::InsideAnotherRepository { at } => {
            let at = Escaped(at);
            format!(
                "{path} is inside {at}, which holds a .git of its own and so is another git \
                 repository; `skeletons` reads and writes a claimed file only in the repository the \
                 workspace belongs to"
            )
        }
        UnsafePathCause::UntrackableName { at } => {
            let component = Escaped(at.rsplit('/').next().unwrap_or(at));
            format!(
                "{path} cannot be tracked, because git refuses to track a file or directory \
                 named \"{component}\"; this is a defect in {skeleton} {version}, not in this \
                 repository"
            )
        }
        UnsafePathCause::NameTooLong { at, bytes, name } => {
            name_too_long_text(path.0, at, *bytes, *name, skeleton.0, version)
        }
        UnsafePathCause::Unreadable { detail } => {
            let detail = Escaped(detail);
            format!("{path} cannot be read: {detail}")
        }
    }
}

/// The message for [`UnsafePathCause::NameTooLong`]: which name does not fit,
/// how long it is, and whose defect that is. A skeleton that names a file no
/// filesystem can hold, or one `sync` cannot stage, is at fault, never the
/// repository it is worn in.
fn name_too_long_text(
    path: &str,
    at: &str,
    bytes: usize,
    name: TooLongName,
    skeleton: &str,
    version: &semver::Version,
) -> String {
    let (path, skeleton) = (Escaped(path), Escaped(skeleton));
    let component = at.rsplit('/').next().unwrap_or(at);
    let limit = NAME_BYTES_MAX;
    match name {
        TooLongName::Claimed => format!(
            "{path} cannot exist, because its name {} is {bytes} bytes, more than the \
             {limit} a file name can hold; this is a defect in {skeleton} {version}, not in this \
             repository",
            Escaped(component)
        ),
        TooLongName::Staging => format!(
            "{path} cannot be written by sync, which stages it beside itself as {}, {bytes} \
             bytes, more than the {limit} a file name can hold; this is a defect in {skeleton} \
             {version}, not in this repository",
            Escaped(&staging_name(component))
        ),
    }
}

/// The same [`UnsafePathCause`] as [`unsafe_path_text`] reads, phrased as a
/// clause about a path the message has already named, for a path that
/// *became* unsafe between two of `sync`'s looks at it: `x above it is now a
/// symbolic link`. Kept beside `unsafe_path_text` and matching the cause
/// exhaustively, so a cause added to one cannot be left out of the other,
/// and both read the same fields (`at`, `on_disk`, `detail`) of the same
/// cause.
pub(crate) fn unsafe_path_change_clause(cause: &UnsafePathCause) -> String {
    match cause {
        UnsafePathCause::SpelledDifferently { at, on_disk, .. } => {
            if on_disk.is_empty() {
                let at = Escaped(at);
                format!("{at} is now found on disk under another spelling")
            } else {
                let names: Vec<&str> = std::iter::once(at.as_str())
                    .chain(on_disk.iter().map(String::as_str))
                    .collect();
                let shown = told_apart(&names);
                let found: Vec<String> = (1..names.len())
                    .map(|index| shown[index].to_owned())
                    .collect();
                format!(
                    "{} is now also present on disk as {}{}",
                    &shown[0],
                    join_and(&found),
                    shown.note_after_semicolon()
                )
            }
        }
        UnsafePathCause::SymbolicLinkAbove { at } => {
            format!("{} above it is now a symbolic link", Escaped(at))
        }
        UnsafePathCause::Symlink => "it is now a symbolic link".to_owned(),
        UnsafePathCause::NotADirectoryAbove { at } => {
            format!("{} above it is now a file", Escaped(at))
        }
        UnsafePathCause::NotAFile => "it is no longer a regular file".to_owned(),
        UnsafePathCause::InsideGitDirectory => "it is now inside .git".to_owned(),
        UnsafePathCause::InsideAnotherRepository { at } => {
            format!("{} above it is now another git repository", Escaped(at))
        }
        UnsafePathCause::UntrackableName { .. } => unreachable!(
            "a name git refuses to track is refused when the claim is built, before any walk of \
             the filesystem, so a path that was fine cannot become one"
        ),
        UnsafePathCause::NameTooLong { .. } => unreachable!(
            "a name that is too long is refused when the claim is built, before any walk of the \
             filesystem, so a path that was fine cannot become one"
        ),
        UnsafePathCause::Unreadable { detail } => {
            format!("it can no longer be read: {}", Escaped(detail))
        }
    }
}

/// Why `skeletons` refuses a spelling mismatch at all — the one clause every
/// [`spelled_differently_text`] shape carries, word for word.
const SPELLED_DIFFERENTLY_REASON: &str = "`skeletons` reads and writes a claimed path only under the \
                                           exact spelling its skeleton gives it, since two \
                                           spellings of one name are one file on some \
                                           filesystems and two on others";

/// The names a [`UnsafePathCause::SpelledDifferently`] message shows, already
/// prepared by [`told_apart`], with the note that goes after the first clause
/// (empty unless the spellings had to be shown as code points).
struct SpelledNames {
    path: String,
    at: String,
    /// The on-disk spellings, already joined in the crate's list grammar.
    found: String,
    note: String,
}

/// The message for [`UnsafePathCause::SpelledDifferently`] — chosen by three
/// independent facts: whether the listing found any on-disk spelling at all
/// (`on_disk`, empty only when the filesystem's own lookup folded the
/// difference away on its own), whether the claimed spelling is itself also
/// present alongside the differing one(s) (`claimed_spelling_present`), and
/// whether the differing component is the claim's own final component
/// (`at == path`) or an ancestor of it.
///
/// Every spelling the message shows goes through [`told_apart`] together, so
/// two that differ only in Unicode normalization are told apart in it.
fn spelled_differently_text(
    path: &str,
    at: &str,
    on_disk: &[String],
    claimed_spelling_present: bool,
) -> String {
    if on_disk.is_empty() {
        // The lookup witness alone: nothing in the listing folds to the
        // claim's `FoldedName`, so there is no on-disk spelling to name.
        let path = Escaped(path);
        return format!(
            "{path} is found on disk under another spelling of {path}; \
             {SPELLED_DIFFERENTLY_REASON}; rename it to {path}"
        );
    }

    let at_is_path = at == path;
    let names: Vec<&str> = [path, at]
        .into_iter()
        .chain(on_disk.iter().map(String::as_str))
        .collect();
    let shown = told_apart(&names);
    let found: Vec<String> = (2..names.len())
        .map(|index| shown[index].to_owned())
        .collect();
    let names = SpelledNames {
        path: shown[0].to_owned(),
        at: shown[1].to_owned(),
        found: join_and(&found),
        note: shown.note_in_parentheses(),
    };

    if claimed_spelling_present {
        spelled_differently_present_text(&names, at_is_path)
    } else if on_disk.len() == 1 {
        spelled_differently_absent_one_text(&names, at_is_path)
    } else {
        spelled_differently_absent_many_text(&names, at_is_path)
    }
}

/// The claimed spelling is itself also present, alongside `names.found` —
/// only possible on a case-sensitive filesystem, where both spellings are two
/// different files at once.
fn spelled_differently_present_text(names: &SpelledNames, at_is_path: bool) -> String {
    let SpelledNames {
        path,
        at,
        found,
        note,
    } = names;
    if at_is_path {
        format!(
            "{path} is also present on disk as {found}{note}; {SPELLED_DIFFERENTLY_REASON}; \
             remove or rename {found}"
        )
    } else {
        format!(
            "{path} is under {at}, which is also present on disk as {found}{note}; \
             {SPELLED_DIFFERENTLY_REASON}; remove or rename {found}"
        )
    }
}

/// The claimed spelling is absent, and the listing found exactly one other
/// spelling (`names.found`) to rename it from.
fn spelled_differently_absent_one_text(names: &SpelledNames, at_is_path: bool) -> String {
    let SpelledNames {
        path,
        at,
        found,
        note,
    } = names;
    if at_is_path {
        format!(
            "{path} is spelled {found} on disk{note}; {SPELLED_DIFFERENTLY_REASON}; rename \
             {found} to {path}"
        )
    } else {
        format!(
            "{path} is under {found} on disk, which its skeleton spells {at}{note}; \
             {SPELLED_DIFFERENTLY_REASON}; rename {found} to {at}"
        )
    }
}

/// The claimed spelling is absent, and the listing found two or more other
/// spellings (`names.found`, already joined) — a case-sensitive filesystem's
/// own possibility, since a case-insensitive one cannot hold more than one
/// spelling of a name that the claim itself is also absent from.
fn spelled_differently_absent_many_text(names: &SpelledNames, at_is_path: bool) -> String {
    let SpelledNames {
        path,
        at,
        found,
        note,
    } = names;
    if at_is_path {
        format!(
            "{path} is spelled {found} on disk{note}; {SPELLED_DIFFERENTLY_REASON}; remove or \
             rename all but one of {found}, and spell that one {at}"
        )
    } else {
        format!(
            "{path} is under {found} on disk, which its skeleton spells {at}{note}; \
             {SPELLED_DIFFERENTLY_REASON}; remove or rename all but one of {found}, and spell \
             that one {at}"
        )
    }
}

/// The message for a [`WearingRefusal`] — always self-contained, since none
/// of these are about one worn skeleton (see [`Refusal::about_one_skeleton`]).
fn wearing_message(wearing: &WearingRefusal) -> String {
    match wearing {
        WearingRefusal::NotATable {
            manifest,
            key: None,
        } => skeletons_not_a_table_message(manifest),
        WearingRefusal::NotATable {
            manifest,
            key: Some(key),
        } => skeleton_not_a_table_message(manifest, key),
        WearingRefusal::ReservedKey {
            manifest,
            key,
            crate_name,
        } => reserved_key_message(manifest, key, crate_name),
        WearingRefusal::NamesNoDependency {
            manifest,
            dependency,
        } => names_no_dependency_message(manifest, dependency),
        WearingRefusal::Unresolved {
            manifest,
            dependency,
        } => unresolved_message(manifest, dependency),
        WearingRefusal::Ambiguous {
            manifest,
            dependency,
            packages,
        } => ambiguous_message(manifest, dependency, packages),
        WearingRefusal::NotASkeleton {
            manifest,
            dependency,
            package: (name, version),
        } => not_a_skeleton_message(manifest, dependency, name, version),
    }
}

/// `[package.metadata.skeletons]` itself is not a table.
fn skeletons_not_a_table_message(manifest: &str) -> String {
    let manifest = Escaped(manifest);
    format!(
        "[package.metadata.skeletons] in {manifest} is not a table; each skeleton the \
         manifest wears is a table under it, [package.metadata.skeletons.<dependency>]"
    )
}

/// One skeleton's entry under `[package.metadata.skeletons]` is not a table.
fn skeleton_not_a_table_message(manifest: &str, key: &str) -> String {
    let (manifest, key) = (Escaped(manifest), Escaped(key));
    format!(
        "[package.metadata.skeletons.{key}] in {manifest} is not a table; it reads \
         [package.metadata.skeletons.{key}] on a line of its own, with the skeleton's \
         options, if any, under it"
    )
}

/// A dependency is declared under `key`, one of the keys a skeleton keeps for
/// its own declarations.
fn reserved_key_message(manifest: &str, key: &str, crate_name: &str) -> String {
    let (manifest, key, crate_name) = (Escaped(manifest), Escaped(key), Escaped(crate_name));
    format!(
        "{manifest} depends on {crate_name} under the key `{key}`, which cannot be worn: \
         `{key}` under [package.metadata.skeletons] belongs to a skeleton's own \
         declaration; give the dependency another key (`{crate_name} = {{ package = \
         \"{crate_name}\", … }}`) and name its table after that key"
    )
}

/// A skeleton's table names a key no dependency is declared under.
fn names_no_dependency_message(manifest: &str, dependency: &str) -> String {
    let (manifest, dependency) = (Escaped(manifest), Escaped(dependency));
    format!(
        "[package.metadata.skeletons.{dependency}] in {manifest} names no dependency of \
         {manifest}; add the dependency, or rename the table to the key the dependency is \
         declared under"
    )
}

/// A dependency is declared, but cargo resolved no package for it.
fn unresolved_message(manifest: &str, dependency: &str) -> String {
    let (manifest, dependency) = (Escaped(manifest), Escaped(dependency));
    format!(
        "{dependency} in {manifest} is declared, but cargo resolved no package for it, so \
         there is no locked skeleton to compare against"
    )
}

/// A dependency resolves to several packages, so its table cannot say which
/// one it wears. Each package is a name and a version.
fn ambiguous_message(manifest: &str, dependency: &str, packages: &[(String, String)]) -> String {
    let (manifest, dependency) = (Escaped(manifest), Escaped(dependency));
    let listed = join_and(
        &packages
            .iter()
            .map(|(name, version)| format!("{} {}", Escaped(name), Escaped(version)))
            .collect::<Vec<_>>(),
    );
    format!(
        "{dependency} in {manifest} resolves to more than one package ({listed}), so \
         [package.metadata.skeletons.{dependency}] cannot say which it wears; declare the \
         dependency at one version"
    )
}

/// A dependency with a skeleton table is a package that is no skeleton.
fn not_a_skeleton_message(manifest: &str, dependency: &str, name: &str, version: &str) -> String {
    let (manifest, dependency) = (Escaped(manifest), Escaped(dependency));
    let (name, version) = (Escaped(name), Escaped(version));
    format!(
        "{dependency} in {manifest} is {name} {version}, which is not a skeleton: its \
         manifest has no [package.metadata.skeletons] table; drop \
         [package.metadata.skeletons.{dependency}], or depend on a skeleton"
    )
}

fn overlap_message(overlap: &Overlap) -> String {
    let claimant_list = join_and(
        &overlap
            .claimants
            .iter()
            .map(|claimant| {
                let (dependency, manifest, skeleton) = (
                    Escaped(&claimant.dependency),
                    Escaped(&claimant.manifest),
                    Escaped(&claimant.skeleton),
                );
                format!(
                    "{dependency} in {manifest} ({skeleton} {})",
                    claimant.version
                )
            })
            .collect::<Vec<_>>(),
    );
    let claimed_by = format!(
        "claimed by {} worn skeletons, {claimant_list}",
        overlap.claimants.len()
    );
    let tail = format!("{claimed_by}; a file belongs to one skeleton, so wear only one of them");

    match overlap.paths.as_slice() {
        [one] => format!("{} is {tail}", Escaped(one.as_str())),
        [first, second] => two_paths_overlap_text(first, second, &claimed_by, &tail),
        many => many_paths_overlap_text(many, &claimed_by),
    }
}

/// The message for an overlap of exactly two paths, worded for how the two
/// relate under the fold. Every spelling it names goes through [`told_apart`]
/// together, so two that differ only in Unicode normalization are told apart.
fn two_paths_overlap_text(
    first_path: &ClaimPath,
    second_path: &ClaimPath,
    claimed_by: &str,
    tail: &str,
) -> String {
    let (first, second) = (first_path.as_str(), second_path.as_str());
    match folded_relation(first_path, second_path) {
        FoldedRelation::SamePath => {
            let shown = told_apart(&[first, second]);
            format!(
                "{} and {} are one name to a filesystem that ignores case or Unicode \
                 normalization{}, and are {tail}",
                &shown[0],
                &shown[1],
                shown.note_in_parentheses()
            )
        }
        FoldedRelation::Nested => {
            let shown = told_apart(&[first, second]);
            format!(
                "{} and {} cannot both exist, since one is inside the other{}, and are {tail}",
                &shown[0],
                &shown[1],
                shown.note_in_parentheses()
            )
        }
        FoldedRelation::SharedDirectorySpelledDifferently {
            first_at,
            second_at,
        } => {
            let shown = told_apart(&[first, second, &first_at, &second_at]);
            format!(
                "{} and {} are in one directory spelled two ways, {} and {}{}, which a \
                 filesystem that ignores case or Unicode normalization takes as one, and are \
                 {claimed_by}; a directory has one spelling on disk, so wear only one of them",
                &shown[0],
                &shown[1],
                &shown[2],
                &shown[3],
                shown.note_in_parentheses()
            )
        }
    }
}

/// The message for an overlap of three or more paths. They can be joined by
/// any mix of the relations above, so the reason is stated to fit every one
/// of them: two different files under one directory spelled two ways are not
/// "one file", and the two-path tail would say they are.
fn many_paths_overlap_text(paths: &[ClaimPath], claimed_by: &str) -> String {
    let names: Vec<&str> = paths.iter().map(ClaimPath::as_str).collect();
    let shown = told_apart(&names);
    let path_list = join_and(
        &(0..names.len())
            .map(|index| shown[index].to_owned())
            .collect::<Vec<_>>(),
    );
    format!(
        "{path_list} cannot all be written, since a filesystem that ignores case or Unicode \
         normalization takes some of them, or directories above them, as one{}, and are \
         {claimed_by}; wear only one of them",
        shown.note_in_parentheses()
    )
}

/// Joins `items` as `"a"`, `"a and b"`, or `"a, b and c"` — this crate's one
/// list grammar, used wherever a message lists claimants, packages or paths,
/// here and in the `sync` messages.
pub(crate) fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        many => {
            let Some((last, rest)) = many.split_last() else {
                unreachable!("many always has at least one item")
            };
            format!("{} and {last}", rest.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::poison::{
        POISON, POISON_FOLDED, assert_escaped_once, assert_every_kind, assert_one_line, claim,
        poison,
    };
    use super::{
        Overlap, Refusal, UnsafePathCause, join_and, option_shape_words, overlap_message,
        render_text, unsafe_path_change_clause, unsafe_path_text, wearing_message,
    };
    use crate::claim::{Claimant, TooLongName};
    use crate::skeleton::{Reason, RenderError, SkeletonIdentity};
    use crate::workspace::{WearingRefusal, WornId};

    #[test]
    fn a_render_refusal_naming_a_file_with_a_newline_stays_on_one_line() {
        // A skeleton's file can be named with a line break in it; the report
        // line for a defect in it must show the break escaped, never as one.
        let error = RenderError::about_file(
            SkeletonIdentity::Named("dependabot".to_owned()),
            "files/a\nb.yml",
            Reason::NotUtf8,
        );
        let worn = WornId {
            manifest: "Cargo.toml".to_owned(),
            key: "dependabot".to_owned(),
        };

        let text = render_text(&worn, "dependabot", &semver::Version::new(0, 1, 0), &error);

        assert!(
            text.starts_with("files/a\\nb.yml: is not valid utf-8;"),
            "the file must show its newline escaped: {text:?}"
        );
        assert_one_line(&text, "the refusal");
    }

    #[test]
    fn join_and_handles_one_two_and_three_items() {
        assert_eq!(join_and(&["a".to_owned()]), "a");
        assert_eq!(join_and(&["a".to_owned(), "b".to_owned()]), "a and b");
        assert_eq!(
            join_and(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
            "a, b and c"
        );
    }

    fn claimant(dependency: &str) -> Claimant {
        Claimant {
            manifest: "Cargo.toml".to_owned(),
            dependency: dependency.to_owned(),
            skeleton: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
        }
    }

    fn overlap(paths: &[&str]) -> Overlap {
        let mut paths: Vec<_> = paths.iter().map(|path| claim(path)).collect();
        paths.sort();
        let claimants = ["one", "two"].into_iter().map(claimant).collect();
        Overlap { paths, claimants }
    }

    #[test]
    fn a_case_only_collision_is_worded_as_one_name_under_the_filesystems_own_fold() {
        let message = overlap_message(&overlap(&[
            ".github/Dependabot.yml",
            ".github/dependabot.yml",
        ]));
        assert!(
            message.starts_with(
                ".github/Dependabot.yml and .github/dependabot.yml are one name to a \
                 filesystem that ignores case or Unicode normalization, and are"
            ),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn an_nfc_nfd_only_collision_is_worded_as_a_case_only_one_and_shows_the_code_points() {
        // "café.txt" precomposed (NFC) against the same visible name
        // decomposed (NFD) fold to the same components without differing in
        // case at all — the case a `to_lowercase` comparison would mis-word
        // as nesting, which is why the two branches are told apart with the
        // render's own fold. The two look alike, so both are shown as code
        // points, with the reason.
        let message = overlap_message(&overlap(&["caf\u{e9}.txt", "cafe\u{301}.txt"]));
        assert!(
            message.starts_with(
                "cafe\u{301}.txt (cafe\\u0301.txt) and caf\u{e9}.txt (caf\\u00E9.txt) are one \
                 name to a filesystem that ignores case or Unicode normalization (the two \
                 spellings differ only in Unicode normalization, so each is followed by its \
                 characters as \\uXXXX code points, as bash 4.3 or later, or zsh, \
                 reads them in $'…' under a UTF-8 locale), and \
                 are claimed by"
            ),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn a_case_only_collision_names_no_code_points() {
        let message = overlap_message(&overlap(&["\u{c9}.txt", "\u{e9}.txt"]));
        assert!(!message.contains("\\u"), "unexpected message: {message}");
    }

    #[test]
    fn a_directory_spelled_two_ways_by_normalization_shows_both_directories_as_code_points() {
        let message = overlap_message(&overlap(&["caf\u{e9}/y", "cafe\u{301}/x"]));
        assert!(
            message.contains(
                "in one directory spelled two ways, cafe\u{301} (cafe\\u0301) and caf\u{e9} \
                 (caf\\u00E9) (the two spellings differ only in Unicode normalization"
            ),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn a_spelling_difference_in_normalization_is_told_apart_in_every_spelled_differently_shape() {
        // The bare `café.yml` was named on both sides of "rename … to …", two
        // strings that display alike. Each name is shown, and is
        // followed by its code points.
        let shown = spelled_differently_text(
            "caf\u{e9}.yml",
            "caf\u{e9}.yml",
            &["cafe\u{301}.yml"],
            false,
        );
        assert_eq!(
            shown,
            "caf\u{e9}.yml (caf\\u00E9.yml) is spelled cafe\u{301}.yml (cafe\\u0301.yml) on disk \
             (the two spellings differ only in Unicode normalization, so each is followed by \
             its characters as \\uXXXX code points, as bash 4.3 or later, or zsh, \
             reads them in $'…' under a UTF-8 locale); \
             `skeletons` reads and writes a claimed path only under the exact spelling its skeleton \
             gives it, since two spellings of one name are one file on some filesystems and two \
             on others; rename cafe\u{301}.yml (cafe\\u0301.yml) to caf\u{e9}.yml (caf\\u00E9.yml)"
        );
        let present =
            spelled_differently_text("caf\u{e9}.yml", "caf\u{e9}.yml", &["cafe\u{301}.yml"], true);
        assert!(
            present.contains("is also present on disk as cafe\u{301}.yml (cafe\\u0301.yml) ("),
            "{present}"
        );
        let under =
            spelled_differently_text("caf\u{e9}/x.yml", "caf\u{e9}", &["cafe\u{301}"], false);
        assert!(
            under.starts_with(
                "caf\u{e9}/x.yml (caf\\u00E9/x.yml) is under cafe\u{301} (cafe\\u0301) on disk, \
                 which its skeleton spells caf\u{e9} (caf\\u00E9) ("
            ),
            "{under}"
        );
        let clause = unsafe_path_change_clause(&UnsafePathCause::SpelledDifferently {
            at: "caf\u{e9}".to_owned(),
            on_disk: vec!["cafe\u{301}".to_owned()],
            claimed_spelling_present: false,
        });
        assert!(
            clause.starts_with(
                "caf\u{e9} (caf\\u00E9) is now also present on disk as cafe\u{301} \
                 (cafe\\u0301); the two spellings differ"
            ),
            "{clause}"
        );
    }

    #[test]
    fn a_folded_ancestor_directory_is_worded_as_nesting() {
        let message = overlap_message(&overlap(&["ci", "ci/x.yml"]));
        assert!(
            message.starts_with(
                "ci and ci/x.yml cannot both exist, since one is inside the other, and are"
            ),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn a_shared_directory_spelled_two_ways_names_the_directory_and_its_own_advice() {
        let message = overlap_message(&overlap(&["A/y", "a/x"]));
        assert!(
            message.starts_with(
                "A/y and a/x are in one directory spelled two ways, A and a, which a filesystem \
                 that ignores case or Unicode normalization takes as one, and are claimed by 2 \
                 worn skeletons, "
            ),
            "unexpected message: {message}"
        );
        assert!(
            message.ends_with("; a directory has one spelling on disk, so wear only one of them"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn three_paths_under_one_directory_spelled_several_ways_are_never_called_one_file() {
        // Three different files, joined only by the directory above them:
        // the message for three or more paths must not say they are one
        // file, as the advice for two identical names does.
        let message = overlap_message(&overlap(&["A/y", "a/x", "a/z"]));
        assert!(
            message.starts_with(
                "A/y, a/x and a/z cannot all be written, since a filesystem that ignores case \
                 or Unicode normalization takes some of them, or directories above them, as \
                 one, and are claimed by "
            ),
            "unexpected message: {message}"
        );
        assert!(
            !message.contains("a file belongs to one skeleton"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn a_deep_shared_directory_names_the_prefix_where_the_spellings_differ() {
        let message = overlap_message(&overlap(&["a/B/y", "a/b/x"]));
        assert!(
            message.contains("in one directory spelled two ways, a/B and a/b, which"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn every_cause_reads_as_a_clause_about_a_path_that_became_unsafe() {
        // The clause is built from the same cause data `unsafe_path_text`
        // reads. Every cause a walk can produce is listed here (a name git
        // refuses to track, or that is too long, is refused when the claim is
        // built, so no walk finds one and its clause is unreachable), and
        // both functions match the cause exhaustively, so a variant added to
        // one without the other does not compile; this pins that each
        // phrasing names the same component the full message does.
        let causes = [
            (
                UnsafePathCause::SpelledDifferently {
                    at: "a".to_owned(),
                    on_disk: vec!["A".to_owned()],
                    claimed_spelling_present: false,
                },
                "a is now also present on disk as A",
            ),
            (
                UnsafePathCause::SpelledDifferently {
                    at: "a".to_owned(),
                    on_disk: Vec::new(),
                    claimed_spelling_present: false,
                },
                "a is now found on disk under another spelling",
            ),
            (
                UnsafePathCause::SymbolicLinkAbove { at: "x".to_owned() },
                "x above it is now a symbolic link",
            ),
            (UnsafePathCause::Symlink, "it is now a symbolic link"),
            (
                UnsafePathCause::NotADirectoryAbove { at: "x".to_owned() },
                "x above it is now a file",
            ),
            (UnsafePathCause::NotAFile, "it is no longer a regular file"),
            (UnsafePathCause::InsideGitDirectory, "it is now inside .git"),
            (
                UnsafePathCause::InsideAnotherRepository { at: "x".to_owned() },
                "x above it is now another git repository",
            ),
            (
                UnsafePathCause::Unreadable {
                    detail: "denied".to_owned(),
                },
                "it can no longer be read: denied",
            ),
        ];
        for (cause, clause) in causes {
            assert_eq!(unsafe_path_change_clause(&cause), clause);
            let full = unsafe_path_text(
                "x/one.yml",
                &cause,
                "a-skeleton",
                &semver::Version::new(0, 1, 0),
            );
            assert!(!full.is_empty(), "{cause:?} must also have a full message");
        }
    }

    #[test]
    fn an_untrackable_name_names_the_component_git_refuses_and_the_skeleton() {
        let cause = UnsafePathCause::UntrackableName {
            at: "d/.git.".to_owned(),
        };
        assert_eq!(
            unsafe_path_text(
                "d/.git./x.yml",
                &cause,
                "a-skeleton",
                &semver::Version::new(0, 1, 0)
            ),
            "d/.git./x.yml cannot be tracked, because git refuses to track a file or directory \
             named \".git.\"; this is a defect in a-skeleton 0.1.0, not in this repository"
        );
    }

    #[test]
    fn an_untrackable_name_shows_an_invisible_character_as_an_escape() {
        let cause = UnsafePathCause::UntrackableName {
            at: ".g\u{200c}it".to_owned(),
        };
        let message = unsafe_path_text(
            ".g\u{200c}it",
            &cause,
            "a-skeleton",
            &semver::Version::new(0, 1, 0),
        );
        assert!(
            message.contains("named \".g\\u200Cit\""),
            "the component must be shown with its invisible character escaped: {message}"
        );
    }

    #[test]
    fn a_name_too_long_to_exist_names_the_name_its_length_and_the_skeleton() {
        let cause = UnsafePathCause::NameTooLong {
            at: "d/nnnn.yml".to_owned(),
            bytes: 256,
            name: TooLongName::Claimed,
        };
        assert_eq!(
            unsafe_path_text(
                "d/nnnn.yml",
                &cause,
                "a-skeleton",
                &semver::Version::new(0, 1, 0)
            ),
            "d/nnnn.yml cannot exist, because its name nnnn.yml is 256 bytes, more than the 255 \
             a file name can hold; this is a defect in a-skeleton 0.1.0, not in this repository"
        );
    }

    #[test]
    fn a_name_sync_cannot_stage_names_the_staging_name_and_its_length() {
        let cause = UnsafePathCause::NameTooLong {
            at: "nnnn.yml".to_owned(),
            bytes: 256,
            name: TooLongName::Staging,
        };
        assert_eq!(
            unsafe_path_text(
                "nnnn.yml",
                &cause,
                "a-skeleton",
                &semver::Version::new(0, 1, 0)
            ),
            "nnnn.yml cannot be written by sync, which stages it beside itself as \
             .nnnn.yml.skeletons-sync, 256 bytes, more than the 255 a file name can hold; this is a \
             defect in a-skeleton 0.1.0, not in this repository"
        );
    }

    fn spelled_differently_text(
        path: &str,
        at: &str,
        on_disk: &[&str],
        claimed_spelling_present: bool,
    ) -> String {
        let cause = UnsafePathCause::SpelledDifferently {
            at: at.to_owned(),
            on_disk: on_disk
                .iter()
                .map(|spelling| (*spelling).to_owned())
                .collect(),
            claimed_spelling_present,
        };
        unsafe_path_text(path, &cause, "a-skeleton", &semver::Version::new(0, 1, 0))
    }

    #[test]
    fn the_final_component_is_absent_with_one_on_disk_spelling() {
        assert_eq!(
            spelled_differently_text(
                ".github/dependabot.yml",
                ".github/dependabot.yml",
                &[".github/DEPENDABOT.YML"],
                false,
            ),
            ".github/dependabot.yml is spelled .github/DEPENDABOT.YML on disk; `skeletons` reads and \
             writes a claimed path only under the exact spelling its skeleton gives it, since \
             two spellings of one name are one file on some filesystems and two on others; \
             rename .github/DEPENDABOT.YML to .github/dependabot.yml"
        );
    }

    #[test]
    fn an_ancestor_is_absent_with_one_on_disk_spelling() {
        assert_eq!(
            spelled_differently_text(".github/dependabot.yml", ".github", &[".GitHub"], false,),
            ".github/dependabot.yml is under .GitHub on disk, which its skeleton spells \
             .github; `skeletons` reads and writes a claimed path only under the exact spelling its \
             skeleton gives it, since two spellings of one name are one file on some \
             filesystems and two on others; rename .GitHub to .github"
        );
    }

    #[test]
    fn the_final_component_is_absent_with_two_on_disk_spellings() {
        assert_eq!(
            spelled_differently_text(
                "dependabot.yml",
                "dependabot.yml",
                &["DEPENDABOT.YML", "Dependabot.Yml"],
                false,
            ),
            "dependabot.yml is spelled DEPENDABOT.YML and Dependabot.Yml on disk; `skeletons` reads \
             and writes a claimed path only under the exact spelling its skeleton gives it, \
             since two spellings of one name are one file on some filesystems and two on \
             others; remove or rename all but one of DEPENDABOT.YML and Dependabot.Yml, and \
             spell that one dependabot.yml"
        );
    }

    #[test]
    fn an_ancestor_is_absent_with_two_on_disk_spellings() {
        assert_eq!(
            spelled_differently_text(
                ".github/dependabot.yml",
                ".github",
                &[".GitHub", ".Github"],
                false,
            ),
            ".github/dependabot.yml is under .GitHub and .Github on disk, which its skeleton \
             spells .github; `skeletons` reads and writes a claimed path only under the exact \
             spelling its skeleton gives it, since two spellings of one name are one file on \
             some filesystems and two on others; remove or rename all but one of .GitHub and \
             .Github, and spell that one .github"
        );
    }

    #[test]
    fn the_claimed_spelling_is_also_present_for_the_final_component() {
        assert_eq!(
            spelled_differently_text(
                "dependabot.yml",
                "dependabot.yml",
                &["DEPENDABOT.YML"],
                true,
            ),
            "dependabot.yml is also present on disk as DEPENDABOT.YML; `skeletons` reads and writes a \
             claimed path only under the exact spelling its skeleton gives it, since two \
             spellings of one name are one file on some filesystems and two on others; remove \
             or rename DEPENDABOT.YML"
        );
    }

    #[test]
    fn the_claimed_spelling_is_also_present_under_a_differing_ancestor() {
        assert_eq!(
            spelled_differently_text(".github/dependabot.yml", ".github", &[".GitHub"], true,),
            ".github/dependabot.yml is under .github, which is also present on disk as \
             .GitHub; `skeletons` reads and writes a claimed path only under the exact spelling its \
             skeleton gives it, since two spellings of one name are one file on some \
             filesystems and two on others; remove or rename .GitHub"
        );
    }

    #[test]
    fn only_the_lookup_witness_fired() {
        assert_eq!(
            spelled_differently_text("café.txt", "café.txt", &[], false),
            "café.txt is found on disk under another spelling of café.txt; `skeletons` reads and \
             writes a claimed path only under the exact spelling its skeleton gives it, since \
             two spellings of one name are one file on some filesystems and two on others; \
             rename it to café.txt"
        );
    }

    fn poisoned_worn() -> WornId {
        WornId {
            manifest: poison(),
            key: poison(),
        }
    }

    /// How many kinds of [`WearingRefusal`] there are. The match in
    /// [`wearing_variant`] has no wildcard, so a variant added without an
    /// arm does not compile, and the sample list must then cover it.
    const WEARING_VARIANTS: usize = 6;

    const fn wearing_variant(refusal: &WearingRefusal) -> usize {
        match refusal {
            WearingRefusal::NotATable { .. } => 0,
            WearingRefusal::ReservedKey { .. } => 1,
            WearingRefusal::NamesNoDependency { .. } => 2,
            WearingRefusal::Unresolved { .. } => 3,
            WearingRefusal::Ambiguous { .. } => 4,
            WearingRefusal::NotASkeleton { .. } => 5,
        }
    }

    fn wearing_samples() -> Vec<WearingRefusal> {
        vec![
            WearingRefusal::NotATable {
                manifest: poison(),
                key: None,
            },
            WearingRefusal::NotATable {
                manifest: poison(),
                key: Some(poison()),
            },
            WearingRefusal::ReservedKey {
                manifest: poison(),
                key: poison(),
                crate_name: poison(),
            },
            WearingRefusal::NamesNoDependency {
                manifest: poison(),
                dependency: poison(),
            },
            WearingRefusal::Unresolved {
                manifest: poison(),
                dependency: poison(),
            },
            WearingRefusal::Ambiguous {
                manifest: poison(),
                dependency: poison(),
                packages: vec![(poison(), poison()), (poison(), poison())],
            },
            WearingRefusal::NotASkeleton {
                manifest: poison(),
                dependency: poison(),
                package: (poison(), poison()),
            },
        ]
    }

    #[test]
    fn every_wearing_refusal_prints_its_outside_text_escaped_once() {
        // Every field of every kind of wearing refusal holds the poison,
        // and the message must show each as the escape. The kinds are counted
        // through an exhaustive match, so a new one cannot go untested.
        let samples = wearing_samples();
        assert_every_kind(
            samples.iter().map(wearing_variant),
            WEARING_VARIANTS,
            "WearingRefusal",
        );

        for sample in &samples {
            assert_escaped_once(&wearing_message(sample), &format!("{sample:?}"));
            assert_escaped_once(
                &Refusal::Wearing(sample.clone()).message(),
                &format!("{sample:?}"),
            );
        }
    }

    /// How many kinds of [`UnsafePathCause`] there are; see
    /// [`WEARING_VARIANTS`].
    const CAUSE_VARIANTS: usize = 10;

    /// Whether a path that became unsafe can be described by a change clause.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ChangeClause {
        /// The cause can be found on a later look at the filesystem.
        Described,
        /// The cause is refused when the claim is built (a name git refuses,
        /// or one that is too long), so no later look ever finds it.
        NeverFound,
    }

    /// The index of a cause's kind, and whether it has a change clause.
    struct CauseKind {
        index: usize,
        change_clause: ChangeClause,
    }

    const fn cause_kind(cause: &UnsafePathCause) -> CauseKind {
        let (index, change_clause) = match cause {
            UnsafePathCause::SpelledDifferently { .. } => (0, ChangeClause::Described),
            UnsafePathCause::SymbolicLinkAbove { .. } => (1, ChangeClause::Described),
            UnsafePathCause::Symlink => (2, ChangeClause::Described),
            UnsafePathCause::NotADirectoryAbove { .. } => (3, ChangeClause::Described),
            UnsafePathCause::NotAFile => (4, ChangeClause::Described),
            UnsafePathCause::InsideGitDirectory => (5, ChangeClause::Described),
            UnsafePathCause::InsideAnotherRepository { .. } => (6, ChangeClause::Described),
            UnsafePathCause::UntrackableName { .. } => (7, ChangeClause::NeverFound),
            UnsafePathCause::NameTooLong { .. } => (8, ChangeClause::NeverFound),
            UnsafePathCause::Unreadable { .. } => (9, ChangeClause::Described),
        };
        CauseKind {
            index,
            change_clause,
        }
    }

    fn cause_samples() -> Vec<UnsafePathCause> {
        let spelled =
            |at: String, on_disk: Vec<String>, present: bool| UnsafePathCause::SpelledDifferently {
                at,
                on_disk,
                claimed_spelling_present: present,
            };
        vec![
            spelled(poison(), Vec::new(), false),
            spelled(poison(), vec![poison()], false),
            spelled(poison(), vec![poison(), poison()], false),
            spelled(poison(), vec![poison()], true),
            spelled(format!("{POISON}/directory"), vec![poison()], false),
            UnsafePathCause::SymbolicLinkAbove { at: poison() },
            UnsafePathCause::Symlink,
            UnsafePathCause::NotADirectoryAbove { at: poison() },
            UnsafePathCause::NotAFile,
            UnsafePathCause::InsideGitDirectory,
            UnsafePathCause::InsideAnotherRepository { at: poison() },
            UnsafePathCause::UntrackableName { at: poison() },
            UnsafePathCause::NameTooLong {
                at: poison(),
                bytes: 256,
                name: TooLongName::Claimed,
            },
            UnsafePathCause::NameTooLong {
                at: poison(),
                bytes: 256,
                name: TooLongName::Staging,
            },
            UnsafePathCause::Unreadable { detail: poison() },
        ]
    }

    #[test]
    fn every_unsafe_path_cause_prints_its_outside_text_escaped_once() {
        // The full message names the path, the cause's own text and the
        // skeleton, all poisoned. A cause that holds no text (a symbolic link
        // on the final component) still shows the path.
        let samples = cause_samples();
        assert_every_kind(
            samples.iter().map(|cause| cause_kind(cause).index),
            CAUSE_VARIANTS,
            "UnsafePathCause",
        );

        for cause in &samples {
            let text = unsafe_path_text(POISON, cause, POISON, &semver::Version::new(0, 1, 0));
            assert_escaped_once(&text, &format!("{cause:?}"));
            let refusal = Refusal::UnsafePath {
                worn: poisoned_worn(),
                skeleton: poison(),
                version: semver::Version::new(0, 1, 0),
                path: poison(),
                cause: cause.clone(),
            };
            assert_escaped_once(&refusal.message(), &format!("{cause:?}"));
            assert_escaped_once(&refusal.text_in_block(), &format!("{cause:?}"));
        }
    }

    #[test]
    fn every_unsafe_path_change_clause_prints_its_outside_text_escaped_once() {
        for cause in &cause_samples() {
            if cause_kind(cause).change_clause == ChangeClause::NeverFound {
                continue;
            }
            let clause = unsafe_path_change_clause(cause);
            // A clause about a cause with no text of its own names nothing to
            // escape, so only the ones that carry text must show it.
            if matches!(
                cause,
                UnsafePathCause::Symlink
                    | UnsafePathCause::NotAFile
                    | UnsafePathCause::InsideGitDirectory
            ) {
                assert!(!clause.is_empty(), "{cause:?}");
            } else {
                assert_escaped_once(&clause, &format!("{cause:?}"));
            }
        }
    }

    #[test]
    fn an_option_shape_refusal_prints_its_option_escaped_once() {
        // `OptionShapeRefusal` can only be built inside `workspace`, so the
        // words it prints are tested through the function that writes them,
        // given the same option name and the shape word the refusal carries.
        assert_escaped_once(&option_shape_words(POISON, "a number"), "the shape words");
    }

    #[test]
    fn a_render_refusal_prints_its_outside_text_escaped_once() {
        // A defect in a file (with and without a line) and a refusal of the
        // wearer's own choice, which names no file.
        let identity = || SkeletonIdentity::Named(poison());
        let errors = [
            RenderError::about_file(identity(), POISON, Reason::NotUtf8),
            RenderError::about_line(
                identity(),
                POISON,
                std::num::NonZeroU32::MIN,
                Reason::NotUtf8,
            ),
            RenderError::about_choice(identity(), Reason::OptionNameInvalid { option: poison() }),
        ];
        for error in errors {
            let has_file = error.file().is_some();
            let refusal = Refusal::Render {
                worn: poisoned_worn(),
                skeleton: poison(),
                version: semver::Version::new(0, 1, 0),
                error,
            };
            assert_escaped_once(&refusal.message(), &format!("message, file {has_file}"));
            assert_escaped_once(
                &refusal.text_in_block(),
                &format!("text in block, file {has_file}"),
            );
        }
    }

    fn poisoned_claimant() -> Claimant {
        Claimant {
            manifest: poison(),
            dependency: poison(),
            skeleton: poison(),
            version: semver::Version::new(0, 1, 0),
        }
    }

    #[test]
    fn an_overlap_of_one_two_or_three_paths_prints_its_outside_text_escaped_once() {
        // One path, then two in each of the three ways they can relate (the
        // same name under the fold, one inside the other, a shared directory
        // spelled two ways), then three.
        let one = POISON.to_owned();
        let folded = POISON_FOLDED.to_owned();
        let inside = format!("{POISON}/x");
        let path_sets: [&[&str]; 5] = [
            &[&one],
            &[&one, &folded],
            &[&one, &inside],
            &[&format!("{POISON}/y"), &format!("{POISON_FOLDED}/x")],
            &[
                &format!("{POISON}/y"),
                &format!("{POISON_FOLDED}/x"),
                &format!("{POISON}/z"),
            ],
        ];
        for texts in path_sets {
            let overlap = Overlap {
                paths: texts.iter().map(|text| claim(text)).collect(),
                claimants: vec![poisoned_claimant(), poisoned_claimant()],
            };
            let message = overlap_message(&overlap);
            assert_escaped_once(&message, &format!("overlap of {texts:?}"));
        }
    }
}
