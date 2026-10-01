//! Reading a crates.io sparse-index file (pure): deciding whether a
//! non-yanked, non-prerelease release newer than the one locked exists.
//!
//! One JSON object per line
//! (<https://doc.rust-lang.org/cargo/reference/registry-index.html>), each
//! carrying at least `vers` and `yanked`; every other field (`name`, `deps`,
//! `cksum`, `features`, `rust_version`, `pubtime`, …) is read by cargo, not
//! by this crate, and `serde`'s default of ignoring unknown fields is what
//! lets a future index format add more of them without breaking this parser.

use semver::Version;
use serde::Deserialize;

/// One line of a sparse-index file, read only as far as `behind` needs.
#[derive(Deserialize)]
struct IndexEntry {
    vers: String,
    #[serde(default)]
    yanked: bool,
}

/// Why a sparse-index response's body could not be read as index lines at
/// all — never a reason to guess: an unparsable line could be hiding a
/// newer release, and reporting `current` on a guess would be exactly the
/// silent "not behind" a network failure must never read as.
#[derive(Debug)]
pub(crate) struct IndexParseFailure {
    pub(crate) detail: String,
}

/// The newest non-yanked, non-prerelease version in `body` whose precedence
/// is greater than `locked`'s — or `None` when there is none, meaning
/// `locked` is already current.
///
/// Precedence (`Version::cmp_precedence`) ignores build metadata, so
/// `1.1.5+spec-1.1.0` and `1.1.6+spec-1.1.0` compare by `1.1.5` and `1.1.6`
/// alone: a version's build metadata containing its own `-` is never
/// mistaken for a prerelease marker, since prerelease status is read from
/// the parsed `Version::pre`, never from the raw string.
///
/// # Errors
///
/// Returns [`IndexParseFailure`] when `body` is not UTF-8, a line is not the
/// expected JSON shape, or a `vers` value does not parse as a version —
/// each one left for the caller to report as `undetermined`, never silently
/// skipped.
pub(crate) fn newest_release_above(
    locked: &Version,
    body: &[u8],
) -> Result<Option<Version>, IndexParseFailure> {
    let text = std::str::from_utf8(body).map_err(|error| IndexParseFailure {
        detail: format!("the index response was not UTF-8: {error}"),
    })?;

    let mut newest: Option<Version> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: IndexEntry = serde_json::from_str(line).map_err(|error| IndexParseFailure {
            detail: format!("a line of the index response could not be read: {error}"),
        })?;
        if entry.yanked {
            continue;
        }
        let version = Version::parse(&entry.vers).map_err(|error| IndexParseFailure {
            detail: format!("`{}` is not a valid version: {error}", entry.vers),
        })?;
        // A prerelease version never counts as newer, the same reason a
        // prerelease tag does not count for a `tag =` pin
        // (`behind.rs::finalize_tag`): a prerelease is not a release a
        // wearer is behind on.
        if !version.pre.is_empty() {
            continue;
        }
        if version.cmp_precedence(locked) != std::cmp::Ordering::Greater {
            continue;
        }
        let is_newer = newest
            .as_ref()
            .is_none_or(|current| version.cmp_precedence(current) == std::cmp::Ordering::Greater);
        if is_newer {
            newest = Some(version);
        }
    }
    Ok(newest)
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use super::newest_release_above;

    fn line(vers: &str, yanked: bool) -> String {
        format!(
            concat!(
                r#"{{"name":"thing","vers":"{vers}","deps":[],"cksum":"a","#,
                r#""features":{{}},"yanked":{yanked}}}"#
            ),
            vers = vers,
            yanked = yanked
        )
    }

    #[test]
    fn a_newer_non_yanked_release_is_reported() {
        let body = format!("{}\n{}\n", line("1.0.0", false), line("1.1.0", false));
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("a well-formed index parses");
        assert_eq!(newest, Some(Version::new(1, 1, 0)));
    }

    #[test]
    fn no_release_above_locked_is_current() {
        let body = format!("{}\n", line("1.0.0", false));
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("a well-formed index parses");
        assert_eq!(newest, None);
    }

    #[test]
    fn a_yanked_newer_release_does_not_count() {
        let body = format!("{}\n{}\n", line("1.0.0", false), line("1.1.0", true));
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("a well-formed index parses");
        assert_eq!(newest, None);
    }

    #[test]
    fn a_prerelease_newer_release_does_not_count() {
        let body = format!("{}\n{}\n", line("1.0.0", false), line("1.1.0-rc.1", false));
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("a well-formed index parses");
        assert_eq!(newest, None);
    }

    #[test]
    fn the_greatest_of_several_newer_releases_is_returned() {
        let body = format!(
            "{}\n{}\n{}\n",
            line("1.0.0", false),
            line("1.2.0", false),
            line("1.1.0", false),
        );
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("a well-formed index parses");
        assert_eq!(newest, Some(Version::new(1, 2, 0)));
    }

    #[test]
    fn build_metadata_containing_a_hyphen_is_not_mistaken_for_a_prerelease() {
        let body = format!(
            "{}\n{}\n",
            line("1.1.5+spec-1.1.0", false),
            line("1.1.6+spec-1.1.0", false),
        );
        let locked = Version::parse("1.1.5+spec-1.1.0").expect("valid");
        let newest = newest_release_above(&locked, body.as_bytes())
            .expect("a well-formed index parses")
            .expect("1.1.6 is newer than 1.1.5");
        assert_eq!(newest.to_string(), "1.1.6+spec-1.1.0");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let body = format!("\n{}\n\n", line("1.0.0", false));
        let newest = newest_release_above(&Version::new(0, 9, 0), body.as_bytes())
            .expect("blank lines must not be treated as entries");
        assert_eq!(newest, Some(Version::new(1, 0, 0)));
    }

    #[test]
    fn an_unparsable_line_is_reported_rather_than_skipped() {
        let body = "not json at all\n".to_owned();
        let error = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect_err("a malformed line must never be silently skipped");
        assert!(error.detail.contains("could not be read"));
    }

    #[test]
    fn a_missing_default_yanked_field_reads_as_not_yanked() {
        let body = r#"{"name":"thing","vers":"1.1.0"}"#.to_owned() + "\n";
        let newest = newest_release_above(&Version::new(1, 0, 0), body.as_bytes())
            .expect("yanked defaults to false");
        assert_eq!(newest, Some(Version::new(1, 1, 0)));
    }

    /// The real, committed crates.io sparse-index captures — see
    /// `captures/readme.md` for how and when each was captured, and what it
    /// proves.
    #[cfg(skeletons_checkout)]
    const SEMVER_CAPTURE: &[u8] = include_bytes!("captures/se/mv/semver");
    #[cfg(skeletons_checkout)]
    const RUSTLS_CAPTURE: &[u8] = include_bytes!("captures/ru/st/rustls");
    #[cfg(skeletons_checkout)]
    const TOML_CAPTURE: &[u8] = include_bytes!("captures/to/ml/toml");

    #[cfg(skeletons_checkout)]
    #[test]
    fn the_real_semver_capture_reports_behind_at_1_0_7() {
        let newest = newest_release_above(&Version::new(1, 0, 7), SEMVER_CAPTURE)
            .expect("the captured index parses")
            .expect("a newer release exists above 1.0.7");
        assert_eq!(newest, Version::new(1, 0, 28));
    }

    /// The lines of the real, committed `semver` capture up to and
    /// including the yanked `1.0.8` (line 46): locked at `1.0.7`, the
    /// answer must be current, because `1.0.8` is yanked and every
    /// prerelease below it is irrelevant. Sliced from the real capture, never
    /// hand-typed: no captured crate's newest entry is yanked, so cutting a
    /// real one off at its yanked release is the nearest a real index gets to
    /// that case.
    #[cfg(skeletons_checkout)]
    #[test]
    fn semver_capture_lines_up_to_the_yanked_version_read_current() {
        let text = std::str::from_utf8(SEMVER_CAPTURE).expect("the capture is UTF-8");
        let Some(yanked_line_index) = text
            .lines()
            .position(|line| line.contains(r#""vers":"1.0.8""#))
        else {
            panic!("the semver capture must still list the yanked 1.0.8 release");
        };
        let mut truncated = String::new();
        for line in text.lines().take(yanked_line_index + 1) {
            truncated.push_str(line);
            truncated.push('\n');
        }

        let newest = newest_release_above(&Version::new(1, 0, 7), truncated.as_bytes())
            .expect("well-formed lines parse");
        assert_eq!(
            newest, None,
            "1.0.8 is yanked and everything else is at or below 1.0.7"
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn the_real_rustls_capture_reports_current_at_0_23_45_despite_newer_prereleases() {
        let newest = newest_release_above(&Version::new(0, 23, 45), RUSTLS_CAPTURE)
            .expect("the captured index parses");
        assert_eq!(
            newest, None,
            "0.24.0-dev.0 and 0.24.0-dev.1 are prereleases and must not count as newer"
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn the_real_toml_capture_reports_behind_with_hyphenated_build_metadata() {
        let locked = Version::parse("1.1.5+spec-1.1.0").expect("valid");
        let newest = newest_release_above(&locked, TOML_CAPTURE)
            .expect("the captured index parses")
            .expect("1.1.6 is newer than 1.1.5");
        assert_eq!(newest.to_string(), "1.1.6+spec-1.1.0");
    }
}
