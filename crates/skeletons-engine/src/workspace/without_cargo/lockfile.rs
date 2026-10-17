//! `Cargo.lock`, read as text: its packages, and which package each dependency entry names.

use serde::Deserialize;

use super::locked_source;
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
    replaced: bool,
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
    /// The package id a `[replace]` in the root manifest redirected this package to.
    replace: Option<String>,
}

/// One dependency entry, split into the parts it spells: `name`, `name version` or
/// `name version (source)`. The source is the package's `source` without its locked commit.
struct DependencyEntry<'text> {
    name: &'text str,
    version: Option<&'text str>,
    source: Option<&'text str>,
}

impl Lockfile {
    /// Reads a lockfile's text, refusing a format other than 3 or 4, a git source whose query is
    /// not exactly one `branch`, `tag` or `rev`, and any dependency entry that does not name
    /// exactly one package.
    ///
    /// # Errors
    ///
    /// Unsupported for a format version other than 3 or 4, or none, and for a git source with a
    /// query of the dependency URL's own, which Cargo keeps. Unreadable for text that is not a
    /// lockfile, and for a dependency entry that names no package or several.
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
        raw.package
            .iter()
            .try_for_each(refused_for_an_unread_query)?;
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

    /// Whether the lockfile writes a `replace` on this package: Cargo resolved it through a
    /// `[replace]` in the root manifest.
    #[must_use]
    pub(crate) const fn is_replaced(&self) -> bool {
        self.replaced
    }
}

/// Refuses `package` when it is locked from a git source whose query is not exactly one
/// reference, which is what Cargo writes for a dependency URL that has a query of its own.
fn refused_for_an_unread_query(package: &RawPackage) -> Result<(), TextError> {
    match package.source.as_deref() {
        Some(source) if locked_source::has_unread_query(source) => {
            Err(TextError::unsupported(format!(
                "Cargo.lock: package `{}` {} is locked from `{source}`, whose query is not one \
                 `branch`, `tag` or `rev`; Cargo keeps a query of the URL's own in the source, \
                 which this reader does not follow",
                package.name, package.version
            )))
        }
        Some(_) | None => Ok(()),
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
        replaced: package.replace.is_some(),
    })
}

/// The position in `packages` of the one package the dependency `entry` of `dependent` names.
///
/// A spelled source is compared with each package's `source` cut at its `#<commit>`, because Cargo
/// writes it that way. A line that spells the commit names no package, and two packages that differ
/// only in their commit are both named by one line.
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
            && spelled.source.is_none_or(|source| {
                candidate.source.as_deref().map(source_without_commit) == Some(source)
            })
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

/// `source` without the `#<commit>` a git package is locked to, which is how a dependency entry
/// spells it. The reference's query (`?rev=`, `?branch=`, `?tag=`) stays, and tells apart the
/// references to one repository.
fn source_without_commit(source: &str) -> &str {
    source
        .split_once('#')
        .map_or(source, |(without_commit, _)| without_commit)
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
    use proptest::prelude::*;

    use super::{LockEntry, Lockfile};
    use crate::workspace::without_cargo::text_error::TextErrorKind;

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
        // Cargo writes the source in the line without the commit the package is locked to.
        let lockfile = parsed(
            r#"
version = 3

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 1.0.0 (git+https://example.invalid/b)"]

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
    fn a_dependency_spelled_with_each_kind_of_git_reference_picks_that_reference() {
        // One `dep` 0.1.0 is locked from the default branch, a `rev`, a `branch` and a `tag` of one
        // repository. Cargo keeps the reference's query in the line and drops the commit, so the
        // four lines differ only in their query and each names one entry.
        let lockfile = parsed(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = [
 "dep 0.1.0 (git+https://example.invalid/dep)",
 "dep 0.1.0 (git+https://example.invalid/dep?rev=3f2a9c1)",
 "dep 0.1.0 (git+https://example.invalid/dep?branch=feat%2Fx)",
 "dep 0.1.0 (git+https://example.invalid/dep?tag=v1)",
]

[[package]]
name = "dep"
version = "0.1.0"
source = "git+https://example.invalid/dep#1111111111111111111111111111111111111111"

[[package]]
name = "dep"
version = "0.1.0"
source = "git+https://example.invalid/dep?rev=3f2a9c1#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"

[[package]]
name = "dep"
version = "0.1.0"
source = "git+https://example.invalid/dep?branch=feat%2Fx#2222222222222222222222222222222222222222"

[[package]]
name = "dep"
version = "0.1.0"
source = "git+https://example.invalid/dep?tag=v1#3333333333333333333333333333333333333333"
"#,
        );

        assert_eq!(lockfile.entries()[0].dependency_indexes(), &[1, 2, 3, 4]);
    }

    #[test]
    fn a_dependency_on_a_git_package_of_the_same_name_and_version_picks_the_git_package() {
        // The case the generated lockfiles once shrank to. A path package `a` renames a git
        // package that is also called `a` and also 0.1.0, and Cargo writes the line below for it
        // (measured on 1.95.0): the full spelling, source without commit, because the short ones
        // would name both packages.
        let lockfile = parsed(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = [
 "a 0.1.0 (git+https://example.invalid/dep)",
]

[[package]]
name = "a"
version = "0.1.0"
source = "git+https://example.invalid/dep#aa0000aa00aaaaaa00aaa0a0aa0000aaa0aaa00a"
"#,
        );

        let entries = lockfile.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source(), None);
        assert_eq!(entries[0].dependency_indexes(), &[1]);
        assert_eq!(
            entries[1].source(),
            Some("git+https://example.invalid/dep#aa0000aa00aaaaaa00aaa0a0aa0000aaa0aaa00a")
        );
        assert_eq!(entries[1].dependency_indexes(), &[] as &[usize]);
    }

    #[test]
    fn a_dependency_spelling_the_commit_names_no_package() {
        // Cargo never writes `#<commit>` in a dependency line, so a line that does is not one it
        // wrote, and it is not read as if it were.
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 1.0.0 (git+https://example.invalid/b#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39)"]

[[package]]
name = "b"
version = "1.0.0"
source = "git+https://example.invalid/b#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39"
"#,
        )
        .expect_err("the line spells a commit");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
        assert_eq!(
            error.detail(),
            "Cargo.lock: package `a` 0.1.0 lists a dependency `b 1.0.0 \
             (git+https://example.invalid/b#3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39)`, which \
             names no package"
        );
    }

    #[test]
    fn two_entries_differing_only_in_commit_are_named_by_one_dependency() {
        // One reference locks to one commit, so Cargo writes no such pair. Were it present, the
        // line could not say which of the two it means.
        let error = Lockfile::parse(
            r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b 1.0.0 (git+https://example.invalid/b)"]

[[package]]
name = "b"
version = "1.0.0"
source = "git+https://example.invalid/b#1111111111111111111111111111111111111111"

[[package]]
name = "b"
version = "1.0.0"
source = "git+https://example.invalid/b#2222222222222222222222222222222222222222"
"#,
        )
        .expect_err("two commits of one reference");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
        assert_eq!(
            error.detail(),
            "Cargo.lock: package `a` 0.1.0 lists a dependency `b 1.0.0 \
             (git+https://example.invalid/b)`, which names more than one package"
        );
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unsupported);
        assert_eq!(
            error.detail(),
            "Cargo.lock has format version 2, and only versions 3 and 4 are read"
        );
    }

    #[test]
    fn a_format_version_newer_than_four_is_unsupported() {
        let error = Lockfile::parse("version = 5\n").expect_err("format 5 is not read");

        assert_eq!(error.kind(), TextErrorKind::Unsupported);
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

        assert_eq!(error.kind(), TextErrorKind::Unsupported);
    }

    #[test]
    fn text_that_is_not_toml_is_unreadable() {
        let error = Lockfile::parse("version = [").expect_err("this is not TOML");

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
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

        assert_eq!(error.kind(), TextErrorKind::Unreadable);
    }

    #[test]
    fn a_lockfile_with_no_packages_has_no_entries() {
        assert!(parsed("version = 4\n").entries().is_empty());
    }

    /// A lockfile of one package `g` 1.0.0 locked from the git `source`.
    fn locked_from(source: &str) -> String {
        format!(
            "version = 4\n\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
        )
    }

    #[test]
    fn a_git_source_with_no_query_or_one_reference_is_read() {
        // The controls for the refusal below: Cargo writes no query for the default branch and
        // one `branch`, `tag` or `rev` otherwise, and each is read, a `/` percent-encoded.
        let commit = "0123456789abcdef0123456789abcdef01234567";
        for query in [
            "",
            "?branch=main",
            "?tag=v1",
            "?rev=3f2a9c1d",
            "?branch=feat%2Fx",
        ] {
            let source = format!("git+https://example.test/g{query}#{commit}");

            assert!(Lockfile::parse(&locked_from(&source)).is_ok(), "{source}");
        }
    }

    #[test]
    fn a_git_source_with_a_query_that_is_not_one_reference_is_unsupported() {
        // Cargo keeps a query the URL had in the source (`?x=1`) and writes the reference after
        // it (`?x=1?branch=dev`); an unknown key (`?ref=v1`), an empty query and two references
        // are none of the shapes that name one. The reader does not model the first two, and
        // the refusal comes from reading the lockfile, before currency can call it stale.
        let commit = "0123456789abcdef0123456789abcdef01234567";
        for query in [
            "?x=1",
            "?x=1?branch=dev",
            "?ref=v1",
            "?",
            "?branch=main&tag=v1",
        ] {
            let source = format!("git+https://example.test/g{query}#{commit}");

            let error = Lockfile::parse(&locked_from(&source)).expect_err(&source);

            assert_eq!(error.kind(), TextErrorKind::Unsupported, "{source}");
            assert_eq!(
                error.detail(),
                format!(
                    "Cargo.lock: package `g` 1.0.0 is locked from `{source}`, whose query is not \
                     one `branch`, `tag` or `rev`; Cargo keeps a query of the URL's own in the \
                     source, which this reader does not follow"
                )
            );
        }
    }

    #[test]
    fn a_registry_or_path_source_is_not_judged_by_a_query() {
        // Only a git source carries a reference in its query; a registry's index URL is left
        // alone, as is a package with no source.
        for source in [
            "registry+https://example.test/index?x=1",
            "sparse+https://example.test/index/?x=1",
        ] {
            assert!(Lockfile::parse(&locked_from(source)).is_ok(), "{source}");
        }
        assert!(
            Lockfile::parse("version = 4\n\n[[package]]\nname = \"g\"\nversion = \"1.0.0\"\n")
                .is_ok()
        );
    }

    /// A package as the generator holds it, before it is written as lockfile text: the source is
    /// kept as the dependency lines spell it, apart from the commit a git source is locked to.
    #[derive(Debug, Clone)]
    struct Generated {
        name: &'static str,
        version: &'static str,
        spelled_source: Option<String>,
        commit: Option<String>,
    }

    impl Generated {
        /// The `source` the package writes: the spelled source and, for git, its locked commit.
        fn source(&self) -> Option<String> {
            self.spelled_source.as_ref().map(|spelled| {
                self.commit
                    .as_ref()
                    .map_or_else(|| spelled.clone(), |commit| format!("{spelled}#{commit}"))
            })
        }

        /// Whether the dependency line `name`, `name version` or `name version (source)` names
        /// this package, by the plain rule that every part written must be equal.
        fn is_named_by(&self, name: &str, version: Option<&str>, source: Option<&str>) -> bool {
            self.name == name
                && version.is_none_or(|written| self.version == written)
                && source.is_none_or(|written| self.spelled_source.as_deref() == Some(written))
        }
    }

    /// A package from small pools of names, versions and sources, so that the same name and
    /// version recur under different sources and the short dependency spellings are ambiguous
    /// often enough to matter.
    fn generated_package() -> impl Strategy<Value = Generated> {
        let source = prop_oneof![
            Just((None, false)),
            Just((Some(CRATES_IO.to_owned()), false)),
            Just((Some("sparse+https://index.crates.io/".to_owned()), false)),
            (0..4_u8, "[a-z]{1,6}").prop_map(|(reference, name)| {
                let query = match reference {
                    0 => String::new(),
                    1 => format!("?rev={name}"),
                    2 => format!("?branch={name}%2Fx"),
                    _ => format!("?tag=v{name}"),
                };
                (
                    Some(format!("git+https://example.invalid/dep{query}")),
                    true,
                )
            }),
        ];
        (
            proptest::sample::select(vec!["a", "b", "c"]),
            proptest::sample::select(vec!["0.1.0", "1.0.0", "2.0.0-rc.1"]),
            source,
            "[0-9a-f]{40}",
        )
            .prop_map(
                |(name, version, (spelled_source, is_git), commit)| Generated {
                    name,
                    version,
                    spelled_source,
                    commit: is_git.then_some(commit),
                },
            )
    }

    /// Packages no two of which share a name, a version and a spelled source, which is how Cargo
    /// tells two locked packages apart: one reference locks to one commit.
    fn generated_packages() -> impl Strategy<Value = Vec<Generated>> {
        proptest::collection::vec(generated_package(), 1..8).prop_map(|candidates| {
            let mut kept: Vec<Generated> = Vec::new();
            for candidate in candidates {
                let repeats = kept.iter().any(|package| {
                    package.name == candidate.name
                        && package.version == candidate.version
                        && package.spelled_source == candidate.spelled_source
                });
                if !repeats {
                    kept.push(candidate);
                }
            }
            kept
        })
    }

    /// The line Cargo writes for the dependency on `target`, in the short or the long spelling
    /// (`0` name, `1` name and version, `2` name, version and source), or `None` when the package
    /// has no source to spell.
    fn spelled_line(target: &Generated, spelling: u8) -> Option<String> {
        match spelling {
            0 => Some(target.name.to_owned()),
            1 => Some(format!("{} {}", target.name, target.version)),
            _ => target
                .spelled_source
                .as_ref()
                .map(|source| format!("{} {} ({source})", target.name, target.version)),
        }
    }

    proptest! {
        /// Generates packages, picks dependencies among them in each of Cargo's three spellings,
        /// writes the lockfile text, reads it back, and requires the entries and every resolved
        /// dependency index to equal the generated ones.
        ///
        /// A dependency whose spelling would name more than one of the generated packages is not
        /// generated: the generator counts, over the packages it holds, how many the line names
        /// by the plain rule in `Generated::is_named_by`, and drops the dependency unless the
        /// count is one. Such a line is refused by the reader, and the unit tests above pin that.
        #[test]
        fn generated_lockfiles_are_read_back_as_generated(
            packages in generated_packages(),
            picks in proptest::collection::vec(
                proptest::collection::vec((0_usize..64, 0_u8..3), 0..5),
                8,
            ),
        ) {
            let mut blocks = vec!["version = 4\n".to_owned()];
            let mut expected = Vec::new();
            for (package, package_picks) in packages.iter().zip(&picks) {
                let mut lines = Vec::new();
                let mut indexes = Vec::new();
                for &(pick, spelling) in package_picks {
                    let index = pick % packages.len();
                    let target = &packages[index];
                    let Some(line) = spelled_line(target, spelling) else { continue };
                    let (version, source) = match spelling {
                        0 => (None, None),
                        1 => (Some(target.version), None),
                        _ => (Some(target.version), target.spelled_source.as_deref()),
                    };
                    let named = packages
                        .iter()
                        .filter(|candidate| candidate.is_named_by(target.name, version, source))
                        .count();
                    if named == 1 {
                        lines.push(format!("{line:?}"));
                        indexes.push(index);
                    }
                }
                let source_line = package
                    .source()
                    .map_or_else(String::new, |source| format!("source = {source:?}\n"));
                let dependencies_line = if lines.is_empty() {
                    String::new()
                } else {
                    format!("dependencies = [{}]\n", lines.join(", "))
                };
                blocks.push(format!(
                    "\n[[package]]\nname = {:?}\nversion = {:?}\n{source_line}{dependencies_line}",
                    package.name, package.version
                ));
                expected.push(LockEntry {
                    name: package.name.to_owned(),
                    version: package.version.to_owned(),
                    source: package.source(),
                    dependency_indexes: indexes,
                    replaced: false,
                });
            }

            let lockfile = Lockfile::parse(&blocks.concat());

            prop_assert_eq!(lockfile, Ok(Lockfile { entries: expected }));
        }
    }
}
