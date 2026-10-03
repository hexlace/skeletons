//! Acceptance: when a table `wear` writes its wearing table under is not a
//! table, `wear` refuses before it runs `cargo add`, so a refusal that was
//! knowable from the manifest costs no `cargo add` and has nothing to undo.
//!
//! The wearing table lives at `[package.metadata.skeletons.<key>]`, so three
//! parents lead to it: `package`, `package.metadata` and
//! `package.metadata.skeletons`. Cargo accepts a `metadata` or a `skeletons`
//! that is not a table (a package's `metadata` is free-form), and `wear` has
//! to refuse it for itself. Cargo does not accept a `package` that is not a
//! table: it refuses the manifest before `wear` can read it, which is a
//! different refusal, pinned here for what it is.
//!
//! A TOML datetime is not a table either, though `cargo metadata` reports one
//! as a JSON object with the single key `$__toml_private_datetime`. A
//! `metadata` or a `skeletons` that is a datetime is refused exactly as one
//! that is an integer is.
//!
//! Each scenario puts a recording `cargo` in front of the real one, through
//! the `CARGO` variable the command line runs cargo by, and reads back which
//! cargo subcommands ran. The recording is proved to see calls by a scenario
//! that wears a skeleton successfully and must record an `add`.

mod support;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use support::wear::{
    ManifestAndLockfile, assert_nothing_was_undone, assert_refused_and_untouched,
    commit_everything, fixture_for_wearing, the_single_refusal_line,
};
use support::{Fixture, Report, TemporaryDirectory, TestOutcome, checked_in_test_skeleton};

/// The variable the recording script reads the real cargo's path from.
const REAL_CARGO_VARIABLE: &str = "RECORDING_CARGO_REAL";
/// The variable the recording script reads its log's path from.
const LOG_VARIABLE: &str = "RECORDING_CARGO_LOG";

/// A `cargo` that appends its arguments, one invocation per line, to a log,
/// then runs the real cargo with them.
struct RecordingCargo {
    // Holds the script and the log; removed when this is dropped.
    directory: TemporaryDirectory,
}

impl RecordingCargo {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TemporaryDirectory::new("recording-cargo")?;
        let script = directory.path().join("cargo");
        // Deviation from RS-SINGLE-TOOLCHAIN (integration tests use
        // `std::process::Command`, not shell scripts): a stand-in for `cargo`
        // has to be an executable file at a path `CARGO` can name, and a
        // two-line POSIX `sh` that records and `exec`s the real cargo is the
        // narrowest one. A Rust stand-in would need a binary target added to
        // the crate for this one test.
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"${LOG_VARIABLE}\"\n\
                 exec \"${REAL_CARGO_VARIABLE}\" \"$@\"\n"
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        Ok(Self { directory })
    }

    fn script(&self) -> PathBuf {
        self.directory.path().join("cargo")
    }

    fn log(&self) -> PathBuf {
        self.directory.path().join("invocations.log")
    }

    /// Runs `wear` of `passthrough-plain` in `fixture` with this recording
    /// cargo standing in for cargo.
    fn wear_passthrough_plain(
        &self,
        fixture: &Fixture,
    ) -> Result<Report, Box<dyn std::error::Error>> {
        let skeleton = checked_in_test_skeleton("passthrough-plain");
        let skeleton = skeleton.to_str().ok_or("path must be UTF-8")?;
        let script = self.script();
        let log = self.log();
        fixture.run_with_env(
            &["skeletons", "wear", "passthrough-plain", "--path", skeleton],
            &[
                ("CARGO", path_text(&script)?),
                (REAL_CARGO_VARIABLE, env!("CARGO")),
                (LOG_VARIABLE, path_text(&log)?),
            ],
        )
    }

    /// The first argument of every cargo invocation recorded, in order.
    ///
    /// A log that does not exist means the recording script never ran, so
    /// cargo never ran: the list is empty. Any other failure to read the log
    /// is an error, so it cannot pass for "nothing ran".
    fn subcommands(&self) -> Result<Vec<String>, std::io::Error> {
        let recorded = match std::fs::read_to_string(self.log()) {
            Ok(recorded) => recorded,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        Ok(recorded
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect())
    }
}

fn path_text(path: &Path) -> Result<&str, Box<dyn std::error::Error>> {
    Ok(path.to_str().ok_or("path must be UTF-8")?)
}

fn not_a_table_line(header: &str) -> String {
    format!(
        "[{header}] in Cargo.toml is not a table, so wear cannot add a wearing table under it; \
         make it a table, then run the `wear` task again"
    )
}

/// Asserts the recording saw cargo run at all (so an empty log means "no
/// `add`", not "not recording") and that none of its runs was an `add`.
fn assert_cargo_ran_but_never_added(recording: &RecordingCargo) -> TestOutcome {
    let subcommands = recording.subcommands()?;
    assert!(
        subcommands
            .iter()
            .any(|subcommand| subcommand == "metadata"),
        "precondition: the recording must have seen wear read the project; it saw {subcommands:?}"
    );
    assert!(
        !subcommands.iter().any(|subcommand| subcommand == "add"),
        "wear must refuse before running `cargo add`; cargo ran: {subcommands:?}"
    );
    Ok(())
}

#[test]
fn a_recording_cargo_sees_the_add_of_a_wear_that_goes_ahead() -> TestOutcome {
    // The control for the scenarios below: with nothing wrong in the
    // manifest, `wear` runs `cargo add`, and the recording must say so.
    let fixture = fixture_for_wearing("")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_eq!(report.exit_code, 0, "stderr: {}", report.stderr);
    let subcommands = recording.subcommands()?;
    assert!(
        subcommands.iter().any(|subcommand| subcommand == "add"),
        "the recording must see `cargo add`; it saw {subcommands:?}"
    );
    Ok(())
}

#[test]
fn a_metadata_that_is_not_a_table_is_refused_before_cargo_add() -> TestOutcome {
    // `[package] metadata = 1`: Cargo accepts it and reports the package's
    // metadata as `1`, so no wearing table can go under it.
    let fixture = fixture_for_wearing("metadata = 1")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_cargo_ran_but_never_added(&recording)?;
    assert_refused_and_untouched(
        &fixture,
        &report,
        &not_a_table_line("package.metadata"),
        &before,
        "Cargo.toml",
    )
}

#[test]
fn a_skeletons_that_is_not_a_table_is_refused_before_cargo_add() -> TestOutcome {
    // `[package.metadata] skeletons = 1`: refused before `cargo add` already;
    // pinned alongside its siblings so the whole class is held together.
    let fixture = fixture_for_wearing("[package.metadata]\nskeletons = 1")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_cargo_ran_but_never_added(&recording)?;
    assert_refused_and_untouched(
        &fixture,
        &report,
        &not_a_table_line("package.metadata.skeletons"),
        &before,
        "Cargo.toml",
    )
}

#[test]
fn a_metadata_that_is_a_datetime_is_refused_before_cargo_add() -> TestOutcome {
    // `[package] metadata = 1979-05-27T07:32:00Z`: Cargo accepts it, and
    // `cargo metadata` reports it as the object
    // `{"$__toml_private_datetime": "1979-05-27T07:32:00Z"}`. That object is
    // how a datetime is spelled in the JSON, not a table the manifest wrote,
    // so no wearing table can go under it.
    let fixture = fixture_for_wearing("metadata = 1979-05-27T07:32:00Z")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_cargo_ran_but_never_added(&recording)?;
    assert_refused_and_untouched(
        &fixture,
        &report,
        &not_a_table_line("package.metadata"),
        &before,
        "Cargo.toml",
    )
}

#[test]
fn a_skeletons_that_is_a_datetime_is_refused_before_cargo_add() -> TestOutcome {
    // `[package.metadata] skeletons = 1979-05-27`: Cargo accepts it, and
    // `cargo metadata` reports the package's metadata as
    // `{"skeletons": {"$__toml_private_datetime": "1979-05-27"}}`. The inner
    // object is a datetime's JSON spelling, not a table, so no wearing table
    // can go under `skeletons`.
    let fixture = fixture_for_wearing("[package.metadata]\nskeletons = 1979-05-27")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_cargo_ran_but_never_added(&recording)?;
    assert_refused_and_untouched(
        &fixture,
        &report,
        &not_a_table_line("package.metadata.skeletons"),
        &before,
        "Cargo.toml",
    )
}

#[test]
fn a_package_that_is_not_a_table_is_refused_by_cargo_before_cargo_add() -> TestOutcome {
    // `package = 1` is not a manifest Cargo will read, so `wear` never sees
    // a `package` that is not a table: Cargo's own refusal of the manifest
    // comes first, and it is that refusal, not `ParentNotATable`, that
    // `wear` reports.
    let fixture = fixture_for_wearing("")?;
    fixture.write("Cargo.toml", b"package = 1\n")?;
    commit_everything(&fixture, "a manifest whose package is not a table")?;
    let before = ManifestAndLockfile::read(&fixture, "Cargo.toml")?;
    let recording = RecordingCargo::new()?;

    let report = recording.wear_passthrough_plain(&fixture)?;

    assert_ne!(report.exit_code, 0, "stdout: {}", report.stdout);
    let line = the_single_refusal_line(&report);
    assert!(
        line.starts_with("cargo metadata --locked failed: "),
        "the refusal must say the read of the project failed; it was: {line}"
    );
    assert!(
        line.contains("invalid type: integer `1`, expected struct TomlPackage"),
        "the refusal must carry Cargo's own reason; it was: {line}"
    );
    assert!(
        !line.contains("is not a table, so wear cannot add"),
        "this is Cargo's refusal, not wear's table one: {line}"
    );
    assert_nothing_was_undone(&report);
    assert_eq!(
        ManifestAndLockfile::read(&fixture, "Cargo.toml")?,
        before,
        "a refused wear must leave the manifest and Cargo.lock byte-identical"
    );
    assert_cargo_ran_but_never_added(&recording)
}
