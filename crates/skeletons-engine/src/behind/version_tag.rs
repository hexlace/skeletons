//! Which tags on a remote name a skeleton's own releases, and a tag as a
//! version (pure).
//!
//! A repository holding more than one skeleton crate tags each one's own
//! releases as `<skeleton>-v<version>` — cargo-release's own workspace
//! default — rather than tagging the repository as a whole. `release_tags`
//! decides, per skeleton, whether the remote uses that shape for it at all,
//! falling back to the plain `vX.Y.Z`/`X.Y.Z` shape a single-crate
//! repository uses.

/// Which tags on a remote name this skeleton's own releases:
/// [`Prefixed`](Self::Prefixed) when the remote holds at least one tag of
/// the shape `<package>-v<version>` for it, [`Plain`](Self::Plain)
/// otherwise — [`release_tags`] is the one function that decides which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagScheme {
    Prefixed,
    Plain,
}

/// Parses `tag` as `<package>-v<semver>`: the prefix exact and
/// case-sensitive, the rest a full semver, with the one `v` cargo-release
/// writes already consumed by the prefix (so `alpha-vv1.2.3` does not
/// parse, and `alpha-v1.2.3-rc.1` parses as a prerelease).
///
/// `None` when `tag` does not begin with exactly `{package}-v`, or when
/// what follows is not a semver — a release named some other way, or a tag
/// belonging to a different package entirely (`version_of_prefixed_tag("a",
/// "a-b-v1.0.0")` is `None`: the prefix `a-v` is not a prefix of
/// `a-b-v1.0.0`).
pub(crate) fn version_of_prefixed_tag(package: &str, tag: &str) -> Option<semver::Version> {
    let mut prefix = String::with_capacity(package.len() + 2);
    prefix.push_str(package);
    prefix.push_str("-v");
    let candidate = tag.strip_prefix(prefix.as_str())?;
    semver::Version::parse(candidate).ok()
}

/// Parses `tag` as a version, stripping one leading `v` first (`v1.2.3` and
/// `1.2.3` both parse; `vv1.2.3` does not, since only one `v` is stripped).
///
/// `None` for a tag that is not a version at all — a release name, a
/// milestone, anything a repository tags for a reason other than marking a
/// release.
pub(crate) fn version_of_plain_tag(tag: &str) -> Option<semver::Version> {
    let candidate = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(candidate).ok()
}

/// Every tag in `names` that names one of `package`'s own releases, and
/// which shape decided that.
///
/// [`TagScheme::Prefixed`] when at least one tag in `names` parses under
/// [`version_of_prefixed_tag`] for `package` — in which case only those
/// tags are returned, whatever other tags `names` also holds (a sibling
/// skeleton's own prefixed tags, or the repository's own plain tags, name
/// nobody's release this function returns). Otherwise
/// [`TagScheme::Plain`], and every tag in `names` is read under
/// [`version_of_plain_tag`] instead, exactly as a single-crate repository
/// reads its tags.
pub(crate) fn release_tags<'a>(
    package: &str,
    names: &'a [String],
) -> (TagScheme, Vec<(&'a str, semver::Version)>) {
    let prefixed: Vec<(&str, semver::Version)> = names
        .iter()
        .filter_map(|name| {
            version_of_prefixed_tag(package, name).map(|version| (name.as_str(), version))
        })
        .collect();
    if prefixed.is_empty() {
        let plain = names
            .iter()
            .filter_map(|name| version_of_plain_tag(name).map(|version| (name.as_str(), version)))
            .collect();
        (TagScheme::Plain, plain)
    } else {
        (TagScheme::Prefixed, prefixed)
    }
}

/// The locked tag's own version, read under whichever shape it itself
/// parses as: the prefixed shape first (so a locked tag like
/// `alpha-v0.1.0` reads as `alpha`'s own `0.1.0`, not rejected for not
/// being a bare version), then the plain shape.
///
/// `None` when `locked_tag` parses as neither — the `tag-not-a-version`
/// undetermined reason.
pub(crate) fn locked_tag_version(package: &str, locked_tag: &str) -> Option<semver::Version> {
    version_of_prefixed_tag(package, locked_tag).or_else(|| version_of_plain_tag(locked_tag))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        TagScheme, locked_tag_version, release_tags, version_of_plain_tag, version_of_prefixed_tag,
    };

    #[test]
    fn a_bare_version_parses() {
        assert_eq!(
            version_of_plain_tag("1.2.3"),
            Some(semver::Version::new(1, 2, 3))
        );
    }

    #[test]
    fn a_v_prefixed_version_parses_with_the_v_stripped() {
        assert_eq!(
            version_of_plain_tag("v1.2.3"),
            Some(semver::Version::new(1, 2, 3))
        );
    }

    #[test]
    fn only_one_leading_v_is_stripped() {
        assert_eq!(version_of_plain_tag("vv1.2.3"), None);
    }

    #[test]
    fn a_non_version_tag_is_none() {
        assert_eq!(version_of_plain_tag("release-candidate"), None);
        assert_eq!(version_of_plain_tag(""), None);
        assert_eq!(version_of_plain_tag("v"), None);
    }

    #[test]
    fn a_prerelease_tag_still_parses_as_a_version() {
        let version = version_of_plain_tag("v1.2.3-rc.1").expect("a prerelease is still a version");
        assert_eq!(version.to_string(), "1.2.3-rc.1");
    }

    #[test]
    fn a_prefixed_tag_parses_with_the_prefix_and_one_v_stripped() {
        assert_eq!(
            version_of_prefixed_tag("alpha", "alpha-v0.2.0"),
            Some(semver::Version::new(0, 2, 0))
        );
    }

    #[test]
    fn a_prefixed_tag_needs_exactly_one_v_after_the_dash() {
        assert_eq!(version_of_prefixed_tag("alpha", "alpha-vv0.2.0"), None);
        assert_eq!(version_of_prefixed_tag("alpha", "alpha-0.2.0"), None);
    }

    #[test]
    fn a_prefixed_tag_naming_a_different_package_does_not_parse_as_this_ones() {
        // `a`'s own prefix is `a-v`, which is not a prefix of `a-b-v1.0.0`:
        // a naive `starts_with(package)` would wrongly match here, since
        // "a-b-v1.0.0" does start with "a". The exact `{package}-v` prefix
        // must not.
        assert_eq!(version_of_prefixed_tag("a", "a-b-v1.0.0"), None);
    }

    #[test]
    fn release_tags_on_a_single_crate_remote_with_plain_tags_only_reads_plain() {
        let names = vec![
            "v0.1.0".to_owned(),
            "v0.2.0".to_owned(),
            "release-candidate".to_owned(),
        ];
        let (scheme, tags) = release_tags("alpha", &names);
        assert_eq!(scheme, TagScheme::Plain);
        assert_eq!(
            tags,
            vec![
                ("v0.1.0", semver::Version::new(0, 1, 0)),
                ("v0.2.0", semver::Version::new(0, 2, 0)),
            ]
        );
    }

    #[test]
    fn release_tags_on_a_single_crate_remote_with_prefixed_tags_only_reads_prefixed() {
        let names = vec!["alpha-v0.1.0".to_owned(), "alpha-v0.2.0".to_owned()];
        let (scheme, tags) = release_tags("alpha", &names);
        assert_eq!(scheme, TagScheme::Prefixed);
        assert_eq!(
            tags,
            vec![
                ("alpha-v0.1.0", semver::Version::new(0, 1, 0)),
                ("alpha-v0.2.0", semver::Version::new(0, 2, 0)),
            ]
        );
    }

    #[test]
    fn release_tags_on_a_many_crate_remote_reads_only_this_packages_own_prefixed_tags() {
        let names = vec![
            "alpha-v0.1.0".to_owned(),
            "alpha-v0.2.0".to_owned(),
            "beta-v0.9.0".to_owned(),
            "v1.0.0".to_owned(),
        ];

        let (alpha_scheme, alpha_tags) = release_tags("alpha", &names);
        assert_eq!(alpha_scheme, TagScheme::Prefixed);
        assert_eq!(
            alpha_tags,
            vec![
                ("alpha-v0.1.0", semver::Version::new(0, 1, 0)),
                ("alpha-v0.2.0", semver::Version::new(0, 2, 0)),
            ]
        );

        let (beta_scheme, beta_tags) = release_tags("beta", &names);
        assert_eq!(beta_scheme, TagScheme::Prefixed);
        assert_eq!(
            beta_tags,
            vec![("beta-v0.9.0", semver::Version::new(0, 9, 0))]
        );
    }

    #[test]
    fn a_remote_with_only_a_non_version_prefixed_tag_reads_plain_not_prefixed() {
        // `alpha-vnot-a-version` looks like the prefixed shape but does not
        // parse as one, so it must not put this skeleton in the prefixed
        // scheme with zero tags — it falls back to reading the same names
        // as plain tags (none of which parse either, here).
        let names = vec!["alpha-vnot-a-version".to_owned()];
        let (scheme, tags) = release_tags("alpha", &names);
        assert_eq!(scheme, TagScheme::Plain);
        assert!(tags.is_empty(), "tags were: {tags:?}");
    }

    #[test]
    fn locked_plain_tag_compares_against_a_remote_that_uses_the_prefixed_scheme() {
        // The remote has since moved to per-skeleton tags, but the commit
        // this wearer locked was tagged before that, under the plain
        // shape. The locked tag's own version must still read, under the
        // fallback inside `locked_tag_version` itself.
        let locked = locked_tag_version("alpha", "v0.1.0").expect("v0.1.0 parses as a plain tag");
        assert_eq!(locked, semver::Version::new(0, 1, 0));

        let names = vec!["alpha-v0.2.0".to_owned()];
        let (scheme, tags) = release_tags("alpha", &names);
        assert_eq!(scheme, TagScheme::Prefixed);
        assert_eq!(tags, vec![("alpha-v0.2.0", semver::Version::new(0, 2, 0))]);
        assert_eq!(
            tags[0].1.cmp_precedence(&locked),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn locked_tag_version_reads_a_prefixed_tag_that_is_not_a_bare_version() {
        // `alpha-v0.1.0` is not a valid plain-tag reading (`strip_prefix('v')`
        // leaves it unchanged, and that does not parse as semver), so only
        // the prefixed reading can produce this answer.
        assert_eq!(
            locked_tag_version("alpha", "alpha-v0.1.0"),
            Some(semver::Version::new(0, 1, 0))
        );
    }

    #[test]
    fn locked_tag_version_is_none_for_a_tag_that_parses_as_neither_shape() {
        assert_eq!(locked_tag_version("alpha", "release-candidate"), None);
    }

    #[test]
    fn a_prefixed_prerelease_tag_still_parses_as_a_version() {
        let names = vec!["alpha-v1.0.0-rc.1".to_owned()];
        let (scheme, tags) = release_tags("alpha", &names);
        assert_eq!(scheme, TagScheme::Prefixed);
        assert_eq!(tags[0].1.to_string(), "1.0.0-rc.1");
    }

    proptest! {
        /// However `version_of_plain_tag` is fed, it never panics: the
        /// whole point of a tag from a remote is that it is untrusted
        /// text.
        #[test]
        fn version_of_plain_tag_never_panics_on_arbitrary_input(tag in ".*") {
            let _outcome = version_of_plain_tag(&tag);
        }

        /// However `version_of_prefixed_tag` is fed, for any package name,
        /// it never panics.
        #[test]
        fn version_of_prefixed_tag_never_panics_on_arbitrary_input(
            package in ".*",
            tag in ".*",
        ) {
            let _outcome = version_of_prefixed_tag(&package, &tag);
        }

        /// However `release_tags` is fed, it never panics, whatever names a
        /// remote reports.
        #[test]
        fn release_tags_never_panics_on_arbitrary_input(
            package in ".*",
            names in proptest::collection::vec(".*", 0..8),
        ) {
            let _outcome = release_tags(&package, &names);
        }

        /// Every real version, optionally `v`-prefixed, round-trips through
        /// the plain shape.
        #[test]
        fn a_well_formed_version_round_trips_through_the_plain_shape(
            major in 0u64..1000,
            minor in 0u64..1000,
            patch in 0u64..1000,
            prefixed in any::<bool>(),
        ) {
            let plain = format!("{major}.{minor}.{patch}");
            let tag = if prefixed { format!("v{plain}") } else { plain };
            let parsed = version_of_plain_tag(&tag).expect("a well-formed version must parse");
            prop_assert_eq!(parsed, semver::Version::new(major, minor, patch));
        }

        /// Every real version, prefixed with a package's own name, round-trips
        /// through the prefixed shape.
        #[test]
        fn a_well_formed_version_round_trips_through_the_prefixed_shape(
            package in "[a-z][a-z0-9-]{0,10}",
            major in 0u64..1000,
            minor in 0u64..1000,
            patch in 0u64..1000,
        ) {
            let tag = format!("{package}-v{major}.{minor}.{patch}");
            let parsed = version_of_prefixed_tag(&package, &tag)
                .expect("a well-formed prefixed version must parse");
            prop_assert_eq!(parsed, semver::Version::new(major, minor, patch));
        }
    }
}
