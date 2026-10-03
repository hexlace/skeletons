//! Running an external command with bounded output, and an optional bounded
//! wait — the one place `check`, `sync` and `wear` ever spawn a process
//! (`cargo metadata`, `cargo add` and `git`).
//!
//! A child's stdout and stderr are read from dedicated threads for as long
//! as the command runs, so neither pipe can fill up and block the child
//! while the caller is doing something else (`try_wait` polling, in
//! particular) — the classic subprocess deadlock this module exists to
//! avoid. Each stream is bounded independently: once its cap is crossed, the
//! rest of that stream is drained and discarded rather than held, and the
//! captured bytes report that they were cut short.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How often the wait loop polls [`Child::try_wait`] for a command with a
/// timeout. Small enough that a fast command is reported back promptly,
/// large enough that polling itself is not the loop's own hot spin.
const SUBPROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The most this crate ever reads from one `git` invocation's stdout or
/// stderr — large enough for any real repository's tag list or porcelain
/// status, small enough that a hostile or misconfigured repository or remote
/// cannot exhaust memory. Applied in one place, `git::run_bounded`, which
/// runs every git process this crate starts, local or remote — so the bound
/// is one number rather than the same value copied at each call site.
pub(crate) const GIT_OUTPUT_BYTES_MAX: u64 = 16 * 1024 * 1024;

/// How long `run` waits for a command, and how much of each output stream it
/// keeps.
pub(crate) struct Limits {
    /// `None` for a command with nothing bounding how long it may run —
    /// `cargo metadata`, which may be downloading locked sources over the
    /// network and has no fixed budget to finish within.
    pub(crate) timeout: Option<Duration>,
    pub(crate) stdout_bytes_max: u64,
    pub(crate) stderr_bytes_max: u64,
}

/// One output stream, captured up to its cap.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Captured {
    bytes: Vec<u8>,
    /// Whether the stream produced more than its cap allowed — the bytes
    /// beyond the cap were read and discarded, never held.
    truncated: bool,
    /// The cap this stream was read under — carried so [`Truncated`] can
    /// state it, without every caller of [`Finished::stdout`] having to
    /// thread its own cap back in just to build the message.
    cap_bytes: u64,
}

/// A stream ran past the cap this crate reads a subprocess's output under.
/// The bytes past the cap were read and discarded, never held — there is no
/// partial output to hand back, since a caller that could act on a
/// truncated stream anyway would first have to know, itself, that it was
/// truncated, and every caller here treats a cut stream as unreadable:
/// reading the workspace fails, `sync` refuses or aborts, and `behind`
/// reports the pin undetermined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Truncated {
    cap_bytes: u64,
}

impl std::fmt::Display for Truncated {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "printed more than {} MiB, the most `skeletons` reads",
            self.cap_bytes / (1024 * 1024)
        )
    }
}

impl std::error::Error for Truncated {}

impl Truncated {
    /// Builds a [`Truncated`] directly, at whatever cap a test wants to
    /// pretend a stream was read under — for a sibling module's own unit
    /// tests that need to hand a classifier an `Err(Truncated)` without
    /// spawning a process large enough to actually cross a real cap.
    #[cfg(test)]
    pub(crate) const fn for_test(cap_bytes: u64) -> Self {
        Self { cap_bytes }
    }
}

/// A command that ran to completion — whatever its own exit status — with
/// its two captured, bounded output streams.
#[derive(Debug)]
pub(crate) struct Finished {
    status: ExitStatus,
    stdout: Captured,
    stderr: Captured,
}

impl Finished {
    /// Builds a `Finished` directly: a command that exited with `code` and
    /// printed `stderr` and nothing else, for a sibling module's own unit
    /// tests that classify what a command said without spawning one.
    #[cfg(test)]
    pub(crate) fn for_test(code: i32, stderr: &[u8]) -> Self {
        use std::os::unix::process::ExitStatusExt;

        let captured = |bytes: &[u8]| Captured {
            bytes: bytes.to_vec(),
            truncated: false,
            cap_bytes: u64::MAX,
        };
        Self {
            // An exit code occupies the second byte of a wait status.
            status: ExitStatus::from_raw(code << 8),
            stdout: captured(b""),
            stderr: captured(stderr),
        }
    }

    pub(crate) fn success(&self) -> bool {
        self.status.success()
    }

    /// The command's own exit code, when it exited normally rather than
    /// being killed by a signal — `behind`'s git remote queries read this to
    /// tell `git ls-remote --exit-code`'s "ref not found" (2) apart from
    /// every other non-zero exit.
    pub(crate) fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// The whole of stdout, or [`Truncated`] when it ran past its own cap —
    /// the only way to read stdout, so a caller can never mistake a cut
    /// stream for a complete one: a caller cannot skip the check, because
    /// there is no other way to reach the bytes at all.
    pub(crate) fn stdout(&self) -> Result<&[u8], Truncated> {
        if self.stdout.truncated {
            Err(Truncated {
                cap_bytes: self.stdout.cap_bytes,
            })
        } else {
            Ok(&self.stdout.bytes)
        }
    }

    /// What stderr held up to its own cap — for a diagnostic only, never
    /// parsed as a complete stream: every caller here reads at most the
    /// first line or two of a git or cargo failure, so a cut tail changes
    /// nothing a caller here would have read anyway.
    pub(crate) fn stderr_head(&self) -> &[u8] {
        &self.stderr.bytes
    }
}

/// Why [`run`] could not produce a [`Finished`].
#[derive(Debug)]
pub(crate) struct SubprocessError {
    kind: SubprocessErrorKind,
}

#[derive(Debug)]
enum SubprocessErrorKind {
    /// The command could not even be spawned — the program does not exist,
    /// or the current user cannot execute it.
    Spawn(std::io::Error),
    /// The child was still running once [`Limits::timeout`] elapsed, and was
    /// killed.
    TimedOut { timeout: Duration },
}

impl SubprocessError {
    /// Whether this command was killed for running past its own timeout.
    ///
    /// The commands that write read it to say a git command timed out rather
    /// than could not be run (`work_tree::run_local`), since the two ask the wearer for
    /// different things. `behind`'s git remote queries do set
    /// [`Limits::timeout`], but they report a failed query by its `Display`
    /// text alone, which reads the same as any other reason a query could not
    /// be answered.
    pub(crate) const fn is_timed_out(&self) -> bool {
        matches!(self.kind, SubprocessErrorKind::TimedOut { .. })
    }

    /// The underlying I/O error, for the one variant that has one.
    pub(crate) const fn io_error(&self) -> Option<&std::io::Error> {
        match &self.kind {
            SubprocessErrorKind::Spawn(error) => Some(error),
            SubprocessErrorKind::TimedOut { .. } => None,
        }
    }
}

impl std::fmt::Display for SubprocessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            SubprocessErrorKind::Spawn(error) => write!(formatter, "failed to run: {error}"),
            SubprocessErrorKind::TimedOut { timeout } => {
                write!(formatter, "timed out after {timeout:?} and was killed")
            }
        }
    }
}

impl std::error::Error for SubprocessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.io_error()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

/// Spawns `command`, with its stdin closed, and waits for it under `limits`,
/// reading both of its output streams the whole time.
///
/// # Errors
///
/// Returns [`SubprocessError`] when the command cannot be spawned at all, or
/// when it is still running once [`Limits::timeout`] elapses — in which case
/// it is killed before this function returns.
pub(crate) fn run(mut command: Command, limits: &Limits) -> Result<Finished, SubprocessError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| SubprocessError {
        kind: SubprocessErrorKind::Spawn(error),
    })?;

    // Postcondition of `Stdio::piped()` above: both handles always exist.
    let (Some(mut stdout_pipe), Some(mut stderr_pipe)) = (child.stdout.take(), child.stderr.take())
    else {
        unreachable!("both streams were just requested as `Stdio::piped()`")
    };
    let stdout_cap = limits.stdout_bytes_max;
    let stderr_cap = limits.stderr_bytes_max;
    let stdout_reader = std::thread::spawn(move || read_bounded(&mut stdout_pipe, stdout_cap));
    let stderr_reader = std::thread::spawn(move || read_bounded(&mut stderr_pipe, stderr_cap));

    let status = wait_bounded(&mut child, limits.timeout)?;

    // `read_bounded` never panics, so a reader thread's `join` only ever
    // fails by propagating whatever it itself resumed — there is nothing
    // sensible to report about the failure beyond that panic itself.
    let stdout = match stdout_reader.join() {
        Ok(captured) => captured,
        Err(payload) => std::panic::resume_unwind(payload),
    };
    let stderr = match stderr_reader.join() {
        Ok(captured) => captured,
        Err(payload) => std::panic::resume_unwind(payload),
    };
    Ok(Finished {
        status,
        stdout,
        stderr,
    })
}

/// Waits for `child` to finish, killing it and reporting a timeout once
/// `timeout` (when given) elapses.
///
/// Polls rather than blocking in [`Child::wait`] so a timeout can actually
/// be enforced — `wait` itself has no bounded form.
fn wait_bounded(
    child: &mut Child,
    timeout: Option<Duration>,
) -> Result<ExitStatus, SubprocessError> {
    let started = Instant::now();
    loop {
        // A `try_wait` that itself errors (checked here on every poll) is an
        // environment failure indistinguishable from "cannot run the
        // program at all" to the caller — reported the same way.
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                return Err(SubprocessError {
                    kind: SubprocessErrorKind::Spawn(error),
                });
            }
        }
        let timed_out = timeout.is_some_and(|timeout| started.elapsed() >= timeout);
        if timed_out {
            // Postcondition of `timed_out`: `timeout` is `Some` whenever
            // this arm is reached.
            let Some(timeout) = timeout else {
                unreachable!("timed_out is only true when timeout is Some")
            };
            // Best-effort: a child that has already exited between the
            // `try_wait` above and here needs no killing, and a `kill` that
            // fails leaves nothing further this function can do about it.
            let _unused = child.kill();
            let _unused = child.wait();
            return Err(SubprocessError {
                kind: SubprocessErrorKind::TimedOut { timeout },
            });
        }
        std::thread::sleep(SUBPROCESS_POLL_INTERVAL);
    }
}

/// The size, in bytes, of each read from a child's pipe — large enough that
/// a normal-sized `cargo metadata` response is read in a handful of calls,
/// small enough that a single read never holds much more than the cap it is
/// about to be checked against.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Reads `reader` to the end (or until the child closes its end), keeping at
/// most `cap_bytes` and discarding the rest.
fn read_bounded(reader: &mut impl Read, cap_bytes: u64) -> Captured {
    let cap = usize::try_from(cap_bytes).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    // Heap-allocated rather than a stack array: `READ_CHUNK_BYTES` is larger
    // than clippy's own stack-array threshold, and this buffer is reused for
    // the whole stream's lifetime regardless.
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];

    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => {
                return Captured {
                    bytes,
                    truncated: false,
                    cap_bytes,
                };
            }
            Ok(read) => read,
            // A broken pipe (the child exited) ends the stream the same as
            // a clean EOF would; whatever was read up to here is final.
            Err(_broken_pipe) => {
                return Captured {
                    bytes,
                    truncated: false,
                    cap_bytes,
                };
            }
        };
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > cap {
            // The cap is already crossed. Draining the remainder without
            // keeping it both bounds this function's own memory and lets
            // the child finish writing instead of blocking on a full pipe
            // nobody is reading from any more.
            let mut sink = std::io::sink();
            let _unused = std::io::copy(reader, &mut sink);
            bytes.truncate(cap);
            return Captured {
                bytes,
                truncated: true,
                cap_bytes,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::{Limits, run};

    /// No bound at all: a fast command's own bytes and exit status are read
    /// back exactly, on both streams.
    fn unbounded_limits() -> Limits {
        Limits {
            timeout: None,
            stdout_bytes_max: u64::MAX,
            stderr_bytes_max: u64::MAX,
        }
    }

    #[test]
    fn a_successful_commands_stdout_is_captured_in_full() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf hello"]);
        let finished = run(command, &unbounded_limits()).expect("sh must run");
        assert!(finished.success());
        assert_eq!(
            finished.stdout().expect("stdout must not be truncated"),
            b"hello"
        );
    }

    #[test]
    fn a_failing_commands_exit_status_and_stderr_are_both_reported() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf oops 1>&2; exit 3"]);
        let finished = run(command, &unbounded_limits()).expect("sh must run");
        assert!(!finished.success());
        assert_eq!(finished.stderr_head(), b"oops");
    }

    #[test]
    fn stdout_and_stderr_are_both_drained_even_when_only_one_is_read_here() {
        // A command that writes enough to each stream to fill an OS pipe
        // buffer, on both streams at once, before it exits — the shape that
        // deadlocks a caller which reads one stream to completion before
        // starting the other. `run` reads both concurrently from the start,
        // so this must complete rather than hang.
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "dd if=/dev/zero bs=200000 count=1 2>/dev/null; \
             dd if=/dev/zero bs=200000 count=1 1>&2 2>/dev/null",
        ]);
        let finished = run(command, &unbounded_limits()).expect("sh must run");
        assert!(finished.success());
        assert_eq!(
            finished
                .stdout()
                .expect("stdout must not be truncated")
                .len(),
            200_000
        );
        assert_eq!(finished.stderr_head().len(), 200_000);
    }

    #[test]
    fn output_past_the_cap_is_unreadable_as_complete() {
        // The mechanism `subprocess.rs`'s own module doc and
        // `Finished::stdout`'s own doc comment name: a caller cannot read a
        // cut stream as though it were whole, because `stdout` has no other
        // way to reach the bytes at all once the cap is crossed.
        let mut command = Command::new("sh");
        command.args(["-c", "printf 0123456789"]);
        let limits = Limits {
            timeout: None,
            stdout_bytes_max: 4,
            stderr_bytes_max: u64::MAX,
        };
        let finished = run(command, &limits).expect("sh must run");
        finished
            .stdout()
            .expect_err("stdout past its cap must be unreadable as complete");
    }

    #[test]
    fn a_truncated_streams_display_states_the_cap_in_mebibytes() {
        // Pure: hand-built at a cap this test controls, so the message is
        // pinned without needing a command whose real output happens to
        // cross a MiB-aligned cap.
        let truncated = super::Truncated {
            cap_bytes: 2 * 1024 * 1024,
        };
        assert_eq!(
            truncated.to_string(),
            "printed more than 2 MiB, the most `skeletons` reads"
        );
    }

    #[test]
    fn a_command_that_never_exits_is_killed_once_its_timeout_elapses() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 60"]);
        let limits = Limits {
            timeout: Some(Duration::from_millis(100)),
            stdout_bytes_max: u64::MAX,
            stderr_bytes_max: u64::MAX,
        };
        let error = run(command, &limits).expect_err("a command past its timeout must be killed");
        assert!(error.is_timed_out());
    }

    #[test]
    fn a_program_that_does_not_exist_is_a_spawn_error_not_a_timeout() {
        let command = Command::new("skeletons-check-nonexistent-program-xyz");
        let error =
            run(command, &unbounded_limits()).expect_err("a missing program must fail to spawn");
        assert!(!error.is_timed_out());
        assert!(error.io_error().is_some());
    }
}
