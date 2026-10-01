//! `cargo ritual skeletons check` and `cargo ritual skeletons sync` both
//! answer, and each prints exactly its help text.
//!
//! No fixture workspace is needed: `--help` answers the same regardless of
//! the working directory.

mod support;

use support::{Sandbox, TestOutcome, run_ritual};

#[test]
fn check_and_sync_are_both_reachable_subcommands() -> TestOutcome {
    // `--help` on each must exit zero and name the subcommand in its own
    // usage line — the cheapest possible proof that clap actually knows
    // about it, independent of anything either command does.
    let sandbox = Sandbox::new()?;
    let current_directory = std::env::current_dir()?;

    let check_help = run_ritual(
        &current_directory,
        &sandbox,
        &["skeletons", "check", "--help"],
    )?;
    assert_eq!(
        check_help.exit_code, 0,
        "`skeletons check --help` must answer; stderr was: {}",
        check_help.stderr
    );
    assert!(
        check_help.stdout.contains("Usage: ritual skeletons check"),
        "expected a `check` usage line; stdout was: {}",
        check_help.stdout
    );

    let sync_help = run_ritual(
        &current_directory,
        &sandbox,
        &["skeletons", "sync", "--help"],
    )?;
    assert_eq!(
        sync_help.exit_code, 0,
        "`skeletons sync --help` must answer; stderr was: {}",
        sync_help.stderr
    );
    assert!(
        sync_help.stdout.contains("Usage: ritual skeletons sync"),
        "expected a `sync` usage line; stdout was: {}",
        sync_help.stdout
    );
    Ok(())
}

/// The exact text `skeletons check --help`/`-h` must print, word for word:
/// `check`'s own `about`, its usage line, and each flag's `///` doc comment as
/// its help text, unwrapped, with no "see more" hint — the same compact form
/// for both `--help` and `-h`.
const CHECK_HELP_TEXT: &str = concat!(
    "compare this repository's files against the skeletons it wears\n\n",
    "Usage: ritual skeletons check [OPTIONS]\n\nOptions:\n",
    "      --drifted      show only bones that have drifted; the exit status still counts ",
    "every bone\n",
    "      --behind       show only skeletons that are behind, or where that is undetermined; ",
    "the exit status still counts every bone\n",
    "      --fail-behind  also fail when a worn skeleton is behind, ",
    "or where that is undetermined\n",
    "      --json         print the answer as one JSON document (format version 1)\n",
    "  -h, --help         Print help\n",
);

#[test]
fn check_help_and_h_print_exactly_the_help_text() -> TestOutcome {
    // `CheckArguments` carries its reasoning as a plain `//` comment above
    // the derive rather than a `///` doc comment, because clap turns a
    // struct's own multi-paragraph doc comment into a `long_about` that
    // `--help` would print instead of the intended `about`, and would also
    // make `-h` end in "(see more with '--help')" instead of a bare "Print
    // help". With the reasoning kept out of the doc comment, `--help` and
    // `-h` print the identical, exact block below.
    let sandbox = Sandbox::new()?;
    let current_directory = std::env::current_dir()?;

    for flag in ["--help", "-h"] {
        let report = run_ritual(&current_directory, &sandbox, &["skeletons", "check", flag])?;
        assert_eq!(
            report.exit_code, 0,
            "`skeletons check {flag}` must answer; stderr was: {}",
            report.stderr
        );
        assert_eq!(
            report.stdout, CHECK_HELP_TEXT,
            "`skeletons check {flag}` must print exactly the help text"
        );
    }
    Ok(())
}

/// The exact text `skeletons sync --help`/`-h` must print, word for word.
const SYNC_HELP_TEXT: &str = concat!(
    "rewrite drifted files to match the skeletons this repository wears\n\n",
    "Usage: ritual skeletons sync\n\nOptions:\n  -h, --help  Print help\n",
);

#[test]
fn sync_help_and_h_print_exactly_the_help_text() -> TestOutcome {
    // `NoArguments` carries its reasoning as a `//` comment above the
    // derive, the same as `CheckArguments`, so clap treats it as a plain
    // `about` with no `long_about`, and this prints the exact text. This
    // guards against the same clap behaviour named above (`rituals`'
    // `declare_leaf` applies `.about(...)` after `augment_args` and never
    // overrides `long_about`).
    let sandbox = Sandbox::new()?;
    let current_directory = std::env::current_dir()?;

    for flag in ["--help", "-h"] {
        let report = run_ritual(&current_directory, &sandbox, &["skeletons", "sync", flag])?;
        assert_eq!(
            report.exit_code, 0,
            "`skeletons sync {flag}` must answer; stderr was: {}",
            report.stderr
        );
        assert_eq!(
            report.stdout, SYNC_HELP_TEXT,
            "`skeletons sync {flag}` must print exactly the help text"
        );
    }
    Ok(())
}
