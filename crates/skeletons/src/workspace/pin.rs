//! How a skeleton is pinned — read from a package's own `source` (a registry, a
//! git URL and query, or nothing at all for a path dependency) and, for a
//! git source, the query string cargo encodes into it.
//!
//! `Pin` decides `behind`'s rule for a worn dependency — which remote, if
//! any, `behind` asks, and how the answer is compared against what is
//! locked. Here it is only ever built and, for the human output and
//! `--json`, displayed; `behind.rs` is what reads it to ask the question.

use std::path::{Path, PathBuf};

use crate::git::ObjectId;

/// The source `cargo metadata` reports for a crates.io dependency, always
/// exactly this string — including when `[source.crates-io] replace-with`
/// serves it from somewhere else, and including in the offline,
/// vendored-directory fixtures `ritual`'s tests use. Exposed for
/// `check/json.rs`, which reports it verbatim as a registry pin's `source`.
pub(crate) const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// How a worn skeleton is pinned to one version, and what a remote could ever
/// say about it being newer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pin {
    /// The default registry: asked for `behind` against the sparse index.
    CratesIo,
    /// Any other registry: `behind` is undetermined, and never queried — an
    /// answered-but-unverified case would be exactly the guess this format
    /// otherwise refuses to make, since nothing here extends past crates.io
    /// itself.
    OtherRegistry { source: String },
    /// `git = "…", tag = "…"`: `behind` when a newer version tag exists.
    Tag {
        url: String,
        tag: String,
        commit: ObjectId,
    },
    /// `git = "…", branch = "…"`: `behind` when there is newer content in
    /// this skeleton's own directory, past the locked commit.
    Branch {
        url: String,
        branch: String,
        commit: ObjectId,
    },
    /// `git = "…"` naming none of `tag`/`branch`/`rev`: judged the same as
    /// `Branch`, against the remote's own default branch.
    DefaultBranch { url: String, commit: ObjectId },
    /// `git = "…", rev = "…"`: pinned, like a path dependency.
    Rev {
        url: String,
        rev: String,
        commit: ObjectId,
    },
    /// A local path dependency: pinned, permanently — there is no remote to
    /// ask.
    Path { directory: PathBuf },
    /// A `source` this crate does not recognise at all — an unknown or
    /// duplicated git query key, or a source shape that is none of the
    /// above. `behind` is undetermined.
    Unrecognised { source: String },
}

impl Pin {
    /// Decides how a package is pinned, from its own `source` field
    /// (`source`) and the directory its own manifest sits in
    /// (`skeleton_directory` — the same `manifest_path.parent()` a worn
    /// dependency's own `skeleton_directory` holds).
    ///
    /// A `None` source means a path dependency — reached either by a plain
    /// `path = "…"` line, or, just as validly, by a `[patch]` table
    /// redirecting an otherwise registry- or git-sourced dependency to a
    /// local directory. Either way `skeleton_directory` already names that
    /// package's real directory: the resolved package's own manifest path
    /// is the one source of truth for where it lives, so `Path` takes it
    /// directly rather than guessing from the wearer's own declared
    /// dependency, which carries no directory at all in the `[patch]`
    /// case.
    pub(crate) fn from_source(source: Option<&str>, skeleton_directory: &Path) -> Self {
        match source {
            None => Self::Path {
                directory: skeleton_directory.to_owned(),
            },
            Some(source) if source == CRATES_IO_SOURCE => Self::CratesIo,
            Some(source) => match source.strip_prefix("git+") {
                Some(git_source) => Self::from_git_source(source, git_source),
                None if source.starts_with("registry+") || source.starts_with("sparse+") => {
                    Self::OtherRegistry {
                        source: source.to_owned(),
                    }
                }
                None => Self::Unrecognised {
                    source: source.to_owned(),
                },
            },
        }
    }

    /// Parses the part of a `git+…` source after the `git+` prefix:
    /// `<url>?<query>#<commit>` or `<url>#<commit>` with no query at all.
    fn from_git_source(whole_source: &str, git_source: &str) -> Self {
        let Some((before_commit, commit)) = git_source.rsplit_once('#') else {
            return Self::Unrecognised {
                source: whole_source.to_owned(),
            };
        };
        let Some(commit) = ObjectId::parse(commit) else {
            return Self::Unrecognised {
                source: whole_source.to_owned(),
            };
        };

        let (url, query) = before_commit
            .split_once('?')
            .map_or_else(|| (before_commit, None), |(url, query)| (url, Some(query)));

        let Some(query) = query else {
            return Self::DefaultBranch {
                url: url.to_owned(),
                commit,
            };
        };

        match parsed_git_query(query) {
            Some(GitQuery::Tag(tag)) => Self::Tag {
                url: url.to_owned(),
                tag,
                commit,
            },
            Some(GitQuery::Branch(branch)) => Self::Branch {
                url: url.to_owned(),
                branch,
                commit,
            },
            Some(GitQuery::Rev(rev)) => Self::Rev {
                url: url.to_owned(),
                rev,
                commit,
            },
            None => Self::Unrecognised {
                source: whole_source.to_owned(),
            },
        }
    }
}

/// One of the three query shapes a git source's query string can decode to.
enum GitQuery {
    Tag(String),
    Branch(String),
    Rev(String),
}

/// Decodes a git source's query string (already percent-decoded key by key,
/// since cargo percent-encodes a `/` inside a branch or tag name) into
/// exactly one of `tag`, `branch` or `rev` — `None` for an unknown key, a
/// duplicated one, or more than one of the three at once.
fn parsed_git_query(query: &str) -> Option<GitQuery> {
    let mut tag = None;
    let mut branch = None;
    let mut rev = None;

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let value = value.into_owned();
        match key.as_ref() {
            "tag" if tag.is_none() => tag = Some(value),
            "branch" if branch.is_none() => branch = Some(value),
            "rev" if rev.is_none() => rev = Some(value),
            _ => return None,
        }
    }

    match (tag, branch, rev) {
        (Some(tag), None, None) => Some(GitQuery::Tag(tag)),
        (None, Some(branch), None) => Some(GitQuery::Branch(branch)),
        (None, None, Some(rev)) => Some(GitQuery::Rev(rev)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use proptest::prelude::*;

    use super::{CRATES_IO_SOURCE, Pin};
    use crate::git::ObjectId;

    /// An arbitrary absolute directory, standing in for a worn dependency's
    /// own `skeleton_directory` in every test below whose `source` is not
    /// `None` — `from_source` only ever reads `skeleton_directory` at all
    /// when deciding a `Path` pin.
    fn skeleton_directory() -> &'static Path {
        Path::new("/workspace/some-skeleton")
    }

    const COMMIT: &str = "3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39";

    /// `text` parsed as an [`ObjectId`], for a test's own expected value.
    fn oid(text: &str) -> ObjectId {
        ObjectId::parse(text).expect("a well-formed test object id")
    }

    #[test]
    fn no_source_is_a_path_pin_naming_the_skeleton_directory_it_was_given() {
        let pin = Pin::from_source(None, Path::new("/abs/ci-skeleton"));
        assert_eq!(
            pin,
            Pin::Path {
                directory: "/abs/ci-skeleton".into()
            }
        );
    }

    #[test]
    fn the_crates_io_source_is_recognised_exactly() {
        let pin = Pin::from_source(Some(CRATES_IO_SOURCE), skeleton_directory());
        assert_eq!(pin, Pin::CratesIo);
    }

    #[test]
    fn another_registrys_source_is_other_registry() {
        let pin = Pin::from_source(
            Some("sparse+https://example.invalid/index/"),
            skeleton_directory(),
        );
        assert_eq!(
            pin,
            Pin::OtherRegistry {
                source: "sparse+https://example.invalid/index/".to_owned()
            }
        );
    }

    #[test]
    fn a_tag_source_parses_the_url_tag_and_commit() {
        let source = format!("git+https://example.invalid/skeleton?tag=v1.0.0#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(
            pin,
            Pin::Tag {
                url: "https://example.invalid/skeleton".to_owned(),
                tag: "v1.0.0".to_owned(),
                commit: oid(COMMIT),
            }
        );
    }

    #[test]
    fn a_branch_source_parses_the_url_branch_and_commit() {
        let source = format!("git+https://example.invalid/skeleton?branch=main#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(
            pin,
            Pin::Branch {
                url: "https://example.invalid/skeleton".to_owned(),
                branch: "main".to_owned(),
                commit: oid(COMMIT),
            }
        );
    }

    #[test]
    fn a_percent_encoded_branch_name_is_decoded() {
        let source = format!("git+https://example.invalid/skeleton?branch=feature%2Fx#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(
            pin,
            Pin::Branch {
                url: "https://example.invalid/skeleton".to_owned(),
                branch: "feature/x".to_owned(),
                commit: oid(COMMIT),
            }
        );
    }

    #[test]
    fn a_rev_source_parses_the_url_rev_and_commit() {
        let source = format!("git+https://example.invalid/skeleton?rev={COMMIT}#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(
            pin,
            Pin::Rev {
                url: "https://example.invalid/skeleton".to_owned(),
                rev: COMMIT.to_owned(),
                commit: oid(COMMIT),
            }
        );
    }

    #[test]
    fn no_query_at_all_is_the_default_branch() {
        let source = format!("git+https://example.invalid/skeleton#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(
            pin,
            Pin::DefaultBranch {
                url: "https://example.invalid/skeleton".to_owned(),
                commit: oid(COMMIT),
            }
        );
    }

    #[test]
    fn an_unknown_query_key_is_unrecognised() {
        let source = format!("git+https://example.invalid/skeleton?ref=v1#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(pin, Pin::Unrecognised { source });
    }

    #[test]
    fn two_query_keys_at_once_is_unrecognised() {
        let source = format!("git+https://example.invalid/skeleton?tag=v1&branch=main#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(pin, Pin::Unrecognised { source });
    }

    #[test]
    fn a_duplicated_query_key_is_unrecognised() {
        let source = format!("git+https://example.invalid/skeleton?tag=v1&tag=v2#{COMMIT}");
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(pin, Pin::Unrecognised { source });
    }

    #[test]
    fn a_commit_that_is_not_forty_hex_digits_is_unrecognised() {
        let source = "git+https://example.invalid/skeleton?tag=v1#short".to_owned();
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(pin, Pin::Unrecognised { source });
    }

    #[test]
    fn a_source_naming_neither_a_registry_nor_git_nor_null_is_unrecognised() {
        let source = "vcs+https://example.invalid/skeleton".to_owned();
        let pin = Pin::from_source(Some(&source), skeleton_directory());
        assert_eq!(pin, Pin::Unrecognised { source });
    }

    proptest! {
        /// Any git source built from a well-formed url, one of the three
        /// query kinds (or none), and a valid 40-hex commit, round-trips
        /// through `from_source` to the matching `Pin` variant carrying the
        /// same three parts back out.
        #[test]
        fn a_well_formed_git_source_round_trips(
            url in "https://[a-z]{3,10}\\.invalid/[a-z]{1,10}",
            commit in "[0-9a-f]{40}",
            choice in 0u8..4,
            name in "[a-zA-Z0-9_/-]{1,12}",
        ) {
            let (query, expected) = match choice {
                0 => (
                    format!("?tag={name}"),
                    Pin::Tag { url: url.clone(), tag: name, commit: oid(&commit) },
                ),
                1 => (
                    format!("?branch={name}"),
                    Pin::Branch { url: url.clone(), branch: name, commit: oid(&commit) },
                ),
                2 => (
                    format!("?rev={name}"),
                    Pin::Rev { url: url.clone(), rev: name, commit: oid(&commit) },
                ),
                _ => (
                    String::new(),
                    Pin::DefaultBranch { url: url.clone(), commit: oid(&commit) },
                ),
            };
            let source = format!("git+{url}{query}#{commit}");
            let pin = Pin::from_source(Some(&source), skeleton_directory());
            prop_assert_eq!(pin, expected);
        }
    }
}
