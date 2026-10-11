//! Acceptance: what `remove` does with the files a skeleton claims.
//!
//! It takes them with it when that is plainly what was asked (every claimed
//! file still matches what the skeleton renders), keeps them all when any has
//! changed, and does exactly what `--delete-files` and `--keep-files` say
//! otherwise. A file git could not give back is left in place and named. Each
//! scenario reads the disk back and holds stdout to whole lines, word for
//! word, built from the fixture's own paths.
//!
//! The scenarios wear the checked-in `nested-dotfiles` skeleton, which claims
//! `.github/dependabot.yml`, `a/b/deep.yml` and `root.yml` (directories to
//! empty, a dot directory among them), or `passthrough-plain`, which claims
//! `plain.yml` alone.

mod support;

use std::collections::BTreeMap;
use std::fs;

use support::remove::{
    Entry, HOLDING_DIRECTORY, MANIFEST, Worn, assert_check_wears_nothing, assert_printed,
    assert_refused, commit_line, kept_other_summary, kept_summary, mark, no_dependency_line,
    porcelain, run_remove, stopped_wearing_line, took_out_line, tree_state,
};
use support::wear::{commit_everything, fixture_for_wearing, git_step};
use support::{Fixture, TestOutcome, json, path_dependency_on_test_skeleton, wearing_table};

const NESTED: &str = "nested-dotfiles";

/// The stopped-wearing line for `nested-dotfiles` worn under its own name.
fn stopped_nested() -> String {
    stopped_wearing_line(NESTED, NESTED, MANIFEST)
}

/// Overwrites `relative` with `bytes` and commits, so the next `remove` runs
/// in a clean tree where that file no longer matches the skeleton.
fn change_and_commit(fixture: &Fixture, relative: &str, bytes: &[u8]) -> TestOutcome {
    fixture.write(relative, bytes)?;
    commit_everything(fixture, "fixture: a local change")
}

#[test]
fn with_every_file_matching_the_wearing_and_the_files_and_emptied_directories_go() -> TestOutcome {
    // Every claimed file is as `sync` wrote it. Bare `remove` must delete all
    // three, take the three directories they leave empty (`.github`, `a/b`
    // and then `a`) with them, leave the workspace root and the unclaimed
    // `src` alone, leave no holding directory behind, and leave git showing
    // exactly the manifest, the lockfile and the three deletions as changes.
    let worn = Worn::synced(NESTED, None)?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            "deleted a/b/deep.yml".to_owned(),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 3),
        ],
    );
    let root = worn.fixture.root();
    for gone in [".github", "a", "root.yml", HOLDING_DIRECTORY] {
        assert!(
            !fs::exists(root.join(gone))?,
            "{gone} must not be left in the work tree"
        );
    }
    assert!(
        fs::exists(root.join("src/lib.rs"))?,
        "files the skeleton does not claim must stay"
    );
    assert_eq!(
        porcelain(&worn.fixture)?,
        [
            "D .github/dependabot.yml",
            "M Cargo.lock",
            "M Cargo.toml",
            "D a/b/deep.yml",
            "D root.yml",
        ],
        "git must see the manifest, the lockfile and the deletions, and nothing else"
    );
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn a_directory_that_still_holds_something_stays() -> TestOutcome {
    // `a` holds the skeleton's `a/b/deep.yml` and also a tracked file of the
    // project's own. Deleting the skeleton's file empties `a/b` but not `a`,
    // so `a/b` goes and `a`, with its file, stays.
    let worn = Worn::synced(NESTED, None)?;
    change_and_commit(&worn.fixture, "a/keep.txt", b"the project's own\n")?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_eq!(report.exit_code, 0, "stderr was: {}", report.stderr);
    let root = worn.fixture.root();
    assert!(
        !fs::exists(root.join("a/b"))?,
        "the directory the deletion emptied must go"
    );
    assert_eq!(
        worn.fixture.read("a/keep.txt")?,
        b"the project's own\n",
        "a directory that still holds a file, and that file, must stay"
    );
    Ok(())
}

#[test]
fn with_one_file_changed_the_wearing_goes_and_every_file_stays_and_is_listed() -> TestOutcome {
    // `root.yml` has a committed local edit. Bare `remove` must stop wearing
    // the skeleton, delete nothing, say which file changed, list all three in
    // path order, and leave every byte as it was.
    let worn = Worn::synced(NESTED, None)?;
    change_and_commit(&worn.fixture, "root.yml", b"root-file: no\n")?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "kept .github/dependabot.yml, which matches the skeleton".to_owned(),
            "kept a/b/deep.yml, which matches the skeleton".to_owned(),
            "kept root.yml, which has changed".to_owned(),
            kept_summary(3, 1),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)?;
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn with_several_files_changed_the_summary_counts_them() -> TestOutcome {
    // Two of the three files have changed, so the summary says "2 have
    // changed". Both changed files are named; the matching one says it matches.
    let worn = Worn::synced(NESTED, None)?;
    change_and_commit(&worn.fixture, "root.yml", b"root-file: no\n")?;
    change_and_commit(&worn.fixture, "a/b/deep.yml", b"nested: no\n")?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "kept .github/dependabot.yml, which matches the skeleton".to_owned(),
            "kept a/b/deep.yml, which has changed".to_owned(),
            "kept root.yml, which has changed".to_owned(),
            kept_summary(3, 2),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)
}

#[test]
fn a_skeleton_with_one_file_says_so_when_that_file_has_changed() -> TestOutcome {
    // `passthrough-plain` claims one file, so the summary is the singular form.
    let worn = Worn::synced("passthrough-plain", None)?;
    change_and_commit(&worn.fixture, "plain.yml", b"name: mine\n")?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &["passthrough-plain"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "passthrough-plain", MANIFEST),
            "kept plain.yml, which has changed".to_owned(),
            kept_summary(1, 1),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)
}

/// Asserts the tree differs from `before` in the manifest and the lockfile
/// and nowhere else, and that the manifest no longer holds a wearing table.
fn assert_only_the_manifest_and_lockfile_differ(
    fixture: &Fixture,
    before: &BTreeMap<String, Entry>,
) -> TestOutcome {
    let after = tree_state(fixture.root())?;
    let others = |state: &'_ BTreeMap<String, Entry>| -> Vec<(String, String)> {
        state
            .iter()
            .filter(|(path, _)| path.as_str() != MANIFEST && path.as_str() != "Cargo.lock")
            .map(|(path, entry)| (path.clone(), format!("{entry:?}")))
            .collect()
    };
    assert_eq!(
        others(&after),
        others(before),
        "every file and directory but the manifest and the lockfile must be as it was"
    );
    let manifest = String::from_utf8(fixture.read(MANIFEST)?)?;
    assert!(
        !manifest.contains("[package.metadata.skeletons."),
        "the wearing table must be gone; manifest was:\n{manifest}"
    );
    Ok(())
}

#[test]
fn delete_files_deletes_a_changed_file_and_does_not_refuse_a_missing_one() -> TestOutcome {
    // `root.yml` has changed and `a/b/deep.yml` is gone. `--delete-files`
    // deletes everything still there, changed or not, prints nothing for the
    // missing file, and does not treat it as a reason to refuse.
    let worn = Worn::synced(NESTED, None)?;
    change_and_commit(&worn.fixture, "root.yml", b"root-file: no\n")?;
    fs::remove_file(worn.fixture.root().join("a/b/deep.yml"))?;
    commit_everything(&worn.fixture, "fixture: a file removed")?;

    let report = run_remove(&worn.fixture, &[NESTED, "--delete-files"])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    let root = worn.fixture.root();
    for gone in [".github/dependabot.yml", "root.yml"] {
        assert!(!fs::exists(root.join(gone))?, "{gone} must be deleted");
    }
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn a_missing_file_is_no_reason_to_keep_the_rest_when_the_others_match() -> TestOutcome {
    // `a/b/deep.yml` is gone and the other two match. Bare `remove` has
    // nothing to guess about: it deletes the two that are there.
    let worn = Worn::synced(NESTED, None)?;
    fs::remove_file(worn.fixture.root().join("a/b/deep.yml"))?;
    commit_everything(&worn.fixture, "fixture: a file removed")?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    assert!(
        !fs::exists(worn.fixture.root().join("root.yml"))?,
        "root.yml must be deleted"
    );
    Ok(())
}

#[test]
fn when_no_claimed_file_is_in_the_work_tree_it_says_there_was_nothing_to_delete() -> TestOutcome {
    // The skeleton was worn and never synced, so its file was never written.
    let worn = Worn::unsynced("passthrough-plain", None)?;

    let report = run_remove(&worn.fixture, &["passthrough-plain"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "passthrough-plain", MANIFEST),
            "none of its files is in the work tree, so there was nothing to delete".to_owned(),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_eq!(
        worn.fixture.read(MANIFEST)?,
        worn.manifest_before_wearing,
        "the manifest must be byte-identical to what it was before wear"
    );
    Ok(())
}

#[test]
fn keep_files_stops_wearing_and_leaves_every_file_changed_or_not() -> TestOutcome {
    // `root.yml` has changed and the others match. `--keep-files` stops
    // wearing the skeleton, says it left every file in place, and touches no
    // file at all.
    let worn = Worn::synced(NESTED, None)?;
    change_and_commit(&worn.fixture, "root.yml", b"root-file: no\n")?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED, "--keep-files"])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "left every file it claims in place".to_owned(),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)?;
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn keep_files_leaves_files_that_all_match_too() -> TestOutcome {
    // The flag decides, not the files: with every file matching, where bare
    // `remove` would delete them, `--keep-files` leaves them.
    let worn = Worn::synced(NESTED, None)?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED, "--keep-files"])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "left every file it claims in place".to_owned(),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)
}

#[test]
fn a_claimed_file_git_ignores_is_left_and_named_while_the_others_are_deleted() -> TestOutcome {
    // `a/b/deep.yml` is taken out of git's index and ignored, so it is on disk
    // but git could not give it back. The other two are tracked and match.
    // `remove` deletes those two, leaves the ignored one in place and says why.
    let worn = Worn::synced(NESTED, None)?;
    git_step(
        &worn.fixture,
        &["rm", "--cached", "--quiet", "--", "a/b/deep.yml"],
    )?;
    support::git::ignore(worn.fixture.root(), "a/b/deep.yml")?;
    commit_everything(&worn.fixture, "fixture: ignore a claimed file")?;
    assert_eq!(
        porcelain(&worn.fixture)?,
        Vec::<String>::new(),
        "precondition: git reports nothing, so the ignored file is no dirt"
    );

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            "left a/b/deep.yml in place: git does not track it, so git could not give it back; \
             delete it by hand if it should go"
                .to_owned(),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    assert_eq!(
        worn.fixture.read("a/b/deep.yml")?,
        b"nested: yes\n",
        "the ignored file must stay as it was"
    );
    Ok(())
}

/// Marks `a/b/deep.yml` with `flag`, runs bare `remove`, and asserts that file
/// is left in place with `reason` and the other two are deleted.
fn assert_a_hidden_file_is_left(flag: &str, reason: &str) -> TestOutcome {
    let worn = Worn::synced(NESTED, None)?;
    mark(&worn.fixture, "a/b/deep.yml", flag)?;
    assert_eq!(
        porcelain(&worn.fixture)?,
        Vec::<String>::new(),
        "precondition: git reports nothing for the marked file"
    );

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            format!(
                "left a/b/deep.yml in place: it is marked {reason} in git's index, so git could \
                 not give it back; delete it by hand if it should go"
            ),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    assert_eq!(
        worn.fixture.read("a/b/deep.yml")?,
        b"nested: yes\n",
        "the marked file must stay as it was"
    );
    Ok(())
}

#[test]
fn a_claimed_file_marked_skip_worktree_is_left_and_named() -> TestOutcome {
    assert_a_hidden_file_is_left("--skip-worktree", "skip-worktree")
}

#[test]
fn a_claimed_file_marked_assume_unchanged_is_left_and_named() -> TestOutcome {
    assert_a_hidden_file_is_left("--assume-unchanged", "assume-unchanged")
}

/// The line for `a/b/deep.yml` when it is left in place for `why`.
fn left_deep_line(why: &str) -> String {
    format!(
        "left a/b/deep.yml in place: {why}, so git could not give it back; delete it by hand if \
         it should go"
    )
}

#[test]
fn an_ignored_claimed_file_that_has_changed_is_left_and_does_not_make_the_others_stay()
-> TestOutcome {
    // `a/b/deep.yml` is taken out of git's index and ignored, then edited on
    // disk, so it differs from the render and git reports nothing. The other
    // two are tracked and match. Git could not give the edited file back, so
    // it is not a change to the project that bare `remove` would protect: the
    // two tracked files are deleted, the ignored one stays with its edit, and
    // it is named with its reason.
    let worn = Worn::synced(NESTED, None)?;
    git_step(
        &worn.fixture,
        &["rm", "--cached", "--quiet", "--", "a/b/deep.yml"],
    )?;
    support::git::ignore(worn.fixture.root(), "a/b/deep.yml")?;
    commit_everything(&worn.fixture, "fixture: ignore a claimed file")?;
    worn.fixture.write("a/b/deep.yml", b"nested: mine\n")?;
    assert_eq!(
        porcelain(&worn.fixture)?,
        Vec::<String>::new(),
        "precondition: git reports nothing, so the edit to the ignored file is no dirt"
    );

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            left_deep_line("git does not track it"),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    assert_eq!(
        worn.fixture.read("a/b/deep.yml")?,
        b"nested: mine\n",
        "the ignored file must stay with the edit it holds"
    );
    for gone in [".github/dependabot.yml", "root.yml"] {
        assert!(!fs::exists(worn.fixture.root().join(gone))?, "{gone}");
    }
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn a_skip_worktree_claimed_file_that_has_changed_is_left_and_does_not_make_the_others_stay()
-> TestOutcome {
    // `a/b/deep.yml` is marked skip-worktree and then edited locally, which
    // git does not report. Git would not read the edit, so it could not give
    // the file back: it is left, named, and the two matching tracked files are
    // deleted.
    let worn = Worn::synced(NESTED, None)?;
    mark(&worn.fixture, "a/b/deep.yml", "--skip-worktree")?;
    worn.fixture.write("a/b/deep.yml", b"nested: mine\n")?;
    assert_eq!(
        porcelain(&worn.fixture)?,
        Vec::<String>::new(),
        "precondition: git reports nothing for the marked file"
    );

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "deleted .github/dependabot.yml".to_owned(),
            left_deep_line("it is marked skip-worktree in git's index"),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 2),
        ],
    );
    assert_eq!(
        worn.fixture.read("a/b/deep.yml")?,
        b"nested: mine\n",
        "the marked file must stay with the edit it holds"
    );
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn a_tracked_file_that_changed_keeps_the_others_and_the_file_git_could_not_give_back_is_named()
-> TestOutcome {
    // `a/b/deep.yml` is ignored and edited (git could not give it back),
    // `root.yml` is tracked and has a committed edit, and
    // `.github/dependabot.yml` matches. The tracked edit is the project's own,
    // so the two files git can give back are kept and nothing is deleted; the
    // ignored file is still named with its reason, in its place in path order,
    // and the summary counts only the files kept for the edit.
    let worn = Worn::synced(NESTED, None)?;
    git_step(
        &worn.fixture,
        &["rm", "--cached", "--quiet", "--", "a/b/deep.yml"],
    )?;
    support::git::ignore(worn.fixture.root(), "a/b/deep.yml")?;
    commit_everything(&worn.fixture, "fixture: ignore a claimed file")?;
    worn.fixture.write("a/b/deep.yml", b"nested: mine\n")?;
    change_and_commit(&worn.fixture, "root.yml", b"root-file: no\n")?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    assert_printed(
        &report,
        &[
            stopped_nested(),
            "kept .github/dependabot.yml, which matches the skeleton".to_owned(),
            left_deep_line("git does not track it"),
            "kept root.yml, which has changed".to_owned(),
            kept_other_summary(2, 1),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)?;
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn when_git_could_not_give_back_any_present_file_each_is_named_and_nothing_else_is_said()
-> TestOutcome {
    // All three claimed files are ignored and edited, so there is nothing for
    // the rule to keep or delete: the three `left` lines stand alone, with no
    // summary, and the last line names no deleted file.
    let worn = Worn::synced(NESTED, None)?;
    let paths = [".github/dependabot.yml", "a/b/deep.yml", "root.yml"];
    for path in paths {
        git_step(&worn.fixture, &["rm", "--cached", "--quiet", "--", path])?;
        support::git::ignore(worn.fixture.root(), path)?;
    }
    commit_everything(&worn.fixture, "fixture: ignore every claimed file")?;
    for path in paths {
        worn.fixture.write(path, b"mine: yes\n")?;
    }
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[NESTED])?;

    let left = |path: &str| {
        format!(
            "left {path} in place: git does not track it, so git could not give it back; delete \
             it by hand if it should go"
        )
    };
    assert_printed(
        &report,
        &[
            stopped_nested(),
            left(".github/dependabot.yml"),
            left("a/b/deep.yml"),
            left("root.yml"),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_only_the_manifest_and_lockfile_differ(&worn.fixture, &before)?;
    assert_check_wears_nothing(&worn.fixture)
}

/// A workspace wearing `nested-dotfiles` with an option the skeleton does not
/// declare, so `check` refuses the wearing and the files it claims cannot be
/// known. Committed clean. With the reasons `check` gives, joined by `; `.
fn fixture_with_unknowable_files() -> Result<(Fixture, String), Box<dyn std::error::Error>> {
    let extra = format!(
        "[dev-dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton(NESTED, NESTED),
        wearing_table(NESTED, "bogus = \"x\"\n"),
    );
    let fixture = fixture_for_wearing(&extra)?;
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    let document = json::parse(&report.stdout)?;
    let mut reasons = Vec::new();
    for refusal in json::refusals(&document)? {
        if json::refusal_dependency(refusal)? == Some(NESTED) {
            reasons.push(json::refusal_message(refusal)?.to_owned());
        }
    }
    assert!(
        !reasons.is_empty(),
        "precondition: check must refuse this wearing; stdout was: {}",
        report.stdout
    );
    Ok((fixture, reasons.join("; ")))
}

#[test]
fn a_wearing_whose_files_cannot_be_known_is_refused_bare_and_with_delete_files() -> TestOutcome {
    // `check` refuses the wearing, so which files it claims is unknown.
    // Deleting any would be a guess, so both ways of deleting refuse, with
    // `check`'s own reasons in the line and `--keep-files` as the way out, and
    // write nothing.
    let (fixture, reasons) = fixture_with_unknowable_files()?;
    let before = tree_state(fixture.root())?;

    for arguments in [vec![NESTED], vec![NESTED, "--delete-files"]] {
        let report = run_remove(&fixture, &arguments)?;

        assert_refused(
            &report,
            &format!(
                "remove cannot tell which files `{NESTED}` claims, so it wrote nothing: \
                 {reasons}; fix that and run the `remove` task again, or give it --keep-files \
                 to stop wearing `{NESTED}` and leave every file in place"
            ),
        );
    }

    assert_eq!(
        tree_state(fixture.root())?,
        before,
        "a refused remove must leave the work tree as it was"
    );
    Ok(())
}

#[test]
fn a_wearing_whose_files_cannot_be_known_proceeds_with_keep_files() -> TestOutcome {
    // The same refused wearing, with `--keep-files`: nothing needs to be known
    // about the files, so the dependency and the table come out, and the
    // first line is the one for a refused wearing that has a dependency.
    let (fixture, _reasons) = fixture_with_unknowable_files()?;

    let report = run_remove(&fixture, &[NESTED, "--keep-files"])?;

    assert_printed(
        &report,
        &[
            took_out_line(NESTED, MANIFEST),
            "left every file it claims in place".to_owned(),
            commit_line(MANIFEST, true, 0),
        ],
    );
    let manifest = String::from_utf8(fixture.read(MANIFEST)?)?;
    assert!(
        !manifest.contains(NESTED),
        "neither the dependency nor the table may remain; manifest was:\n{manifest}"
    );
    Ok(())
}

#[test]
fn a_wearing_table_with_no_dependency_is_taken_out_with_keep_files() -> TestOutcome {
    // `ghost` has a wearing table and no dependency. With `--keep-files` the
    // table comes out and the first line says no dependency was declared; the
    // lockfile is not named, since nothing about it changed.
    let fixture = fixture_for_wearing(&wearing_table("ghost", ""))?;

    let report = run_remove(&fixture, &["ghost", "--keep-files"])?;

    assert_printed(
        &report,
        &[
            no_dependency_line("ghost", MANIFEST),
            "left every file it claims in place".to_owned(),
            commit_line(MANIFEST, false, 0),
        ],
    );
    let manifest = String::from_utf8(fixture.read(MANIFEST)?)?;
    assert!(
        !manifest.contains("ghost"),
        "the table must be gone; manifest was:\n{manifest}"
    );
    Ok(())
}
