//! Running one git [`Command`] under bounds, the one way this crate does.
//!
//! Every git process this crate runs is built by [`super::command()`] and
//! then run here, so the output caps ([`GIT_OUTPUT_BYTES_MAX`]) are stated
//! once and a timeout is the only thing a caller chooses.

use std::process::Command;
use std::time::Duration;

use super::local_timeout;
use crate::subprocess::{self, Finished, GIT_OUTPUT_BYTES_MAX, Limits, SubprocessError};

/// Runs `command` under [`LOCAL_GIT_TIMEOUT`](super::LOCAL_GIT_TIMEOUT) —
/// the bound for a local, read-only git question, which a test build can
/// shorten (see [`super::local_timeout()`]) — and the shared output caps.
///
/// # Errors
///
/// Whatever [`subprocess::run`] reports: the process could not be started,
/// outlived its timeout, or could not be waited on. Each caller words that
/// failure in its own terms.
pub(crate) fn run_local(command: Command) -> Result<Finished, SubprocessError> {
    run_bounded(command, local_timeout())
}

/// Runs `command` under `timeout` and the shared output caps. Used
/// directly only for a git process that reaches a remote, which chooses its
/// own bound rather than taking
/// [`LOCAL_GIT_TIMEOUT`](super::LOCAL_GIT_TIMEOUT).
///
/// # Errors
///
/// As [`run_local`].
pub(crate) fn run_bounded(
    command: Command,
    timeout: Duration,
) -> Result<Finished, SubprocessError> {
    let limits = Limits {
        timeout: Some(timeout),
        stdout_bytes_max: GIT_OUTPUT_BYTES_MAX,
        stderr_bytes_max: GIT_OUTPUT_BYTES_MAX,
    };
    subprocess::run(command, &limits)
}
