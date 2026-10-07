//! The directory a task was run from, which every task starts by reading.

use std::io;
use std::path::PathBuf;

use rituals::Failure;

/// Returns the directory the process is running in.
///
/// # Errors
///
/// Returns a [`Failure`] when the operating system cannot say, for instance
/// because the directory was removed under the running process. The operating
/// system's error is the failure's cause, so the refusal line ritual prints
/// shows it once, after the message.
pub(crate) fn read() -> Result<PathBuf, Failure> {
    std::env::current_dir().map_err(unreadable)
}

/// The refusal for a working directory the operating system could not read.
///
/// The message names the situation only; `error` rides along as the cause and
/// is what ritual appends when it prints the refusal, so putting it in the
/// message as well would print it twice.
fn unreadable(error: io::Error) -> Failure {
    Failure::new("could not read the current working directory").caused_by(error)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::unreadable;

    #[test]
    fn an_unreadable_directory_is_refused_with_its_cause_printed_once() {
        // Hands `unreadable` the error the operating system gives for a
        // working directory that no longer exists (errno 2, whose text std
        // fixes on Linux and macOS), then renders the failure as ritual's
        // dispatch prints it after `<bin>: `, which is `with_causes()`. The
        // line must hold the message and the cause exactly once each: a
        // message that also quoted the error would print the cause twice.
        let failure = unreadable(io::Error::from_raw_os_error(2));

        assert_eq!(
            failure.with_causes().to_string(),
            "could not read the current working directory: No such file or directory (os error 2)"
        );
    }
}
