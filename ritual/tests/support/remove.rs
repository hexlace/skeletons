//! Fixtures and assertions the `remove*` acceptance files share: a workspace
//! that wears a checked-in test skeleton with its files written and
//! committed, the exact lines `remove` prints, and a snapshot of the whole
//! work tree for the scenarios that must leave it as it was.
//!
//! The expected lines are built from the fixture's own values (the dependency
//! key, the crate, the manifest path, the file paths), never loosened to a
//! substring, so a message that drifts by a word fails the scenario that
//! holds it.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::wear::{
    COMMAND_LINE_PACKAGE, commit_everything, fixture_for_wearing, git_step, the_single_refusal_line,
};
use super::{
    Fixture, Report, TestOutcome, checked_in_test_skeleton, json, path_dependency_on,
    wearing_table, write_minimal_skeleton, write_package_manifest,
};

/// The command line's manifest in a workspace whose root package is the
/// command line's own package, as `remove` names it.
pub(crate) const MANIFEST: &str = "Cargo.toml";

/// The version every checked-in test skeleton declares.
pub(crate) const SKELETON_VERSION: &str = "0.0.0";

/// What `check` prints for a workspace that wears nothing.
pub(crate) const WEARS_NOTHING: &str = "this workspace wears no skeletons; a manifest wears one \
     with a [package.metadata.skeletons.<dependency>] table beside the dependency\n";

/// The refusal for a work tree holding exactly one uncommitted change, whole.
pub(crate) const ONE_UNCOMMITTED_CHANGE_LINE: &str = concat!(
    "the working tree has 1 uncommitted change, so remove wrote nothing: it writes only into a ",
    "clean working tree, where git holds the manifest it changes, any Cargo.lock git tracks and ",
    "the files it deletes; commit, stash or move it, then run the `remove` task again",
);

/// The path of a checked-in test skeleton, as text a command line can take.
pub(crate) fn skeleton_path(name: &str) -> Result<String, Box<dyn Error>> {
    Ok(checked_in_test_skeleton(name)
        .to_str()
        .ok_or("path must be UTF-8")?
        .to_owned())
}

/// Runs `wear` for the checked-in `skeleton` (under `key` when given) and
/// commits what it wrote, so the next command starts from a clean tree.
pub(crate) fn wear(fixture: &Fixture, skeleton: &str, key: Option<&str>) -> TestOutcome {
    let path = skeleton_path(skeleton)?;
    let mut arguments = vec!["skeletons", "wear", skeleton];
    if let Some(key) = key {
        arguments.push(key);
    }
    arguments.extend(["--path", &path]);
    let report = fixture.run(&arguments)?;
    assert_eq!(
        report.exit_code, 0,
        "precondition: wear must succeed; stderr was: {}",
        report.stderr
    );
    commit_everything(fixture, "fixture: wear")
}

/// Runs `sync` and commits what it wrote.
pub(crate) fn sync_and_commit(fixture: &Fixture) -> TestOutcome {
    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "precondition: sync must succeed; stderr was: {}",
        report.stderr
    );
    commit_everything(fixture, "fixture: sync")
}

/// A workspace that wears one checked-in skeleton, with its files written and
/// everything committed, and the command line's manifest as it was before
/// the skeleton was worn.
pub(crate) struct Worn {
    pub(crate) fixture: Fixture,
    pub(crate) manifest_before_wearing: Vec<u8>,
}

impl Worn {
    /// Wears `skeleton` under `key` (the crate's name if `None`), then syncs.
    pub(crate) fn synced(skeleton: &str, key: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let worn = Self::unsynced(skeleton, key)?;
        sync_and_commit(&worn.fixture)?;
        Ok(worn)
    }

    /// Wears `skeleton` under `key` and commits, writing none of its files.
    pub(crate) fn unsynced(skeleton: &str, key: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let fixture = fixture_for_wearing("")?;
        let manifest_before_wearing = fixture.read(MANIFEST)?;
        wear(&fixture, skeleton, key)?;
        Ok(Self {
            fixture,
            manifest_before_wearing,
        })
    }
}

/// Runs `remove` with `arguments` after the subcommand's own name.
pub(crate) fn run_remove(fixture: &Fixture, arguments: &[&str]) -> Result<Report, Box<dyn Error>> {
    let mut command = vec!["skeletons", "remove"];
    command.extend_from_slice(arguments);
    fixture.run(&command)
}

/// The first success line for a wearing that resolved to a skeleton with a
/// known crate and locked version, whether or not its files can be known.
pub(crate) fn stopped_wearing_line(crate_name: &str, key: &str, manifest: &str) -> String {
    format!(
        "stopped wearing {crate_name} {SKELETON_VERSION}: took the dependency `{key}` and \
         [package.metadata.skeletons.{key}] out of {manifest}"
    )
}

/// The first success line for a wearing with a dependency that is no skeleton
/// to name with a version: a dependency that is not a skeleton, or one that
/// did not resolve to a locked package.
pub(crate) fn took_out_line(key: &str, manifest: &str) -> String {
    format!("took the dependency `{key}` and [package.metadata.skeletons.{key}] out of {manifest}")
}

/// The first success line for a wearing table with no dependency declared
/// under its key.
pub(crate) fn no_dependency_line(key: &str, manifest: &str) -> String {
    format!(
        "took [package.metadata.skeletons.{key}] out of {manifest}; no dependency was declared \
         under `{key}`"
    )
}

/// The last success line: what to commit.
pub(crate) fn commit_line(manifest: &str, lockfile_changed: bool, deleted: usize) -> String {
    let deleted_files = match deleted {
        0 => None,
        1 => Some("the 1 deleted file".to_owned()),
        count => Some(format!("the {count} deleted files")),
    };
    let list = match (lockfile_changed, deleted_files) {
        (false, None) => manifest.to_owned(),
        (true, None) => format!("{manifest} and Cargo.lock"),
        (false, Some(files)) => format!("{manifest} and {files}"),
        (true, Some(files)) => format!("{manifest}, Cargo.lock and {files}"),
    };
    format!("commit {list} to record it")
}

/// The summary line after the `kept` lines of a bare `remove` that kept
/// everything: `total` files, `changed` of them changed.
pub(crate) fn kept_summary(total: usize, changed: usize) -> String {
    if total == 1 {
        return "kept its file, because it has changed from what the skeleton renders: it is the \
                project's own now, to keep or to delete by hand"
            .to_owned();
    }
    let verb = if changed == 1 { "has" } else { "have" };
    format!(
        "kept all {total} of its files, because {changed} {verb} changed from what the skeleton \
         renders: they are the project's own now, to keep or to delete by hand"
    )
}

/// The summary line after the lines of a bare `remove` that kept the files it
/// could delete and left at least one more: `kept` files kept, `changed` of
/// them changed.
pub(crate) fn kept_other_summary(kept: usize, changed: usize) -> String {
    if kept == 1 {
        return "kept its other file, because it has changed from what the skeleton renders: it \
                is the project's own now, to keep or to delete by hand"
            .to_owned();
    }
    let verb = if changed == 1 { "has" } else { "have" };
    format!(
        "kept its other {kept} files, because {changed} {verb} changed from what the skeleton \
         renders: they are the project's own now, to keep or to delete by hand"
    )
}

/// Asserts `remove` succeeded and printed exactly `lines` on stdout.
pub(crate) fn assert_printed(report: &Report, lines: &[String]) {
    assert_eq!(
        report.exit_code, 0,
        "remove must succeed; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    let expected = format!("{}\n", lines.join("\n"));
    assert_eq!(
        report.stdout, expected,
        "remove must print exactly these lines on stdout"
    );
}

/// Asserts `remove` refused with exit status 1 and exactly one stderr line,
/// `message` after the command line's prefix.
pub(crate) fn assert_refused(report: &Report, message: &str) {
    assert_eq!(
        report.exit_code, 1,
        "remove must refuse with status 1; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        the_single_refusal_line(report),
        message,
        "remove must refuse with exactly this line"
    );
}

/// Asserts `check` finds the workspace wearing nothing.
pub(crate) fn assert_check_wears_nothing(fixture: &Fixture) -> TestOutcome {
    let report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        report.exit_code, 0,
        "check must succeed; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        report.stdout, WEARS_NOTHING,
        "check must find nothing worn after remove"
    );
    Ok(())
}

/// The dependency keys `check` lists as worn, in the order it lists them.
pub(crate) fn keys_check_lists(fixture: &Fixture) -> Result<Vec<String>, Box<dyn Error>> {
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "check must succeed; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    let document = json::parse(&report.stdout)?;
    json::skeletons(&document)?
        .iter()
        .map(|row| Ok(json::skeleton_dependency(row)?.to_owned()))
        .collect()
}

/// `git status --porcelain` in the fixture, one entry per line with git's
/// two status columns collapsed, in git's order.
pub(crate) fn porcelain(fixture: &Fixture) -> Result<Vec<String>, Box<dyn Error>> {
    let status = git_step(fixture, &["status", "--porcelain"])?;
    Ok(status
        .lines()
        .map(|line| line.trim_start().to_owned())
        .collect())
}

/// One entry of the work tree: a file's bytes and mode, or a directory's mode.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    File { bytes: Vec<u8>, mode: u32 },
    Directory { mode: u32 },
}

/// Every file and directory under the fixture's root, but `.git` and
/// `target`, by workspace-relative path.
pub(crate) fn tree_state(root: &Path) -> Result<BTreeMap<String, Entry>, Box<dyn Error>> {
    let mut state = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_str() == Some(".git") || name.to_str() == Some("target") {
                continue;
            }
            let path = entry.path();
            let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
            let metadata = fs::symlink_metadata(&path)?;
            let mode = metadata.permissions().mode() & 0o7777;
            if metadata.is_dir() {
                state.insert(relative, Entry::Directory { mode });
                pending.push(path);
            } else {
                let bytes = fs::read(&path)?;
                state.insert(relative, Entry::File { bytes, mode });
            }
        }
    }
    Ok(state)
}

/// A directory made unwritable for the length of a scenario, restored to the
/// mode it had when dropped so the fixture can be torn down.
pub(crate) struct ReadOnlyDirectory {
    path: PathBuf,
    original_mode: u32,
}

impl ReadOnlyDirectory {
    /// Makes `path` mode `0o555`. Returns `None`, with the mode restored, when
    /// this process can still create a file in it (a superuser), since such a
    /// process cannot show what an unwritable directory does.
    pub(crate) fn make(path: &Path) -> Result<Option<Self>, Box<dyn Error>> {
        let original_mode = fs::metadata(path)?.permissions().mode();
        fs::set_permissions(path, fs::Permissions::from_mode(0o555))?;
        let guard = Self {
            path: path.to_path_buf(),
            original_mode,
        };
        let probe = path.join(".probe");
        match fs::File::create(&probe) {
            Ok(_) => {
                fs::remove_file(&probe)?;
                Ok(None)
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(Some(guard)),
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for ReadOnlyDirectory {
    fn drop(&mut self) {
        // Best-effort: a failed test should not also fail at cleanup.
        let _unused =
            fs::set_permissions(&self.path, fs::Permissions::from_mode(self.original_mode));
    }
}

/// Marks `relative` as a file git was told not to read, with one `update-index`
/// call per flag.
pub(crate) fn mark(fixture: &Fixture, relative: &str, flag: &str) -> TestOutcome {
    git_step(fixture, &["update-index", flag, "--", relative])?;
    Ok(())
}

/// A dependency on the checked-in `skeleton`, taken by path, under `key`.
pub(crate) fn dependency_under_key(key: &str, skeleton: &str) -> Result<String, Box<dyn Error>> {
    let path = super::one_line::toml_string(&skeleton_path(skeleton)?);
    Ok(format!(
        "{key} = {{ package = \"{skeleton}\", path = {path} }}\n"
    ))
}

/// A workspace with two members: the command line's own package at `ritual`,
/// wearing nothing, and `tools`, which wears the checked-in `passthrough-plain`
/// under `key`. Committed clean, lockfile included.
pub(crate) fn fixture_wearing_in_another_member(key: &str) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    super::write_workspace_root(fixture.root(), &["ritual", "tools"])?;
    write_package_manifest(fixture.root(), "ritual", COMMAND_LINE_PACKAGE, "")?;
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        dependency_under_key(key, "passthrough-plain")?,
        super::wearing_table(key, ""),
    );
    write_package_manifest(fixture.root(), "tools", "tools", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

/// The directory, inside a fixture from [`fixture_wearing_a_skeleton_that_claims`],
/// holding the skeleton it wears.
pub(crate) const CLAIMING_DIRECTORY: &str = "claiming-skeleton";

/// The crate and the dependency key of the skeleton
/// [`fixture_wearing_a_skeleton_that_claims`] writes.
pub(crate) const CLAIMING: &str = "claiming";

/// A workspace whose root package is the command line's own and wears
/// [`CLAIMING`], a skeleton written into the fixture that claims exactly
/// `claims`, each a path and the bytes the skeleton renders there. Committed
/// clean, lockfile included, and not synced.
///
/// No checked-in skeleton claims `Cargo.lock` or a Cargo manifest, so a
/// scenario that needs one builds the skeleton here, as the scenarios that need
/// a file name of their own do.
pub(crate) fn fixture_wearing_a_skeleton_that_claims(
    claims: &[(&str, &str)],
) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let skeleton = fixture.root().join(CLAIMING_DIRECTORY);
    let (first, rest) = claims.split_first().ok_or("a skeleton claims a file")?;
    write_minimal_skeleton(&skeleton, CLAIMING, first.0, first.1)?;
    for (path, contents) in rest {
        fixture.write(
            &format!("{CLAIMING_DIRECTORY}/files/{path}"),
            contents.as_bytes(),
        )?;
    }
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        path_dependency_on(CLAIMING, &skeleton),
        wearing_table(CLAIMING, ""),
    );
    write_package_manifest(fixture.root(), "", COMMAND_LINE_PACKAGE, &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

/// Makes the skeleton [`fixture_wearing_a_skeleton_that_claims`] wrote render
/// `relative` as the bytes the work tree holds there now, and commits, so the
/// file matches what the skeleton renders.
pub(crate) fn make_the_skeleton_render_what_is_there(
    fixture: &Fixture,
    relative: &str,
) -> TestOutcome {
    let bytes = fixture.read(relative)?;
    fixture.write(&format!("{CLAIMING_DIRECTORY}/files/{relative}"), &bytes)?;
    commit_everything(fixture, "fixture: the skeleton renders what is there")
}

/// A workspace whose root package is the command line's own and wears
/// `overlap-a` and `overlap-b`, two checked-in skeletons that both claim
/// `shared.txt`, with that file in the work tree holding what `overlap-b`
/// renders. Committed clean, lockfile included, and not synced, so `check`
/// refuses both wearings for the overlap.
pub(crate) fn fixture_wearing_two_skeletons_that_claim_one_path() -> Result<Fixture, Box<dyn Error>>
{
    let extra = format!(
        "[dev-dependencies]\n{}{}\n{}\n{}",
        dependency_under_key("overlap-a", "overlap-a")?,
        dependency_under_key("overlap-b", "overlap-b")?,
        wearing_table("overlap-a", ""),
        wearing_table("overlap-b", ""),
    );
    let fixture = fixture_for_wearing(&extra)?;
    fixture.write("shared.txt", b"claimed by overlap-b\n")?;
    commit_everything(&fixture, "fixture: the shared file")?;
    Ok(fixture)
}

/// The reasons `check` gives for refusing the wearing under `key`, joined by
/// `; `, which is how `remove` words them when it cannot tell which files the
/// wearing claims.
pub(crate) fn check_refusal_reasons(
    fixture: &Fixture,
    key: &str,
) -> Result<String, Box<dyn Error>> {
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    let document = json::parse(&report.stdout)?;
    let mut reasons = Vec::new();
    for refusal in json::refusals(&document)? {
        if json::refusal_dependency(refusal)? == Some(key) {
            reasons.push(json::refusal_message(refusal)?.to_owned());
        }
    }
    assert!(
        !reasons.is_empty(),
        "precondition: check must refuse this wearing; stdout was: {}",
        report.stdout
    );
    Ok(reasons.join("; "))
}
