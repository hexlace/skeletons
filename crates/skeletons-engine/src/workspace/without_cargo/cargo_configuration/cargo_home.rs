//! Where Cargo keeps its home: the one directory of configuration that is the user's rather than
//! the workspace's.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::workspace::without_cargo::file_system::CARGO_DIRECTORY_NAME;

/// The two environment variables that decide where the Cargo home is, as one value.
///
/// The reader takes them as a value rather than from the process wherever it can, because the
/// Cargo home is per-user state: a test that read the real environment would read the
/// configuration of whoever runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workspace::without_cargo) struct HomeVariables {
    /// `CARGO_HOME`, when it is set.
    cargo_home: Option<OsString>,
    /// `HOME`, when it is set.
    home: Option<OsString>,
}

impl HomeVariables {
    /// The variables of this process, which is where every caller outside a test reads them.
    pub(in crate::workspace::without_cargo) fn from_process() -> Self {
        Self {
            cargo_home: std::env::var_os("CARGO_HOME"),
            home: std::env::var_os("HOME"),
        }
    }

    /// Neither variable set, so that there is no Cargo home.
    #[cfg(test)]
    pub(in crate::workspace::without_cargo) const fn unset() -> Self {
        Self {
            cargo_home: None,
            home: None,
        }
    }

    /// `CARGO_HOME` set to `path`, and `HOME` not set.
    #[cfg(test)]
    pub(in crate::workspace::without_cargo) fn cargo_home(path: &Path) -> Self {
        Self {
            cargo_home: Some(path.as_os_str().to_owned()),
            home: None,
        }
    }

    /// `HOME` set to `path`, and `CARGO_HOME` not set.
    #[cfg(test)]
    pub(in crate::workspace::without_cargo) fn home(path: &Path) -> Self {
        Self {
            cargo_home: None,
            home: Some(path.as_os_str().to_owned()),
        }
    }
}

/// The Cargo home, found as Cargo finds it from `variables`, or `None` when neither variable
/// names one.
///
/// A `CARGO_HOME` that is not empty is the home, taken from `start` when it is relative, because
/// the Cargo that `check` runs starts in the directory it was given. Without one, the home is
/// `.cargo` in a `HOME` that is not empty. Cargo asks the system's user database when `HOME` is
/// not set either, and this reads the environment only, so there it finds none.
///
/// `start` is a real directory, as `std::fs::canonicalize` gives one.
pub(in crate::workspace::without_cargo) fn cargo_home_directory(
    variables: &HomeVariables,
    start: &Path,
) -> Option<PathBuf> {
    assert!(
        start.is_absolute(),
        "the start is the canonical form of a directory, which is absolute"
    );
    let cargo_home = non_empty(variables.cargo_home.as_deref());
    let home = non_empty(variables.home.as_deref());
    let directory = match (cargo_home, home) {
        // `join` keeps an absolute path as it is and puts a relative one under `start`.
        (Some(cargo_home), _) => start.join(cargo_home),
        (None, Some(home)) => start.join(home).join(CARGO_DIRECTORY_NAME),
        (None, None) => return None,
    };
    assert!(
        directory.is_absolute(),
        "a path joined onto an absolute start is absolute"
    );
    Some(directory)
}

/// `value` when it is set to something.
fn non_empty(value: Option<&OsStr>) -> Option<&OsStr> {
    value.filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{HomeVariables, cargo_home_directory};

    const START: &str = "/w/root";

    fn variables(cargo_home: Option<&str>, home: Option<&str>) -> HomeVariables {
        HomeVariables {
            cargo_home: cargo_home.map(OsString::from),
            home: home.map(OsString::from),
        }
    }

    fn directory(cargo_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        cargo_home_directory(&variables(cargo_home, home), Path::new(START))
    }

    #[test]
    fn an_absolute_cargo_home_is_the_home_as_it_is() {
        // `CARGO_HOME` wins over `HOME`, and an absolute one is not joined onto anything.
        assert_eq!(
            directory(Some("/elsewhere/cargo"), Some("/users/me")),
            Some(PathBuf::from("/elsewhere/cargo"))
        );
    }

    #[test]
    fn a_relative_cargo_home_is_taken_from_the_directory_cargo_starts_in() {
        // Cargo's `home` crate joins a relative `CARGO_HOME` onto its working directory, which
        // for the Cargo `check` runs is the directory it was given.
        assert_eq!(
            directory(Some("chome"), None),
            Some(PathBuf::from("/w/root/chome"))
        );
    }

    #[test]
    fn an_empty_cargo_home_falls_to_the_cargo_directory_in_home() {
        // Cargo filters an empty `CARGO_HOME` out as if it were not set.
        assert_eq!(
            directory(Some(""), Some("/users/me")),
            Some(PathBuf::from("/users/me/.cargo"))
        );
    }

    #[test]
    fn an_unset_cargo_home_falls_to_the_cargo_directory_in_home() {
        assert_eq!(
            directory(None, Some("/users/me")),
            Some(PathBuf::from("/users/me/.cargo"))
        );
    }

    #[test]
    fn a_relative_home_is_taken_from_the_directory_cargo_starts_in_too() {
        // The `.cargo` that `HOME` gives is read relative to Cargo's working directory when
        // `HOME` is relative.
        assert_eq!(
            directory(None, Some("me")),
            Some(PathBuf::from("/w/root/me/.cargo"))
        );
    }

    #[test]
    fn an_empty_or_unset_home_with_no_cargo_home_is_no_home() {
        assert_eq!(directory(None, None), None);
        assert_eq!(directory(None, Some("")), None);
        assert_eq!(directory(Some(""), None), None);
        assert_eq!(directory(Some(""), Some("")), None);
    }

    #[test]
    fn the_unset_variables_name_no_home() {
        assert_eq!(
            cargo_home_directory(&HomeVariables::unset(), Path::new(START)),
            None
        );
    }

    #[test]
    fn the_variables_a_test_builds_name_the_home_they_were_given() {
        assert_eq!(
            cargo_home_directory(
                &HomeVariables::cargo_home(Path::new("/c/home")),
                Path::new(START)
            ),
            Some(PathBuf::from("/c/home"))
        );
        assert_eq!(
            cargo_home_directory(&HomeVariables::home(Path::new("/h")), Path::new(START)),
            Some(PathBuf::from("/h/.cargo"))
        );
    }
}
