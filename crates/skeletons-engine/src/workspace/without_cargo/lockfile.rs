//! `Cargo.lock`, read as text: its packages, and which package each dependency entry names.

use serde::Deserialize;

use super::text_error::TextError;

/// The lockfile format versions this reader follows. Cargo writes `3` from 1.53 and `4` from 1.78;
/// `1` and `2` are older, and a lockfile that writes no version at all is one of those.
const SUPPORTED_FORMAT_VERSIONS: [i64; 2] = [3, 4];

/// A `Cargo.lock`, with every dependency entry resolved to the package it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lockfile {
    entries: Vec<LockEntry>,
}

/// One `[[package]]` of a `Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockEntry {
    name: String,
    version: String,
    source: Option<String>,
    dependency_indexes: Vec<usize>,
}

#[derive(Deserialize)]
struct RawFormat {
    version: Option<i64>,
}

#[derive(Deserialize)]
struct RawLockfile {
    #[serde(default)]
    package: Vec<RawPackage>,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// One dependency entry, split into the parts it spells: `name`, `name version` or
/// `name version (source)`.
struct DependencyEntry<'text> {
    name: &'text str,
    version: Option<&'text str>,
    source: Option<&'text str>,
}

impl Lockfile {
    /// Reads a lockfile's text, refusing a format other than 3 or 4 and any dependency entry that
    /// does not name exactly one package.
    ///
    /// # Errors
    ///
    /// Unsupported for a format version other than 3 or 4, or none. Unreadable for text that is
    /// not a lockfile, and for a dependency entry that names no package or several.
    pub(crate) fn parse(text: &str) -> Result<Self, TextError> {
        let format: RawFormat = toml::from_str(text).map_err(|error| {
            TextError::unreadable(format!("Cargo.lock is not valid TOML: {error}"))
        })?;
        match format.version {
            Some(version) if SUPPORTED_FORMAT_VERSIONS.contains(&version) => {}
            Some(version) => {
                return Err(TextError::unsupported(format!(
                    "Cargo.lock has format version {version}, and only versions 3 and 4 are read"
                )));
            }
            None => {
                return Err(TextError::unsupported(
                    "Cargo.lock has no format version, which is an older format than versions 3 \
                     and 4, the only ones read",
                ));
            }
        }

        let raw: RawLockfile = toml::from_str(text).map_err(|error| {
            TextError::unreadable(format!("Cargo.lock is not a lockfile: {error}"))
        })?;
        let entries = raw
            .package
            .iter()
            .map(|package| resolved_entry(package, &raw.package))
            .collect::<Result<Vec<_>, _>>()?;

        let entry_count = entries.len();
        for entry in &entries {
            for &index in &entry.dependency_indexes {
                assert!(
                    index < entry_count,
                    "resolution only ever yields the position of an entry that exists"
                );
            }
        }
        Ok(Self { entries })
    }

    /// Every package, in the order the lockfile lists them.
    #[must_use]
    pub(crate) fn entries(&self) -> &[LockEntry] {
        &self.entries
    }
}

impl LockEntry {
    /// The package's name.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The package's version, as the lockfile writes it.
    #[must_use]
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// The package's source, or `None` for a path package.
    #[must_use]
    pub(crate) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// The positions, in [`Lockfile::entries`], of the packages this one depends on, in the order
    /// the lockfile lists them.
    #[must_use]
    pub(crate) fn dependency_indexes(&self) -> &[usize] {
        &self.dependency_indexes
    }
}

/// `package` with each of its dependency entries resolved among `packages`.
fn resolved_entry(package: &RawPackage, packages: &[RawPackage]) -> Result<LockEntry, TextError> {
    let dependency_indexes = package
        .dependencies
        .iter()
        .map(|entry| resolved_dependency(package, entry, packages))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LockEntry {
        name: package.name.clone(),
        version: package.version.clone(),
        source: package.source.clone(),
        dependency_indexes,
    })
}

/// The position in `packages` of the one package the dependency `entry` of `dependent` names.
fn resolved_dependency(
    dependent: &RawPackage,
    entry: &str,
    packages: &[RawPackage],
) -> Result<usize, TextError> {
    let spelled = spelled_entry(entry).ok_or_else(|| {
        TextError::unreadable(format!(
            "Cargo.lock: package `{}` {} lists a dependency `{entry}`, which is none of the \
             spellings `name`, `name version` and `name version (source)`",
            dependent.name, dependent.version
        ))
    })?;

    let mut matches = packages.iter().enumerate().filter(|(_, candidate)| {
        candidate.name == spelled.name
            && spelled
                .version
                .is_none_or(|version| candidate.version == version)
            && spelled
                .source
                .is_none_or(|source| candidate.source.as_deref() == Some(source))
    });
    match (matches.next(), matches.next()) {
        (Some((index, _)), None) => Ok(index),
        (None, _) => Err(TextError::unreadable(format!(
            "Cargo.lock: package `{}` {} lists a dependency `{entry}`, which names no package",
            dependent.name, dependent.version
        ))),
        (Some(_), Some(_)) => Err(TextError::unreadable(format!(
            "Cargo.lock: package `{}` {} lists a dependency `{entry}`, which names more than one \
             package",
            dependent.name, dependent.version
        ))),
    }
}

/// Splits a dependency entry into the parts it spells, or `None` when it is none of Cargo's three
/// spellings.
fn spelled_entry(entry: &str) -> Option<DependencyEntry<'_>> {
    let Some((name, rest)) = entry.split_once(' ') else {
        return Some(DependencyEntry {
            name: entry,
            version: None,
            source: None,
        });
    };
    let Some((version, parenthesised)) = rest.split_once(' ') else {
        return Some(DependencyEntry {
            name,
            version: Some(rest),
            source: None,
        });
    };
    let source = parenthesised.strip_prefix('(')?.strip_suffix(')')?;
    Some(DependencyEntry {
        name,
        version: Some(version),
        source: Some(source),
    })
}

#[cfg(test)]
mod tests {
    use super::Lockfile;

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    /// Reads `text` as a lockfile, failing the test when it is refused.
    fn parsed(text: &str) -> Lockfile {
        Lockfile::parse(text).expect("the lockfile in this test is well formed")
    }

    #[test]
    fn a_dependency_spelled_as_a_bare_name_resolves_to_the_one_package_of_that_name() {
        // `a` depends on `b`, written as only its name, which the lockfile does when one
        // package of that name is locked.
        let lockfile = parsed(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b"]

[[package]]
name = "b"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
        );

        let entries = lockfile.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name(), "a");
        assert_eq!(entries[0].version(), "0.1.0");
        assert_eq!(entries[0].source(), None);
        assert_eq!(entries[0].dependency_indexes(), &[1]);
        assert_eq!(entries[1].source(), Some(CRATES_IO));
        assert_eq!(entries[1].dependency_indexes(), &[] as &[usize]);
    }

    #[test]
    fn a_dependency_spelled_with_a_version_picks_that_version_among_two() {
        // Two versions of `b` are locked, so the bare name would be ambiguous; `b 2.0.0` is not.
        let lockfile = parsed(
            r#"
version = 3

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 2.0.0"]

[[package]]
name = "b"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "b"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
        );

        assert_eq!(lockfile.entries()[0].dependency_indexes(), &[2]);
    }

    #[test]
    fn a_dependency_spelled_with_a_source_picks_that_source_among_two() {
        // One version of `b` is locked from two sources, so only the full spelling names one.
        let lockfile = parsed(
            r#"
version = 3

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 1.0.0 (git+https://example.invalid/b#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39)"]

[[package]]
name = "b"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "b"
version = "1.0.0"
source = "git+https://example.invalid/b#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"
"#,
        );

        assert_eq!(lockfile.entries()[0].dependency_indexes(), &[2]);
    }

    #[test]
    fn a_dependency_naming_no_package_is_unreadable() {
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["missing"]
"#,
        )
        .expect_err("`missing` is locked nowhere");

        assert!(error.is_unreadable());
        assert_eq!(
            error.detail(),
            "Cargo.lock: package `a` 0.1.0 lists a dependency `missing`, which names no package"
        );
    }

    #[test]
    fn a_dependency_with_a_version_that_matches_nothing_is_unreadable() {
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 9.9.9"]

[[package]]
name = "b"
version = "1.0.0"
"#,
        )
        .expect_err("`b` is locked at 1.0.0 only");

        assert!(error.is_unreadable());
    }

    #[test]
    fn a_dependency_naming_several_packages_is_unreadable() {
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b"]

[[package]]
name = "b"
version = "1.0.0"

[[package]]
name = "b"
version = "2.0.0"
"#,
        )
        .expect_err("`b` is locked twice, and the bare name does not say which");

        assert!(error.is_unreadable());
        assert_eq!(
            error.detail(),
            "Cargo.lock: package `a` 0.1.0 lists a dependency `b`, which names more than one \
             package"
        );
    }

    #[test]
    fn a_dependency_with_a_source_that_is_not_in_parentheses_is_unreadable() {
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 1.0.0 registry+https://example.invalid/index"]

[[package]]
name = "b"
version = "1.0.0"
"#,
        )
        .expect_err("a source must be written in parentheses");

        assert!(error.is_unreadable());
    }

    #[test]
    fn format_versions_three_and_four_are_read() {
        for version in [3, 4] {
            let text = format!("version = {version}\n");
            assert!(Lockfile::parse(&text).is_ok(), "format {version} is read");
        }
    }

    #[test]
    fn format_version_two_is_unsupported() {
        let error = Lockfile::parse("version = 2\n").expect_err("format 2 is not read");

        assert!(error.is_unsupported());
        assert_eq!(
            error.detail(),
            "Cargo.lock has format version 2, and only versions 3 and 4 are read"
        );
    }

    #[test]
    fn a_format_version_newer_than_four_is_unsupported() {
        let error = Lockfile::parse("version = 5\n").expect_err("format 5 is not read");

        assert!(error.is_unsupported());
    }

    #[test]
    fn a_lockfile_with_no_format_version_is_unsupported() {
        let error = Lockfile::parse(
            r#"
[[package]]
name = "a"
version = "0.1.0"
"#,
        )
        .expect_err("a lockfile with no version is an older format");

        assert!(error.is_unsupported());
    }

    #[test]
    fn text_that_is_not_toml_is_unreadable() {
        let error = Lockfile::parse("version = [").expect_err("this is not TOML");

        assert!(error.is_unreadable());
    }

    #[test]
    fn a_package_missing_its_version_is_unreadable() {
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
"#,
        )
        .expect_err("a package needs a version");

        assert!(error.is_unreadable());
    }

    #[test]
    fn a_lockfile_with_no_packages_has_no_entries() {
        assert!(parsed("version = 4\n").entries().is_empty());
    }
}
