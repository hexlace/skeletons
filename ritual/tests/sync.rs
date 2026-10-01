//! Acceptance: `sync` writes drifted files back to match, all or nothing.
//!
//! Every dirty-worktree scenario builds a real git repository at the
//! fixture's own workspace root (`Fixture::init_git_repository`), commits a
//! clean baseline, then introduces exactly one kind of "content git could
//! not give back", before running `sync` and reading the claimed file's
//! bytes back — never trusting the exit code alone to say what was or was
//! not written.

mod support;

use std::path::Path;

use support::git::SkeletonRepository;
use support::passthrough_plain::{PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain};
use support::{
    Fixture, checked_in_refused_skeleton, checked_in_test_skeleton, path_dependency_on,
    path_dependency_on_test_skeleton, wearing_table, write_package_manifest,
};

#[test]
fn sync_creates_a_missing_file_matching_the_render_and_then_check_reports_matches()
-> support::TestOutcome {
    // `plain.yml` never existed. `sync` must create it, matching the
    // render exactly, and a `check` run immediately afterward must report
    // it as matching.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.init_git_repository()?;

    let sync_report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        sync_report.exit_code, 0,
        "stderr was: {}",
        sync_report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        PASSTHROUGH_PLAIN_RENDER,
        "sync must create the missing file matching the render exactly"
    );

    let check_report = fixture.run(&["skeletons", "check"])?;
    assert_eq!(
        check_report.exit_code, 0,
        "check must report matches right after sync; stderr was: {}",
        check_report.stderr
    );
    Ok(())
}

#[test]
fn sync_refuses_when_a_claimed_path_holds_modified_tracked_content_and_writes_nothing()
-> support::TestOutcome {
    // `plain.yml` is committed holding the correct render, then modified
    // without committing. `sync` must refuse rather than overwrite
    // uncommitted work, naming the path, and the file's bytes must be
    // exactly the uncommitted content afterward — proved by reading it
    // back, not by trusting the exit code alone.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.init_git_repository()?;
    let uncommitted_content = b"name: modified-without-committing\nvalue: 42\n".to_vec();
    fixture.write("plain.yml", &uncommitted_content)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must refuse against a modified-but-uncommitted claimed path; stderr was: {}",
        report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("plain.yml"),
        "the refusal must name the dirty path; combined output was: {combined}"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        uncommitted_content,
        "a refused sync must leave the claimed file's bytes exactly as they were"
    );
    Ok(())
}

#[test]
fn sync_refuses_when_a_claimed_path_holds_an_untracked_file_and_writes_nothing()
-> support::TestOutcome {
    // `plain.yml` was never committed at all — an untracked file sitting
    // at a path `sync` would write. Git cannot give this back either, so
    // it refuses exactly as for a modified tracked file.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.init_git_repository()?; // committed without plain.yml
    let untracked_content = b"never committed at all\n".to_vec();
    fixture.write("plain.yml", &untracked_content)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must refuse against an untracked claimed path; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        untracked_content,
        "a refused sync must leave the untracked file's bytes exactly as they were"
    );
    Ok(())
}

#[test]
fn sync_refuses_when_a_claimed_path_holds_an_ignored_file_and_writes_nothing()
-> support::TestOutcome {
    // `plain.yml` is listed in `.gitignore`, so `git status` says nothing
    // about it at all — but git still cannot give its content back if
    // overwritten, so it counts as dirty the same as an untracked file.
    let fixture = fixture_wearing_passthrough_plain()?;
    support::git::ignore(fixture.root(), "plain.yml")?;
    fixture.init_git_repository()?; // .gitignore committed, plain.yml absent
    let ignored_content = b"present on disk, ignored, never committed\n".to_vec();
    fixture.write("plain.yml", &ignored_content)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must refuse against an ignored claimed path; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        ignored_content,
        "a refused sync must leave the ignored file's bytes exactly as they were"
    );
    Ok(())
}

#[test]
fn sync_outside_a_git_working_tree_refuses_and_writes_nothing() -> support::TestOutcome {
    // No `git init` at all: with no version control, sync has no undo, so
    // it must refuse rather than write anything. Exit code 1 alone is also
    // what a stub that always refused would report; the assertion that
    // actually proves it is the second one below — that `plain.yml` was
    // never created.
    let fixture = fixture_wearing_passthrough_plain()?;
    // Deliberately no `fixture.init_git_repository()` call.

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync outside a git working tree must refuse with a task refusal, not a usage error; \
         stderr was: {}",
        report.stderr
    );
    assert!(
        fixture.read("plain.yml").is_err(),
        "sync outside a git working tree must write nothing at all"
    );
    Ok(())
}

#[test]
fn sync_wearing_nothing_outside_a_git_working_tree_says_so_and_exits_zero() -> support::TestOutcome
{
    // A workspace wearing no skeletons at all needs no git: sync would write
    // nothing regardless of what it wears, so the wears-nothing answer
    // must come first, before any check that the workspace is even a git
    // work tree. Deliberately no skeleton dependency and no
    // `fixture.init_git_repository()` call, so a workspace-wearing check
    // run after the work-tree check would instead see "not inside a git
    // work tree" and exit 1.
    let fixture = Fixture::new()?;
    write_package_manifest(fixture.root(), "", "wearer", "")?;
    fixture.generate_lockfile()?;
    let before = support::snapshot_workspace_files(fixture.root(), &["target"])?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "a workspace wearing nothing must exit zero even outside a git work tree; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        report.stdout,
        "this workspace wears no skeletons; a manifest wears one with a \
         [package.metadata.skeletons.<dependency>] table beside the dependency\n"
    );
    let after = support::snapshot_workspace_files(fixture.root(), &["target"])?;
    assert_eq!(
        before, after,
        "a workspace wearing nothing must have nothing written to it"
    );
    Ok(())
}

#[test]
fn sync_is_all_or_nothing_when_one_worn_skeleton_is_refused() -> support::TestOutcome {
    // `passthrough-plain` (healthy, its file missing) and `unknown-key`
    // (checked in under `refused/`, always refused) are worn together.
    // Because one worn skeleton is refused, `sync` must write nothing at
    // all — not even `passthrough-plain`'s own, otherwise-uncomplicated
    // file.
    let fixture = Fixture::new()?;
    let broken_skeleton = checked_in_refused_skeleton("unknown-key");
    let extra = format!(
        "[dependencies]\n{}{}\n{}{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        path_dependency_on("unknown-key", &broken_skeleton),
        wearing_table("passthrough-plain", ""),
        wearing_table("unknown-key", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must fail overall when any worn skeleton is refused; stderr was: {}",
        report.stderr
    );
    assert!(
        fixture.read("plain.yml").is_err(),
        "sync must write nothing at all, including the healthy skeleton's own file, when \
         another worn skeleton is refused"
    );
    Ok(())
}

#[test]
fn sync_never_writes_outside_the_workspace_root_through_a_symlink() -> support::TestOutcome {
    // `nested-dotfiles` claims `.github/dependabot.yml`, `a/b/deep.yml`,
    // and `root.yml`. Replacing `a` with a symlink to a directory outside
    // the workspace, before `sync` runs, must refuse rather than follow it
    // — and, since `sync` is all-or-nothing, none of the *other* claimed
    // files may be written either.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("nested-dotfiles", "nested-dotfiles"),
        wearing_table("nested-dotfiles", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    let outside_target = support::TemporaryDirectory::new("symlink-target-outside-workspace")?;
    support::symlink(outside_target.path(), &fixture.root().join("a"))?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must refuse a claimed path reached through a symlink escaping the workspace root; \
         stderr was: {}",
        report.stderr
    );
    // Exit 1 and an empty outside directory are what any refusal leaves,
    // including one that has misread the symlink as something else: a
    // directory walk that treated the link as a plain file would refuse the
    // same path with the same exit code and write nothing either. The refusal
    // that proves the link was read as a link is the one that names it.
    assert!(
        report
            .stdout
            .contains("a/b/deep.yml is under a symbolic link, a;"),
        "sync must refuse by saying the claimed path is under a symbolic link, and naming it; \
         stdout was: {}",
        report.stdout
    );
    assert!(
        !outside_target.path().join("b").exists(),
        "sync must never write through the symlink to the directory it points at"
    );
    assert!(
        fixture.read("root.yml").is_err(),
        "sync is all-or-nothing: an unrelated claimed file must not be written either"
    );
    assert!(
        fixture.read(".github/dependabot.yml").is_err(),
        "sync is all-or-nothing: an unrelated claimed file must not be written either"
    );
    Ok(())
}

#[test]
fn sync_makes_no_network_request_even_though_its_skeleton_is_behind() -> support::TestOutcome {
    // Locks against a skeleton repository, then deletes that repository
    // outright before running `sync` — so any attempt to reach it for a
    // `behind` answer would fail loudly. `sync` must still succeed and
    // write the correct bytes: whether a skeleton is behind has no bearing
    // on what `sync` writes, and it never asks.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("sync-no-network", "thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\nsync-no-network = {{ git = \"{}\", tag = \"1.0.0\" }}\n\n{}",
        repository.file_url(),
        wearing_table("sync-no-network", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    // The remote is now gone entirely — a positive control on its own: if
    // `sync` ever did ask, it would fail loudly right here, since there is
    // nothing left at the remote's `file://` URL to answer.
    repository.remove()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "sync must succeed even though its skeleton's own remote is unreachable, since it \
         never asks; stderr was: {}",
        report.stderr
    );
    assert_eq!(fixture.read("thing.txt")?, b"v1\n");
    Ok(())
}

/// A fixture wearing two worn skeletons — `semver` at `1.0.7`, from a
/// vendored crates.io source, and `sync-remote-log-git`, tagged `1.0.0`,
/// from `repository` — the two kinds of skeleton
/// `sync_makes_no_network_request_as_observed_through_the_remote_query_log`
/// needs, since those are the only two kinds that ever have anything to ask
/// a remote about.
fn fixture_wearing_a_registry_skeleton_and_a_git_skeleton(
    repository: &SkeletonRepository,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    support::write_vendored_crates_io_skeleton(
        fixture.sandbox().cargo_home(),
        "semver",
        "1.0.7",
        "registry-thing.txt",
        "locked at 1.0.7\n",
    )?;
    let extra = format!(
        "[dependencies]\n\
         sync-remote-log-git = {{ git = \"{}\", tag = \"1.0.0\" }}\n\
         {}\n\
         {}{}",
        repository.file_url(),
        support::registry_dependency("semver", "1.0.7"),
        wearing_table("sync-remote-log-git", ""),
        wearing_table("semver", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

#[test]
fn sync_makes_no_network_request_as_observed_through_the_remote_query_log() -> support::TestOutcome
{
    // The same rule as the test above, proved a second, more direct way:
    // rather than inferring "no query was made" from the remote's absence,
    // this reads the test-only remote-query log `skeletons` appends one line to
    // immediately *before* each query it would make, for a workspace
    // wearing both a crates.io-sourced skeleton and a git one — the two
    // kinds of skeleton that ever have anything to ask. `sync` must leave
    // the log empty (or never create it at all), while `check` against the
    // very same workspace *does* append to it, which is the positive
    // control proving the log would have caught a query had `sync` made
    // one.
    let repository = SkeletonRepository::new()?;
    repository.commit_skeleton("sync-remote-log-git", "git-thing.txt", "v1\n", "v1")?;
    repository.tag("1.0.0")?;
    let fixture = fixture_wearing_a_registry_skeleton_and_a_git_skeleton(&repository)?;

    let log_directory = support::TemporaryDirectory::new("sync-remote-log")?;
    let sync_log_path = log_directory.path().join("sync.log");
    let sync_report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            "SKELETONS_TEST_ONLY_REMOTE_LOG",
            sync_log_path.to_str().ok_or("log path must be UTF-8")?,
        )],
    )?;
    assert_eq!(
        sync_report.exit_code, 0,
        "stderr was: {}",
        sync_report.stderr
    );
    let sync_log = support::read_remote_log(&sync_log_path)?;
    assert_eq!(
        sync_log, "",
        "sync must append nothing to the remote-query log; log was: {sync_log:?}"
    );

    assert_check_logs_a_query_for_both_skeletons(&fixture, &repository, log_directory.path())?;
    Ok(())
}

/// The positive control for
/// `sync_makes_no_network_request_as_observed_through_the_remote_query_log`:
/// the very same workspace, asked with `check` instead of `sync`, must
/// produce log entries for both skeletons — proving the log genuinely
/// records a query when one happens, so `sync`'s own empty log means
/// something.
fn assert_check_logs_a_query_for_both_skeletons(
    fixture: &Fixture,
    repository: &SkeletonRepository,
    log_directory: &Path,
) -> support::TestOutcome {
    let check_log_path = log_directory.join("check.log");
    let check_report = fixture.run_with_env(
        &["skeletons", "check", "--json"],
        &[
            (
                "SKELETONS_TEST_ONLY_CRATES_IO_INDEX",
                support::captured_crates_io_index()
                    .to_str()
                    .ok_or("index path must be UTF-8")?,
            ),
            (
                "SKELETONS_TEST_ONLY_REMOTE_LOG",
                check_log_path.to_str().ok_or("log path must be UTF-8")?,
            ),
        ],
    )?;
    assert_eq!(
        check_report.exit_code, 0,
        "stderr was: {}",
        check_report.stderr
    );
    let check_log = support::read_remote_log(&check_log_path)?;
    assert!(
        check_log.contains("crates-io semver"),
        "check must log a crates-io query for semver, proving the log mechanism itself works; \
         log was: {check_log:?}"
    );
    assert!(
        check_log.contains(&repository.file_url()),
        "check must log a git query for the git-pinned skeleton too; log was: {check_log:?}"
    );
    Ok(())
}

#[test]
fn sync_refuses_against_unrelated_uncommitted_changes_elsewhere() -> support::TestOutcome {
    // The dirty-worktree refusal is not scoped to the paths sync is about
    // to change: an uncommitted, unrelated file sitting anywhere in the
    // worktree blocks sync too. Git staying silent about a path only proves
    // it clean if git can actually see it, and a rule scoped to the claimed
    // paths would let `.github` (a submodule), a nested non-submodule
    // repository and an NFD-spelled path all lose real content silently. See
    // `sync_worktree.rs` for the whole-worktree suite this test belongs to.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.init_git_repository()?; // committed without plain.yml
    let unrelated_content = b"never claimed by any skeleton\n";
    fixture.write("unrelated.txt", unrelated_content)?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_ne!(
        report.exit_code, 0,
        "an unrelated uncommitted file anywhere in the worktree must block sync; stdout \
         was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("unrelated.txt"),
        "the refusal must name the unrelated file that is actually blocking sync; combined \
         output was: {combined:?}"
    );
    assert!(
        fixture.read("plain.yml").is_err(),
        "sync must write nothing at all — not even the otherwise-uncomplicated claimed file — \
         while unrelated dirt sits elsewhere in the worktree"
    );
    assert_eq!(
        fixture.read("unrelated.txt")?,
        unrelated_content,
        "the unrelated, unclaimed file must be left exactly as it was"
    );
    Ok(())
}

#[test]
fn sync_never_stages_commits_or_stashes_anything() -> support::TestOutcome {
    // Writing a previously missing claimed file necessarily changes what
    // `git status` reports (a new untracked file appears) — that is
    // `sync` writing to the working tree, which is its job. What it must
    // never do is stage that file into the index, commit anything, or
    // touch the stash. The new file must show as untracked (`??`), never
    // staged (`A `), and the stash list must stay empty throughout.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.init_git_repository()?;

    let stash_before = support::git::stash_list(fixture.root(), fixture.sandbox().home())?;
    assert_eq!(
        stash_before, "",
        "the fixture must start with an empty stash"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let status_after = support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?;
    assert!(
        status_after.lines().any(|line| line == "?? plain.yml"),
        "the newly written file must be untracked, not staged; status was: {status_after}"
    );
    assert!(
        !status_after.lines().any(|line| line.starts_with("A ")),
        "sync must never stage anything; status was: {status_after}"
    );

    let stash_after = support::git::stash_list(fixture.root(), fixture.sandbox().home())?;
    assert_eq!(stash_after, "", "sync must never stash anything");
    Ok(())
}

#[test]
fn sync_is_all_or_nothing_when_worn_skeletons_overlap_even_with_an_unrelated_drifted_file()
-> support::TestOutcome {
    // `overlap-a` and `overlap-b` both claim `shared.txt` — wearing both
    // together is a refusal on its own, no matter what either skeleton's own
    // file holds. `passthrough-plain`, worn alongside them, claims a wholly
    // separate, non-overlapping path (`plain.yml`) that is independently
    // drifted. Because `sync` writes nothing at all when a worn skeleton is
    // refused, two claims overlap, or a path it would change is dirty, the
    // overlap refusal must stop it from writing anything at all — including
    // `passthrough-plain`'s own, unrelated, already-drifted file — proved by
    // reading every claimed path's bytes back unchanged, not by trusting the
    // exit code alone.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}{}\n{}\n{}{}{}",
        path_dependency_on_test_skeleton("overlap-a", "overlap-a"),
        path_dependency_on_test_skeleton("overlap-b", "overlap-b"),
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("overlap-a", ""),
        wearing_table("overlap-b", ""),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    let preexisting_shared = b"pre-existing shared.txt, must not change\n".to_vec();
    fixture.write("shared.txt", &preexisting_shared)?;
    let drifted_plain = b"name: drifted-before-sync\nvalue: 0\n".to_vec();
    fixture.write("plain.yml", &drifted_plain)?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must fail overall when worn skeletons overlap; stderr was: {}",
        report.stderr
    );
    assert_eq!(
        fixture.read("shared.txt")?,
        preexisting_shared,
        "an overlapping claimed path must be left exactly as it was"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        drifted_plain,
        "sync is all-or-nothing: an unrelated, non-overlapping drifted file must not be written \
         either, once any claim overlaps"
    );
    Ok(())
}

#[test]
fn sync_writes_nothing_beyond_the_claimed_files_it_updates_or_creates() -> support::TestOutcome {
    // Beyond the claimed files sync is meant to write, running it must leave
    // the rest of the workspace exactly as it was — no manifest, cache, or
    // stored hash anywhere else. Wears `nested-dotfiles` (three claimed
    // files, all missing) and `passthrough-plain` (its own claimed file
    // already matching, so untouched). A full, recursive snapshot of the
    // workspace, `.git/` included (sync stages and commits nothing) and
    // excluding only `target/` (cargo's own incidental build output), must
    // show only the claimed paths sync actually wrote as different
    // afterward — nothing else in the tree, however deeply nested, may
    // change.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}\n{}{}",
        path_dependency_on_test_skeleton("nested-dotfiles", "nested-dotfiles"),
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("nested-dotfiles", ""),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?; // already matches
    fixture.init_git_repository()?;

    let before = support::snapshot_workspace_files(fixture.root(), &["target"])?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);

    let after = support::snapshot_workspace_files(fixture.root(), &["target"])?;

    let mut changed_paths: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(path.as_str()) != after.get(path.as_str()))
        .collect();
    changed_paths.sort();
    changed_paths.dedup();

    let expected_claimed_paths = [
        ".github/dependabot.yml".to_owned(),
        "a/b/deep.yml".to_owned(),
        "root.yml".to_owned(),
    ];
    assert_eq!(
        changed_paths,
        expected_claimed_paths.iter().collect::<Vec<_>>(),
        "sync must change exactly the claimed files it writes, and nothing else anywhere in the \
         workspace"
    );
    Ok(())
}

#[test]
fn sync_refuses_a_claimed_file_present_only_under_a_different_spelling() -> support::TestOutcome {
    // On a case-insensitive filesystem (such as a default macOS volume),
    // looking up `.github/dependabot.yml` finds the untracked
    // `.github/DEPENDABOT.YML`, which `sync` must never overwrite in place —
    // content git never had, reported as success. It must instead refuse,
    // leave the file's bytes exactly as they were, create no second file
    // beside it, and leave `git status` unchanged — and the identical outcome
    // holds on a case-sensitive filesystem (such as a Linux one), where the
    // untracked `DEPENDABOT.YML` is simply missing under the claimed
    // spelling, and `sync` must not create a second file there instead.
    // Nothing here branches on which platform is running: both must give one
    // answer, not one per filesystem.
    //
    // `nested-dotfiles` also claims `a/b/deep.yml` and `root.yml`; both are
    // written matching their renders and committed, so the only thing under
    // test is the spelling of the `.github` claim.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("nested-dotfiles", "nested-dotfiles"),
        wearing_table("nested-dotfiles", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("root.yml", b"root-file: yes\n")?;
    fixture.write("a/b/deep.yml", b"nested: yes\n")?;
    fixture.init_git_repository()?;
    // Written after the commit, so it sits untracked at the path `sync`
    // would otherwise write `.github/dependabot.yml` to.
    fixture.write(".github/DEPENDABOT.YML", b"PRECIOUS")?;

    let status_before = support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "sync must refuse a claim present only under a different spelling; stderr was: {}",
        report.stderr
    );
    assert!(
        report
            .stdout
            .contains(".github/dependabot.yml is spelled .github/DEPENDABOT.YML on disk"),
        "sync must name the on-disk spelling in its refusal; stdout was: {}",
        report.stdout
    );
    assert_eq!(
        fixture.read(".github/DEPENDABOT.YML")?,
        b"PRECIOUS",
        "a refused sync must leave the differently spelled file's bytes exactly as they were"
    );
    assert_eq!(
        support::list_top_level_entries(&fixture.root().join(".github"), &[])?,
        vec!["DEPENDABOT.YML".to_owned()],
        "sync must create no second file beside the differently spelled one"
    );
    let status_after = support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?;
    assert_eq!(
        status_before, status_after,
        "a refused sync must leave git status exactly as it was"
    );
    Ok(())
}

#[test]
fn sync_never_runs_a_worn_skeletons_build_script() -> support::TestOutcome {
    // The same `build-script-marker` skeleton `check_drift.rs`'s
    // `a_worn_skeleton_carrying_a_build_script_is_rendered_and_compared_like_any_other`
    // exercises, but through `sync` instead: its `build.rs` writes a marker
    // file to a path this test names through an environment variable, if
    // and only if it is ever executed. `thing.txt` starts drifted (changed),
    // so `sync` genuinely has something to write; the marker file must
    // still never appear, and the file it writes must match the render
    // exactly — proving `sync`, like `check`, never runs anything a worn
    // skeleton's own crate ships.
    let fixture = Fixture::new()?;
    let skeleton_path = checked_in_test_skeleton("build-script-marker");
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on("build-script-marker", &skeleton_path),
        wearing_table("build-script-marker", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.write("thing.txt", b"drifted before sync ever runs\n")?;
    fixture.init_git_repository()?;

    let marker_directory = support::TemporaryDirectory::new("sync-build-script-marker")?;
    let marker_path = marker_directory.path().join("build-script-ran.marker");

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            support::BUILD_SCRIPT_MARKER_VARIABLE,
            marker_path
                .to_str()
                .ok_or("marker path must be valid UTF-8")?,
        )],
    )?;

    assert_eq!(
        report.exit_code, 0,
        "a worn skeleton's own build script must never run, and sync must still write its file; \
         stderr was: {}",
        report.stderr
    );
    assert!(
        !marker_path.exists(),
        "the worn skeleton's build script must never run, but its marker file exists at {}",
        marker_path.display()
    );
    assert_eq!(
        fixture.read("thing.txt")?,
        b"build script must never run\n",
        "sync must write the render's own bytes, exactly"
    );
    Ok(())
}

#[test]
fn sync_with_a_missing_lockfile_aborts_and_writes_nothing() -> support::TestOutcome {
    // No `Cargo.lock` is ever generated for this fixture. `sync` reads the
    // workspace with `cargo metadata --locked --offline`, and `--locked`
    // refuses to write a lockfile that is missing or out of date, so this
    // must abort the whole command (exit 1) before ever resolving what the
    // workspace wears — never silently generating a lockfile of its own,
    // and never writing the claimed file it would otherwise create.
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    // Deliberately no `fixture.generate_lockfile()` call, and no pre-existing
    // `plain.yml`: both must stay exactly as absent as they started.
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 1,
        "a missing lockfile must abort sync with a task refusal, not exit 0 or 2; stdout was: \
         {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report
            .stderr
            .lines()
            .any(|line| line.contains("Cargo.lock is missing or out of date")),
        "the abort must name the missing-or-out-of-date lockfile; stderr was: {}",
        report.stderr
    );
    assert!(
        fixture.read("plain.yml").is_err(),
        "a missing lockfile must leave sync having written nothing at all"
    );
    assert!(
        fixture.read("Cargo.lock").is_err(),
        "sync reads a lockfile without ever writing one, even when aborting because it is \
         missing"
    );
    Ok(())
}
