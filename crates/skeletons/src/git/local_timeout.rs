//! How long a local git question waits, and the one seam that shortens it for
//! a test.
//!
//! The wait is [`LOCAL_GIT_TIMEOUT`] everywhere, except in a test build that
//! sets `SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS`: a test that needs a
//! git command to time out would otherwise wait out the whole production
//! bound. Outside a test build the variable is not read at all.

use std::time::Duration;

use super::LOCAL_GIT_TIMEOUT;

/// The environment variable that overrides [`LOCAL_GIT_TIMEOUT`], in whole
/// seconds, in a test build. It exists only where the seam does: this crate's
/// own tests, and a dependent's that enable `test-util`.
#[cfg(any(test, feature = "test-util"))]
const LOCAL_GIT_TIMEOUT_VARIABLE: &str = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS";

/// The wait a local git question gets: [`LOCAL_GIT_TIMEOUT`], unless the
/// test build's environment overrides it.
///
/// # Panics
///
/// If `SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS` is set to anything but a
/// whole number of seconds above zero.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn local_timeout() -> Duration {
    timeout_override(std::env::var_os(LOCAL_GIT_TIMEOUT_VARIABLE).as_deref())
        .unwrap_or(LOCAL_GIT_TIMEOUT)
}

/// The wait a local git question gets: [`LOCAL_GIT_TIMEOUT`], always, since a
/// production build has no seam to override it.
#[cfg(not(any(test, feature = "test-util")))]
pub(crate) const fn local_timeout() -> Duration {
    LOCAL_GIT_TIMEOUT
}

/// The override `value` asks for, or `None` when the variable is unset.
///
/// A variable that is set must hold a whole number of seconds above zero.
/// Anything else is a mistake in the test that set it, and reading it as
/// "unset" would leave that test waiting out the production bound with
/// nothing to say why, while a zero would time out every git command
/// instantly. It stops the test with the variable's name instead.
///
/// # Panics
///
/// If `value` is set and is not a whole number of seconds above zero.
#[cfg(any(test, feature = "test-util"))]
fn timeout_override(value: Option<&std::ffi::OsStr>) -> Option<Duration> {
    let value = value?;
    let seconds = value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(
        seconds > 0,
        "{LOCAL_GIT_TIMEOUT_VARIABLE} must be a whole number of seconds above zero, but it is \
         {value:?}"
    );
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::time::Duration;

    use super::{LOCAL_GIT_TIMEOUT, timeout_override};

    #[test]
    fn an_unset_variable_leaves_the_production_bound() {
        // With nothing to read, no override is asked for, and
        // `local_timeout` falls through to the constant.
        assert_eq!(timeout_override(None), None);
        assert_eq!(LOCAL_GIT_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn a_whole_number_of_seconds_is_the_wait() {
        // A short wait is the point of the seam, and the largest one an
        // env var can carry must not overflow into something shorter.
        for (text, expected) in [
            ("1", Duration::from_secs(1)),
            ("2", Duration::from_secs(2)),
            ("30", Duration::from_secs(30)),
            ("007", Duration::from_secs(7)),
            (
                "18446744073709551615",
                Duration::from_secs(18_446_744_073_709_551_615),
            ),
        ] {
            assert_eq!(
                timeout_override(Some(OsStr::new(text))),
                Some(expected),
                "{text:?}"
            );
        }
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn zero_seconds_is_refused_rather_than_timing_out_every_command() {
        let _ = timeout_override(Some(OsStr::new("0")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn an_empty_value_is_refused_rather_than_read_as_unset() {
        let _ = timeout_override(Some(OsStr::new("")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn a_fraction_is_refused() {
        let _ = timeout_override(Some(OsStr::new("1.5")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn a_negative_number_is_refused() {
        let _ = timeout_override(Some(OsStr::new("-1")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn text_that_is_not_a_number_is_refused() {
        let _ = timeout_override(Some(OsStr::new("soon")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn a_number_too_large_for_seconds_is_refused() {
        let _ = timeout_override(Some(OsStr::new("18446744073709551616")));
    }

    #[test]
    #[should_panic(
        expected = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS must be a whole number"
    )]
    fn a_value_that_is_not_utf_8_is_refused() {
        use std::os::unix::ffi::OsStrExt as _;
        let _ = timeout_override(Some(OsStr::from_bytes(&[0xff, 0xfe])));
    }
}
