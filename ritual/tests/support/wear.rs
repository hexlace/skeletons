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

use super::{Fixture, write_package_manifest};

/// The package the `ritual` binary under test is built from. `wear` writes
/// into the package the running command line names, so a fixture a `wear`
/// scenario runs in must have a workspace member called exactly this.
pub(crate) const COMMAND_LINE_PACKAGE: &str = "skeletons-ritual";

/// A workspace whose root package is the command line's own package, with
/// `extra` appended to its manifest, a committed lockfile, and a git
/// repository holding one clean commit of everything so far.
///
/// `wear` and `sync` both refuse an unclean work tree, so a scenario starts
/// from a clean one and introduces whatever else it needs on purpose.
pub(crate) fn fixture_for_wearing(extra: &str) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_package_manifest(fixture.root(), "", COMMAND_LINE_PACKAGE, extra)?;
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

/// What `ritual` appends when it undid a change it had already made, or
/// tried to and could not, and does not append to a refusal that came before
/// it changed anything: "put the project back as it found it", "put the
/// project back except for …" and "could not put back …".
const UNDO_NOTICES: [&str; 2] = ["put the project back", "could not put back"];

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

/// Asserts that stderr reports no undo, done or failed: a refusal made before
/// anything changed has nothing to undo, and reporting one would mean
/// something had been changed first.
pub(crate) fn assert_nothing_was_undone(report: &super::Report) {
    assert!(
        !UNDO_NOTICES
            .iter()
            .any(|notice| report.stderr.contains(notice)),
        "a refusal before any change must not report an undo; stderr was:\n{}",
        report.stderr
    );
}

/// The refusal for a work tree holding exactly one uncommitted change, whole.
pub(crate) const ONE_UNCOMMITTED_CHANGE_LINE: &str = concat!(
    "the working tree has 1 uncommitted change, so wear wrote nothing: it writes only into a ",
    "clean working tree, where git holds the manifest it changes and any Cargo.lock git ",
    "tracks; commit, stash or move it, then run the `wear` task again",
);

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

/// Runs a git command in `fixture`'s workspace, as a fixture-building step
/// that must succeed.
pub(crate) fn git_step(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn Error>> {
    super::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

/// Runs `wear` on the checked-in `passthrough-plain` skeleton, by path, with
/// the key defaulted: the invocation a refusal scenario that is about the
/// project, not the arguments, needs.
pub(crate) fn wear_passthrough_plain(fixture: &Fixture) -> Result<super::Report, Box<dyn Error>> {
    let skeleton_path = super::checked_in_test_skeleton("passthrough-plain");
    let skeleton_path = skeleton_path.to_str().ok_or("path must be UTF-8")?;
    fixture.run(&[
        "skeletons",
        "wear",
        "passthrough-plain",
        "--path",
        skeleton_path,
    ])
}

/// Asserts the whole story of a refusal made before anything was written:
/// a non-zero exit, `line` as one whole stderr line, no claim that anything
/// was undone, and the manifest and lockfile exactly as `before` read them.
pub(crate) fn assert_refused_and_untouched(
    fixture: &Fixture,
    report: &super::Report,
    line: &str,
    before: &ManifestAndLockfile,
    manifest_relative: &str,
) -> Result<(), Box<dyn Error>> {
    assert_ne!(
        report.exit_code, 0,
        "wear must refuse; stdout was: {}",
        report.stdout
    );
    assert_refused_with_line(report, line);
    assert_nothing_was_undone(report);
    assert_eq!(
        &ManifestAndLockfile::read(fixture, manifest_relative)?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    Ok(())
}

/// The one non-blank line `report`'s stderr holds, after the command line's
/// prefix. Fails the calling test if stderr holds none or several.
pub(crate) fn the_single_refusal_line(report: &super::Report) -> &str {
    let lines: Vec<&str> = report
        .stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(
        lines.len() == 1,
        "stderr must hold exactly one line; it was:\n{}",
        report.stderr
    );
    assert!(
        lines[0].starts_with(REFUSAL_PREFIX),
        "the line must start with `{REFUSAL_PREFIX}`: {}",
        lines[0]
    );
    &lines[0][REFUSAL_PREFIX.len()..]
}

/// The refusal for a file git is told not to read, worded as `sync` words it
/// with `wear` in it. `marked` is how the line names the flags, `options` the
/// `git update-index` options that clear exactly those.
pub(crate) fn hidden_line(path: &str, marked: &str, options: &str) -> String {
    format!(
        "{path} is marked {marked} in git's index, so git does not read its bytes from the work \
         tree and would ignore what wear wrote there: run `git update-index {options} -- \
         {path}`, then run the `wear` task again"
    )
}

pub(crate) const SKIP_WORKTREE: (&str, &str) = ("skip-worktree", "--no-skip-worktree");
pub(crate) const ASSUME_UNCHANGED: (&str, &str) = ("assume-unchanged", "--no-assume-unchanged");
pub(crate) const BOTH_FLAGS: (&str, &str) = (
    "skip-worktree and assume-unchanged",
    "--no-skip-worktree --no-assume-unchanged",
);

/// Builds a clean workspace, optionally gives `hidden` a local edit, marks
/// it with `flags` (as `git update-index` options), and asserts `wear`
/// refuses with the line for `(marked, options)` and changes nothing.
pub(crate) fn assert_a_hidden_file_is_refused(
    hidden: &str,
    locally_edited: bool,
    flags: &[&str],
    (marked, options): (&str, &str),
) -> super::TestOutcome {
    let fixture = fixture_for_wearing("")?;
    if locally_edited {
        let mut edited = fixture.read(hidden)?;
        edited.extend_from_slice(b"\n# a local edit git is told not to look at\n");
        fixture.write(hidden, &edited)?;
    }
    // One `update-index` per flag: given both in a single call, git keeps
    // only `--assume-unchanged`, so the state the scenario names is never
    // built by accident.
    for flag in flags {
        git_step(&fixture, &["update-index", flag, "--", hidden])?;
    }
    let expected_tag = match (
        flags.contains(&"--skip-worktree"),
        flags.contains(&"--assume-unchanged"),
    ) {
        (true, true) => "s",
        (true, false) => "S",
        (false, true) => "h",
        (false, false) => "H",
    };
    let listed = git_step(&fixture, &["ls-files", "-v", "--", hidden])?;
    assert_eq!(
        listed.split(' ').next(),
        Some(expected_tag),
        "precondition: git must print the tag the scenario means; it printed: {listed}"
    );
    assert_eq!(
        git_step(&fixture, &["status", "--porcelain"])?,
        "",
        "precondition: git reports nothing for the hidden file, which is what makes it a trap"
    );
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;

    let report = wear_passthrough_plain(&fixture)?;

    assert_refused_and_untouched(
        &fixture,
        &report,
        &hidden_line(hidden, marked, options),
        &before,
        "Cargo.toml",
    )
}
