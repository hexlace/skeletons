//! Fixtures and assertions the `sync_*` acceptance files share when the
//! skeleton a scenario wears has to be built for that scenario: a claimed
//! path that no checked-in test skeleton has, or two skeletons that
//! together produce a collision.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{
    Fixture, Report, TemporaryDirectory, git, isolate_from_the_enclosing_repository,
    path_dependency_on, path_dependency_on_test_skeleton, require_success, run_ritual,
    wearing_table, write_minimal_skeleton, write_package_manifest,
};

/// A wearing workspace together with the skeleton directories it wears.
///
/// The skeletons are taken by `path =`, so they must outlive every `ritual`
/// run against the workspace; holding their directories here is what keeps
/// them until the test ends.
pub(crate) struct WearingFixture {
    /// The workspace `ritual` runs in. Its manifest and lockfile are written;
    /// no git repository exists yet and no claimed file has been created.
    pub(crate) fixture: Fixture,
    _skeletons: Vec<TemporaryDirectory>,
}

/// Builds a workspace wearing one skeleton crate per entry of `skeletons`,
/// each `(package name, [(path under files/, contents)])`.
pub(crate) fn fixture_wearing(
    skeletons: &[(&str, &[(&str, &str)])],
) -> Result<WearingFixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let mut directories = Vec::with_capacity(skeletons.len());
    let mut dependencies = String::from("[dependencies]\n");
    let mut wearing = String::new();

    for (package_name, files) in skeletons {
        let directory = TemporaryDirectory::new("sync-ad-hoc-skeleton")?;
        let Some((first_path, first_contents)) = files.first() else {
            return Err(format!("skeleton {package_name} needs at least one file").into());
        };
        write_minimal_skeleton(directory.path(), package_name, first_path, first_contents)?;
        for (path, contents) in &files[1..] {
            super::write(
                directory.path(),
                &format!("files/{path}"),
                contents.as_bytes(),
            )?;
        }
        dependencies.push_str(&path_dependency_on(package_name, directory.path()));
        wearing.push_str(&wearing_table(package_name, ""));
        wearing.push('\n');
        directories.push(directory);
    }

    write_package_manifest(
        fixture.root(),
        "",
        "wearer",
        &format!("{dependencies}\n{wearing}"),
    )?;
    fixture.generate_lockfile()?;
    Ok(WearingFixture {
        fixture,
        _skeletons: directories,
    })
}

/// A wearing workspace that lives in a directory *below* the root of the
/// repository that will hold it, so every path git is asked about carries a
/// repository prefix that the workspace-relative claim does not.
///
/// The fixture's own root is the repository root; the workspace is
/// `directory` beneath it, and `ritual` runs there.
pub(crate) struct NestedWorkspace {
    /// The fixture whose root is the repository root. No git repository
    /// exists yet, and no claimed file has been created.
    pub(crate) fixture: Fixture,
    directory: String,
}

impl NestedWorkspace {
    /// Builds a workspace at `directory` (one relative path, no trailing
    /// slash) wearing the checked-in `passthrough-plain` skeleton, which
    /// claims `plain.yml`: its manifest and lockfile are written.
    pub(crate) fn wearing_passthrough_plain(directory: &str) -> Result<Self, Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let extra = format!(
            "[dependencies]\n{}\n{}",
            path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
            wearing_table("passthrough-plain", ""),
        );
        write_package_manifest(fixture.root(), directory, "wearer", &extra)?;
        let nested = Self {
            fixture,
            directory: directory.to_owned(),
        };
        nested.generate_lockfile()?;
        Ok(nested)
    }

    /// The workspace directory `ritual` runs in.
    pub(crate) fn workspace(&self) -> PathBuf {
        self.fixture.root().join(&self.directory)
    }

    /// Writes `content` at `relative`, under the workspace directory.
    pub(crate) fn write(&self, relative: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        super::write(&self.workspace(), relative, content)
    }

    /// Reads the bytes at `relative`, under the workspace directory.
    pub(crate) fn read(&self, relative: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        super::read(&self.workspace(), relative)
    }

    /// Runs `ritual` with `arguments` in the workspace directory.
    pub(crate) fn run(&self, arguments: &[&str]) -> Result<Report, Box<dyn Error>> {
        run_ritual(&self.workspace(), self.fixture.sandbox(), arguments)
    }

    fn generate_lockfile(&self) -> Result<(), Box<dyn Error>> {
        let mut command = super::cargo_command();
        command
            .current_dir(self.workspace())
            .env("CARGO_HOME", self.fixture.sandbox().cargo_home())
            .env("HOME", self.fixture.sandbox().home())
            .env_remove("CARGO_TARGET_DIR")
            .arg("generate-lockfile");
        isolate_from_the_enclosing_repository(&mut command);
        require_success(command, "cargo generate-lockfile")?;
        Ok(())
    }
}

/// Makes git's cached stat data for each of `relative_paths` trustworthy, so
/// nothing `sync` runs afterwards re-hashes them.
///
/// A tracked file whose modification time is not older than the index's own
/// is "racily clean": git cannot trust its stat data, so `git status` runs
/// the file through its clean filter to compare content. A scenario whose
/// point is *when* a filter runs has to take that run out of the picture, or
/// the filter fires during the whole-tree status and the scenario proves
/// something other than what it says. Every path is given a modification
/// time long in the past and the index is refreshed, so the run under test
/// sees only what `sync` itself asks git to do.
pub(crate) fn settle_index(
    fixture: &Fixture,
    relative_paths: &[&str],
) -> Result<(), Box<dyn Error>> {
    // 2020-01-01T00:00:00Z: earlier than any index this suite writes.
    const LONG_AGO_SECONDS: u64 = 1_577_836_800;
    for relative in relative_paths {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.root().join(relative))?;
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(LONG_AGO_SECONDS))?;
    }
    git::run(
        fixture.root(),
        fixture.sandbox().home(),
        &["update-index", "--refresh"],
        "git update-index --refresh",
    )?;
    Ok(())
}

/// Asserts `report` is a refusal — not a success and not a crash — and that
/// its combined output contains every one of `needles`.
///
/// A panic exits 101, which is non-zero too, so "did not succeed" alone
/// cannot tell a refusal from `sync` falling over; this checks both, then
/// checks what the message says.
pub(crate) fn assert_refused_naming(report: &Report, needles: &[&str]) {
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert_ne!(
        report.exit_code, 0,
        "sync must refuse; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_ne!(
        report.exit_code, 101,
        "sync must refuse, not panic; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    for needle in needles {
        assert!(
            combined.contains(needle),
            "the refusal must name {needle:?}; combined output was: {combined:?}"
        );
    }
}

/// Whether any entry under `root` (never entering `.git`) is named like one
/// of `sync`'s own staging files (`.<name>.skeletons-sync`), compared without
/// regard to case so a spelling git or the filesystem folded still counts.
pub(crate) fn any_staging_file_remains(root: &Path) -> Result<bool, Box<dyn Error>> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == ".git" {
                continue;
            }
            if name.ends_with(".skeletons-sync") {
                return Ok(true);
            }
            if entry.file_type()?.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    Ok(false)
}
