//! The words a pin's own header prints after `from`, and the exact text
//! `--json`'s own `pin.detail` carries — one function, so `check/table.rs`
//! and `check/json.rs` cannot describe the same pin two different ways.
//! This also carries the rule for how a path pin's directory is shown,
//! since that rule feeds both outputs through this same function.

use std::path::{Component, Path};

use crate::skeleton::Escaped;
use crate::workspace::{self, Pin};

/// The words `check`'s human header prints after `from`, and the text
/// `--json`'s own `pin.detail` carries. Every url, tag, branch, rev, registry
/// and path in it came from the repository's own manifest and lockfile, so
/// each is escaped here, once, and the header and `pin.detail` read the same.
/// The pin's own fields in `--json` (`url`, `tag`, `path`, …) keep the exact
/// text.
pub(super) fn pin_detail(pin: &Pin, root: &Path) -> String {
    match pin {
        Pin::CratesIo => "crates.io".to_owned(),
        Pin::OtherRegistry { source } => format!("registry {}", Escaped(source)),
        Pin::Tag { url, tag, .. } => {
            let (url, tag) = (Escaped(url), Escaped(tag));
            format!("tag {tag} of {url}")
        }
        Pin::Branch {
            url,
            branch,
            commit,
        } => {
            let (url, branch) = (Escaped(url), Escaped(branch));
            format!("branch {branch} of {url} at {}", commit.abbreviated())
        }
        Pin::DefaultBranch { url, commit } => format!(
            "the default branch of {} at {}",
            Escaped(url),
            commit.abbreviated()
        ),
        Pin::Rev { url, rev, .. } => {
            let (url, rev) = (Escaped(url), Escaped(rev));
            format!("rev {rev} of {url}")
        }
        Pin::Path { directory } => {
            format!("path {}", Escaped(&path_pin_shown(root, directory)))
        }
        Pin::Unrecognised { source } => Escaped(source).to_string(),
    }
}

/// How a path pin's own directory is shown: relative to the workspace root,
/// `/`-separated, when the two share at least one real directory
/// below the filesystem root — the common case, a sibling checkout (tested
/// by `sharing_only_a_distant_common_ancestor_is_still_shown_relative`,
/// below). When the only thing they share is the filesystem root itself,
/// the relative form would be the absolute path with one `..` per level of
/// `root` in front of it: exactly the same machine-specific directories, a
/// home directory's own user name among them, and harder to read for it.
/// That case is shown absolute instead, `/`-separated, as the operating
/// system gives it (tested by
/// `sharing_nothing_but_the_filesystem_root_is_shown_absolute`, below).
///
/// This is the one stated exception to "every path is relative to the
/// workspace root" (`.docs/wearing.md`); every other path this crate reports
/// is always inside the workspace, so it never needs one.
pub(super) fn path_pin_shown(root: &Path, directory: &Path) -> String {
    // `cargo metadata` gives every path this crate reads as absolute
    // (verified in `workspace/schema.rs`'s own test of a path dependency);
    // the component-counting rule below only means what this doc comment
    // says when both sides are anchored at the same filesystem root.
    assert!(root.is_absolute(), "the workspace root is always absolute");
    assert!(
        directory.is_absolute(),
        "a path pin's directory is always absolute"
    );
    if shares_a_directory_below_the_filesystem_root(root, directory) {
        workspace::relative_to_root(root, directory)
    } else {
        absolute_display(directory)
    }
}

/// Whether `root` and `directory` share any component past the filesystem
/// root itself, which is the one component `/` ([`Component::RootDir`]).
fn shares_a_directory_below_the_filesystem_root(root: &Path, directory: &Path) -> bool {
    let root_components: Vec<_> = root.components().collect();
    let directory_components: Vec<_> = directory.components().collect();
    let shared = root_components
        .iter()
        .zip(directory_components.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let root_only = root_components
        .iter()
        .take_while(|component| matches!(component, Component::RootDir))
        .count();
    shared > root_only
}

/// `directory`, exactly as the operating system gives it, `/`-separated —
/// never through [`workspace::relative_to_root`], which only ever climbs
/// with `..` and would still carry every one of `root`'s own components in
/// front of `directory`'s.
fn absolute_display(directory: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in directory.components() {
        match component {
            Component::Prefix(_) => unreachable!("a Unix path has no prefix component"),
            Component::RootDir => parts.push(String::new()),
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir | Component::ParentDir => {
                unreachable!("cargo metadata never gives a `.`/`..` path component")
            }
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{path_pin_shown, pin_detail};
    use crate::git::ObjectId;
    use crate::survey::poison::{POISON, assert_escaped_once, assert_every_kind};
    use crate::workspace::Pin;

    /// `text` parsed as an [`ObjectId`], for a test's own expected value.
    fn oid(text: &str) -> ObjectId {
        ObjectId::parse(text).expect("a well-formed test object id")
    }

    #[test]
    fn a_directory_inside_the_root_is_shown_relative_with_no_leading_dots() {
        assert_eq!(
            path_pin_shown(Path::new("/w"), Path::new("/w/sub/skeleton")),
            "sub/skeleton"
        );
    }

    #[test]
    fn a_sibling_directory_is_shown_relative_with_one_leading_dot_dot() {
        assert_eq!(
            path_pin_shown(Path::new("/w/root"), Path::new("/w/skeleton")),
            "../skeleton"
        );
    }

    // Sharing only a grandparent (`/Users`) two levels up still counts as
    // sharing a directory below the filesystem root, so this is shown
    // relative too — climbing once per level of `root` beyond the shared
    // prefix, exactly as `workspace::relative_to_root` always has.
    #[test]
    fn sharing_only_a_distant_common_ancestor_is_still_shown_relative() {
        assert_eq!(
            path_pin_shown(Path::new("/Users/a/w"), Path::new("/Users/b/skeleton")),
            "../../b/skeleton"
        );
    }

    // The only thing `/w` and `/Users/a/skeleton` share is `/` itself, which does
    // not count as a shared real directory: the relative form would be
    // `../Users/a/skeleton`, carrying the same machine-specific directory while
    // being harder to read, so this reads absolute instead.
    #[test]
    fn sharing_nothing_but_the_filesystem_root_is_shown_absolute() {
        assert_eq!(
            path_pin_shown(Path::new("/w"), Path::new("/Users/a/skeleton")),
            "/Users/a/skeleton"
        );
    }

    /// How many kinds of [`Pin`] there are. The match in [`pin_kind`] has no
    /// wildcard, so a kind added without an arm does not compile, and the
    /// samples must then cover it.
    const PIN_KINDS: usize = 8;

    const fn pin_kind(pin: &Pin) -> usize {
        match pin {
            Pin::CratesIo => 0,
            Pin::OtherRegistry { .. } => 1,
            Pin::Tag { .. } => 2,
            Pin::Branch { .. } => 3,
            Pin::DefaultBranch { .. } => 4,
            Pin::Rev { .. } => 5,
            Pin::Path { .. } => 6,
            Pin::Unrecognised { .. } => 7,
        }
    }

    #[test]
    fn every_kind_of_pin_prints_its_outside_text_escaped_once() {
        // Every text field of every kind of pin holds the poison. The
        // detail is one line and shows each as the escape, once, whether it
        // is a url, a tag, a branch, a rev, a registry, an unrecognised
        // source or a path.
        let commit = oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39");
        let text = || POISON.to_owned();
        let pins = [
            Pin::OtherRegistry { source: text() },
            Pin::Tag {
                url: text(),
                tag: text(),
                commit: commit.clone(),
            },
            Pin::Branch {
                url: text(),
                branch: text(),
                commit: commit.clone(),
            },
            Pin::DefaultBranch {
                url: text(),
                commit: commit.clone(),
            },
            Pin::Rev {
                url: text(),
                rev: text(),
                commit,
            },
            Pin::Path {
                directory: Path::new(&format!("/w/{POISON}")).to_owned(),
            },
            Pin::Unrecognised { source: text() },
        ];
        assert_every_kind(
            pins.iter().chain([&Pin::CratesIo]).map(pin_kind),
            PIN_KINDS,
            "Pin",
        );

        for pin in &pins {
            let detail = pin_detail(pin, Path::new("/w/root"));
            assert_escaped_once(&detail, &format!("{pin:?}"));
        }
    }

    /// One row per [`Pin`] variant: `pin_detail` is the one function both
    /// `check/table.rs`'s header and `check/json.rs`'s `pin.detail` call, so
    /// this table is the whole of what either output can ever say.
    #[test]
    fn pin_detail_has_its_own_words_for_every_pin_variant() {
        let root = Path::new("/w/root");
        for (pin, expected) in pin_detail_cases() {
            assert_eq!(pin_detail(&pin, root), expected, "pin was: {pin:?}");
        }
    }

    /// The table [`pin_detail_has_its_own_words_for_every_pin_variant`]
    /// checks every row of, one per [`Pin`] variant: the exact words
    /// [`pin_detail`] must produce for it, against the workspace root
    /// `/w/root`.
    fn pin_detail_cases() -> Vec<(Pin, &'static str)> {
        vec![
            (Pin::CratesIo, "crates.io"),
            (
                Pin::OtherRegistry {
                    source: "sparse+https://example.invalid/index/".to_owned(),
                },
                "registry sparse+https://example.invalid/index/",
            ),
            (
                Pin::Tag {
                    url: "https://example.invalid/skeleton".to_owned(),
                    tag: "v0.3.0".to_owned(),
                    commit: oid("8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f"),
                },
                "tag v0.3.0 of https://example.invalid/skeleton",
            ),
            (
                Pin::Branch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    branch: "main".to_owned(),
                    commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
                },
                "branch main of https://example.invalid/skeleton at 3f2a9c1",
            ),
            (
                Pin::DefaultBranch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
                },
                "the default branch of https://example.invalid/skeleton at 3f2a9c1",
            ),
            (
                Pin::Rev {
                    url: "https://example.invalid/skeleton".to_owned(),
                    rev: "deadbee".to_owned(),
                    commit: oid("3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"),
                },
                "rev deadbee of https://example.invalid/skeleton",
            ),
            (
                Pin::Path {
                    directory: Path::new("/w/ci-skeleton").to_owned(),
                },
                "path ../ci-skeleton",
            ),
            (
                Pin::Unrecognised {
                    source: "registry+file:///nowhere".to_owned(),
                },
                "registry+file:///nowhere",
            ),
        ]
    }
}
