//! Workspaces whose skeleton comes from a git repository or a registry, the
//! `Cargo.lock` entry of a package as the lockfile writes it, and the child
//! processes that run a test body under an environment the test chooses.
//!
//! A test reads the lockfile's text, never the library's view of it, so the
//! identity it expects of a locked package is what cargo wrote.

// Each test binary that includes this file uses only the helpers its own tests
// need, as `support` does, and `allow`, not `expect`, for the same reason.
#![allow(
    dead_code,
    reason = "helpers shared across separately compiled test binaries"
)]
// The module is private to each test binary, as `support` is.
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is the true visibility in a private module with no external consumer"
)]

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::support::{Fixture, TestOutcome, git, wearing_table, write_file};

/// One `[[package]]` entry of a `Cargo.lock`: the text of `name`, `version`
/// and `source` as written there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockEntry {
    /// The text after `name = `.
    pub(crate) name: String,
    /// The text after `version = `.
    pub(crate) version: String,
    /// The text after `source = `.
    pub(crate) source: String,
}

/// The `[[package]]` entry named `name` in the project's `Cargo.lock`, read
/// from the file's text, or an error saying what the file held instead.
pub(crate) fn lock_entry(project: &Path, name: &str) -> Result<LockEntry, Box<dyn Error>> {
    let text = fs::read_to_string(project.join("Cargo.lock"))?;
    let mut entries: Vec<[Option<String>; 3]> = Vec::new();
    for line in text.lines() {
        if line == "[[package]]" {
            entries.push([None, None, None]);
        } else if let Some(entry) = entries.last_mut() {
            for (index, key) in ["name", "version", "source"].into_iter().enumerate() {
                let prefix = format!("{key} = \"");
                if let Some(value) = line
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.strip_suffix('"'))
                {
                    entry[index] = Some(value.to_owned());
                }
            }
        }
    }
    let matching: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry[0].as_deref() == Some(name))
        .collect();
    match matching.as_slice() {
        [[Some(name), Some(version), Some(source)]] => Ok(LockEntry {
            name: name.clone(),
            version: version.clone(),
            source: source.clone(),
        }),
        other => Err(
            format!("expected one sourced entry for {name} in Cargo.lock, got {other:?}").into(),
        ),
    }
}

/// A fixture whose member `tools` wears `house-rules` taken from a git
/// repository by `file://` URL, with its lockfile made by a cargo whose home
/// has fetched it. The fixture's own cargo home is the one `check` needs.
pub(crate) fn wearing_from_git() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let url = git::commit_repository(&fixture.house_rules_directory(), &fixture.home())?;
    fixture.write_member(
        "tools",
        &format!("rules = {{ package = \"house-rules\", git = \"{url}\" }}\n"),
        &wearing_table("rules"),
    )?;
    fixture.cargo(&["generate-lockfile"])?;
    Ok(fixture)
}

/// The name of the crate [`wearing_from_registry`] vendors.
pub(crate) const REGISTRY_PACKAGE: &str = "registry-rules";
/// The version of the crate [`wearing_from_registry`] vendors.
pub(crate) const REGISTRY_VERSION: &str = "2.0.0";
/// The one file the vendored crate claims.
pub(crate) const REGISTRY_FILE: &str = "notes/registry.txt";
/// What the vendored crate renders at [`REGISTRY_FILE`].
pub(crate) const REGISTRY_RENDERED: &str = "registry rules\n";

/// A fixture whose member `tools` wears [`REGISTRY_PACKAGE`], taken from the
/// default registry, which the fixture's cargo home replaces with a directory
/// of vendored crates so that nothing is fetched. The package's lockfile source
/// is the registry's, as for any crate from crates.io. Returns the fixture and
/// the directory the crate was vendored to.
pub(crate) fn wearing_from_registry() -> Result<(Fixture, PathBuf), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let vendor = fixture.cargo_home().join("vendor");
    let directory = vendor.join(REGISTRY_PACKAGE);
    write_file(
        &directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{REGISTRY_PACKAGE}\"\nversion = \"{REGISTRY_VERSION}\"\n\
             edition = \"2021\"\npublish = false\n\n[package.metadata.skeletons]\n"
        )
        .as_bytes(),
    )?;
    write_file(&directory.join("src/lib.rs"), b"")?;
    write_file(
        &directory.join("files").join(REGISTRY_FILE),
        REGISTRY_RENDERED.as_bytes(),
    )?;
    write_file(
        &directory.join(".cargo-checksum.json"),
        format!("{{\"files\":{{}},\"package\":\"{}\"}}", "0".repeat(64)).as_bytes(),
    )?;
    write_file(
        &fixture.cargo_home().join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with = \"vendored\"\n\n\
             [source.vendored]\ndirectory = \"{}\"\n",
            vendor.display()
        )
        .as_bytes(),
    )?;
    fixture.write_member(
        "tools",
        &format!("{REGISTRY_PACKAGE} = \"{REGISTRY_VERSION}\"\n"),
        &wearing_table(REGISTRY_PACKAGE),
    )?;
    fixture.cargo(&["generate-lockfile"])?;
    Ok((fixture, directory))
}

/// Set, to anything, only in the process a parent test starts to run the body.
/// Its presence is what makes a run of the test the child.
const CHILD_MARKER: &str = "SKELETONS_LIBRARY_TEST_CHILD";

/// Whether this process is the child a parent test started.
pub(crate) fn is_child() -> bool {
    std::env::var_os(CHILD_MARKER).is_some()
}

/// A path the parent handed to the child under `name`, or an error saying it
/// was not handed one.
pub(crate) fn handed_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("the parent did not hand over {name}").into())
}

/// How the child's environment is arranged.
pub(crate) enum Environment<'directory> {
    /// `CARGO_HOME` and `HOME` are these directories; everything else is
    /// inherited.
    Homes {
        cargo_home: &'directory Path,
        home: &'directory Path,
    },
    /// `CARGO_HOME` and `HOME` are empty directories under `empty`, `PATH` is
    /// an empty directory too, and `CARGO` is removed: nothing on the machine
    /// can supply a cargo, its configuration or its cache.
    NoCargoAtAll { empty: &'directory Path },
}

/// Starts this test binary again to run only `test`, in `environment`, with
/// each of `handed` set for it, and requires that the child ran that one test
/// to a pass.
///
/// A child that matched no test exits 0, so success alone proves nothing: only
/// a body that ran and passed prints both lines the check looks for.
pub(crate) fn run_in_child(
    test: &str,
    environment: &Environment<'_>,
    handed: &[(&str, &OsStr)],
) -> TestOutcome {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--exact", test]).env(CHILD_MARKER, "1");
    git::isolate(&mut command);
    match environment {
        Environment::Homes { cargo_home, home } => {
            command.env("CARGO_HOME", cargo_home).env("HOME", home);
        }
        Environment::NoCargoAtAll { empty } => {
            let cargo_home = empty.join("cargo-home");
            let home = empty.join("home");
            let path = empty.join("path");
            for directory in [&cargo_home, &home, &path] {
                fs::create_dir_all(directory)?;
            }
            command
                .env("CARGO_HOME", cargo_home)
                .env("HOME", home)
                .env("PATH", path)
                .env_remove("CARGO");
        }
    }
    for (name, value) in handed {
        command.env(name, value);
    }
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success()
        && stdout.contains(&format!("test {test} ... ok"))
        && stdout.contains("1 passed")
    {
        Ok(())
    } else {
        Err(format!(
            "the child did not run {test} to a pass ({status}); stdout: {stdout}; \
             stderr: {stderr}",
            status = output.status,
        )
        .into())
    }
}
