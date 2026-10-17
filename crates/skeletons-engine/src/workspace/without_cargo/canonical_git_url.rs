//! A git dependency's URL as Cargo compares it.
//!
//! Cargo calls two git sources one when their URLs are equal after `CanonicalUrl::new`, so
//! `https://github.com/Foo/Bar.git/` and `https://github.com/foo/bar` are one source, and the
//! lockfile holds only the spelling Cargo met first. This is that function, applied to the
//! URL parser's own output: by the time the rules run the parser has lowercased the scheme and
//! the host, mapped a Unicode host to Punycode, dropped a default port, resolved dot segments
//! and escaped the path, and the rules read what is left. Cargo's function is the same in 1.85
//! and 1.95.

use std::fmt;

use url::{ParseError, Position, Url};

/// A git URL after Cargo's canonicalisation, which is how Cargo tells whether two spellings are
/// the one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalGitUrl(Url);

/// Why a text is not a URL Cargo reads as a git source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotACargoGitUrl {
    /// The URL parser refuses the text, such as `git@github.com:foo/bar.git`.
    Unparseable(ParseError),
    /// The text parses but is not a URL that has a path to take segments from, such as
    /// `github.com:foo/bar`, which parses with the scheme `github.com`.
    CannotBeABase,
}

impl CanonicalGitUrl {
    /// Parses `text` as the URL parser does and canonicalises it as Cargo does: a trailing `/`
    /// is dropped, a GitHub URL is read as `https` with its path in lower case, and a `.git`
    /// suffix is dropped, once.
    ///
    /// # Errors
    ///
    /// [`NotACargoGitUrl`] for a text the parser refuses, which Cargo refuses the manifest
    /// for, and for a URL that cannot be a base, which Cargo refuses at the same place.
    pub(crate) fn parse(text: &str) -> Result<Self, NotACargoGitUrl> {
        let mut url = Url::parse(text).map_err(NotACargoGitUrl::Unparseable)?;
        if url.cannot_be_a_base() {
            return Err(NotACargoGitUrl::CannotBeABase);
        }
        without_trailing_slash(&mut url);
        let mut url = github_lowercased(url)?;
        without_dot_git(&mut url);
        Ok(Self(url))
    }

    /// The canonical URL as text, which tests state their expectations in.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Drops the one empty segment that a trailing `/` ends `url`'s path with. A root path `/` stays,
/// and `//` becomes `/`.
fn without_trailing_slash(url: &mut Url) {
    assert!(
        !url.cannot_be_a_base(),
        "only a URL that can be a base has path segments to take one from"
    );
    if url.path().ends_with('/') {
        let Ok(mut segments) = url.path_segments_mut() else {
            unreachable!("a URL that can be a base has path segments");
        };
        segments.pop_if_empty();
    }
}

/// `url` as Cargo reads a GitHub URL, which GitHub treats as case-insensitive: when its host is
/// `github.com`, the same URL over `https` with its path in lower case.
///
/// The host is compared as the parser left it, so `ssh://git@GitHub.com/…`, whose host is opaque
/// to the parser and keeps its case, `www.github.com` and `github.com.` are not GitHub URLs.
/// The user name and a port that is not the default survive the move to `https`.
fn github_lowercased(url: Url) -> Result<Url, NotACargoGitUrl> {
    if url.host_str() != Some("github.com") {
        return Ok(url);
    }
    // Cargo unwraps this parse. A text it cannot read is an error here, because this reader
    // never panics on a manifest.
    let mut https = Url::parse(&format!("https{}", &url[Position::AfterScheme..]))
        .map_err(NotACargoGitUrl::Unparseable)?;
    assert!(
        !https.cannot_be_a_base(),
        "an `https` URL with a host can be a base"
    );
    let path = https.path().to_lowercase();
    https.set_path(&path);
    Ok(https)
}

/// Drops a `.git` that `url`'s path ends with, from the last segment, once: `a.git.git` is
/// `a.git`.
fn without_dot_git(url: &mut Url) {
    assert!(
        !url.cannot_be_a_base(),
        "only a URL that can be a base has path segments to take one from"
    );
    // A path ends with `.git` exactly when its last segment does.
    let Some(last) = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|segment| segment.strip_suffix(".git"))
        .map(str::to_owned)
    else {
        return;
    };
    let Ok(mut segments) = url.path_segments_mut() else {
        unreachable!("a URL that can be a base has path segments");
    };
    segments.pop().push(&last);
}

impl fmt::Display for NotACargoGitUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparseable(error) => error.fmt(formatter),
            Self::CannotBeABase => formatter.write_str("cannot-be-a-base-URLs are not supported"),
        }
    }
}

impl std::error::Error for NotACargoGitUrl {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unparseable(error) => Some(error),
            Self::CannotBeABase => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{CanonicalGitUrl, NotACargoGitUrl};

    /// The canonical form of `text`, which every test here gives a URL the parser accepts.
    fn canonical(text: &str) -> CanonicalGitUrl {
        CanonicalGitUrl::parse(text).expect("a URL cargo reads")
    }

    /// Requires `left` and `right` to be one source to Cargo.
    fn assert_one_source(left: &str, right: &str) {
        assert_eq!(canonical(left), canonical(right), "{left} against {right}");
    }

    /// Requires `left` and `right` to be two sources to Cargo.
    fn assert_two_sources(left: &str, right: &str) {
        assert_ne!(canonical(left), canonical(right), "{left} against {right}");
    }

    #[test]
    fn a_trailing_slash_is_not_part_of_the_source() {
        // Cargo's `test_canonicalize_idents_by_stripping_trailing_url_slash`.
        assert_one_source(
            "https://github.com/PistonDevelopers/piston/",
            "https://github.com/PistonDevelopers/piston",
        );
    }

    #[test]
    fn a_github_url_is_compared_in_lower_case() {
        // Cargo's `test_canonicalize_idents_by_lowercasing_github_urls`.
        assert_one_source(
            "https://github.com/PistonDevelopers/piston",
            "https://github.com/pistondevelopers/piston",
        );
    }

    #[test]
    fn a_dot_git_suffix_is_not_part_of_the_source() {
        // Cargo's `test_canonicalize_idents_by_stripping_dot_git`.
        assert_one_source(
            "https://github.com/PistonDevelopers/piston",
            "https://github.com/PistonDevelopers/piston.git",
        );
    }

    #[test]
    fn a_github_url_is_the_same_source_over_any_protocol() {
        // Cargo's `test_canonicalize_idents_different_protocols` and `github_sources_equal`.
        assert_one_source(
            "https://github.com/PistonDevelopers/piston",
            "git://github.com/PistonDevelopers/piston",
        );
        assert_one_source("https://github.com/foo/bar", "git://github.com/foo/bar");
    }

    #[test]
    fn the_parser_decides_a_github_host_before_the_rules_run() {
        // The spellings of `https://github.com/foo/bar` that the URL parser normalises to it,
        // each measured to be current against a lockfile for it on Cargo 1.85 and 1.95.
        for spelling in [
            "https://GitHub.com/Foo/Bar",
            "https://github.com:443/foo/bar",
            "http://github.com/Foo/Bar",
            "HTTPS://github.com/foo/bar",
            "https://github.com/foo/bar.GIT",
            "https://github.com/foo/bar.git/",
        ] {
            assert_one_source(spelling, "https://github.com/foo/bar");
        }
    }

    #[test]
    fn a_url_that_is_not_github_is_not_lower_cased() {
        assert_two_sources("https://example.com/Foo/Bar", "https://example.com/foo/bar");
        // `ssh://` and `git://` hosts are opaque to the parser, so their case is kept and
        // `GitHub.com` is not `github.com` to the rules.
        assert_two_sources(
            "ssh://git@GitHub.com/Foo/Bar",
            "ssh://git@github.com/foo/bar",
        );
    }

    #[test]
    fn a_host_that_only_looks_like_github_is_another_source() {
        assert_two_sources(
            "https://www.github.com/Foo/Bar",
            "https://github.com/foo/bar",
        );
        assert_two_sources("https://github.com./foo/bar", "https://github.com/foo/bar");
        // The re-parse as `https` keeps the user name, so the ssh remote is not the web one.
        assert_two_sources("ssh://git@github.com/foo/bar", "https://github.com/foo/bar");
    }

    #[test]
    fn the_parser_makes_a_host_lower_case_and_drops_a_default_port() {
        assert_one_source("https://Example.COM/a", "https://example.com/a");
        assert_one_source("https://example.com:443/a", "https://example.com/a");
        assert_one_source("http://example.com:80/a", "http://example.com/a");
        assert_two_sources("https://example.com:8443/a", "https://example.com/a");
    }

    #[test]
    fn the_parser_maps_a_unicode_host_to_punycode() {
        assert_one_source(
            "https://BÜCHER.example/a",
            "https://xn--bcher-kva.example/a",
        );
    }

    #[test]
    fn the_parser_resolves_dot_segments_and_escapes_a_path() {
        assert_one_source("https://example.com/a/./b/../c", "https://example.com/a/c");
        assert_one_source(
            "https://example.com/my repo",
            "https://example.com/my%20repo",
        );
    }

    #[test]
    fn a_file_url_naming_localhost_is_the_url_without_a_host() {
        assert_one_source("file://localhost/tmp/repo", "file:///tmp/repo");
        assert_one_source("file:/tmp/repo", "file:///tmp/repo");
        assert_two_sources("file://host/tmp/repo", "file:///tmp/repo");
    }

    #[test]
    fn a_file_url_loses_a_trailing_slash_and_a_dot_git_but_keeps_its_case() {
        assert_one_source("file:///tmp/repo/", "file:///tmp/repo");
        assert_one_source("file:///tmp/repo.git", "file:///tmp/repo");
        assert_two_sources("file:///tmp/Repo", "file:///tmp/repo");
    }

    #[test]
    fn an_escape_is_not_decoded() {
        // Over-merging would call two URLs one source that Cargo keeps apart, and the lockfile
        // would then be called current when `cargo` asks for it to be rewritten.
        assert_two_sources("file:///tmp/re%70o", "file:///tmp/repo");
        assert_two_sources("https://example.com/a%2fb", "https://example.com/a%2Fb");
    }

    #[test]
    fn dot_git_is_stripped_once_and_after_lower_casing() {
        assert_eq!(
            canonical("https://example.com/a.git.git").as_str(),
            "https://example.com/a.git"
        );
        assert_eq!(
            canonical("https://github.com/Foo/Bar.GIT").as_str(),
            "https://github.com/foo/bar"
        );
        assert_eq!(
            canonical("https://example.com/.git").as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn only_one_trailing_empty_segment_is_removed() {
        assert_eq!(
            canonical("https://example.com/a//").as_str(),
            "https://example.com/a/"
        );
        assert_eq!(
            canonical("https://example.com/").as_str(),
            "https://example.com/"
        );
        assert_eq!(
            canonical("https://example.com").as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn a_github_path_is_lower_cased_after_it_is_escaped() {
        // The escape's hex digits are lower-cased with the rest of the path, and a character
        // outside ASCII is escaped before the case is changed, so its case is not folded.
        assert_eq!(
            canonical("https://github.com/foo/BÉR").as_str(),
            "https://github.com/foo/b%c3%89r"
        );
    }

    #[test]
    fn a_scp_style_address_is_not_a_url() {
        let Err(error) = CanonicalGitUrl::parse("git@github.com:foo/bar.git") else {
            panic!("an scp-style address is refused");
        };

        assert!(
            matches!(error, NotACargoGitUrl::Unparseable(_)),
            "{error:?}"
        );
    }

    #[test]
    fn a_host_and_path_with_no_scheme_is_a_url_that_cannot_be_a_base() {
        let Err(error) = CanonicalGitUrl::parse("github.com:foo/bar") else {
            panic!("a text with no authority is refused");
        };

        assert_eq!(error, NotACargoGitUrl::CannotBeABase);
        assert_eq!(error.to_string(), "cannot-be-a-base-URLs are not supported");
    }

    /// A git URL text that the parser accepts, over the parts the rules read: the scheme, the
    /// host's case, a port, a user name, the path's segments and case, `.git` and a trailing
    /// slash.
    fn git_url_text() -> impl Strategy<Value = String> {
        (
            prop::sample::select(vec!["https", "http", "git", "ssh"]),
            prop::sample::select(vec![
                "github.com",
                "GitHub.com",
                "GITHUB.COM",
                "www.github.com",
                "example.com",
                "Example.COM",
            ]),
            prop::option::of(prop::sample::select(vec!["22", "80", "443", "8443"])),
            prop::option::of(Just("git@")),
            prop::collection::vec(
                prop::sample::select(vec!["foo", "Foo", "bar", "Bar", "a b", "%2F", ""]),
                0..4,
            ),
            prop::sample::select(vec!["", ".git", ".GIT", ".git.git"]),
            prop::sample::select(vec!["", "/"]),
        )
            .prop_map(|(scheme, host, port, user, segments, suffix, slash)| {
                format!(
                    "{scheme}://{}{host}{}/{}{suffix}{slash}",
                    user.unwrap_or(""),
                    port.map(|port| format!(":{port}")).unwrap_or_default(),
                    segments.join("/")
                )
            })
    }

    proptest! {
        /// Writes a generated git URL as the parser serialises it, which is the text Cargo puts
        /// in a lockfile, and canonicalises that text: it must be the canonical form of the URL
        /// as it was written. A lockfile is read back and compared on the next run, so this is
        /// what keeps it current when nothing has changed.
        ///
        /// It is the parser's serialisation that the lockfile holds and not the canonical form,
        /// and the two differ: the canonical form of the canonical form of `…/a.git.git` is
        /// `…/a`, because Cargo removes a `.git` once per canonicalisation.
        #[test]
        fn a_lockfile_holds_a_url_that_canonicalises_as_the_manifest_spelling_does(
            text in git_url_text(),
        ) {
            let written = canonical(&text);
            let serialised = url::Url::parse(&text).expect("a URL the parser reads");

            let locked = CanonicalGitUrl::parse(serialised.as_str());

            prop_assert_eq!(locked, Ok(written));
        }
    }
}
