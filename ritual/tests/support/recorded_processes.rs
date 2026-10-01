//! Ending a process a test started indirectly, and cannot otherwise reach.
//!
//! A repository's content filter runs as a child of `git`, and killing a
//! timed-out `git` does not reach it. A test that stalls such a filter has the
//! filter write its own process id to a file, and holds a
//! [`KillRecordedProcesses`] over that file so the filter does not outlive the
//! test.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Kills, when dropped, every process recorded in `process_id_file`, one
/// process id per line.
///
/// Dropping runs on every way out of the test, a failed assertion and an early
/// `?` included. Declare it after the directory that holds `process_id_file`,
/// so it drops first, while the file still exists.
pub(crate) struct KillRecordedProcesses {
    pub(crate) process_id_file: PathBuf,
}

impl Drop for KillRecordedProcesses {
    fn drop(&mut self) {
        // Best-effort: no file means the process never ran. The kill happens
        // while the recorded process is still well inside its sleep, so its
        // id has not been given to another process. Only ids that name a
        // single process reach `kill` (see `killable_process_ids`).
        let Ok(recorded) = std::fs::read_to_string(&self.process_id_file) else {
            return;
        };
        for process_id in killable_process_ids(&recorded) {
            let _unused = Command::new("kill")
                .args(["-KILL", &process_id.to_string()])
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// The process ids in `recorded`, one per line, that name a single ordinary
/// process: a positive whole number above 1.
///
/// Anything else is skipped. `kill` reads `0` as the caller's own process
/// group and `-1` as every process the user may signal, and id 1 is `init`,
/// so a damaged or hostile line must never reach it.
pub(crate) fn killable_process_ids(recorded: &str) -> Vec<u32> {
    recorded
        .lines()
        .map(str::trim)
        // `parse` also accepts a leading `+`, which a recorded id never has.
        .filter(|line| line.bytes().all(|byte| byte.is_ascii_digit()))
        .filter_map(|line| line.parse::<u32>().ok())
        .filter(|process_id| *process_id > 1)
        .collect()
}
