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

    /// Pauses for `duration` before the caller polls again. The caller chooses
    /// the length of each pause, so the clock never decides a polling rate.
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

/// One pause a [`TestClock`] was asked for: the instant it began at on the
/// clock, and how long it was asked to last.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct Wait {
    began: Instant,
    duration: Duration,
}

/// A clock for unit tests that never lets real time pass.
///
/// It starts at one real instant and moves only when told to wait, by exactly
/// the duration asked for, returning at once. A timeout is therefore reached
/// after a few polls however long the child would really have run. It also
/// counts the reads and records every wait it is asked for, so a test can show
/// that a command with no timeout never consulted it, and what pauses a
/// bounded one asked for. A test can also hang one action on the first pause
/// that begins at a chosen instant, so something outside the clock happens at
/// a known point in the clock's own events.
#[cfg(test)]
pub(crate) struct TestClock {
    now: std::cell::Cell<Instant>,
    reads: std::cell::Cell<u32>,
    waits: std::cell::RefCell<Vec<Wait>>,
    hook: std::cell::RefCell<Option<Hook>>,
}

/// An action waiting for the first pause that begins at or after an instant.
#[cfg(test)]
struct Hook {
    instant: Instant,
    action: Box<dyn FnOnce()>,
}

#[cfg(test)]
impl TestClock {
    /// Returns a clock standing at the current real instant.
    pub(crate) fn new() -> Self {
        Self {
            now: std::cell::Cell::new(Instant::now()),
            reads: std::cell::Cell::new(0),
            waits: std::cell::RefCell::new(Vec::new()),
            hook: std::cell::RefCell::new(None),
        }
    }

    /// Returns how many times the time has been read from this clock.
    pub(crate) fn times_read(&self) -> u32 {
        self.reads.get()
    }

    /// Returns whether this clock has been asked to pause at all.
    pub(crate) fn has_waited(&self) -> bool {
        !self.waits.borrow().is_empty()
    }

    /// Returns the length of each pause that began strictly before `instant`,
    /// in the order they were asked for.
    pub(crate) fn waits_begun_before(&self, instant: Instant) -> Vec<Duration> {
        self.waits
            .borrow()
            .iter()
            .filter(|wait| wait.began < instant)
            .map(|wait| wait.duration)
            .collect()
    }

    /// Runs `action` inside the first pause that begins at or after
    /// `instant`, before the clock moves on. It runs at most once, and a
    /// second call replaces an action that has not run yet.
    ///
    /// Tying the action to a pause rather than to a count of reads makes its
    /// place among the clock's events something a test can name: the pause
    /// that begins at the deadline is the first one after the deadline was
    /// decided.
    pub(crate) fn on_first_wait_from(&self, instant: Instant, action: impl FnOnce() + 'static) {
        *self.hook.borrow_mut() = Some(Hook {
            instant,
            action: Box::new(action),
        });
    }
}

#[cfg(test)]
impl Clock for TestClock {
    fn now(&self) -> Instant {
        self.reads.set(self.reads.get() + 1);
        self.now.get()
    }

    fn wait(&self, duration: Duration) {
        let began = self.now.get();
        self.waits.borrow_mut().push(Wait { began, duration });
        // Taken out before it runs, so an action that touches this clock does
        // not find the hook still borrowed.
        let due = self.hook.borrow_mut().take_if(|hook| hook.instant <= began);
        if let Some(hook) = due {
            (hook.action)();
        }
        self.now.set(self.now.get() + duration);
    }
}
