//! The Cargo home that `check_without_cargo` reads the configuration of: found
//! from `CARGO_HOME` and, without it, from `HOME`, and refused when it asks for
//! something only cargo follows.
//!
//! The environment of a process cannot be changed from inside a test, so each
//! case runs in a child process born with the variables it needs, as the
//! other tests of `check_without_cargo` do. The oracle is a fixed value, the
//! refusal the library documents: `paths` in a configuration file is refused by
//! name, with the file in the detail. The files cargo itself reads for the same
//! directory are compared with cargo in the engine's tests.

#[path = "support/lock.rs"]
mod lock;
mod support;

use skeletons_library::{PackageLocations, check_without_cargo};
use support::{Fixture, TestOutcome, write_file};

/// What a configuration file that overrides a package writes. The directory it
/// names does not matter to a reader that refuses it by name.
const PATHS_OVERRIDE: &[u8] = b"paths = [\"../override\"]\n";

/// The variable under which the parent hands the child the configuration file
/// it wrote, whose path the refusal must name.
const CONFIGURATION: &str = "SKELETONS_LIBRARY_TEST_CONFIGURATION";

/// The variable under which the parent hands the child the project.
const PROJECT: &str = "SKELETONS_LIBRARY_TEST_PROJECT";

/// Requires, in the child, that `check_without_cargo` refuses the handed project
/// because of `paths` in the handed configuration file, and says so in the
/// words the library documents.
fn assert_refused_for_paths_in_the_handed_file() -> TestOutcome {
    let project = lock::handed_path(PROJECT)?;
    let configuration = lock::handed_path(CONFIGURATION)?;

    let Err(error) = check_without_cargo(&project, &PackageLocations::new()) else {
        return Err("check_without_cargo read a project whose cargo home overrides paths".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert_eq!(
        error.detail(),
        Some(format!("`paths` in {}", configuration.display()).as_str())
    );
    Ok(())
}

const CARGO_HOME_TEST: &str = "a_paths_override_in_the_cargo_home_is_refused";

#[test]
fn a_paths_override_in_the_cargo_home_is_refused() -> TestOutcome {
    // Given a project with a lockfile that is current, and a `CARGO_HOME` whose
    // `config.toml` writes `paths`, the child's `check_without_cargo` is
    // refused as something only cargo reads, and the detail names that file.
    // Without the Cargo home being read it would give a report.
    if lock::is_child() {
        return assert_refused_for_paths_in_the_handed_file();
    }
    let fixture = Fixture::wearing_house_rules()?;
    let configuration = fixture.cargo_home().join("config.toml");
    write_file(&configuration, PATHS_OVERRIDE)?;

    lock::run_in_child(
        CARGO_HOME_TEST,
        &lock::Environment::Homes {
            cargo_home: &fixture.cargo_home(),
            home: &fixture.home(),
        },
        &[
            (PROJECT, fixture.project().as_os_str()),
            (CONFIGURATION, configuration.as_os_str()),
        ],
    )
}

const HOME_TEST: &str = "a_paths_override_in_the_cargo_directory_of_home_is_refused";

#[test]
fn a_paths_override_in_the_cargo_directory_of_home_is_refused() -> TestOutcome {
    // The same with `CARGO_HOME` not set at all: cargo then keeps its home in
    // `.cargo` under `HOME`, so a `config.toml` there is the one read, and the
    // refusal names it.
    if lock::is_child() {
        return assert_refused_for_paths_in_the_handed_file();
    }
    let fixture = Fixture::wearing_house_rules()?;
    let configuration = fixture.home().join(".cargo").join("config.toml");
    write_file(&configuration, PATHS_OVERRIDE)?;

    lock::run_in_child(
        HOME_TEST,
        &lock::Environment::HomeOnly {
            home: &fixture.home(),
        },
        &[
            (PROJECT, fixture.project().as_os_str()),
            (CONFIGURATION, configuration.as_os_str()),
        ],
    )
}
