//! Acceptance: `remove` refuses what it must, says what is wrong in one whole
//! line, and writes nothing; and when it fails after it has begun writing, it
//! puts the manifest and the lockfile back and names the files it had deleted.
//!
//! Every scenario reads the whole work tree before and after (every file's
//! bytes and mode, every directory's mode, `Cargo.lock` included) and
//! compares them, never trusting an exit code alone. Byte-identical trees
//! cannot tell a refusal made before any write from a failure that rolled
//! back, so each scenario also holds stderr to the whole line, word for
//! word, filled in from the fixture.

mod support;

use std::error::Error;

use support::one_line::toml_string;
use support::remove::{
    MANIFEST, ONE_UNCOMMITTED_CHANGE_LINE, ReadOnlyDirectory, Worn, assert_printed, assert_refused,
    commit_line, dependency_under_key, fixture_wearing_in_another_member,
    fixture_wearing_two_skeletons_that_claim_one_path, keys_check_lists, porcelain, run_remove,
    stopped_wearing_line, sync_and_commit, tree_state, wear,
};
use support::wear::{COMMAND_LINE_PACKAGE, fixture_for_wearing};
use support::{
    Fixture, TestOutcome, wearing_table, write_minimal_skeleton, write_package_manifest,
};

#[test]
fn a_key_wins_over_a_crate_of_the_same_name() -> TestOutcome {
    // `passthrough-plain` is worn under the key `nested-dotfiles`, and
    // `nested-dotfiles` the crate is worn under the key `other`. The argument
    // `nested-dotfiles` is both a key and a crate, and must mean the key:
    // refusing it as ambiguous would make that key impossible to remove. The
    // wearing under `other` is left alone, files and all.
    let fixture = fixture_for_wearing("")?;
    wear(&fixture, "passthrough-plain", Some("nested-dotfiles"))?;
    wear(&fixture, "nested-dotfiles", Some("other"))?;
    sync_and_commit(&fixture)?;

    let report = run_remove(&fixture, &["nested-dotfiles"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "nested-dotfiles", MANIFEST),
            "deleted plain.yml".to_owned(),
            commit_line(MANIFEST, true, 1),
        ],
    );
    assert_eq!(
        keys_check_lists(&fixture)?,
        vec!["other".to_owned()],
        "the wearing under the other key must stay worn"
    );
    assert_eq!(
        fixture.read("root.yml")?,
        b"root-file: yes\n",
        "the other wearing's files must stay"
    );
    Ok(())
}

#[test]
fn a_crate_that_exactly_one_key_wears_is_that_wearing() -> TestOutcome {
    // Two skeletons are worn, `nested-dotfiles` under `tidy` and
    // `passthrough-plain` under `plain`. Naming the crate `nested-dotfiles`,
    // which no key is called, must take out `tidy` and nothing else.
    let fixture = fixture_for_wearing("")?;
    wear(&fixture, "nested-dotfiles", Some("tidy"))?;
    wear(&fixture, "passthrough-plain", Some("plain"))?;
    sync_and_commit(&fixture)?;

    let report = run_remove(&fixture, &["nested-dotfiles"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("nested-dotfiles", "tidy", MANIFEST),
            "deleted .github/dependabot.yml".to_owned(),
            "deleted a/b/deep.yml".to_owned(),
            "deleted root.yml".to_owned(),
            commit_line(MANIFEST, true, 3),
        ],
    );
    assert_eq!(
        keys_check_lists(&fixture)?,
        vec!["plain".to_owned()],
        "the wearing under the other key must stay worn"
    );
    assert_eq!(
        fixture.read("plain.yml")?,
        b"name: unchanged\nvalue: 42\n",
        "the other wearing's file must stay"
    );
    Ok(())
}

/// The directory, inside the fixture, holding the second `passthrough-plain`.
const NEWER_COPY_DIRECTORY: &str = "newer-passthrough-plain";

/// A workspace whose manifest wears `passthrough-plain` under `ci` and, as a
/// second package of that name Cargo tells apart by its version, under
/// `ci-next`, synced and committed clean.
///
/// Cargo refuses two dependencies on one package under different names, so
/// the second key takes a copy of the skeleton, written into the fixture,
/// that declares another version and a file of its own, since two wearings
/// cannot claim one file.
fn fixture_wearing_one_crate_under_two_keys() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_minimal_skeleton(
        &fixture.root().join(NEWER_COPY_DIRECTORY),
        "passthrough-plain",
        "newer.yml",
        "name: newer\n",
    )?;
    let copy_manifest = format!("{NEWER_COPY_DIRECTORY}/Cargo.toml");
    let declared = String::from_utf8(fixture.read(&copy_manifest)?)?;
    assert!(
        declared.contains("version = \"0.0.0\""),
        "precondition: the copy must declare the version this test replaces"
    );
    fixture.write(
        &copy_manifest,
        declared
            .replace("version = \"0.0.0\"", "version = \"0.0.1\"")
            .as_bytes(),
    )?;
    let copy_path = toml_string(&fixture.root().join(NEWER_COPY_DIRECTORY).to_string_lossy());
    let extra = format!(
        "[dev-dependencies]\n{}ci-next = {{ package = \"passthrough-plain\", \
         path = {copy_path} }}\n\n{}\n{}",
        dependency_under_key("ci", "passthrough-plain")?,
        wearing_table("ci", ""),
        wearing_table("ci-next", ""),
    );
    write_package_manifest(fixture.root(), "", COMMAND_LINE_PACKAGE, &extra)?;
    fixture.generate_lockfile()?;
    fixture.init_git_repository()?;
    sync_and_commit(&fixture)?;
    assert_eq!(
        keys_check_lists(&fixture)?,
        vec!["ci".to_owned(), "ci-next".to_owned()],
        "precondition: Cargo must accept the workspace and check must list both keys as worn"
    );
    Ok(fixture)
}

#[test]
fn a_crate_that_several_keys_wear_is_refused_with_a_key_to_use_instead() -> TestOutcome {
    // `passthrough-plain` is worn under `ci` and `ci-next`. Naming the crate
    // is ambiguous, so `remove` refuses, names both keys, and shows the
    // command with the first key, carrying the flag that was typed.
    let fixture = fixture_wearing_one_crate_under_two_keys()?;
    let before = tree_state(fixture.root())?;
    let message = "`passthrough-plain` is worn under more than one key in Cargo.toml, `ci` and \
                   `ci-next`, so remove wrote nothing; remove one by its key, such as `cargo \
                   ritual skeletons remove ci";

    let bare = run_remove(&fixture, &["passthrough-plain"])?;
    assert_refused(&bare, &format!("{message}`"));

    let flagged = run_remove(&fixture, &["passthrough-plain", "--keep-files"])?;
    assert_refused(&flagged, &format!("{message} --keep-files`"));

    assert_eq!(
        tree_state(fixture.root())?,
        before,
        "a refused remove must leave the work tree as it was"
    );
    Ok(())
}

#[test]
fn a_name_that_is_neither_a_key_nor_a_worn_crate_is_refused_listing_the_keys() -> TestOutcome {
    // `nope` is no key and no crate of any wearing. With two wearings the
    // refusal lists their keys in key order; with none it says the manifest
    // wears no skeleton. Both leave the work tree as it was.
    let wearing = Worn::unsynced("nested-dotfiles", Some("tidy"))?;
    wear(&wearing.fixture, "passthrough-plain", None)?;
    let before = tree_state(wearing.fixture.root())?;

    let report = run_remove(&wearing.fixture, &["nope"])?;

    assert_refused(
        &report,
        "`nope` is neither a key Cargo.toml wears a skeleton under nor the crate of one, so \
         remove wrote nothing; its keys are `passthrough-plain` and `tidy`, so give the \
         `remove` task one of them",
    );
    assert_eq!(
        tree_state(wearing.fixture.root())?,
        before,
        "a refused remove must leave the work tree as it was"
    );

    let bare = fixture_for_wearing("")?;
    let before = tree_state(bare.root())?;

    let report = run_remove(&bare, &["nope"])?;

    assert_refused(
        &report,
        "`nope` is neither a key Cargo.toml wears a skeleton under nor the crate of one, so \
         remove wrote nothing; Cargo.toml wears no skeleton",
    );
    assert_eq!(
        tree_state(bare.root())?,
        before,
        "a refused remove must leave the work tree as it was"
    );
    Ok(())
}

#[test]
fn a_wearing_in_another_members_manifest_is_refused_and_pointed_at() -> TestOutcome {
    // The command line's package is `ritual`; `tools` wears a skeleton under
    // `tidy`. `remove` changes only the command line's manifest, so it
    // refuses, whether the wearing is named by its key or by its crate, and
    // says which manifest to edit by hand.
    let fixture = fixture_wearing_in_another_member("tidy")?;
    let before = tree_state(fixture.root())?;

    for argument in ["tidy", "passthrough-plain"] {
        let report = run_remove(&fixture, &[argument])?;

        assert_refused(
            &report,
            &format!(
                "`{argument}` is worn in tools/Cargo.toml as `tidy`, and remove changes only \
                 ritual/Cargo.toml, the manifest of the package this command line is built \
                 from, so remove wrote nothing; take the dependency and the \
                 [package.metadata.skeletons.<key>] table out of that manifest by hand"
            ),
        );
    }

    assert_eq!(
        tree_state(fixture.root())?,
        before,
        "a refused remove must leave every manifest as it was"
    );
    Ok(())
}

#[test]
fn an_overlap_with_the_wearing_is_refused_without_telling_the_wearer_to_run_remove_again()
-> TestOutcome {
    // `overlap-a` and `overlap-b` both claim `shared.txt`, so `check` refuses
    // both and `remove` cannot tell which files `overlap-a` claims. Running
    // `remove` is how one stops wearing one of them, so the line must not send
    // the wearer back to it: bare and with `--delete-files` it names the other
    // wearing and the shared path and offers `--keep-files`, and writes
    // nothing. `--keep-files` then stops wearing `overlap-a`, naming its crate
    // and version because it resolved to a skeleton even though its files
    // cannot be known, leaves `shared.txt` where it is, and `check` no longer
    // refuses `overlap-b`.
    let fixture = fixture_wearing_two_skeletons_that_claim_one_path()?;
    let before = tree_state(fixture.root())?;
    let line = "remove cannot tell which files `overlap-a` claims, so it wrote nothing: \
                shared.txt is also claimed by `overlap-b` in Cargo.toml (overlap-b 0.0.0), and a \
                file belongs to one skeleton; give it --keep-files to stop wearing `overlap-a` \
                and leave shared.txt to `overlap-b`";

    for arguments in [&["overlap-a"][..], &["overlap-a", "--delete-files"]] {
        let report = run_remove(&fixture, arguments)?;

        assert_refused(&report, line);
        assert_eq!(
            tree_state(fixture.root())?,
            before,
            "a refused remove must leave the work tree as it was: {arguments:?}"
        );
    }

    let report = run_remove(&fixture, &["overlap-a", "--keep-files"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("overlap-a", "overlap-a", MANIFEST),
            "left every file it claims in place".to_owned(),
            commit_line(MANIFEST, true, 0),
        ],
    );
    assert_eq!(
        fixture.read("shared.txt")?,
        b"claimed by overlap-b\n",
        "the shared file must stay where it was"
    );
    assert_eq!(
        keys_check_lists(&fixture)?,
        vec!["overlap-b".to_owned()],
        "check must no longer refuse the other wearing for the overlap"
    );
    Ok(())
}

#[test]
fn both_file_flags_together_are_a_usage_error_and_nothing_changes() -> TestOutcome {
    // `--delete-files` and `--keep-files` exclude each other. The parser says
    // so, with its usual exit status 2, before `remove` looks at the project.
    let worn = Worn::synced("passthrough-plain", None)?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(
        &worn.fixture,
        &["passthrough-plain", "--delete-files", "--keep-files"],
    )?;

    assert_eq!(
        report.exit_code, 2,
        "a usage error exits 2; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report
            .stderr
            .contains("the argument '--delete-files' cannot be used with '--keep-files'"),
        "stderr must name the two flags; it was:\n{}",
        report.stderr
    );
    assert_eq!(
        tree_state(worn.fixture.root())?,
        before,
        "a usage error must leave the work tree as it was"
    );
    Ok(())
}

#[test]
fn remove_without_a_name_is_a_usage_error_and_nothing_changes() -> TestOutcome {
    // The key or crate is required; the parser names it.
    let worn = Worn::synced("passthrough-plain", None)?;
    let before = tree_state(worn.fixture.root())?;

    let report = run_remove(&worn.fixture, &[])?;

    assert_eq!(
        report.exit_code, 2,
        "a usage error exits 2; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert!(
        report
            .stderr
            .contains("the following required arguments were not provided")
            && report.stderr.contains("<KEY|CRATE>"),
        "stderr must name the missing argument; it was:\n{}",
        report.stderr
    );
    assert_eq!(
        tree_state(worn.fixture.root())?,
        before,
        "a usage error must leave the work tree as it was"
    );
    Ok(())
}

#[test]
fn a_work_tree_with_an_uncommitted_change_is_refused_and_nothing_is_written() -> TestOutcome {
    // An untracked file makes the tree dirty. `remove` writes only into a
    // clean tree, so it lists the path on stdout, sums up on stderr with the
    // whole line, and leaves both the tree and `git status` exactly as they
    // were.
    let worn = Worn::synced("passthrough-plain", None)?;
    worn.fixture.write("notes.txt", b"not committed\n")?;
    let tree_before = tree_state(worn.fixture.root())?;
    let status_before = porcelain(&worn.fixture)?;

    let report = run_remove(&worn.fixture, &["passthrough-plain"])?;

    assert_refused(&report, ONE_UNCOMMITTED_CHANGE_LINE);
    assert!(
        report
            .stdout
            .lines()
            .any(|line| line == "notes.txt is untracked"),
        "stdout must list the dirty path as its own line; stdout was:\n{}",
        report.stdout
    );
    assert_eq!(
        tree_state(worn.fixture.root())?,
        tree_before,
        "a refused remove must leave the work tree as it was"
    );
    assert_eq!(
        porcelain(&worn.fixture)?,
        status_before,
        "git must report exactly what it reported before"
    );
    Ok(())
}

#[test]
fn a_failed_delete_puts_back_the_manifest_and_lockfile_and_names_both_lists() -> TestOutcome {
    // `nested-dotfiles` claims `.github/dependabot.yml`, `a/b/deep.yml` and
    // `root.yml`, all matching. `a/b` is made unwritable, so deleting
    // `deep.yml` fails after `remove` has written the manifest and the
    // lockfile and deleted `dependabot.yml`. The run must end on the failure,
    // and leave the manifest and the lockfile as they were, `dependabot.yml`
    // deleted (git has it), and `deep.yml` and `root.yml` as they were. The
    // one line must say what became of the project and name the file deleted
    // and the two that were not. A process that can write into a 0o555
    // directory anyway (a superuser) cannot show this, so there the scenario
    // says `skipped:` and passes.
    let worn = Worn::synced("nested-dotfiles", None)?;
    let root = worn.fixture.root();
    let Some(_unwritable) = ReadOnlyDirectory::make(&root.join("a/b"))? else {
        support::one_line::skip("this process can write into a 0o555 directory");
        return Ok(());
    };
    let mut expected_tree = tree_state(root)?;
    expected_tree.remove(".github/dependabot.yml");

    let report = run_remove(&worn.fixture, &["nested-dotfiles"])?;

    assert_eq!(
        report.exit_code, 1,
        "remove must fail with status 1; stdout was: {}; stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(
        support::wear::the_single_refusal_line(&report),
        "remove could not delete a/b/deep.yml: Permission denied (os error 13); ritual put the \
         project back as it found it; the file remove had already deleted is not put back, and \
         git has it: .github/dependabot.yml; not deleted: a/b/deep.yml and root.yml",
        "the failure must name what became of every claimed file, word for word"
    );
    assert_eq!(
        tree_state(root)?,
        expected_tree,
        "the manifest, the lockfile and every file but the one deleted must be as they were"
    );
    assert_eq!(
        porcelain(&worn.fixture)?,
        ["D .github/dependabot.yml"],
        "git must report the one deletion and nothing else"
    );
    Ok(())
}
