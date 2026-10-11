//! Workspaces and skeletons for the tests, written fresh into a temporary
//! directory for each test.
//!
//! A test reaches the library only through its public API and builds every
//! input it reads. The two skeletons and the project that wears them are
//! written here as text, and the lockfile is made by the cargo that built the
//! tests. The cargo and git processes the fixture starts each get a
//! `CARGO_HOME` and `HOME` of its own, so they never read or write the cargo
//! cache of whoever runs the tests. The call under test is not one of them: it
//! runs cargo in the test process, with the environment that process inherited,
//! its real `CARGO_HOME` included, and the tests never change that environment.

// Every file under `tests/` is compiled as its own binary and includes this
// module afresh, and each one uses only the helpers its own tests need. A
// helper one binary does not reach is dead code from that binary's point of
// view and alive in a sibling. `allow`, not `expect`: an `expect` would itself
// warn in whichever binary happens to use everything.
#![allow(
    dead_code,
    reason = "helpers shared across separately compiled test binaries"
)]
// This module is private to each test binary, which has no external consumer.
// `pub(crate)` is the visibility that is true; nursery's `redundant_pub_crate`
// wants `pub`, which `unreachable_pub` refuses.
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is the true visibility in a private module with no external consumer"
)]

pub(crate) mod git;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use skeletons_library::{ClaimedFile, Files, Refusal, Report, SkeletonsNamed, WornSkeleton};
use tempfile::TempDir;

/// What every test returns. The error path carries only a failure to build an
/// input or to run a process; what a test verifies is in its assertions.
pub(crate) type TestOutcome = Result<(), Box<dyn Error>>;

/// Where `house-rules` claims its first file, relative to the workspace root.
pub(crate) const DEPENDABOT_PATH: &str = ".github/dependabot.yml";
/// What `house-rules` renders at [`DEPENDABOT_PATH`] when `cadence` is its
/// default, `weekly`.
pub(crate) const DEPENDABOT_RENDERED: &str = "interval: \"weekly\"\n";
/// Where `house-rules` claims its second file, which `other-rules` claims too.
pub(crate) const HOUSE_NOTES_PATH: &str = "notes/house.txt";
/// What `house-rules` renders at [`HOUSE_NOTES_PATH`].
pub(crate) const HOUSE_NOTES_RENDERED: &str = "house rules\n";
/// What `other-rules` renders at [`HOUSE_NOTES_PATH`].
pub(crate) const OTHER_AT_HOUSE_NOTES_RENDERED: &str = "other house text\n";
/// Where only `other-rules` claims a file.
pub(crate) const OTHER_NOTES_PATH: &str = "notes/other.txt";
/// What `other-rules` renders at [`OTHER_NOTES_PATH`].
pub(crate) const OTHER_NOTES_RENDERED: &str = "other notes\n";

/// A temporary directory holding two skeletons and a project, and the home
/// directories every child process is given.
///
/// ```text
/// <base>/home/                      HOME of every child process
/// <base>/cargo-home/                CARGO_HOME of every child process
/// <base>/skeletons/house-rules/     the skeleton, version 4.5.6, one enum option
/// <base>/skeletons/other-rules/     a second skeleton, version 1.2.3, no options
/// <base>/project/                   the workspace root, with member `tools`
/// ```
///
/// The skeletons sit beside the project, not inside it.
pub(crate) struct Fixture {
    base: TempDir,
}

impl Fixture {
    /// Writes both skeletons and a project whose one member, `tools`, wears
    /// nothing yet. No lockfile exists until [`Self::generate_lockfile`].
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let fixture = Self {
            base: tempfile::Builder::new()
                .prefix("skeletons-library-")
                .tempdir()?,
        };
        fs::create_dir_all(fixture.home())?;
        fs::create_dir_all(fixture.cargo_home())?;
        fixture.write_house_rules("4.5.6")?;
        fixture.write_other_rules()?;
        fixture.write_workspace_manifest(&["tools"])?;
        fixture.write_member("tools", "", "")?;
        Ok(fixture)
    }

    /// A fixture whose `tools` wears `house-rules` under the key `rules`, with
    /// its lockfile made.
    pub(crate) fn wearing_house_rules() -> Result<Self, Box<dyn Error>> {
        let fixture = Self::new()?;
        fixture.write_member(
            "tools",
            &fixture.rules_dependency(),
            &wearing_table("rules"),
        )?;
        fixture.generate_lockfile()?;
        Ok(fixture)
    }

    /// The workspace root the tests ask about.
    pub(crate) fn project(&self) -> PathBuf {
        self.base.path().join("project")
    }

    /// The directory a child process is given as `HOME`.
    pub(crate) fn home(&self) -> PathBuf {
        self.base.path().join("home")
    }

    /// The directory a child process is given as `CARGO_HOME`.
    pub(crate) fn cargo_home(&self) -> PathBuf {
        self.base.path().join("cargo-home")
    }

    /// The directory `house-rules` is written to.
    pub(crate) fn house_rules_directory(&self) -> PathBuf {
        self.base.path().join("skeletons").join("house-rules")
    }

    /// The directory `other-rules` is written to.
    pub(crate) fn other_rules_directory(&self) -> PathBuf {
        self.base.path().join("skeletons").join("other-rules")
    }

    /// The dependency line taking `house-rules` by path under the key `rules`.
    pub(crate) fn rules_dependency(&self) -> String {
        path_dependency("rules", "house-rules", &self.house_rules_directory())
    }

    /// The dependency line taking `other-rules` by path under the key `more`.
    pub(crate) fn more_dependency(&self) -> String {
        path_dependency("more", "other-rules", &self.other_rules_directory())
    }

    /// Writes `contents` at `relative` under the project, creating whatever
    /// directories lead there.
    pub(crate) fn write(&self, relative: &str, contents: &[u8]) -> Result<(), Box<dyn Error>> {
        write_file(&self.project().join(relative), contents)
    }

    /// Writes the workspace root's manifest, naming `members`.
    pub(crate) fn write_workspace_manifest(&self, members: &[&str]) -> Result<(), Box<dyn Error>> {
        let quoted: Vec<String> = members
            .iter()
            .map(|member| format!("\"{member}\""))
            .collect();
        let manifest = format!(
            "[workspace]\nmembers = [{}]\nresolver = \"3\"\n",
            quoted.join(", ")
        );
        self.write("Cargo.toml", manifest.as_bytes())
    }

    /// Writes the member `name`, with `dependencies` under `[dependencies]` and
    /// `rest` after them, both verbatim: whatever follows `[dependencies]`, such
    /// as `[build-dependencies]` or the wearing tables.
    pub(crate) fn write_member(
        &self,
        name: &str,
        dependencies: &str,
        rest: &str,
    ) -> Result<(), Box<dyn Error>> {
        let manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             publish = false\n\n[dependencies]\n{dependencies}\n{rest}"
        );
        self.write(&format!("{name}/Cargo.toml"), manifest.as_bytes())?;
        self.write(&format!("{name}/src/lib.rs"), b"")
    }

    /// Writes a crate `name` at `version` into the skeletons directory beside
    /// the project, with an empty library and `manifest_tail` after its package
    /// table verbatim, and returns the directory it was written to.
    pub(crate) fn write_crate(
        &self,
        name: &str,
        version: &str,
        manifest_tail: &str,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let directory = self.base.path().join("skeletons").join(name);
        write_file(
            &directory.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n\
                 publish = false\n\n{manifest_tail}"
            )
            .as_bytes(),
        )?;
        write_file(&directory.join("src/lib.rs"), b"")?;
        Ok(directory)
    }

    /// Makes `Cargo.lock` with the cargo that built the tests, offline: every
    /// source is a path.
    pub(crate) fn generate_lockfile(&self) -> Result<(), Box<dyn Error>> {
        self.cargo(&["generate-lockfile", "--offline"]).map(drop)
    }

    /// Runs the cargo that built the tests in the project, with a `CARGO_HOME`
    /// and `HOME` of its own, and fails unless it succeeds.
    pub(crate) fn cargo(&self, arguments: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(env!("CARGO"));
        command
            .current_dir(self.project())
            .env("CARGO_HOME", self.cargo_home())
            .env("HOME", self.home())
            .env_remove("CARGO_TARGET_DIR")
            .args(arguments);
        git::isolate(&mut command);
        require_success(command, "cargo")
    }

    /// Writes `house-rules` at `version`, replacing whatever was there.
    pub(crate) fn write_house_rules(&self, version: &str) -> Result<(), Box<dyn Error>> {
        let house = self.house_rules_directory();
        write_file(
            &house.join("Cargo.toml"),
            format!(
                "[package]\nname = \"house-rules\"\nversion = \"{version}\"\n\
                 edition = \"2021\"\npublish = false\n\n\
                 [package.metadata.skeletons.options.cadence]\ntype = \"enum\"\n\
                 values = [\"daily\", \"weekly\", \"monthly\"]\ndefault = \"weekly\"\n"
            )
            .as_bytes(),
        )?;
        write_file(&house.join("src/lib.rs"), b"")?;
        write_file(
            &house.join("files").join(DEPENDABOT_PATH),
            b"interval: \"{{cadence}}\"\n",
        )?;
        write_file(
            &house.join("files").join(HOUSE_NOTES_PATH),
            HOUSE_NOTES_RENDERED.as_bytes(),
        )
    }

    fn write_other_rules(&self) -> Result<(), Box<dyn Error>> {
        let other = self.other_rules_directory();
        write_file(
            &other.join("Cargo.toml"),
            b"[package]\nname = \"other-rules\"\nversion = \"1.2.3\"\nedition = \"2021\"\n\
              publish = false\n\n[package.metadata.skeletons]\n",
        )?;
        write_file(&other.join("src/lib.rs"), b"")?;
        write_file(
            &other.join("files").join(HOUSE_NOTES_PATH),
            OTHER_AT_HOUSE_NOTES_RENDERED.as_bytes(),
        )?;
        write_file(
            &other.join("files").join(OTHER_NOTES_PATH),
            OTHER_NOTES_RENDERED.as_bytes(),
        )
    }
}

/// The `[package.metadata.skeletons.<key>]` table that marks `key` as worn,
/// recording no option.
pub(crate) fn wearing_table(key: &str) -> String {
    format!("[package.metadata.skeletons.{key}]\n")
}

/// A dependency line taking `package` by path from `directory` under `key`.
pub(crate) fn path_dependency(key: &str, package: &str, directory: &Path) -> String {
    format!(
        "{key} = {{ package = \"{package}\", path = \"{}\" }}\n",
        directory.display()
    )
}

/// Writes `contents` at `path`, creating whatever directories lead there.
pub(crate) fn write_file(path: &Path, contents: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

/// Runs `command` and fails, with everything it printed, unless it succeeds.
pub(crate) fn require_success(mut command: Command, what: &str) -> Result<Output, Box<dyn Error>> {
    let output = command.output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "{what} failed ({status}); stdout: {stdout}; stderr: {stderr}",
            status = output.status,
            stdout = String::from_utf8_lossy(&output.stdout),
            stderr = String::from_utf8_lossy(&output.stderr),
        )
        .into())
    }
}

/// The one skeleton the report holds under `package`, or an error naming what
/// the lookup answered instead.
pub(crate) fn worn_once<'report>(
    report: &'report Report,
    package: &str,
) -> Result<&'report WornSkeleton, Box<dyn Error>> {
    match report.skeletons_named(package) {
        SkeletonsNamed::Worn([only]) => Ok(only),
        other => Err(format!("expected {package} worn once, the lookup answered {other:?}").into()),
    }
}

/// The files of a skeleton that was checked whole, or an error naming what
/// `files()` answered instead.
pub(crate) fn checked_files(skeleton: &WornSkeleton) -> Result<&[ClaimedFile], Box<dyn Error>> {
    match skeleton.files() {
        Files::Checked(files) => Ok(files),
        other => Err(format!(
            "expected {} to be Checked, files() answered {other:?}",
            skeleton.package()
        )
        .into()),
    }
}

/// Where a refusal puts a wearing: its manifest, its dependency key, and the
/// package its dependency names.
pub(crate) type WearingTriple<'refusal> =
    (&'refusal str, Option<&'refusal str>, Option<&'refusal str>);

/// The manifest, dependency key and package of each wearing a refusal names,
/// in order.
pub(crate) fn wearings_of(refusal: &Refusal) -> Vec<WearingTriple<'_>> {
    refusal
        .wearings()
        .iter()
        .map(|wearing| {
            (
                wearing.manifest().to_str().unwrap_or("?"),
                wearing.dependency_key(),
                wearing.package(),
            )
        })
        .collect()
}

/// The paths a refusal names, as text.
pub(crate) fn paths_of(refusal: &Refusal) -> Vec<&str> {
    refusal
        .paths()
        .iter()
        .map(|path| path.to_str().unwrap_or("?"))
        .collect()
}

/// The refusals and the compared files of a skeleton whose files were refused.
pub(crate) type RefusedFiles<'report> = (&'report [Refusal], &'report [ClaimedFile]);

/// The refusals and compared files of a skeleton whose files were refused, or
/// an error naming what `files()` answered instead.
pub(crate) fn refused(skeleton: &WornSkeleton) -> Result<RefusedFiles<'_>, Box<dyn Error>> {
    match skeleton.files() {
        Files::Refused {
            refusals, compared, ..
        } => Ok((refusals, compared)),
        other => Err(format!(
            "expected {} to be Refused, files() answered {other:?}",
            skeleton.dependency_key()
        )
        .into()),
    }
}

/// The refusals the lookup for `package` answered as `Undetermined`, or an
/// error naming what it answered instead.
pub(crate) fn undetermined<'report>(
    report: &'report Report,
    package: &str,
) -> Result<&'report [Refusal], Box<dyn Error>> {
    match report.skeletons_named(package) {
        SkeletonsNamed::Undetermined(refusals) => Ok(refusals),
        other => Err(format!("expected {package} to be Undetermined, found {other:?}").into()),
    }
}
