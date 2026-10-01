//! The `skeletons` bundle's process-level contract, exercised by running a command
//! line that mounts it.
//!
//! The bundle's unit test checks which children `skeletons::task()`
//! declares. Nothing there observes what a command line does with them once
//! the bundle is mounted: which command actually reaches `check`, what
//! reaches which stream, and with which exit status. Mounting the bundle
//! under some key other than `skeletons`, or flattening it into the top level,
//! leaves that test green.
//!
//! So these tests launch this repository's own command line, `ritual`, which
//! mounts the bundle under `skeletons` the way a project that wears skeletons does,
//! and read back the only three things a caller can see: the exit status
//! code, the bytes on stdout, and the bytes on stderr. Output that `skeletons`
//! decides is compared byte for byte. Output clap formats, the usage and
//! error text, is held only to what the mount decides about it: which
//! stream, which status, and which command path the usage line names.

mod support;

use support::{Report, Sandbox, TestOutcome};

/// The start of the usage line clap writes for the top level.
const TOP_LEVEL_USAGE: &str = "Usage: ritual <COMMAND>";

/// The start of the usage line clap writes for the `skeletons` bundle, which names
/// the key it is mounted under.
const SKELETONS_USAGE: &str = "Usage: ritual skeletons <COMMAND>";

/// The exit status of a command line that was itself wrong.
///
/// A literal, not imported: `rituals::run` keeps 1 for a task's own refusal
/// and 2 for an argument error clap raised, and a usage error must never be
/// reported with 1.
const USAGE_EXIT_CODE: i32 = 2;

/// Runs `ritual` with `arguments`, isolated the way every other test in this
/// directory is, from the directory the tests run in.
///
/// No fixture workspace is built: every invocation here is answered before
/// `ritual` reads anything from its working directory, so this file's runner
/// differs from [`support::Fixture::run`] only in having no workspace of its
/// own.
fn run_ritual(arguments: &[&str]) -> Result<Report, Box<dyn std::error::Error>> {
    let sandbox = Sandbox::new()?;
    let current_directory = std::env::current_dir()?;
    support::run_ritual(&current_directory, &sandbox, arguments)
}

/// Asserts that `report` is a usage error: status 2, nothing on stdout, and
/// on stderr a usage line that starts with `usage`.
fn assert_usage_error(report: &Report, usage: &str) {
    assert_eq!(
        report.exit_code, USAGE_EXIT_CODE,
        "a usage error exits {USAGE_EXIT_CODE}; stderr was: {}",
        report.stderr
    );
    assert_eq!(report.stdout, "", "a usage error writes nothing to stdout");
    assert!(
        report.stderr.lines().any(|line| line.starts_with(usage)),
        "expected a line starting {usage:?} on stderr; stderr was: {}",
        report.stderr
    );
}

#[test]
fn an_unknown_command_under_the_skeletons_key_is_a_usage_error() -> TestOutcome {
    // The bundle answers only the commands it declares; any other name under
    // its key is a usage error that names the bundle, not a run of anything.
    let report = run_ritual(&["skeletons", "nonexistent"])?;

    assert_usage_error(&report, SKELETONS_USAGE);
    Ok(())
}

#[test]
fn the_skeletons_bundle_is_not_flattened_into_the_top_level() -> TestOutcome {
    // Only the bundle mounted under the bin's own name is flattened, and
    // that is ritual's. A bare `ritual check` answering would mean the
    // `skeletons` bundle had been mounted under `ritual` beside it.
    let report = run_ritual(&["check"])?;

    assert_usage_error(&report, TOP_LEVEL_USAGE);
    Ok(())
}

#[test]
fn skeletons_alone_is_a_usage_error_naming_the_bundle() -> TestOutcome {
    // A bundle is a group, so naming it without a child asks for nothing.
    let report = run_ritual(&["skeletons"])?;

    assert_usage_error(&report, SKELETONS_USAGE);
    Ok(())
}

#[test]
fn check_rejects_an_unknown_argument() -> TestOutcome {
    // `check` takes no positional arguments, so one given is a usage error
    // like any other malformed invocation.
    let report = run_ritual(&["skeletons", "check", "extra"])?;

    assert_usage_error(&report, "Usage: ritual skeletons check");
    Ok(())
}

#[test]
fn ritual_management_tasks_are_at_the_top_level() -> TestOutcome {
    // ritual's own bundle is mounted under the bin's name, so it is
    // flattened: `cargo ritual regenerate` is how this repository keeps its
    // generated `src/main.rs` current, and contributing.md says so.
    let report = run_ritual(&["regenerate", "--help"])?;

    assert_eq!(report.exit_code, 0, "`ritual regenerate --help` exits 0");
    assert!(
        report.stdout.contains("Usage: ritual regenerate"),
        "expected regenerate at the top level; stdout was: {}",
        report.stdout
    );
    Ok(())
}
