//! Whether a locked package's `source` is the one a dependency declaration asks for.

use crate::workspace::crates_io_source::CRATES_IO_SOURCE;
use crate::workspace::git_query::{GitQuery, parsed_git_query};
use crate::workspace::without_cargo::canonical_git_url::CanonicalGitUrl;
use crate::workspace::without_cargo::package_manifest::{DeclaredSource, GitReference};

/// Whether `locked`, a package's `source` in `Cargo.lock` (`None` for a path package), is what a
/// declaration from `declared` resolves to.
///
/// A path declaration is locked with no source. A git declaration is locked with its url as
/// Cargo wrote it, which is not always how the manifest spells it, so the two are compared as
/// Cargo canonicalises them, and with its one reference as a decoded query, so
/// `branch=feat%2Fx` is the branch `feat/x`; a declaration naming no reference is locked with no
/// query. A registry declaration with no `registry =` name is locked from crates.io exactly, and
/// one with a name from some registry: this reader does not interpret registry names, so it
/// cannot say which index a name stands for.
pub(super) fn agrees(locked: Option<&str>, declared: &DeclaredSource) -> bool {
    match (declared, locked) {
        (DeclaredSource::Path { .. }, None) => true,
        (DeclaredSource::Path { .. }, Some(_))
        | (DeclaredSource::Git { .. } | DeclaredSource::Registry { .. }, None) => false,
        (DeclaredSource::Git { url, reference }, Some(source)) => {
            git_agrees(source, url, reference)
        }
        (DeclaredSource::Registry { name }, Some(source)) => {
            registry_agrees(source, name.as_deref())
        }
    }
}

/// Whether the lock's git `source` has a query that is not exactly one of `tag`, `branch` and
/// `rev`: a query of the dependency URL's own, which Cargo keeps in the source and writes the
/// reference after (`git+<url>?x=1?branch=dev#<commit>`), or one that names no reference.
///
/// This reader compares no such source, so reading the lockfile refuses it by name, before
/// [`agrees`] could call the lock out of date for a source Cargo reads back as current. A source
/// that is not a git one has no such query.
pub(super) fn has_unread_query(source: &str) -> bool {
    match split_git_source(source) {
        Some((_url, Some(query))) => parsed_git_query(query).is_none(),
        Some((_, None)) | None => false,
    }
}

/// Whether the git `source` `git+<url>[?<query>]#<commit>` is `url` at `reference`, the URLs
/// compared as Cargo canonicalises them. A locked URL that is not one Cargo reads is not `url`,
/// and neither is a source whose query is not exactly one reference, which the lockfile read
/// refuses before it gets here ([`has_unread_query`]).
fn git_agrees(source: &str, url: &CanonicalGitUrl, reference: &GitReference) -> bool {
    let Some((locked_url, query)) = split_git_source(source) else {
        return false;
    };
    CanonicalGitUrl::parse(locked_url).as_ref() == Ok(url)
        && locked_reference(query).as_ref() == Some(reference)
}

/// The URL and the query of the git `source` `git+<url>[?<query>]#<commit>`, cut at the first
/// `#` and then the first `?`; `None` for a source that is not a git one.
fn split_git_source(source: &str) -> Option<(&str, Option<&str>)> {
    let without_kind = source.strip_prefix("git+")?;
    let without_commit = without_kind
        .split_once('#')
        .map_or(without_kind, |(before, _commit)| before);
    Some(match without_commit.split_once('?') {
        Some((url, query)) => (url, Some(query)),
        None => (without_commit, None),
    })
}

/// The reference a git source's `query` names: the remote's default branch when there is none,
/// and nothing when the query is not exactly one of `tag`, `branch` and `rev`, which is the
/// query [`has_unread_query`] reports.
fn locked_reference(query: Option<&str>) -> Option<GitReference> {
    query.map_or(Some(GitReference::DefaultBranch), |query| {
        parsed_git_query(query).map(|decoded| match decoded {
            GitQuery::Tag(tag) => GitReference::Tag(tag),
            GitQuery::Branch(branch) => GitReference::Branch(branch),
            GitQuery::Rev(rev) => GitReference::Rev(rev),
        })
    })
}

/// Whether the registry `source` is the one a declaration naming `registry` asks for.
fn registry_agrees(source: &str, registry: Option<&str>) -> bool {
    match registry {
        None => source == CRATES_IO_SOURCE,
        Some(_) => source.starts_with("registry+") || source.starts_with("sparse+"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{agrees, has_unread_query};
    use crate::workspace::without_cargo::canonical_git_url::CanonicalGitUrl;
    use crate::workspace::without_cargo::package_manifest::{DeclaredSource, GitReference};

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn git(reference: GitReference) -> DeclaredSource {
        git_at("https://example.test/g", reference)
    }

    /// A git declaration of the URL `url` as the manifest writes it.
    fn git_at(url: &str, reference: GitReference) -> DeclaredSource {
        DeclaredSource::Git {
            url: CanonicalGitUrl::parse(url).expect("a URL cargo reads"),
            reference,
        }
    }

    fn path() -> DeclaredSource {
        DeclaredSource::Path {
            directory: PathBuf::from("/w/dep"),
        }
    }

    #[test]
    fn a_path_declaration_is_locked_with_no_source_only() {
        assert!(agrees(None, &path()));
        assert!(!agrees(
            Some("registry+https://example.test/index"),
            &path()
        ));
    }

    #[test]
    fn only_a_git_source_with_a_query_that_is_not_one_reference_has_an_unread_query() {
        // The query is read as one `branch`, `tag` or `rev`; a query of the URL's own before it,
        // an unknown key and an empty query are not, and a source that is not a git one has none.
        for query in ["?branch=main", "?tag=v1", "?rev=3f2a9c1d", ""] {
            let source = format!("git+https://example.test/g{query}#{COMMIT}");
            assert!(!has_unread_query(&source), "{source}");
        }
        for query in ["?x=1", "?x=1?branch=dev", "?ref=v1", "?"] {
            let source = format!("git+https://example.test/g{query}#{COMMIT}");
            assert!(has_unread_query(&source), "{source}");
        }
        assert!(!has_unread_query("registry+https://example.test/index?x=1"));
    }

    #[test]
    fn a_git_or_registry_declaration_is_never_locked_with_no_source() {
        assert!(!agrees(None, &git(GitReference::DefaultBranch)));
        assert!(!agrees(None, &DeclaredSource::Registry { name: None }));
    }

    #[test]
    fn a_git_declaration_with_no_reference_is_locked_with_no_query() {
        let source = format!("git+https://example.test/g#{COMMIT}");

        assert!(agrees(Some(&source), &git(GitReference::DefaultBranch)));
        assert!(!agrees(
            Some(&format!("git+https://example.test/g?branch=main#{COMMIT}")),
            &git(GitReference::DefaultBranch)
        ));
    }

    #[test]
    fn a_changed_branch_disagrees() {
        let source = format!("git+https://example.test/g?branch=main#{COMMIT}");

        assert!(agrees(
            Some(&source),
            &git(GitReference::Branch("main".to_owned()))
        ));
        assert!(!agrees(
            Some(&source),
            &git(GitReference::Branch("dev".to_owned()))
        ));
    }

    #[test]
    fn a_percent_encoded_branch_is_compared_decoded() {
        let source = format!("git+https://example.test/g?branch=feat%2Fx#{COMMIT}");

        assert!(agrees(
            Some(&source),
            &git(GitReference::Branch("feat/x".to_owned()))
        ));
        assert!(!agrees(
            Some(&source),
            &git(GitReference::Branch("feat%2Fx".to_owned()))
        ));
    }

    #[test]
    fn a_branch_is_not_a_tag_or_a_rev_of_the_same_name() {
        let source = format!("git+https://example.test/g?branch=v1#{COMMIT}");

        assert!(!agrees(
            Some(&source),
            &git(GitReference::Tag("v1".to_owned()))
        ));
        assert!(!agrees(
            Some(&source),
            &git(GitReference::Rev("v1".to_owned()))
        ));
    }

    #[test]
    fn a_changed_git_url_disagrees() {
        let source = format!("git+https://example.test/other?tag=v1#{COMMIT}");

        assert!(!agrees(
            Some(&source),
            &git(GitReference::Tag("v1".to_owned()))
        ));
    }

    #[test]
    fn a_git_url_respelled_the_way_cargo_canonicalises_it_agrees() {
        // Cargo writes the lockfile with the first spelling it saw of a source, and reads the
        // other spellings as the same one: the trailing slash, `.git`, GitHub's case and its
        // protocol, a host's case and a default port.
        let source = format!("git+https://github.com/Foo/Bar#{COMMIT}");

        for spelling in [
            "https://github.com/Foo/Bar/",
            "https://github.com/foo/bar",
            "https://github.com/Foo/Bar.git",
            "https://GitHub.com/Foo/Bar",
            "https://github.com:443/foo/bar",
            "git://github.com/foo/bar",
        ] {
            assert!(
                agrees(
                    Some(&source),
                    &git_at(spelling, GitReference::DefaultBranch)
                ),
                "{spelling}"
            );
        }
        assert!(agrees(
            Some(&format!("git+https://example.com/Foo/Bar.git/#{COMMIT}")),
            &git_at(
                "https://Example.COM:443/Foo/Bar",
                GitReference::DefaultBranch
            )
        ));
    }

    #[test]
    fn a_git_url_cargo_keeps_apart_disagrees() {
        // Respellings that name another source to Cargo: the case of a path off GitHub, a
        // `www.` host, an ssh remote with its user name, and an escape the parser keeps.
        let github = format!("git+https://github.com/Foo/Bar#{COMMIT}");
        for spelling in [
            "https://www.github.com/Foo/Bar",
            "ssh://git@github.com/foo/bar",
        ] {
            assert!(
                !agrees(
                    Some(&github),
                    &git_at(spelling, GitReference::DefaultBranch)
                ),
                "{spelling}"
            );
        }
        let elsewhere = format!("git+https://example.com/Foo/Bar#{COMMIT}");
        assert!(!agrees(
            Some(&elsewhere),
            &git_at("https://example.com/foo/bar", GitReference::DefaultBranch)
        ));
        let escaped = format!("git+file:///tmp/re%70o#{COMMIT}");
        assert!(!agrees(
            Some(&escaped),
            &git_at("file:///tmp/repo", GitReference::DefaultBranch)
        ));
    }

    #[test]
    fn a_respelled_git_url_still_has_to_name_the_same_reference() {
        let source = format!("git+https://example.test/g?branch=main#{COMMIT}");

        assert!(agrees(
            Some(&source),
            &git_at(
                "https://example.test/g.git/",
                GitReference::Branch("main".to_owned())
            )
        ));
        assert!(!agrees(
            Some(&source),
            &git_at(
                "https://example.test/g.git/",
                GitReference::Branch("dev".to_owned())
            )
        ));
    }

    #[test]
    fn a_locked_git_url_cargo_cannot_read_disagrees() {
        let source = format!("git+git@github.com:foo/bar.git#{COMMIT}");

        assert!(!agrees(
            Some(&source),
            &git_at("https://github.com/foo/bar", GitReference::DefaultBranch)
        ));
    }

    #[test]
    fn a_git_declaration_locked_from_a_registry_disagrees() {
        assert!(!agrees(
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            &git(GitReference::DefaultBranch)
        ));
    }

    #[test]
    fn a_registry_declaration_with_no_name_is_locked_from_crates_io_exactly() {
        let declared = DeclaredSource::Registry { name: None };

        assert!(agrees(
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            &declared
        ));
        assert!(!agrees(
            Some("registry+https://example.test/index"),
            &declared
        ));
        assert!(!agrees(
            Some(&format!("git+https://example.test/g#{COMMIT}")),
            &declared
        ));
    }

    #[test]
    fn a_registry_declaration_with_a_name_is_locked_from_some_registry() {
        let declared = DeclaredSource::Registry {
            name: Some("mine".to_owned()),
        };

        assert!(agrees(
            Some("registry+https://example.test/index"),
            &declared
        ));
        assert!(agrees(
            Some("sparse+https://example.test/index/"),
            &declared
        ));
        assert!(!agrees(
            Some(&format!("git+https://example.test/g#{COMMIT}")),
            &declared
        ));
    }
}
