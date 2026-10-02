//! Fixtures and assertions the `wear_*` acceptance files share: a workspace
//! that is the command line's own package and wears nothing yet, and the
//! small readers that say what `wear` wrote into its manifest.
//!
//! The manifests are read as text rather than parsed: `ritual`'s
//! dev-dependencies stay to what these tests cannot do without, and the two
//! facts every scenario needs (which section a dependency landed in, and that
//! a wearing table is there and empty) are plain lines of a file `wear` is
//! meant to leave readable.

use std::error::Error;
use std::path::Path;

use super::{Fixture, write, write_package_manifest};

/// The package the `ritual` binary under test is built from. `wear` writes
/// into the package the running command line names, so a fixture a `wear`
/// scenario runs in must have a workspace member called exactly this.
pub(crate) const COMMAND_LINE_PACKAGE: &str = "skeletons-ritual";

/// Writes the command line's own package under `relative_dir` of `root`
/// (the root itself when empty): the manifest `extra` is appended to, and,
/// beside the library every fixture package has, the binary target a command
/// line always has, since the package `wear` writes into is the one that
/// built the running binary.
pub(crate) fn write_command_line_package(
    root: &Path,
    relative_dir: &str,
    extra: &str,
) -> Result<(), Box<dyn Error>> {
    write_package_manifest(root, relative_dir, COMMAND_LINE_PACKAGE, extra)?;
    let prefix = if relative_dir.is_empty() {
        String::new()
    } else {
        format!("{relative_dir}/")
    };
    write(
        root,
        &format!("{prefix}src/main.rs"),
        b"// nothing: a binary target is what makes a package a command line.\nfn main() {}\n",
    )
}

/// A workspace whose root package is the command line's own package, with
/// `extra` appended to its manifest, a committed lockfile, and a git
/// repository holding one clean commit of everything so far.
///
/// `wear` and `sync` both refuse an unclean work tree, so a scenario starts
/// from a clean one and introduces whatever else it needs on purpose.
pub(crate) fn fixture_for_wearing(extra: &str) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_command_line_package(fixture.root(), "", extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

/// Commits everything present, so the next `sync` starts from a clean tree.
pub(crate) fn commit_everything(fixture: &Fixture, message: &str) -> Result<(), Box<dyn Error>> {
    let home = fixture.sandbox().home();
    super::git::run(fixture.root(), home, &["add", "--all"], "git add")?;
    super::git::run(
        fixture.root(),
        home,
        &["commit", "--quiet", "--message", message],
        "git commit",
    )?;
    Ok(())
}

/// The two files a refused or failed `wear` must leave exactly as they were:
/// the command line crate's manifest and the workspace lockfile, as bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ManifestAndLockfile {
    manifest: Vec<u8>,
    lockfile: Vec<u8>,
}

impl ManifestAndLockfile {
    /// Reads the manifest at `manifest_relative` and `Cargo.lock`, both under
    /// the fixture's root.
    pub(crate) fn read(fixture: &Fixture, manifest_relative: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            manifest: fixture.read(manifest_relative)?,
            lockfile: fixture.read("Cargo.lock")?,
        })
    }
}

/// Everything `ritual` printed, stdout then stderr, so an assertion about a
/// message does not depend on which stream the message went to.
pub(crate) fn combined_output(report: &super::Report) -> String {
    format!("{}{}", report.stdout, report.stderr)
}

/// What every message `ritual` refuses with starts with, on stderr.
const REFUSAL_PREFIX: &str = "ritual: ";

/// What `ritual` appends when it undid a change it had already made, and
/// does not append to a refusal that came before it changed anything.
const ROLLBACK_NOTICE: &str = "put the project back";

/// Asserts that stderr holds `message` as one whole line, after the
/// command line's prefix, and nothing else on that line.
pub(crate) fn assert_refused_with_line(report: &super::Report, message: &str) {
    let expected = format!("{REFUSAL_PREFIX}{message}");
    assert!(
        report.stderr.lines().any(|line| line == expected),
        "stderr must hold exactly this line:\n{expected}\nstderr was:\n{}",
        report.stderr
    );
}

/// Asserts that stderr does not claim the project was put back: a refusal
/// made before anything changed has nothing to undo, and saying so would
/// mean something had been changed first.
pub(crate) fn assert_nothing_was_undone(report: &super::Report) {
    assert!(
        !report.stderr.contains(ROLLBACK_NOTICE),
        "a refusal before any change must not report an undo; stderr was:\n{}",
        report.stderr
    );
}

/// The non-blank lines of the `[header]` table in `manifest`, or `None` when
/// the manifest has no such table. The table ends at the next line that opens
/// another table.
pub(crate) fn table_lines<'manifest>(
    manifest: &'manifest str,
    header: &str,
) -> Option<Vec<&'manifest str>> {
    let mut lines = manifest.lines();
    lines.find(|line| line.trim() == format!("[{header}]"))?;
    Some(
        lines
            .take_while(|line| !line.trim_start().starts_with('['))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect(),
    )
}

/// Checks that `manifest` declares `key` under `[dev-dependencies]` as a
/// dependency on `package`, never under `[dependencies]`, and that the
/// wearing table named for `key` is present and holds nothing.
///
/// A table or declaration that is missing comes back as an error naming it, a
/// wrong one fails an assertion, so either way the calling test fails with
/// the manifest in front of the reader.
pub(crate) fn assert_worn_as_a_dev_dependency(
    manifest: &str,
    key: &str,
    package: &str,
) -> Result<(), Box<dyn Error>> {
    let dev_dependencies = table_lines(manifest, "dev-dependencies")
        .ok_or_else(|| format!("no [dev-dependencies] table; manifest was:\n{manifest}"))?;
    let declaration = dev_dependencies
        .iter()
        .find(|line| line.starts_with(&format!("{key} ")) || line.starts_with(&format!("{key}=")))
        .ok_or_else(|| format!("no `{key}` under [dev-dependencies]; manifest was:\n{manifest}"))?;
    assert!(
        declaration.contains(package),
        "the `{key}` dependency must name the package `{package}`; it was: {declaration}"
    );

    let ordinary = table_lines(manifest, "dependencies").unwrap_or_default();
    assert!(
        !ordinary.iter().any(
            |line| line.starts_with(&format!("{key} ")) || line.starts_with(&format!("{key}="))
        ),
        "`{key}` must not be declared under [dependencies]; manifest was:\n{manifest}"
    );

    let wearing =
        table_lines(manifest, &format!("package.metadata.skeletons.{key}")).ok_or_else(|| {
            format!("no [package.metadata.skeletons.{key}] table; manifest was:\n{manifest}")
        })?;
    let wearing: Vec<&&str> = wearing
        .iter()
        .filter(|line| !line.starts_with('#'))
        .collect();
    assert!(
        wearing.is_empty(),
        "the wearing table must be written empty; it held: {wearing:?}"
    );
    Ok(())
}
