//! A repository whose content filter stalls, for the tests that watch `sync`
//! give up on a local `git` command that takes too long.
//!
//! [`arm_slow_filter`] configures the filter and arms it after the baseline
//! commit, so only the `sync` run under test meets a filter that sleeps. That
//! run must be given [`LOCAL_GIT_TIMEOUT_VARIABLE`] set to
//! [`LOCAL_GIT_TIMEOUT_SECONDS`].

use std::error::Error;

use super::recorded_processes::KillRecordedProcesses;
use super::shell_quote::shell_quoted;
use super::sync::settle_index;
use super::{Fixture, TemporaryDirectory};

/// The environment variable that overrides how long `sync` waits for a local
/// `git` command, in whole seconds. Only a test build reads it.
pub(crate) const LOCAL_GIT_TIMEOUT_VARIABLE: &str = "SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS";

/// How long `sync` waits for a local git command in a run that uses the
/// override, in seconds, as the override's value. It bounds every local git
/// command in that `sync` run, not only the filtered one, so it has to be long
/// enough that an ordinary command on a loaded machine finishes well inside
/// it.
pub(crate) const LOCAL_GIT_TIMEOUT_SECONDS: &str = "5";

/// How long the slow filter sleeps once armed, in seconds: several times
/// [`LOCAL_GIT_TIMEOUT_SECONDS`], so the timeout is what ends the filter and
/// a slow machine cannot finish it first.
const FILTER_SLEEP_SECONDS: u32 = 30;

/// What keeps an armed slow filter cleaned up. Hold it until the test ends.
///
/// Killing the timed-out `git` does not reach the filter it started, which
/// would sleep on for the rest of [`FILTER_SLEEP_SECONDS`]. The killer is
/// declared before the directory that holds its file, so it drops first, while
/// the file still exists.
pub(crate) struct SlowFilter {
    _kill_the_filter: KillRecordedProcesses,
    _scripts: TemporaryDirectory,
}

/// Makes `fixture` a git repository whose file `relative_path`, committed
/// with `committed` as its bytes, goes through a clean and smudge filter that
/// sleeps once armed, and arms it.
///
/// The index is settled before arming, so the filter does not run during the
/// whole-tree status `sync` starts with.
///
/// # Errors
///
/// Returns the error of whichever fixture step fails.
pub(crate) fn arm_slow_filter(
    fixture: &Fixture,
    relative_path: &str,
    committed: &[u8],
) -> Result<SlowFilter, Box<dyn Error>> {
    let scripts = TemporaryDirectory::new("slow-filter")?;
    let armed = scripts.path().join("armed");
    let process_id_file = scripts.path().join("process-ids");
    // `exec` makes the recorded process id the sleeping process itself, not a
    // shell waiting on a child that would outlive it.
    let script = format!(
        "#!/bin/sh\nif [ -e {armed} ]; then echo $$ >> {process_id_file}; \
         exec sleep {FILTER_SLEEP_SECONDS}; fi\ncat\n",
        armed = shell_quoted(&armed),
        process_id_file = shell_quoted(&process_id_file)
    );
    super::write(scripts.path(), "filter.sh", script.as_bytes())?;
    let command = format!("sh {}", shell_quoted(&scripts.path().join("filter.sh")));
    let git = |arguments: &[&str]| {
        super::git::run(
            fixture.root(),
            fixture.sandbox().home(),
            arguments,
            "git (fixture step)",
        )
    };
    git(&["init", "--quiet"])?;
    git(&["config", "filter.slow.clean", &command])?;
    git(&["config", "filter.slow.smudge", &command])?;
    fixture.write(
        ".gitattributes",
        format!("{relative_path} filter=slow\n").as_bytes(),
    )?;
    fixture.write(relative_path, committed)?;
    git(&["add", "--all"])?;
    git(&["commit", "--quiet", "--message", "fixture: baseline"])?;
    settle_index(fixture, &[relative_path])?;
    std::fs::write(&armed, b"")?;
    Ok(SlowFilter {
        _kill_the_filter: KillRecordedProcesses { process_id_file },
        _scripts: scripts,
    })
}
