//! The names git refuses to track besides `.git` itself.
//!
//! Git will not add a path with a component it reads as `.git`, and it reads
//! more than that one spelling as `.git`. A file can be created at such a
//! name on disk and never committed, so a claim there could sit untracked for
//! ever. The rule is git's, taken from its source (v2.53.0) rather than
//! from memory: `read-cache.c`, `verify_path_internal` decides which
//! components are refused and calls the two readers below, with git's own
//! path protections switched on, as `git add` does; `path.c` holds the
//! reading [`is_dotgit_or_short_name`] follows, and `utf8.c` the one
//! [`is_dotgit_through_ignorable`] follows.
//!
//! Only whole components are judged. `.` and `..` are refused by git too, and
//! are not here: a directory entry is never named either, so no claim's
//! component is.

/// The sixteen code points git skips when it reads a name as `.git`
/// (`utf8.c`, `next_hfs_char`): a filesystem that ignores them stores
/// `.g<one of these>it` as the name `.git`, so git refuses it. Skipped
/// anywhere in the name, before the dot and after the `t` included.
const IGNORABLE: [char; 16] = [
    '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
    '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}', '\u{206f}', '\u{feff}',
];

/// Whether git refuses to track a path with `component` as one of its
/// components, in any of the readings [`is_dotgit_through_ignorable`] and
/// [`is_dotgit_or_short_name`] name.
///
/// A backslash inside a component starts a new name for the second reading:
/// git decides that `sub\.git` holds `.git`, so it refuses it. A backslash
/// that is the component's first character does not: `verify_path_internal`
/// consumes the first character of each component before it looks for a
/// backslash, so `\.git` is a name git accepts, and this follows it.
///
/// The exact spelling `.git` (any ASCII case) is refused by git as well, and
/// is not judged here: [`super::is_git_directory_name`] owns it, for a reason
/// of its own, so no name is both, and neither answer depends on which is
/// asked first.
pub(super) fn is_untrackable_name(component: &str) -> bool {
    if super::is_git_directory_name(component) {
        return false;
    }
    if is_dotgit_through_ignorable(component) {
        return true;
    }
    if is_dotgit_or_short_name(component) {
        return true;
    }
    component.match_indices('\\').any(|(index, _)| {
        if index == 0 {
            // The first character of a component is never a separator.
            return false;
        }
        is_dotgit_or_short_name(&component[index + 1..])
    })
}

/// `.git` in any ASCII case with any of [`IGNORABLE`] skipped anywhere:
/// `utf8.c`, `is_hfs_dotgit`.
fn is_dotgit_through_ignorable(component: &str) -> bool {
    let mut kept = component
        .chars()
        .filter(|character| !IGNORABLE.contains(character));
    for wanted in ['.', 'g', 'i', 't'] {
        match kept.next() {
            Some(character) if character.to_ascii_lowercase() == wanted => {}
            Some(_) | None => return false,
        }
    }
    kept.next().is_none()
}

/// `.git` or `git~1` in any ASCII case, followed by nothing, or by a run of
/// spaces and periods and then nothing, a colon (a stream of that name, and
/// whatever follows it) or a backslash (which git reads as a separator); the
/// reading in `path.c` that `verify_path_internal` calls. The run is over as
/// soon as any other character appears, so `.gitx` and `.git.x` are ordinary
/// names.
fn is_dotgit_or_short_name(name: &str) -> bool {
    let after = [".git", "git~1"].iter().find_map(|prefix| {
        let head = name.get(..prefix.len())?;
        head.eq_ignore_ascii_case(prefix)
            .then(|| &name[prefix.len()..])
    });
    let Some(after) = after else {
        return false;
    };
    for character in after.chars() {
        match character {
            ' ' | '.' => {}
            ':' | '\\' => return true,
            _ => return false,
        }
    }
    true
}
