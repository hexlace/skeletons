//! Acceptance: what `sync` proved about a file is still true when it
//! replaces that file.
//!
//! `sync` proves every claimed path one after another, stages every write,
//! and only then renames each into place. Proving one path runs git, and
//! git runs the repository's content filters: arbitrary commands that may
//! touch any file in the work tree, as an editor save or a formatter would.
//! A file proven early and touched by a later path's proof is not the file
//! that was proven, and replacing it loses what was written in between while
//! `sync` reports success.
//!
//! Each test here reads the protected file's bytes back rather than trusting
//! the exit code, and checks the refusal names the path.

mod support;

use support::sync::{assert_refused_naming, fixture_wearing, settle_index};
use support::{Fixture, TemporaryDirectory};

fn git(fixture: &Fixture, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        arguments,
        "git (fixture step)",
    )
}

/// A filter script that passes its input through unchanged and, once armed
/// (the existence of an `armed` file beside it), first writes `precious`
/// into `poisoned` (a path relative to the top of the work tree) and logs
/// which direction ran.
///
/// The script is inert until armed so that the baseline commit, and any
/// setup that runs git, never trigger it: the only run that may fire it is
/// the `sync` under test.
struct InterferingFilter {
    directory: TemporaryDirectory,
}

impl InterferingFilter {
    fn new(
        work_tree: &std::path::Path,
        poisoned: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TemporaryDirectory::new("sync-interfering-filter")?;
        let armed = directory.path().join("armed");
        let log = directory.path().join("log");
        let script = format!(
            "#!/bin/sh\n\
             if [ -e '{armed}' ]; then\n\
             echo \"$1\" >> '{log}'\n\
             printf precious > '{target}'\n\
             fi\n\
             cat\n",
            armed = armed.display(),
            log = log.display(),
            target = work_tree.join(poisoned).display(),
        );
        support::write(directory.path(), "filter.sh", script.as_bytes())?;
        Ok(Self { directory })
    }

    fn configure(&self, fixture: &Fixture) -> support::TestOutcome {
        let script = self.directory.path().join("filter.sh");
        // Both directions write, so the scenario does not depend on which
        // direction `sync` asks git to run when it proves a path.
        git(
            fixture,
            &[
                "config",
                "filter.interfere.clean",
                &format!("sh {} clean", script.display()),
            ],
        )?;
        git(
            fixture,
            &[
                "config",
                "filter.interfere.smudge",
                &format!("sh {} smudge", script.display()),
            ],
        )?;
        Ok(())
    }

    fn arm(&self) -> support::TestOutcome {
        std::fs::write(self.directory.path().join("armed"), b"")?;
        Ok(())
    }

    /// Which filter directions ran while armed.
    fn invocations(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("log")).unwrap_or_default()
    }
}

/// Commits `committed` (a path and its bytes each) under a `.gitattributes`
/// line that routes `b.yml` through `filter`, settles the index so the
/// filter cannot run during the whole-tree `git status`, and asserts the
/// preconditions every scenario here shares: git calls the work tree clean,
/// and nothing has run the filter while it was armed. The filter is left
/// unarmed.
fn commit_settled_under_filter(
    fixture: &Fixture,
    filter: &InterferingFilter,
    committed: &[(&str, &[u8])],
) -> support::TestOutcome {
    git(fixture, &["init", "--quiet"])?;
    filter.configure(fixture)?;
    fixture.write(".gitattributes", b"b.yml filter=interfere\n")?;
    for (path, bytes) in committed {
        fixture.write(path, bytes)?;
    }
    git(fixture, &["add", "--all"])?;
    git(
        fixture,
        &["commit", "--quiet", "--message", "fixture: baseline"],
    )?;
    let paths: Vec<&str> = committed.iter().map(|(path, _)| *path).collect();
    settle_index(fixture, &paths)?;
    assert_eq!(
        support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?,
        "",
        "precondition: git must call the work tree clean"
    );
    assert_eq!(
        filter.invocations(),
        "",
        "precondition: nothing has run the filter while it was armed yet"
    );
    Ok(())
}

#[test]
fn a_file_touched_while_a_later_path_is_proven_is_not_overwritten() -> support::TestOutcome {
    // The skeleton claims `a.yml` and `b.yml`; both are committed, clean,
    // and drifted from their renders. A filter on `b.yml` writes
    // `precious` into `a.yml` (standing in for an editor save or a
    // formatter) whenever git runs it. `sync` proves `a.yml` first, then
    // runs `b.yml`'s filter while proving `b.yml`. `a.yml` now holds
    // something git has never seen; replacing it destroys it. `sync` must
    // refuse, name `a.yml`, and leave `precious` in place.
    //
    // Re-checking only that `a.yml` is still a regular file is not enough:
    // it is, so `sync` would print `updated a.yml`, exit 0, and `precious`
    // would be gone. Each write's *content* is re-verified against what was
    // proven immediately before that write is renamed into place, i.e. after
    // every later path's filter has run.
    //
    // The index is settled first (`settle_index`): a racily clean `b.yml`
    // would run the filter during the whole-tree `git status` instead, which
    // makes `sync` refuse earlier for a different reason and proves nothing
    // about the window between proof and rename.
    //
    // Platform: macOS and Linux alike; the filter is POSIX `sh`.
    const A_COMMITTED: &[u8] = b"old-a\n";
    const B_COMMITTED: &[u8] = b"old-b\n";

    let wearing = fixture_wearing(&[(
        "two-files",
        &[("a.yml", "render-a\n"), ("b.yml", "render-b\n")],
    )])?;
    let fixture = &wearing.fixture;
    let filter = InterferingFilter::new(fixture.root(), "a.yml")?;

    commit_settled_under_filter(
        fixture,
        &filter,
        &[("a.yml", A_COMMITTED), ("b.yml", B_COMMITTED)],
    )?;
    filter.arm()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_ne!(
        filter.invocations(),
        "",
        "the filter on b.yml never ran during sync, so this scenario never reached the window it \
         is about; stdout was: {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_eq!(
        String::from_utf8_lossy(&fixture.read("a.yml")?),
        "precious",
        "what the filter wrote into a.yml must survive: sync replaced a file that changed after \
         it was proven; stdout was: {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_refused_naming(&report, &["a.yml"]);
    assert_eq!(
        fixture.read("b.yml")?,
        B_COMMITTED,
        "sync is all or nothing: b.yml must not be rewritten when a.yml's write was refused"
    );
    Ok(())
}

#[test]
fn a_file_created_while_a_later_path_is_proven_is_not_replaced() -> support::TestOutcome {
    // The skeleton claims `a.yml`, which does not exist,
    // and `b.yml`, which is committed and clean, with the same filter on
    // `b.yml`. `sync` proves `a.yml` absent, then `b.yml`'s filter creates
    // `a.yml`. `sync` must refuse, name `a.yml`, and leave what the filter
    // wrote.
    //
    // This pins that a file that appears between the proof and the write is
    // never replaced, whichever step catches it: the re-check that an absent
    // target is still absent after every proof has run, or the commit that
    // refuses to replace a file already there.
    //
    // Platform: macOS and Linux alike; the filter is POSIX `sh`.
    const B_COMMITTED: &[u8] = b"old-b\n";

    let wearing = fixture_wearing(&[(
        "two-files",
        &[("a.yml", "render-a\n"), ("b.yml", "render-b\n")],
    )])?;
    let fixture = &wearing.fixture;
    let filter = InterferingFilter::new(fixture.root(), "a.yml")?;

    commit_settled_under_filter(fixture, &filter, &[("b.yml", B_COMMITTED)])?;
    assert!(
        fixture.read("a.yml").is_err(),
        "precondition: a.yml does not exist before sync runs"
    );
    filter.arm()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_ne!(
        filter.invocations(),
        "",
        "the filter on b.yml never ran during sync, so this scenario never reached the window it \
         is about; stdout was: {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_eq!(
        String::from_utf8_lossy(&fixture.read("a.yml")?),
        "precious",
        "what the filter wrote into a.yml must survive; stdout was: {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    assert_refused_naming(&report, &["a.yml"]);
    assert_eq!(fixture.read("b.yml")?, B_COMMITTED);
    Ok(())
}
