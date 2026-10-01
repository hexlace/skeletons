//! Acceptance: an empty `text` value, a `text` value holding a newline, a
//! wearing-table key holding a newline and a manifest key holding a newline are
//! each refused on one line, through the real `check`.
//!
//! Each scenario runs `check` twice against the same repository, once for the
//! human report and once with `--json`, and asserts the refusal's exact words.
//! A refusal that echoed a newline raw would split one refusal across two
//! lines of the report and put a control character into a `--json` string, so
//! every scenario also reads the output back looking for one.
//!
//! - `text-optional` declares `assignee`, a `text` with no default. The first
//!   three scenarios record a bad value, or a bad key, in its wearing table.
//! - `unknown-key-with-newline` carries a key holding a newline in its own
//!   manifest, so the refusal is about the skeleton rather than the wearer.

mod support;

use support::{
    Fixture, path_dependency_on, path_dependency_on_test_skeleton, wearing_table,
    write_package_manifest,
};

/// Wears `dependency` (a checked-in skeleton under `renders/`, or under
/// `refused/` when `refused` is set) with `options_toml` recorded, runs `check`
/// as text and as `--json`, and asserts that both refuse exactly once, as
/// `kind`, and print `refusal` on one line.
///
/// `refusal` is the refusal's own words: the human report prints it after
/// `  refused: `, and `--json` carries it after the skeleton's name and
/// version in `message`.
fn assert_refused_on_one_line(
    dependency: &str,
    refused: bool,
    options_toml: &str,
    kind: &str,
    refusal: &str,
) -> support::TestOutcome {
    let fixture = Fixture::new()?;
    let dependency_line = if refused {
        path_dependency_on(
            dependency,
            &support::checked_in_refused_skeleton(dependency),
        )
    } else {
        path_dependency_on_test_skeleton(dependency, dependency)
    };
    let extra = format!(
        "[dependencies]\n{dependency_line}\n{}",
        wearing_table(dependency, options_toml),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;

    assert_text_report_refuses(&fixture, refusal)?;
    assert_json_report_refuses(&fixture, dependency, kind, refusal)
}

/// Runs `check` for `fixture` as text and asserts that it fails with
/// `refusal`, whole, on the one line after `  refused: `, and on no other.
fn assert_text_report_refuses(fixture: &Fixture, refusal: &str) -> support::TestOutcome {
    let text = fixture.run(&["skeletons", "check"])?;

    assert_eq!(
        text.exit_code, 1,
        "a refused skeleton must fail the command; stdout was: {}; stderr was: {}",
        text.stdout, text.stderr
    );
    let expected_line = format!("  refused: {refusal}\n");
    assert!(
        text.stdout.contains(&expected_line),
        "the report must hold the refusal, whole, on one line; expected: {expected_line:?}; \
         stdout was: {:?}",
        text.stdout
    );
    let refusal_lines = text
        .stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("refused:"))
        .count();
    assert_eq!(
        refusal_lines, 1,
        "the report must hold exactly one refusal line; stdout was: {:?}",
        text.stdout
    );
    Ok(())
}

/// Runs `check --json` for `fixture` and asserts that it fails with exactly
/// one refusal of `kind`, whose message is `dependency`'s name and version
/// followed by `refusal`, with no control character in it.
fn assert_json_report_refuses(
    fixture: &Fixture,
    dependency: &str,
    kind: &str,
    refusal: &str,
) -> support::TestOutcome {
    let json = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(
        json.exit_code, 1,
        "a refused skeleton must fail the command with --json; stdout was: {}; stderr was: {}",
        json.stdout, json.stderr
    );
    let document = support::json::parse(&json.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(
        refusals.len(),
        1,
        "exactly one refusal was expected; refusals were: {refusals:?}"
    );
    let row = refusals
        .first()
        .ok_or("exactly one refusal was expected, and none was found")?;
    assert_eq!(support::json::refusal_kind(row)?, kind);
    let message = support::json::refusal_message(row)?;
    assert_eq!(
        message,
        format!("{dependency} in Cargo.toml ({dependency} 0.0.0): {refusal}"),
        "the message must be the refusal's own words, in full"
    );
    assert!(
        !message.chars().any(char::is_control),
        "the message must hold no control character; message was: {message:?}"
    );
    Ok(())
}

#[test]
fn an_empty_text_value_is_refused_naming_the_wearing_table_and_the_option() -> support::TestOutcome
{
    // `assignee = ""` is a value the wearer wrote and the tool will not fill
    // with: unset drops the lines, and an empty string is not "unset".
    assert_refused_on_one_line(
        "text-optional",
        false,
        "assignee = \"\"\n",
        "option-refused",
        "[package.metadata.skeletons.text-optional] in Cargo.toml: option `assignee` was given an \
         empty value",
    )
}

#[test]
fn a_text_value_holding_a_newline_is_refused_on_one_line_with_the_newline_shown()
-> support::TestOutcome {
    // The TOML string `"octo\ncat"` holds a real newline. The refusal quotes
    // it back, and must show it as the two characters backslash and `n`,
    // never as a line break inside the report.
    assert_refused_on_one_line(
        "text-optional",
        false,
        "assignee = \"octo\\ncat\"\n",
        "option-refused",
        "[package.metadata.skeletons.text-optional] in Cargo.toml: option `assignee` was given \
         `octo\\ncat`, which holds a control character",
    )
}

#[test]
fn a_wearing_table_key_holding_a_newline_is_refused_on_one_line() -> support::TestOutcome {
    // A key inside the wearing table is a name the wearer chose for an option.
    // This one holds a newline, so it can only be reported as undeclared; the
    // report must show it escaped, on the one line, as it would any other name.
    assert_refused_on_one_line(
        "text-optional",
        false,
        "assignee = \"octocat\"\n\"a\\nb\" = \"v\"\n",
        "option-refused",
        "[package.metadata.skeletons.text-optional] in Cargo.toml: option `a\\nb` is not declared \
         by the skeleton",
    )
}

#[test]
fn a_key_holding_a_newline_in_the_skeletons_own_manifest_is_refused_on_one_line()
-> support::TestOutcome {
    // Cargo accepts any key under `package.metadata`, so the skeleton's own
    // manifest can carry one holding a newline. That is a defect in the
    // skeleton, not in the wearer, and it is reported as one, on one line.
    assert_refused_on_one_line(
        "unknown-key-with-newline",
        true,
        "",
        "skeleton-invalid",
        "Cargo.toml: declares unknown key `package.metadata.skeletons.a\\nb`; this is a defect \
         in unknown-key-with-newline 0.0.0, not in this repository",
    )
}
