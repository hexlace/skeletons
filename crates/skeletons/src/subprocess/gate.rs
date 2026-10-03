//! A child process that a test holds shut until the test lets it through,
//! and that leaves a mark only if it ever gets through.
//!
//! A timeout test has to prove that the child was killed. Giving the child a
//! lifetime in real time (`sleep 60`) makes a missing kill a minute-long hang
//! instead of a failure. A [`Gate`] gives it a lifetime in test-clock events
//! instead: the child blocks reading a FIFO until the test releases it, and
//! writes a marker file only once it has read. A child that was killed before
//! the release never writes the marker; one that was not killed does, so a
//! missing kill is a red assertion and never a hang.
//!
//! The FIFO is made with the `mkfifo` utility, and the child is a POSIX `sh`.
//! Both are POSIX utilities that the tests rely on being present: Linux and
//! macOS are the only targets, and std has no way to make a FIFO.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::clock::Clock;
use super::{Finished, Limits, SubprocessError, TestClock, run_with_clock};

/// What the child reads, and what it does once it has read it.
///
/// Deviation from RS-SINGLE-TOOLCHAIN (integration tests use
/// `std::process::Command`, not shell scripts): the child has to block on a
/// FIFO and then leave a file, and a one-line POSIX `sh` is the narrowest
/// program that does both without a binary target added to the crate for
/// tests. The `;` is deliberate: whatever makes `read` return, including a
/// FIFO that was never made, still writes the marker, so a broken gate fails
/// a test and never passes one.
const GATED_SCRIPT: &str = r#"read _ < "$1"; : > "$2""#;

/// A FIFO and a marker file in a temporary directory, removed when dropped.
pub(crate) struct Gate {
    directory: TempDir,
    /// The write end, held open from the release on. The release writes the
    /// line before the child may have opened the FIFO, or after it was
    /// killed; holding the descriptor keeps that line buffered for a reader
    /// that arrives late and stops a reader that left from making the write
    /// fail.
    writer: RefCell<Option<File>>,
}

impl Gate {
    /// Makes a gate whose FIFO nothing has been written to yet.
    pub(crate) fn new() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let status = Command::new("mkfifo")
            .arg(directory.path().join("gate"))
            .status()
            .expect("the `mkfifo` utility must be installed");
        assert!(status.success(), "`mkfifo` failed: {status}");
        Self {
            directory,
            writer: RefCell::new(None),
        }
    }

    /// Returns a command that blocks until [`Gate::release`], then writes the
    /// marker and exits.
    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new("sh");
        command
            .args(["-c", GATED_SCRIPT, "sh"])
            .arg(self.fifo_path())
            .arg(self.marker_path());
        command
    }

    /// Lets the child through by writing one line to the FIFO.
    ///
    /// Opening a FIFO for both reading and writing never blocks, so this
    /// returns at once whether or not the child has opened its end yet.
    pub(crate) fn release(&self) {
        let mut writer = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.fifo_path())
            .expect("the FIFO opens for reading and writing");
        writer
            .write_all(b"\n")
            .expect("one line fits in an empty FIFO");
        let previous = self.writer.borrow_mut().replace(writer);
        assert!(previous.is_none(), "the gate was released twice");
    }

    /// Returns whether [`Gate::release`] has been called.
    pub(crate) fn was_released(&self) -> bool {
        self.writer.borrow().is_some()
    }

    /// Returns whether the child got past the release and wrote the marker.
    pub(crate) fn child_ran_past_release(&self) -> bool {
        self.marker_path().exists()
    }

    fn fifo_path(&self) -> PathBuf {
        self.directory.path().join("gate")
    }

    fn marker_path(&self) -> PathBuf {
        self.directory.path().join("marker")
    }
}

/// What [`run_gated_past`] leaves behind for a test to assert on.
pub(crate) struct GatedRun {
    /// What `run_with_clock` returned. With the kill missing this is `Ok`, so
    /// a test checks the gate before it unwraps this.
    pub(crate) result: Result<Finished, SubprocessError>,
    pub(crate) gate: Rc<Gate>,
    pub(crate) clock: TestClock,
    /// The instant on `clock` at which the timeout elapses, and the one the
    /// gate was hung on.
    pub(crate) deadline: Instant,
}

/// Runs a gated child with `timeout` on a [`TestClock`], opening the gate in
/// the first pause the clock is asked for at or after the deadline.
///
/// That pause is the first clock event after `wait_until_timeout` decided to
/// kill, so the release lands after the kill and never before it. A child
/// that was killed never gets through and leaves no marker; one that was not
/// killed is let through, writes the marker and exits, and the run ends at
/// once instead of waiting the child out. Tying the release to the deadline
/// here, in one place, is what makes every gated test sound.
pub(crate) fn run_gated_past(timeout: Duration) -> GatedRun {
    let gate = Rc::new(Gate::new());
    let limits = Limits {
        timeout: Some(timeout),
        stdout_bytes_max: u64::MAX,
        stderr_bytes_max: u64::MAX,
    };
    let clock = TestClock::new();
    let deadline = clock.now() + timeout;
    let release = Rc::clone(&gate);
    clock.on_first_wait_from(deadline, move || release.release());

    let result = run_with_clock(gate.command(), &limits, &clock);

    GatedRun {
        result,
        gate,
        clock,
        deadline,
    }
}
