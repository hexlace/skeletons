//! Acceptance: `cargo ritual skeletons remove <KEY|CRATE>` undoes `wear`.
//!
//! Each scenario runs the real `ritual` binary in a disposable workspace whose
//! package is `skeletons-ritual`, wears a checked-in test skeleton with `wear`,
//! writes its files with `sync`, commits, and then removes it. The story is
//! that the manifest comes back byte for byte as it was before `wear`, the
//! success lines are exactly the ones `.docs/wearing.md` gives, and `check`
//! afterwards no longer lists the skeleton. The manifests here have no
//! dependency table that was empty before `wear`; one that was comes back
//! without its header, which the manifest edit's own tests hold.

mod support;

use support::remove::{
    MANIFEST, Worn, assert_check_wears_nothing, assert_printed, commit_line, dependency_under_key,
    keys_check_lists, run_remove, stopped_wearing_line, sync_and_commit, wear,
};
use support::wear::{COMMAND_LINE_PACKAGE, fixture_for_wearing};
use support::{Sandbox, TestOutcome, run_ritual, wearing_table};

#[test]
fn removing_by_key_takes_out_the_dependency_and_the_table_and_the_files() -> TestOutcome {
    // `passthrough-plain` is worn under its own name, synced and committed.
    // Removing it by that key must print its three lines, each held whole,
    // leave the manifest exactly as it was before `wear` wrote anything, take
    // the skeleton out of `Cargo.lock`, and leave `check` with nothing worn.
    let worn = Worn::synced("passthrough-plain", None)?;

    let report = run_remove(&worn.fixture, &["passthrough-plain"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "passthrough-plain", MANIFEST),
            "deleted plain.yml".to_owned(),
            commit_line(MANIFEST, true, 1),
        ],
    );
    assert_eq!(
        worn.fixture.read(MANIFEST)?,
        worn.manifest_before_wearing,
        "the manifest must be byte-identical to what it was before wear"
    );
    let lockfile = String::from_utf8(worn.fixture.read("Cargo.lock")?)?;
    assert!(
        !lockfile.contains("name = \"passthrough-plain\""),
        "the lockfile must no longer pin the skeleton; it was:\n{lockfile}"
    );
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn removing_by_crate_takes_out_a_wearing_made_under_another_key() -> TestOutcome {
    // `nested-dotfiles` is worn under the key `tidy`, so the key and the crate
    // differ. Naming the crate must find the one wearing of it, take out the
    // renamed dependency and the table named for `tidy`, and restore the
    // manifest to its pre-wear bytes.
    let worn = Worn::synced("nested-dotfiles", Some("tidy"))?;

    let report = run_remove(&worn.fixture, &["nested-dotfiles"])?;

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
        worn.fixture.read(MANIFEST)?,
        worn.manifest_before_wearing,
        "the manifest must be byte-identical to what it was before wear"
    );
    assert_check_wears_nothing(&worn.fixture)
}

#[test]
fn removing_one_wearing_leaves_the_other_worn_and_the_manifest_as_it_was_between() -> TestOutcome {
    // Two skeletons are worn, one after the other, so both land in the same
    // `[dev-dependencies]` table. Removing the second must put the manifest
    // back to the bytes it had after the first (the table and its header
    // stay, since the first still needs them), leave the first's files where
    // they are, and leave `check` listing only the first.
    let worn = Worn::synced("nested-dotfiles", None)?;
    let manifest_after_first = worn.fixture.read(MANIFEST)?;
    wear(&worn.fixture, "passthrough-plain", None)?;
    support::remove::sync_and_commit(&worn.fixture)?;

    let report = run_remove(&worn.fixture, &["passthrough-plain"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "passthrough-plain", MANIFEST),
            "deleted plain.yml".to_owned(),
            commit_line(MANIFEST, true, 1),
        ],
    );
    assert_eq!(
        worn.fixture.read(MANIFEST)?,
        manifest_after_first,
        "only the removed skeleton's dependency and table may leave the manifest"
    );
    assert_eq!(
        worn.fixture.read("root.yml")?,
        b"root-file: yes\n",
        "the other skeleton's files must stay"
    );
    assert_eq!(
        keys_check_lists(&worn.fixture)?,
        vec!["nested-dotfiles".to_owned()],
        "check must list only the skeleton still worn"
    );
    Ok(())
}

#[test]
fn the_comments_on_the_headers_the_removal_takes_out_stay_in_the_manifest() -> TestOutcome {
    // The wearer wrote `passthrough-plain`'s dependency table and wearing
    // table by hand, with a comment above each header and one beside the
    // first. Removing the wearing empties `[dev-dependencies]` and takes both
    // headers out, so the three comments must still be in the manifest, in
    // file order, where the headers were; nothing else of theirs is.
    let extra = format!(
        "# the tools the checks need\n[dev-dependencies] # kept sorted\n{}\n\
         # what the skeleton recorded\n{}",
        dependency_under_key("passthrough-plain", "passthrough-plain")?,
        wearing_table("passthrough-plain", ""),
    );
    let fixture = fixture_for_wearing(&extra)?;
    sync_and_commit(&fixture)?;

    let report = run_remove(&fixture, &["passthrough-plain"])?;

    assert_printed(
        &report,
        &[
            stopped_wearing_line("passthrough-plain", "passthrough-plain", MANIFEST),
            "deleted plain.yml".to_owned(),
            commit_line(MANIFEST, true, 1),
        ],
    );
    assert_eq!(
        String::from_utf8(fixture.read(MANIFEST)?)?,
        format!(
            "[package]\nname = \"{COMMAND_LINE_PACKAGE}\"\nversion = \"0.1.0\"\n\
             edition = \"2021\"\npublish = false\n\n# the tools the checks need\n\
             # kept sorted\n\n# what the skeleton recorded\n\n"
        ),
        "the comments must stay and the headers and the entry must go"
    );
    assert_check_wears_nothing(&fixture)
}

/// The exact text `skeletons remove --help`/`-h` must print, word for word.
const REMOVE_HELP_TEXT: &str = concat!(
    "stop wearing a skeleton: take out its dependency and wearing table, and its files if none ",
    "has changed\n\n",
    "Usage: ritual skeletons remove [OPTIONS] <KEY|CRATE>\n\nArguments:\n",
    "  <KEY|CRATE>  the wearing's dependency key, or the skeleton's crate\n\nOptions:\n",
    "      --delete-files  delete every file the skeleton claims, changed ones included\n",
    "      --keep-files    leave every file the skeleton claims where it is\n",
    "  -h, --help          Print help\n",
);

#[test]
fn remove_help_and_h_print_exactly_the_help_text() -> TestOutcome {
    // Runs outside any workspace: `--help` answers the same wherever it is
    // asked, and the text is held word for word, for both spellings.
    let sandbox = Sandbox::new()?;
    let current_directory = std::env::current_dir()?;

    for flag in ["--help", "-h"] {
        let report = run_ritual(&current_directory, &sandbox, &["skeletons", "remove", flag])?;
        assert_eq!(
            report.exit_code, 0,
            "`skeletons remove {flag}` must answer; stderr was: {}",
            report.stderr
        );
        assert_eq!(
            report.stdout, REMOVE_HELP_TEXT,
            "`skeletons remove {flag}` must print exactly the help text"
        );
    }
    Ok(())
}
