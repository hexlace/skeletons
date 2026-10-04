//! Acceptance: `sync`'s dirty-worktree refusal — the whole worktree must be
//! clean (not only the paths `sync` is about to write), and every claimed
//! path needs positive proof git can give its current bytes back, not
//! merely git's silence about it.
//!
//! Git staying silent about a path only proves the path is clean if git can
//! see that path, and a dirty entry git reports that cannot be matched to a
//! claim must refuse rather than be dropped. The scenarios below are the
//! ones where a query scoped to the claimed paths would have missed real
//! uncommitted work: a submodule, a nested non-submodule repository, an
//! NFD-spelled file on macOS, and the index flags and sparse checkouts that
//! make git stop reporting a path. Each is worked through in turn.
//!
//! Every refusal test here reads the protected file's own bytes back
//! afterward, never trusting the exit code alone, and checks the refusal's
//! own message names the path it is about.

mod support;

use support::passthrough_plain::{
    PASSTHROUGH_PLAIN_RENDER, fixture_wearing_passthrough_plain,
    fixture_wearing_passthrough_plain_with_clean_baseline,
};
use support::{
    Fixture, TemporaryDirectory, path_dependency_on, path_dependency_on_test_skeleton,
    wearing_table, write_minimal_skeleton, write_package_manifest,
};

/// Asserts `report` refused (non-zero exit) and that its combined output
/// names `needle` — the one property every refusal test here checks beyond
/// the exit code: not merely that something failed, but that the message
/// names the actual cause.
fn assert_refused_naming(report: &support::Report, needle: &str) {
    assert_ne!(
        report.exit_code, 0,
        "sync must refuse; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains(needle),
        "the refusal must name {needle:?}; combined output was: {combined:?}"
    );
}

// ---------------------------------------------------------------------
// Dirt at the claimed path itself, including shapes plain `git status`
// hides or misreports: it shows nothing at all for an edit under
// `--assume-unchanged` or `--skip-worktree`, and a name spelled another way
// or a nested repository can read as something else.
// ---------------------------------------------------------------------

/// A fixture wearing `nested-dotfiles` (claims `.github/dependabot.yml`,
/// `a/b/deep.yml`, `root.yml`), with `root.yml` and `a/b/deep.yml` already
/// written matching their own renders — so every scenario below is about
/// the spelling or visibility of `.github/dependabot.yml` alone, and
/// `sync`'s own all-or-nothing rule is what proves the other two claimed
/// files were never touched by a refusal caused by the third.
fn fixture_wearing_nested_dotfiles_with_two_of_three_matching()
-> Result<Fixture, Box<dyn std::error::Error>> {
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
    Ok(fixture)
}

/// Asserts a refused `sync` against `fixture_wearing_nested_dotfiles_with_two_of_three_matching`
/// left every one of the three claimed files untouched (all-or-nothing),
/// given `dependabot_expected` — what `.github/dependabot.yml` must hold
/// (its own precious, pre-existing content).
#[expect(
    clippy::expect_used,
    reason = "a test assertion helper; a read that fails here is this suite's own fixture being \
              wrong, exactly what `Fixture::read`'s other callers propagate with `?` instead — \
              this helper has no `Result` of its own to propagate one through"
)]
fn assert_nested_dotfiles_all_or_nothing(fixture: &Fixture, dependabot_expected: &[u8]) {
    assert_eq!(
        fixture
            .read(".github/dependabot.yml")
            .expect("must still exist"),
        dependabot_expected,
        "the file at risk must survive with its own precious content exactly"
    );
    assert_eq!(
        fixture.read("root.yml").expect("must still exist"),
        b"root-file: yes\n",
        "sync is all-or-nothing: an unrelated, already-matching claimed file must not be \
         rewritten by a refused run either"
    );
    assert_eq!(
        fixture.read("a/b/deep.yml").expect("must still exist"),
        b"nested: yes\n",
        "sync is all-or-nothing: an unrelated, already-matching claimed file must not be \
         rewritten by a refused run either"
    );
}

/// Builds a plain (non-submodule) git repository at a fresh temporary
/// directory, holding one file, committed — the source a submodule is
/// added from, or a repository planted directly inside a wearing tree.
fn build_plain_repository_with_file(
    file_name: &str,
    content: &[u8],
) -> Result<(TemporaryDirectory, TemporaryDirectory), Box<dyn std::error::Error>> {
    let directory = TemporaryDirectory::new("sync-worktree-plain-repository")?;
    let home = TemporaryDirectory::new("sync-worktree-plain-repository-home")?;
    support::write(directory.path(), file_name, content)?;
    support::git::run(
        directory.path(),
        home.path(),
        &["init", "--quiet"],
        "git init",
    )?;
    support::git::run(directory.path(), home.path(), &["add", "--all"], "git add")?;
    support::git::run(
        directory.path(),
        home.path(),
        &["commit", "--quiet", "--message", "source repository"],
        "git commit",
    )?;
    Ok((directory, home))
}

#[test]
fn a_submodule_edit_at_a_claimed_path_is_refused_and_content_preserved() -> support::TestOutcome {
    // `.github` is a submodule, and `.github/dependabot.yml` — inside it —
    // holds an uncommitted edit.
    // `submodule.<name>.ignore=all` and `diff.ignoreSubmodules=all` are set
    // too, proving `sync` sees the edit despite configuration that would
    // otherwise hide it from a plain `git status`.
    let (source, source_home) =
        build_plain_repository_with_file("dependabot.yml", b"dependabot: true\n")?;
    let fixture = fixture_wearing_nested_dotfiles_with_two_of_three_matching()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();

    support::git::run(root, home, &["init", "--quiet"], "git init")?;
    let source_url = format!("file://{}", source.path().display());
    support::git::run(
        root,
        home,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            &source_url,
            ".github",
        ],
        "git submodule add",
    )?;
    support::git::run(
        root,
        home,
        &["config", "submodule.\".github\".ignore", "all"],
        "git config submodule ignore",
    )?;
    support::git::run(
        root,
        home,
        &["config", "diff.ignoreSubmodules", "all"],
        "git config diff.ignoreSubmodules",
    )?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "fixture: add submodule"],
        "git commit",
    )?;
    drop(source_home);

    let edited = b"dependabot: EDITED, NEVER COMMITTED\n";
    fixture.write(".github/dependabot.yml", edited)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    // `.github` holds a `.git` of its own, so the claim walk `check` and
    // `sync` share refuses `.github/dependabot.yml` as lying inside another
    // repository before git is asked anything. The whole-tree question
    // (rule (a), "The whole work tree is clean", in `.docs/design.md`) that a
    // submodule edit is dirty even under `ignore=all` is pinned on its own, in
    // `crates/skeletons/src/work_tree/clean.rs`, where no claim walk stands in front
    // of it.
    assert_refused_naming(
        &report,
        ".github/dependabot.yml is inside .github, which holds a .git of its own",
    );
    assert_nested_dotfiles_all_or_nothing(&fixture, edited);
    Ok(())
}

#[test]
fn a_nested_non_submodule_repository_holding_an_untracked_file_at_a_claimed_path_is_refused()
-> support::TestOutcome {
    // A nested repository, untracked file: `.github/` is its own,
    // unrelated `git init` — never registered as a submodule at all — and
    // `dependabot.yml` inside it is untracked (as far as *that* inner
    // repository is concerned). The superproject's own scoped status sees
    // nothing there at all.
    let fixture = fixture_wearing_nested_dotfiles_with_two_of_three_matching()?;
    fixture.init_git_repository()?;

    // `.github/` does not exist at all yet (`nested-dotfiles`'s own claim
    // there is what this test is about) — `git init` needs the directory
    // to already exist as its own working directory.
    std::fs::create_dir_all(fixture.root().join(".github"))?;
    let inner_home = TemporaryDirectory::new("sync-worktree-nested-repo-home")?;
    support::git::run(
        &fixture.root().join(".github"),
        inner_home.path(),
        &["init", "--quiet"],
        "git init (nested)",
    )?;
    let untracked = b"dependabot: untracked inside a nested repository\n";
    fixture.write(".github/dependabot.yml", untracked)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    // The superproject's own status shows an entirely unrelated nested
    // repository as one untracked directory, but the claim walk gets there
    // first: `.github` holds a `.git`, so the claim inside it is refused as
    // lying in another repository, before git is asked anything.
    assert_refused_naming(
        &report,
        ".github/dependabot.yml is inside .github, which holds a .git of its own",
    );
    assert_nested_dotfiles_all_or_nothing(&fixture, untracked);
    Ok(())
}

#[test]
fn a_nested_non_submodule_repository_holding_a_committed_edited_claimed_file_is_refused()
-> support::TestOutcome {
    // A nested repository, tracked edit: the same nested, non-submodule
    // repository, except `dependabot.yml` is committed *inside* it, then
    // edited without being committed again — the tracked-but-modified
    // shape, entirely invisible to the superproject the same way.
    let fixture = fixture_wearing_nested_dotfiles_with_two_of_three_matching()?;
    fixture.init_git_repository()?;

    let inner_home = TemporaryDirectory::new("sync-worktree-nested-repo-home-2")?;
    let inner_root = fixture.root().join(".github");
    support::write(
        &inner_root,
        "dependabot.yml",
        b"dependabot: committed inside nested\n",
    )?;
    support::git::run(
        &inner_root,
        inner_home.path(),
        &["init", "--quiet"],
        "git init (nested)",
    )?;
    support::git::run(
        &inner_root,
        inner_home.path(),
        &["add", "--all"],
        "git add (nested)",
    )?;
    support::git::run(
        &inner_root,
        inner_home.path(),
        &["commit", "--quiet", "--message", "nested: initial"],
        "git commit (nested)",
    )?;

    let edited = b"dependabot: EDITED inside the nested repository, not committed\n";
    fixture.write(".github/dependabot.yml", edited)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    // As above: whether the file inside the nested repository is itself
    // tracked by it makes no difference to the claim walk, which refuses
    // any claim inside a directory that holds a `.git`.
    assert_refused_naming(
        &report,
        ".github/dependabot.yml is inside .github, which holds a .git of its own",
    );
    assert_nested_dotfiles_all_or_nothing(&fixture, edited);
    Ok(())
}

/// `None` when git lists the untracked `nfd_name` in the precomposed (NFC)
/// spelling of the same name, which is the premise of the NFD scenario below;
/// otherwise what git listed instead, for the skip message.
fn git_does_not_list_as_precomposed(
    fixture: &Fixture,
    nfd_name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let listed = support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        &[
            "-c",
            "core.quotepath=false",
            "ls-files",
            "--others",
            "--",
            nfd_name,
        ],
        "git ls-files --others",
    )?;
    Ok((listed.trim() != "caf\u{e9}.txt").then_some(listed))
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn an_nfd_spelled_claimed_file_untracked_on_disk_is_refused_on_a_precomposing_filesystem()
-> support::TestOutcome {
    // A spelling git normalises: on macOS, `core.precomposeunicode=true`
    // (the default) makes git report a path as NFC even when the claim,
    // and the file actually on disk, are spelled NFD — the exact byte
    // sequences differ, so a plain string comparison between the claim and
    // whatever git reports never matches, and the untracked file at that
    // path would be treated as though nothing were there at all.
    //
    // The premise — git reporting an NFD-named file back as NFC — depends on
    // the filesystem and on git's `core.precomposeunicode`, so it is checked
    // at run time below, once the file exists, and the test prints a visible
    // skip when it does not hold.

    // "café.txt", spelled NFD: `e` followed by a combining acute accent
    // (U+0301), never the single precomposed `é` (U+00E9) codepoint —
    // constructed explicitly so this file's own on-disk encoding (however
    // an editor normalized it) can never silently turn this into the NFC
    // form instead.
    let nfd_name = "cafe\u{0301}.txt";
    let skeleton = TemporaryDirectory::new("sync-worktree-nfd-skeleton")?;
    write_minimal_skeleton(skeleton.path(), "nfd-skeleton", nfd_name, "nfd-content\n")?;

    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on("nfd-skeleton", skeleton.path()),
        wearing_table("nfd-skeleton", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;

    let untracked = b"precious, exists nowhere else, spelled NFD on disk\n";
    fixture.write(nfd_name, untracked)?;

    if let Some(listed) = git_does_not_list_as_precomposed(&fixture, nfd_name)? {
        eprintln!(
            "skipped: this filesystem/git does not report an NFD name as NFC (listed as {listed:?})"
        );
        return Ok(());
    }

    let report = fixture.run(&["skeletons", "sync"])?;
    // sync must refuse an untracked file sitting at an NFD-spelled claimed
    // path even though git reports the directory's own listing back as NFC:
    // the working tree is not clean, and the file is named as untracked, in
    // whichever spelling git reports it.
    assert_refused_naming(&report, "uncommitted change");
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains(&format!("{nfd_name} is untracked"))
            || combined.contains("caf\u{e9}.txt is untracked"),
        "the refusal must name the untracked file; combined output was: {combined:?}"
    );
    assert_eq!(
        fixture.read(nfd_name)?,
        untracked,
        "the untracked NFD-spelled file's own content must survive exactly"
    );
    Ok(())
}

#[test]
fn an_assume_unchanged_edit_at_a_claimed_path_is_refused() -> support::TestOutcome {
    // `git update-index --assume-unchanged` makes `git status` itself
    // report nothing at all for the path, even though the working tree has
    // genuinely diverged from what git holds — proven separately, first,
    // by confirming the scoped status query really does go silent.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();

    support::git::run(
        root,
        home,
        &["update-index", "--assume-unchanged", "plain.yml"],
        "git update-index --assume-unchanged",
    )?;
    let edited = b"name: EDITED, hidden by --assume-unchanged\nvalue: 0\n";
    fixture.write("plain.yml", edited)?;

    let status = support::git::status_porcelain(root, home)?;
    assert_eq!(
        status, "",
        "the positive control: git status itself must report nothing for an \
         --assume-unchanged path, which is exactly what makes this a sharp edge; status was: \
         {status:?}"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "plain.yml");
    assert_eq!(
        fixture.read("plain.yml")?,
        edited,
        "an --assume-unchanged edit must survive a refused sync exactly"
    );
    Ok(())
}

#[test]
fn a_skip_worktree_edit_at_a_claimed_path_is_refused() -> support::TestOutcome {
    // The same shape as --assume-unchanged, via sparse checkout's own
    // `--skip-worktree` bit instead.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();

    support::git::run(
        root,
        home,
        &["update-index", "--skip-worktree", "plain.yml"],
        "git update-index --skip-worktree",
    )?;
    let edited = b"name: EDITED, hidden by --skip-worktree\nvalue: 0\n";
    fixture.write("plain.yml", edited)?;

    let status = support::git::status_porcelain(root, home)?;
    assert_eq!(
        status, "",
        "the positive control: git status itself must report nothing for a --skip-worktree \
         path; status was: {status:?}"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "plain.yml");
    assert_eq!(
        fixture.read("plain.yml")?,
        edited,
        "a --skip-worktree edit must survive a refused sync exactly"
    );
    Ok(())
}

#[test]
fn an_intent_to_add_entry_at_a_claimed_path_is_refused() -> support::TestOutcome {
    // `git add -N` (`--intent-to-add`) stages a placeholder entry with no
    // real committed content behind it — `skeletons`' own positive-proof rule
    // says a claim's index entry must not be intent-to-add, since there is
    // no real blob behind it for git to check out in the first place.
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();

    let content = b"name: never really added, only intent-to-add\nvalue: 0\n";
    fixture.write("plain.yml", content)?;
    support::git::run(root, home, &["add", "-N", "plain.yml"], "git add -N")?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "plain.yml");
    assert_eq!(
        fixture.read("plain.yml")?,
        content,
        "an intent-to-add file's own content must survive a refused sync exactly"
    );
    Ok(())
}

#[test]
fn a_conflicted_claimed_path_is_refused() -> support::TestOutcome {
    // A real, unresolved merge conflict: `plain.yml` sits at stages 1, 2
    // and 3 in the index, its own working-tree bytes holding conflict
    // markers — content that exists nowhere as a single, clean blob at all.
    let fixture = fixture_wearing_passthrough_plain()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();

    fixture.write("plain.yml", b"base\n")?;
    support::git::run(root, home, &["init", "--quiet"], "git init")?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "base"],
        "git commit",
    )?;
    support::git::run(
        root,
        home,
        &["checkout", "--quiet", "-b", "feature"],
        "git checkout -b feature",
    )?;
    fixture.write("plain.yml", b"feature\n")?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "feature"],
        "git commit",
    )?;
    support::git::run(
        root,
        home,
        &["checkout", "--quiet", "main"],
        "git checkout main",
    )?;
    fixture.write("plain.yml", b"main\n")?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "main"],
        "git commit",
    )?;
    let merge = support::git::run_allow_failure(root, home, &["merge", "--no-edit", "feature"])?;
    assert!(
        !merge.status.success(),
        "the merge must genuinely conflict, or this fixture is not testing a conflicted path"
    );
    let conflicted = fixture.read("plain.yml")?;
    assert!(
        conflicted.windows(7).any(|window| window == b"<<<<<<<"),
        "the conflicted file must hold real conflict markers; content was: {:?}",
        String::from_utf8_lossy(&conflicted)
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "plain.yml");
    assert_eq!(
        fixture.read("plain.yml")?,
        conflicted,
        "a conflicted claimed path's own conflict markers must survive a refused sync exactly"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// The whole-worktree rule: dirt sitting elsewhere in the tree (never at
// any path sync is about to write) still blocks it.
// ---------------------------------------------------------------------

#[test]
fn a_mode_only_change_elsewhere_blocks_sync() -> support::TestOutcome {
    use std::os::unix::fs::PermissionsExt as _;

    // A mode-only change can never be the reason sync would touch the path
    // it happens on: sync only ever writes a path whose *bytes* have
    // drifted from the render, and a mode-only change leaves those bytes
    // untouched, so a mode-only change sitting at a *claimed* path is never
    // reachable as its own scenario — sync would see that path as already
    // matching and never look at it at all. What matters is the
    // whole-worktree rule: a mode-only change
    // on some unrelated, unclaimed file is still real, uncommitted work
    // git holds no record of, so it must still block sync exactly like
    // any other unrelated dirt does.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("unrelated.txt", b"content never touched by this test\n")?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(
        root,
        home,
        &["config", "core.fileMode", "true"],
        "git config core.fileMode",
    )?;

    let path = fixture.root().join("unrelated.txt");
    let mut permissions = std::fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions)?;

    let status = support::git::status_porcelain(root, home)?;
    // `status_porcelain` trims its own output (`git::run`'s own contract),
    // which removes the leading blank index-status column from the very
    // first line — so a real mode-only change (` M unrelated.txt`, blank
    // index column, `M` worktree column) reads back as `M unrelated.txt`
    // here, not ` M unrelated.txt`.
    assert_eq!(
        status, "M unrelated.txt",
        "the positive control: a mode-only change must show as modified in plain git status; \
         status was: {status:?}"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "unrelated.txt");
    assert!(
        fixture.read("plain.yml").is_err(),
        "the claimed file must never be written while a mode-only change sits elsewhere, \
         uncommitted"
    );
    Ok(())
}

#[test]
fn an_unrelated_untracked_file_elsewhere_blocks_sync() -> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    let elsewhere = b"never claimed by any skeleton\n";
    fixture.write("unrelated.txt", elsewhere)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "unrelated.txt");
    assert!(
        fixture.read("plain.yml").is_err(),
        "the claimed file must never be written while unrelated dirt sits elsewhere in the tree"
    );
    assert_eq!(
        fixture.read("unrelated.txt")?,
        elsewhere,
        "the unrelated file's own content must be left exactly as it was"
    );
    Ok(())
}

#[test]
fn an_unrelated_modified_tracked_file_elsewhere_blocks_sync() -> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("unrelated.txt", b"original, committed content\n")?;
    fixture.init_git_repository()?;
    let modified = b"modified, never committed\n";
    fixture.write("unrelated.txt", modified)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "unrelated.txt");
    assert!(
        fixture.read("plain.yml").is_err(),
        "the claimed file must never be written while unrelated dirt sits elsewhere in the tree"
    );
    assert_eq!(fixture.read("unrelated.txt")?, modified);
    Ok(())
}

#[test]
fn an_unrelated_staged_file_elsewhere_blocks_sync() -> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    let staged = b"staged, never committed\n";
    support::git::write_and_stage(
        fixture.root(),
        fixture.sandbox().home(),
        "unrelated.txt",
        staged,
    )?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "unrelated.txt");
    assert!(
        fixture.read("plain.yml").is_err(),
        "the claimed file must never be written while unrelated staged dirt sits elsewhere"
    );
    assert_eq!(fixture.read("unrelated.txt")?, staged);
    Ok(())
}

#[test]
fn status_show_untracked_no_does_not_hide_an_untracked_file_elsewhere() -> support::TestOutcome {
    // `status.showUntrackedFiles=no` makes a bare `git status` omit
    // untracked files entirely; the whole-worktree question has to pass an
    // explicit `--untracked-files=…` flag rather than trust the
    // configured default.
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        &["config", "status.showUntrackedFiles", "no"],
        "git config status.showUntrackedFiles",
    )?;
    let elsewhere = b"never claimed, hidden by status.showUntrackedFiles=no\n";
    fixture.write("unrelated.txt", elsewhere)?;

    let status = support::git::status_porcelain(fixture.root(), fixture.sandbox().home())?;
    assert_eq!(
        status, "",
        "the positive control: plain `git status` must itself see nothing here, which is what \
         makes the configuration a sharp edge; status was: {status:?}"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "unrelated.txt");
    assert!(fixture.read("plain.yml").is_err());
    assert_eq!(fixture.read("unrelated.txt")?, elsewhere);
    Ok(())
}

#[test]
fn an_ignored_file_elsewhere_does_not_block_sync() -> support::TestOutcome {
    // Ignored files never count as dirt, anywhere in the tree — the one
    // deliberate exception to the whole-worktree rule, since a project's build
    // output, `target/` above all, is ignored and making it block would make
    // `sync` unusable. This fixture stands `build-output/` in for it: it has
    // no `target/`, and the scenario only needs some ignored directory.
    let fixture = fixture_wearing_passthrough_plain()?;
    support::git::ignore(fixture.root(), "build-output/")?;
    fixture.init_git_repository()?;
    fixture.write("build-output/whatever.bin", b"incidental build output\n")?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "an ignored file elsewhere must never block sync; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        PASSTHROUGH_PLAIN_RENDER,
        "sync must still do its actual job — creating the claimed file — despite the ignored \
         file elsewhere"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Content filters on the claimed path itself: a genuinely clean file must
// never be refused just because its on-disk bytes are filtered; a
// genuinely edited one still must be.
// ---------------------------------------------------------------------

/// Builds a fixture wearing `passthrough-plain`, committing `plain.yml`
/// with `committed_content` (deliberately never equal to the render, so
/// `sync` always has something to write) under `repository_config` and,
/// when `gitattributes` is given, a `.gitattributes` holding that text —
/// both in place before the initial `git add`, so a content filter actually
/// applies at commit time — then runs `git checkout -- plain.yml` so the
/// working tree afterward holds exactly what git itself considers the
/// correct, filtered on-disk form — never a form this test merely guesses at.
fn fixture_with_committed_and_checked_out_plain_yml(
    repository_config: &[(&str, &str)],
    gitattributes: Option<&str>,
    committed_content: &[u8],
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = fixture_wearing_passthrough_plain()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(root, home, &["init", "--quiet"], "git init")?;
    for (key, value) in repository_config {
        support::git::run(root, home, &["config", key, value], "git config")?;
    }
    if let Some(attributes) = gitattributes {
        fixture.write(".gitattributes", attributes.as_bytes())?;
    }
    fixture.write("plain.yml", committed_content)?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &[
            "commit",
            "--quiet",
            "--message",
            "fixture: filtered baseline",
        ],
        "git commit",
    )?;
    support::git::run(
        root,
        home,
        &["checkout", "--", "plain.yml"],
        "git checkout -- plain.yml",
    )?;
    Ok(fixture)
}

#[test]
fn a_clean_file_under_a_configured_clean_smudge_filter_is_not_refused() -> support::TestOutcome {
    let fixture = fixture_with_committed_and_checked_out_plain_yml(
        &[
            ("filter.upper.clean", "tr 'a-z' 'A-Z'"),
            ("filter.upper.smudge", "tr 'A-Z' 'a-z'"),
        ],
        Some("plain.yml filter=upper\n"),
        b"name: lowercase baseline\nvalue: 1\n",
    )?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "a genuinely clean file under a real clean/smudge filter must not be refused; stdout \
         was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("plain.yml")?, PASSTHROUGH_PLAIN_RENDER);
    Ok(())
}

#[test]
fn an_edited_file_under_a_configured_clean_smudge_filter_is_refused() -> support::TestOutcome {
    let fixture = fixture_with_committed_and_checked_out_plain_yml(
        &[
            ("filter.upper.clean", "tr 'a-z' 'A-Z'"),
            ("filter.upper.smudge", "tr 'A-Z' 'a-z'"),
        ],
        Some("plain.yml filter=upper\n"),
        b"name: lowercase baseline\nvalue: 1\n",
    )?;
    let edited = b"name: edited, never committed\nvalue: 9\n".to_vec();
    fixture.write("plain.yml", &edited)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "plain.yml");
    assert_eq!(fixture.read("plain.yml")?, edited);
    Ok(())
}

#[test]
fn a_required_filter_whose_command_fails_causes_a_refusal() -> support::TestOutcome {
    // The filter is configured to always fail *after* the baseline commit
    // (which needed a working filter, or none, to succeed at all) — so git
    // itself cannot answer whether `plain.yml` is clean or not, and that
    // uncertainty, for a filter marked `required`, must refuse rather than
    // assume either way.
    let fixture = fixture_wearing_passthrough_plain()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    let committed_content = b"name: never matches the render, always drifted\nvalue: 1\n";
    fixture.write("plain.yml", committed_content)?;
    fixture.init_git_repository()?;

    std::fs::create_dir_all(root.join(".git/info"))?;
    support::write(root, ".git/info/attributes", b"plain.yml filter=broken\n")?;
    support::git::run(
        root,
        home,
        &["config", "filter.broken.clean", "false"],
        "git config",
    )?;
    support::git::run(
        root,
        home,
        &["config", "filter.broken.smudge", "cat"],
        "git config",
    )?;
    support::git::run(
        root,
        home,
        &["config", "filter.broken.required", "true"],
        "git config",
    )?;
    // Git runs a path's filter only when it has to compare content, and it
    // skips that whenever the file's stat data still matches the index. The
    // refusal under test comes from the whole-tree status running the
    // failing filter, so rewriting the same bytes (a newer modification
    // time) makes that comparison always happen; without it, the outcome
    // depends on whether the commit and the run land in the same timestamp
    // tick, and a run where status trusts the cached stat data reaches the
    // proof instead, where the file is exactly what git would check out.
    fixture.write("plain.yml", committed_content)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    // A refusal, not a silent guess either way: `git status` itself fails,
    // and the refusal carries git's own reason, naming the broken filter.
    assert_refused_naming(&report, "git status failed");
    assert_refused_naming(&report, "filter 'broken' failed");
    assert_eq!(
        fixture.read("plain.yml")?,
        committed_content,
        "the file must be left exactly as it was when its own required filter cannot answer"
    );
    Ok(())
}

/// Commits `plain.yml` in `fixture`'s repository under a required `lfs`
/// filter, then checks it out again so git sees the file as clean.
fn commit_plain_under_lfs_filter(fixture: &Fixture) -> support::TestOutcome {
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(root, home, &["init", "--quiet"], "git init")?;
    support::git::run(
        root,
        home,
        &["config", "filter.lfs.process", "git-lfs filter-process"],
        "git config filter.lfs.process",
    )?;
    support::git::run(
        root,
        home,
        &["config", "filter.lfs.clean", "git-lfs clean -- %f"],
        "git config filter.lfs.clean",
    )?;
    support::git::run(
        root,
        home,
        &["config", "filter.lfs.smudge", "git-lfs smudge -- %f"],
        "git config filter.lfs.smudge",
    )?;
    support::git::run(
        root,
        home,
        &["config", "filter.lfs.required", "true"],
        "git config filter.lfs.required",
    )?;
    fixture.write(".gitattributes", b"plain.yml filter=lfs -text\n")?;
    fixture.write(
        "plain.yml",
        b"content tracked by git-lfs, never matching the render\n",
    )?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "fixture: lfs baseline"],
        "git commit",
    )?;
    support::git::run(
        root,
        home,
        &["checkout", "--", "plain.yml"],
        "git checkout -- plain.yml",
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
fn a_clean_git_lfs_pointer_is_not_refused() -> support::TestOutcome {
    // Premise: `git lfs` is installed on the machine running this suite.
    // Checked at run time; a machine without it sees a visible skip rather
    // than a silent absence of coverage.
    // git itself failing to start is a fault in this machine's setup that
    // every other test here would hit too, so it fails the test; only `git
    // lfs` exiting unsuccessfully means git-lfs is not installed.
    let probe = std::process::Command::new("git")
        .args(["lfs", "version"])
        .output()?;
    if !probe.status.success() {
        eprintln!(
            "skipped: git-lfs is not installed on this machine ({}: {})",
            probe.status,
            String::from_utf8_lossy(&probe.stderr).trim()
        );
        return Ok(());
    }

    let fixture = fixture_wearing_passthrough_plain()?;
    commit_plain_under_lfs_filter(&fixture)?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "a genuinely clean git-lfs pointer must not be refused; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read("plain.yml")?, PASSTHROUGH_PLAIN_RENDER);
    Ok(())
}

// ---------------------------------------------------------------------
// Git not answering straight.
// ---------------------------------------------------------------------

#[test]
fn git_dir_redirecting_the_repository_causes_a_refusal_naming_the_variable() -> support::TestOutcome
{
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    let elsewhere_git_dir = TemporaryDirectory::new("sync-worktree-elsewhere-git-dir")?;

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            "GIT_DIR",
            elsewhere_git_dir
                .path()
                .to_str()
                .ok_or("GIT_DIR path must be UTF-8")?,
        )],
    )?;
    assert_refused_naming(&report, "GIT_DIR");
    assert!(
        fixture.read("plain.yml").is_err(),
        "sync must write nothing at all once GIT_DIR redirects which repository it answers about"
    );
    Ok(())
}

#[test]
fn git_work_tree_redirecting_the_working_tree_causes_a_refusal_naming_the_variable()
-> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;
    let elsewhere_work_tree = TemporaryDirectory::new("sync-worktree-elsewhere-work-tree")?;

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            "GIT_WORK_TREE",
            elsewhere_work_tree
                .path()
                .to_str()
                .ok_or("GIT_WORK_TREE path must be UTF-8")?,
        )],
    )?;
    assert_refused_naming(&report, "GIT_WORK_TREE");
    assert!(fixture.read("plain.yml").is_err());
    Ok(())
}

#[test]
fn git_index_file_set_to_empty_still_causes_a_refusal_naming_the_variable() -> support::TestOutcome
{
    // Captured against real git: `GIT_INDEX_FILE=` (set, but empty) still
    // changes what `ls-files` answers — `sync` refuses on the variable
    // being *set at all*, whatever its value, never on whether its value
    // is non-empty.
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;

    let report = fixture.run_with_env(&["skeletons", "sync"], &[("GIT_INDEX_FILE", "")])?;
    assert_refused_naming(&report, "GIT_INDEX_FILE");
    assert!(fixture.read("plain.yml").is_err());
    Ok(())
}

#[test]
fn a_bare_repository_is_refused() -> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(root, home, &["init", "--quiet"], "git init")?;
    support::git::run(root, home, &["add", "--all"], "git add")?;
    support::git::run(
        root,
        home,
        &["commit", "--quiet", "--message", "fixture: initial state"],
        "git commit",
    )?;

    // Turns the wearing repository itself into a bare one, in place: no
    // working tree, only the `.git` database — `git rev-parse
    // --is-inside-work-tree` then answers `false`.
    support::git::run(
        root,
        home,
        &["config", "core.bare", "true"],
        "git config core.bare",
    )?;

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_naming(&report, "not inside a git work tree");
    assert!(fixture.read("plain.yml").is_err());
    Ok(())
}

#[test]
fn dubious_ownership_causes_a_refusal_naming_safe_directory_or_ownership() -> support::TestOutcome {
    // `GIT_TEST_ASSUME_DIFFERENT_OWNER=1` fakes the dubious-ownership
    // refusal without needing root or a second real user — git itself
    // then refuses every command in the repository with `fatal: detected
    // dubious ownership`, exactly as it would against a repository another
    // user actually owns.
    let fixture = fixture_wearing_passthrough_plain_with_clean_baseline()?;

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")],
    )?;
    assert_ne!(
        report.exit_code, 0,
        "sync must refuse rather than proceed against a repository git itself calls dubious \
         ownership; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let combined = format!("{}{}", report.stdout, report.stderr);
    assert!(
        combined.contains("safe.directory") || combined.contains("ownership"),
        "the refusal must name the actual cause (safe.directory / dubious ownership), not a \
         generic \"not a work tree\" message; combined output was: {combined:?}"
    );
    assert!(fixture.read("plain.yml").is_err());
    Ok(())
}

#[test]
fn a_linked_worktree_works_normally() -> support::TestOutcome {
    // A `.git` *file* (pointing at the real repository's own `worktrees/`
    // administrative area) rather than a `.git` directory — `sync` must
    // treat this exactly like an ordinary repository, not refuse it as
    // though it were something unusual.
    let fixture = fixture_wearing_passthrough_plain()?;
    let primary_root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(primary_root, home, &["init", "--quiet"], "git init")?;
    support::git::run(primary_root, home, &["add", "--all"], "git add")?;
    support::git::run(
        primary_root,
        home,
        &["commit", "--quiet", "--message", "fixture: initial state"],
        "git commit",
    )?;

    let linked = TemporaryDirectory::new("sync-worktree-linked")?;
    support::git::run(
        primary_root,
        home,
        &[
            "worktree",
            "add",
            "--quiet",
            &linked.path().display().to_string(),
            "-b",
            "linked-worktree-branch",
        ],
        "git worktree add",
    )?;
    assert!(
        linked.path().join(".git").is_file(),
        "a linked worktree's own `.git` must be a file, not a directory — otherwise this test \
         is not exercising the shape it claims to"
    );

    // Run against the linked worktree's own root, where the wearer's manifest
    // and lockfile already live (checked out by `git worktree add`), not the
    // primary checkout `Fixture::run` runs in.
    let report = support::run_ritual(linked.path(), fixture.sandbox(), &["skeletons", "sync"])?;
    assert_eq!(
        report.exit_code, 0,
        "sync must work normally inside a linked worktree; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr,
    );
    assert_eq!(
        std::fs::read(linked.path().join("plain.yml"))?,
        PASSTHROUGH_PLAIN_RENDER,
        "sync must actually write the claimed file inside the linked worktree"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// A path git was told not to materialise is tracked, not absent.
// ---------------------------------------------------------------------

/// Asserts a refused `sync` named `path` as tracked in git's index but
/// absent from the work tree — the cause, in the message's own words, not
/// merely a failure — then that nothing was created at `path` and that
/// `HEAD` still holds `committed`, byte for byte. The last two are what a
/// false `created` line would break: git ignores a file written at a path
/// its index says to skip, so a "success" leaves the committed bytes in
/// force.
fn assert_refused_as_tracked_but_absent(
    fixture: &Fixture,
    report: &support::Report,
    path: &str,
    committed: &str,
) -> support::TestOutcome {
    assert_refused_naming(report, &format!("{path} is tracked in git's index"));
    assert_refused_naming(report, "absent from the work tree");
    assert_refused_naming(report, "skip-worktree or a sparse checkout");
    assert!(
        fixture.read(path).is_err(),
        "sync must not create {path}: git ignores a file written where its index says to skip; \
         stdout was: {}, stderr was: {}",
        report.stdout,
        report.stderr
    );
    let held = support::git::run(
        fixture.root(),
        fixture.sandbox().home(),
        &["show", &format!("HEAD:{path}")],
        "git show HEAD:<path>",
    )?;
    assert_eq!(
        held,
        committed.trim_end(),
        "HEAD must still hold the committed bytes"
    );
    Ok(())
}

const PLAIN_COMMITTED: &str = "name: committed\nvalue: 999\n";

#[test]
fn a_skip_worktree_path_absent_from_the_work_tree_is_refused_not_created() -> support::TestOutcome {
    // `plain.yml` is committed with other bytes, marked skip-worktree, and
    // removed: `git status` reports nothing, so a check that reads "absent"
    // from the file system alone would report `created` and write a file
    // git then ignores while `HEAD` keeps `value: 999`.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PLAIN_COMMITTED.as_bytes())?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(
        root,
        home,
        &["update-index", "--skip-worktree", "plain.yml"],
        "git update-index --skip-worktree",
    )?;
    std::fs::remove_file(root.join("plain.yml"))?;
    assert_eq!(
        support::git::status_porcelain(root, home)?,
        "",
        "the positive control: git status itself reports nothing for a removed skip-worktree path"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_as_tracked_but_absent(&fixture, &report, "plain.yml", PLAIN_COMMITTED)
}

#[test]
fn a_non_cone_sparse_checkout_excluding_a_claimed_file_is_refused_not_created()
-> support::TestOutcome {
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PLAIN_COMMITTED.as_bytes())?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(
        root,
        home,
        &["sparse-checkout", "set", "--no-cone", "/*", "!/plain.yml"],
        "git sparse-checkout set --no-cone",
    )?;
    assert!(
        fixture.read("plain.yml").is_err(),
        "the premise: the sparse checkout must have removed plain.yml from the work tree"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_as_tracked_but_absent(&fixture, &report, "plain.yml", PLAIN_COMMITTED)
}

#[test]
fn a_cone_sparse_checkout_excluding_a_claimed_file_in_a_subdirectory_is_refused_not_created()
-> support::TestOutcome {
    // `nested-dotfiles` claims `.github/dependabot.yml`; the cone keeps
    // only `a/` and `src/` (the wearer's own crate), so `.github/` leaves
    // the work tree while its entry stays in the index. The other two
    // claimed files match their renders, so the one refusal is the whole
    // reason nothing is written.
    let dependabot_committed = "dependabot: committed\n";
    let fixture = fixture_wearing_nested_dotfiles_with_two_of_three_matching()?;
    fixture.write(".github/dependabot.yml", dependabot_committed.as_bytes())?;
    fixture.init_git_repository()?;
    let root = fixture.root();
    let home = fixture.sandbox().home();
    support::git::run(
        root,
        home,
        &["sparse-checkout", "set", "--cone", "a", "src"],
        "git sparse-checkout set --cone",
    )?;
    assert!(
        fixture.read(".github/dependabot.yml").is_err(),
        "the premise: the cone must have removed .github/ from the work tree"
    );

    let report = fixture.run(&["skeletons", "sync"])?;
    assert_refused_as_tracked_but_absent(
        &fixture,
        &report,
        ".github/dependabot.yml",
        dependabot_committed,
    )
}

// ---------------------------------------------------------------------
// Inside a git hook: git exports GIT_INDEX_FILE (and GIT_DIR) to every
// hook it runs, and a commit's index may be a temporary one.
// ---------------------------------------------------------------------

/// What one `skeletons` subcommand did when a real `git commit`'s own
/// pre-commit hook ran it.
struct HookRun {
    exit_code: String,
    output: String,
}

/// Writes the repository-local pre-commit hook that records what
/// `ritual skeletons <subcommand>` did, and points `core.hooksPath` at it.
fn install_recording_pre_commit_hook(fixture: &Fixture) -> support::TestOutcome {
    use std::os::unix::fs::PermissionsExt as _;

    let root = fixture.root();
    let home = fixture.sandbox().home();
    let hooks = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks)?;
    let hook = hooks.join("pre-commit");
    // Deviation from RS-SINGLE-TOOLCHAIN (integration tests use
    // `std::process::Command`, not shell scripts): git runs a hook itself, as
    // an executable file under `core.hooksPath`, so the hook cannot be a
    // `Command` the test runs; it has to be a file. The few POSIX `sh` lines
    // only run the bundle and record its output and exit code, and a shell
    // script is the narrowest form of that. A Rust stand-in would need a
    // binary target added to the crate for this one test.
    std::fs::write(
        &hook,
        "#!/bin/sh\n\
         \"$SKELETONS_TEST_RITUAL\" skeletons \"$SKELETONS_TEST_SUBCOMMAND\" \
         >\"$SKELETONS_TEST_OUTPUT\" 2>&1\n\
         echo $? >\"$SKELETONS_TEST_EXIT_CODE\"\n\
         exit 0\n",
    )?;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    let hooks_path = hooks
        .to_str()
        .ok_or("the hooks directory path must be UTF-8")?;
    support::git::run(
        root,
        home,
        &["config", "core.hooksPath", hooks_path],
        "git config core.hooksPath",
    )?;
    Ok(())
}

/// Stages a file unrelated to any bone, so a commit has something to commit.
fn stage_unrelated_file(fixture: &Fixture) -> support::TestOutcome {
    let root = fixture.root();
    let home = fixture.sandbox().home();
    fixture.write(
        "unrelated.txt",
        b"staged so the commit has something to commit\n",
    )?;
    support::git::run(
        root,
        home,
        &["add", "unrelated.txt"],
        "git add unrelated.txt",
    )?;
    Ok(())
}

/// Commits a staged, unrelated file in `fixture`'s repository with a
/// repository-local pre-commit hook that runs `ritual skeletons <subcommand>`,
/// and returns what the hook saw.
///
/// The hook never fails the commit: it records the subcommand's exit code
/// and combined output in files beside the fixture, so the test reads them
/// back rather than parsing what git relays. `core.hooksPath` is set
/// locally so the hook is the one this fixture wrote whatever the machine
/// running the suite configures; the fixture's own `HOME` already keeps a
/// global `core.hooksPath` out.
fn run_in_pre_commit_hook(
    fixture: &Fixture,
    subcommand: &str,
) -> Result<HookRun, Box<dyn std::error::Error>> {
    install_recording_pre_commit_hook(fixture)?;
    stage_unrelated_file(fixture)?;

    let root = fixture.root();
    let home = fixture.sandbox().home();
    let records = TemporaryDirectory::new("sync-worktree-hook-records")?;
    let output = records.path().join("output");
    let exit_code = records.path().join("exit-code");
    let ritual = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ritual"));
    let cargo_home = fixture.sandbox().cargo_home().to_path_buf();
    support::git::run_with_env(
        root,
        home,
        &[
            "commit",
            "--quiet",
            "--message",
            "fixture: commit that runs a hook",
        ],
        &[
            ("SKELETONS_TEST_RITUAL", ritual.as_path()),
            (
                "SKELETONS_TEST_SUBCOMMAND",
                std::path::Path::new(subcommand),
            ),
            ("SKELETONS_TEST_OUTPUT", output.as_path()),
            ("SKELETONS_TEST_EXIT_CODE", exit_code.as_path()),
            ("CARGO_HOME", cargo_home.as_path()),
        ],
        "git commit",
    )?;
    Ok(HookRun {
        exit_code: std::fs::read_to_string(&exit_code)?.trim().to_owned(),
        output: std::fs::read_to_string(&output)?,
    })
}

/// A fixture wearing `passthrough-plain` whose `plain.yml` is committed
/// holding exactly the render, so every bone matches.
fn fixture_with_every_bone_matching() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.init_git_repository()?;
    Ok(fixture)
}

#[test]
fn sync_in_a_pre_commit_hook_with_every_bone_matching_needs_no_git_and_exits_zero()
-> support::TestOutcome {
    // Nothing would be written, so nothing can be overwritten: there is
    // nothing for git's redirecting variables to make unsafe, and the hook
    // must not be told to unset one.
    let fixture = fixture_with_every_bone_matching()?;

    let hook = run_in_pre_commit_hook(&fixture, "sync")?;

    assert_eq!(
        hook.exit_code, "0",
        "the hook's sync output was: {}",
        hook.output
    );
    assert!(
        hook.output
            .contains("every bone already matches; nothing was written"),
        "the hook's sync must say every bone already matches; output was: {:?}",
        hook.output
    );
    Ok(())
}

#[test]
fn sync_in_a_pre_commit_hook_with_a_drifted_bone_refuses_and_says_to_run_it_outside()
-> support::TestOutcome {
    // Something would be written, and inside `git commit <paths>` or
    // `git commit -a` the index git is using may be a temporary one that a
    // file written now is not part of: refusing is right, and the message
    // must say what to do about it.
    let fixture = fixture_wearing_passthrough_plain()?;
    let drifted = b"name: drifted\nvalue: 1\n";
    fixture.write("plain.yml", drifted)?;
    fixture.init_git_repository()?;

    let hook = run_in_pre_commit_hook(&fixture, "sync")?;

    assert_eq!(
        hook.exit_code, "1",
        "the hook's sync output was: {}",
        hook.output
    );
    assert!(
        hook.output.contains("GIT_INDEX_FILE"),
        "the refusal must name GIT_INDEX_FILE; output was: {:?}",
        hook.output
    );
    assert!(
        hook.output.contains("outside a git hook"),
        "the refusal must say to run sync outside a git hook; output was: {:?}",
        hook.output
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        drifted,
        "a refused sync must leave the drifted file exactly as it was"
    );
    Ok(())
}

#[test]
fn check_in_a_pre_commit_hook_works_and_reports_drift() -> support::TestOutcome {
    // The guard behind the sentence the refusal above ends on: `check`
    // works inside a hook. It reads the workspace and asks git nothing
    // about the wearer's repository, so what git exports is harmless to it.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", b"name: drifted\nvalue: 1\n")?;
    fixture.init_git_repository()?;

    let hook = run_in_pre_commit_hook(&fixture, "check")?;

    assert!(
        hook.output.contains("plain.yml"),
        "check inside a hook must report the drifted file; output was: {:?}",
        hook.output
    );
    assert!(
        hook.output.contains("drifted"),
        "check inside a hook must say the file drifted; output was: {:?}",
        hook.output
    );
    assert_ne!(
        hook.exit_code, "0",
        "check reports drift with a failing exit"
    );
    Ok(())
}

#[test]
fn a_missing_git_dir_does_not_stop_a_sync_with_nothing_to_write() -> support::TestOutcome {
    // With every bone matching, `sync` needs no git at all, so a
    // redirecting variable — here one pointing nowhere — changes nothing.
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.write("plain.yml", PASSTHROUGH_PLAIN_RENDER)?;
    fixture.init_git_repository()?;
    let elsewhere = TemporaryDirectory::new("sync-worktree-nonexistent-git-dir")?;
    let nonexistent = elsewhere.path().join("does-not-exist");

    let report = fixture.run_with_env(
        &["skeletons", "sync"],
        &[(
            "GIT_DIR",
            nonexistent.to_str().ok_or("GIT_DIR path must be UTF-8")?,
        )],
    )?;

    assert_eq!(
        report.exit_code, 0,
        "stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        report.stdout,
        "every bone already matches; nothing was written\n"
    );
    Ok(())
}
