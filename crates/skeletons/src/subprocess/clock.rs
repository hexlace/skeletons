//! The clock [`wait_until_timeout`](super::wait_until_timeout) reads the time
//! from and pauses on between polls: the wall clock in production, and a clock
//! that never lets real time pass in unit tests. A command with no timeout
//! never consults it.

use std::time::{Duration, Instant};

/// The one seam through which a command's timeout is measured.
///
/// [`wait_until_timeout`](super::wait_until_timeout) reads the time and pauses
/// between polls only through this trait, so a test can reach the timeout path
/// without a real bound ever having to elapse. It is consulted only when a
/// command has a timeout. Production uses [`SystemClock`].
pub(crate) trait Clock {
    /// Returns the current instant.
    fn now(&self) -> Instant;

    /// Pauses for `duration` before the caller polls again.
    fn wait(&self, duration: Duration);
}

/// The wall clock: [`Instant::now`] for the time and [`std::thread::sleep`]
/// for the pause.
pub(super) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// A clock for unit tests that never lets real time pass.
///
/// It starts at one real instant and moves only when told to wait, by exactly
/// the duration asked for, returning at once. A timeout is therefore reached
/// after a few polls however long the child would really have run. It also
/// counts the reads and waits it is asked for, so a test can show that a
/// command with no timeout never consulted it.
#[cfg(test)]
pub(crate) struct TestClock {
    now: std::cell::Cell<Instant>,
    reads: std::cell::Cell<u32>,
    waits: std::cell::Cell<u32>,
}

#[cfg(test)]
impl TestClock {
    /// Returns a clock standing at the current real instant.
    pub(crate) fn new() -> Self {
        Self {
            now: std::cell::Cell::new(Instant::now()),
            reads: std::cell::Cell::new(0),
            waits: std::cell::Cell::new(0),
        }
    }

    /// Returns how many times the time has been read from this clock.
    pub(crate) fn times_read(&self) -> u32 {
        self.reads.get()
    }

    /// Returns how many times this clock has been asked to pause.
    pub(crate) fn times_waited(&self) -> u32 {
        self.waits.get()
    }
}

#[cfg(test)]
impl Clock for TestClock {
    fn now(&self) -> Instant {
        self.reads.set(self.reads.get() + 1);
        self.now.get()
    }

    fn wait(&self, duration: Duration) {
        self.waits.set(self.waits.get() + 1);
        self.now.set(self.now.get() + duration);
    }
}
