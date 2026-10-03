//! Running an external command with bounded output, and an optional bounded
//! wait — the one place `check`, `sync` and `wear` ever spawn a process
//! (`cargo metadata`, `cargo add` and `git`).
//!
//! A child's stdout and stderr are read from dedicated threads for as long
//! as the command runs, so neither pipe can fill up and block the child
//! while the caller is doing something else — blocked in [`Child::wait`], or
//! polling [`Child::try_wait`] under a timeout — the classic subprocess
//! deadlock this module exists to avoid. Each stream is bounded
//! independently: once its cap is crossed, the rest of that stream is drained
//! and discarded rather than held, and the captured bytes report that they
//! were cut short.

pub(crate) mod clock;
#[cfg(test)]
pub(crate) mod gate;

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

#[cfg(test)]
pub(crate) use clock::TestClock;
use clock::{Clock, SystemClock};

/// The longest [`wait_until_timeout`] pauses between two polls of
/// [`Child::try_wait`]. A command that runs this long is not about to finish,
/// so polling it faster would only be the loop's own hot spin.
const SUBPROCESS_POLL_INTERVAL_MAX: Duration = Duration::from_millis(50);

/// The first pause [`wait_until_timeout`] takes between polls. A command that
/// exits within about a millisecond is reported back that promptly, and
/// because each pause doubles until [`SUBPROCESS_POLL_INTERVAL_MAX`], a slow
/// one is still polled only a few times a second.
const SUBPROCESS_POLL_INTERVAL_MIN: Duration = Duration::from_millis(1);

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
    /// network and has no fixed budget to finish within. Such a command is
    /// waited on with a blocking [`Child::wait`], never polled.
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
pub(crate) fn run(command: Command, limits: &Limits) -> Result<Finished, SubprocessError> {
    run_with_clock(command, limits, &SystemClock)
}

/// Runs `command` like [`run`], measuring [`Limits::timeout`] against `clock`
/// instead of the wall clock.
///
/// A unit test can thereby drive a command past its timeout without any real
/// time passing.
///
/// # Errors
///
/// The same as [`run`].
pub(crate) fn run_with_clock(
    mut command: Command,
    limits: &Limits,
    clock: &impl Clock,
) -> Result<Finished, SubprocessError> {
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

    let status = wait_bounded(&mut child, limits.timeout, clock)?;

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
/// `timeout` (when given) elapses on `clock`.
///
/// With no timeout there is nothing to measure, so the wait blocks in
/// [`wait_for_exit`] and never touches `clock`. With one, [`wait_until_timeout`]
/// polls, because [`Child::wait`] has no bounded form.
fn wait_bounded(
    child: &mut Child,
    timeout: Option<Duration>,
    clock: &impl Clock,
) -> Result<ExitStatus, SubprocessError> {
    match timeout {
        None => wait_for_exit(child),
        Some(timeout) => wait_until_timeout(child, timeout, clock),
    }
}

/// Blocks in [`Child::wait`] until `child` exits.
///
/// A `wait` that itself errors is an environment failure indistinguishable
/// from "cannot run the program at all" to the caller, so it is reported the
/// same way.
fn wait_for_exit(child: &mut Child) -> Result<ExitStatus, SubprocessError> {
    child.wait().map_err(|error| SubprocessError {
        kind: SubprocessErrorKind::Spawn(error),
    })
}

/// Returns the poll step that follows `step`: double it, up to
/// [`SUBPROCESS_POLL_INTERVAL_MAX`].
fn poll_step_after(step: Duration) -> Duration {
    assert!(
        step >= SUBPROCESS_POLL_INTERVAL_MIN,
        "a poll step below the minimum: {step:?}"
    );
    assert!(
        step <= SUBPROCESS_POLL_INTERVAL_MAX,
        "a poll step above the cap: {step:?}"
    );
    let next = step.saturating_mul(2).min(SUBPROCESS_POLL_INTERVAL_MAX);
    assert!(
        next >= step,
        "the poll step shrank from {step:?} to {next:?}"
    );
    assert!(
        next <= SUBPROCESS_POLL_INTERVAL_MAX,
        "the poll step passed the cap: {next:?}"
    );
    next
}

/// Returns how long to pause after a poll: `step`, cut to `remaining` so the
/// pause never carries the wait past the timeout.
fn poll_pause(step: Duration, remaining: Duration) -> Duration {
    assert!(
        remaining > Duration::ZERO,
        "a pause was asked for with no time left"
    );
    let pause = step.min(remaining);
    assert!(pause > Duration::ZERO, "a zero pause from step {step:?}");
    assert!(
        pause <= step,
        "a pause of {pause:?} is longer than the step {step:?}"
    );
    assert!(
        pause <= remaining,
        "a pause of {pause:?} is longer than the {remaining:?} left"
    );
    pause
}

/// Polls `child` until it exits, killing it and reporting a timeout once
/// `timeout` elapses on `clock`.
///
/// The pause between polls starts at [`SUBPROCESS_POLL_INTERVAL_MIN`] and
/// doubles up to [`SUBPROCESS_POLL_INTERVAL_MAX`], so a command that exits
/// quickly is noticed quickly and a long one is polled gently. The last pause
/// is cut to what is left of `timeout`, so the deadline is met exactly rather
/// than overshot. Both the elapsed time and every pause come from `clock`
/// alone.
fn wait_until_timeout(
    child: &mut Child,
    timeout: Duration,
    clock: &impl Clock,
) -> Result<ExitStatus, SubprocessError> {
    let started = clock.now();
    let mut step = SUBPROCESS_POLL_INTERVAL_MIN;
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
        let remaining = timeout.saturating_sub(clock.now().duration_since(started));
        if remaining.is_zero() {
            // Best-effort: a child that has already exited between the
            // `try_wait` above and here needs no killing, and a `kill` that
            // fails leaves nothing further this function can do about it
            // but wait for the child, which `reap_after_kill` does.
            let _unused = child.kill();
            reap_after_kill(child, clock);
            return Err(SubprocessError {
                kind: SubprocessErrorKind::TimedOut { timeout },
            });
        }
        clock.wait(poll_pause(step, remaining));
        step = poll_step_after(step);
    }
}

/// Reaps `child` after it was killed, polling through `clock` rather than
/// blocking in [`Child::wait`].
///
/// The first pause comes before the first poll, so one clock event always
/// follows the kill. A timeout test can hang an action on that event and tell
/// a killed child from one that was never killed with no real time passing:
/// the action lets a gated child through, and only a child that outlived the
/// kill gets any further. A blocking `wait` leaves no such event behind it.
///
/// A poll that errors ends the reap as a `wait` that errored did, because
/// nothing here can do more about a child it cannot reap. A child the kill
/// did not end is waited for, as a blocking `wait` would have.
fn reap_after_kill(child: &mut Child, clock: &impl Clock) {
    let mut step = SUBPROCESS_POLL_INTERVAL_MIN;
    // Deviation from TS-BOUNDED (all loops have a fixed upper bound): the
    // loop ends when the child is reaped. The kill is what normally makes
    // that prompt, but nothing here checks that it took: a child the kill
    // did not end, such as one in uninterruptible sleep, is waited for
    // exactly as long as the blocking `Child::wait` this replaced would have
    // waited. A bound would mean giving up on a child that may still be
    // running and leaving it unreaped, which is worse than waiting for it.
    loop {
        clock.wait(step);
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => step = poll_step_after(step),
        }
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

    use super::gate::{Gate, GatedRun, run_gated_past};
    use super::{Limits, TestClock, poll_pause, poll_step_after, run, run_with_clock};

    /// A command that writes enough to each stream to fill an OS pipe buffer,
    /// on both streams at once, before it exits.
    const FILL_BOTH_PIPES: [&str; 2] = [
        "-c",
        "dd if=/dev/zero bs=200000 count=1 2>/dev/null; \
         dd if=/dev/zero bs=200000 count=1 1>&2 2>/dev/null",
    ];

    /// How many bytes [`FILL_BOTH_PIPES`] writes to each stream.
    const FILL_BOTH_PIPES_BYTES: usize = 200_000;

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
        command.args(FILL_BOTH_PIPES);
        let finished = run(command, &unbounded_limits()).expect("sh must run");
        assert!(finished.success());
        assert_eq!(
            finished
                .stdout()
                .expect("stdout must not be truncated")
                .len(),
            FILL_BOTH_PIPES_BYTES
        );
        assert_eq!(finished.stderr_head().len(), FILL_BOTH_PIPES_BYTES);
    }

    #[test]
    fn an_unbounded_command_is_waited_on_without_the_clock() {
        // With no timeout there is nothing to measure, so the call must block
        // in `Child::wait` and never read the time or pause on the clock. The
        // child fills both pipes, so the blocking wait is also shown not to
        // deadlock while the reader threads drain them. A fresh test clock
        // counts every read and pause it is asked for; both counts must be
        // zero.
        let mut command = Command::new("sh");
        command.args(FILL_BOTH_PIPES);
        let clock = TestClock::new();

        let finished = run_with_clock(command, &unbounded_limits(), &clock).expect("sh must run");

        assert!(finished.success());
        assert_eq!(
            finished
                .stdout()
                .expect("stdout must not be truncated")
                .len(),
            FILL_BOTH_PIPES_BYTES
        );
        assert_eq!(finished.stderr_head().len(), FILL_BOTH_PIPES_BYTES);
        assert_eq!(clock.times_read(), 0, "the clock was read");
        assert!(!clock.has_waited(), "the clock was waited on");
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
    fn the_gate_holds_its_child_until_released() {
        // The positive control for every gated test below: a missing kill
        // shows up there as the marker existing, so the marker has to be
        // something a child can really write. Released before it runs, the
        // gated child must read its line, write the marker and exit cleanly.
        // Without that, "the marker is absent" would hold for a gate that
        // never lets anything through.
        let gate = Gate::new();
        assert!(
            !gate.child_ran_past_release(),
            "the marker was there at once"
        );
        gate.release();

        let finished = run(gate.command(), &unbounded_limits()).expect("sh must run");

        assert!(finished.success());
        assert!(
            gate.child_ran_past_release(),
            "the released child left no marker"
        );
    }

    #[test]
    fn a_command_that_never_exits_is_killed_once_its_timeout_elapses() {
        // The child blocks on a gate that opens only inside the first pause
        // the test clock is asked for at or after the deadline, which is the
        // first one after `wait_until_timeout` has decided to kill. A child
        // that was killed never gets through, so it leaves no marker; one that
        // was not killed is let through, writes the marker and exits, and the
        // test fails at once instead of waiting the child out. The gated run
        // is set up by `run_gated_past`. Four things are checked: the gate
        // opened, so the test reached the deadline; the child left no
        // marker; the result is a timeout; and the pauses before the
        // deadline add up to exactly the timeout on the injected clock, so
        // the bound was measured there and not on the wall clock.
        let timeout = Duration::from_millis(100);

        let GatedRun {
            result,
            gate,
            clock,
            deadline,
        } = run_gated_past(timeout);

        assert!(
            gate.was_released(),
            "the clock never paused at the deadline, so the child was never let through"
        );
        assert!(
            !gate.child_ran_past_release(),
            "the child ran past its release, so it was never killed"
        );
        let error = result.expect_err("a command past its timeout must be killed");
        assert!(error.is_timed_out());
        let measured: Duration = clock.waits_begun_before(deadline).iter().sum();
        assert_eq!(
            measured, timeout,
            "the timeout was not measured on the injected clock"
        );
    }

    #[test]
    fn a_bounded_command_pauses_from_one_millisecond_doubling_to_the_cap() {
        // A child that outlives its timeout makes `wait_until_timeout` poll
        // all the way to the deadline, so the pauses it asks the test clock
        // for are its whole schedule: from the minimum, doubling to the cap,
        // then cut to what is left of the timeout. Only the pauses begun
        // before the deadline are compared, which is the polling itself. The
        // child is a gated one, so a missing kill is a failed assertion and
        // not a wait.
        let GatedRun {
            result,
            gate,
            clock,
            deadline,
        } = run_gated_past(Duration::from_millis(300));

        assert!(
            !gate.child_ran_past_release(),
            "the child ran past its release, so it was never killed"
        );
        let error = result.expect_err("a command past its timeout must be killed");
        assert!(error.is_timed_out());
        let millis = |count: u64| Duration::from_millis(count);
        assert_eq!(
            clock.waits_begun_before(deadline),
            [1, 2, 4, 8, 16, 32, 50, 50, 50, 50, 37].map(millis)
        );
    }

    #[test]
    fn the_poll_schedule_doubles_to_the_cap_and_never_passes_the_timeout() {
        // Pure: the step doubles until it reaches the cap and stays there, and
        // a pause is the step unless that would run past what is left of the
        // timeout, in which case it is exactly what is left.
        let millis = |count: u64| Duration::from_millis(count);
        for (step, next) in [
            (1, 2),
            (2, 4),
            (4, 8),
            (8, 16),
            (16, 32),
            (32, 50),
            (50, 50),
        ] {
            assert_eq!(
                poll_step_after(millis(step)),
                millis(next),
                "after {step} ms"
            );
        }
        for (step, remaining, pause) in [(8, 100, 8), (8, 8, 8), (8, 5, 5), (50, 1, 1)] {
            assert_eq!(
                poll_pause(millis(step), millis(remaining)),
                millis(pause),
                "a {step} ms step with {remaining} ms left"
            );
        }
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
