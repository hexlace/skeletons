//! The library makes no network request.

mod support;

use support::{Fixture, TestOutcome, git, wearing_table};

#[test]
fn a_skeleton_that_was_never_fetched_is_an_error_and_not_a_download() -> TestOutcome {
    // Given a skeleton committed to a git repository the test creates and taken
    // by `git = "file://…"`, with a lockfile made by a cargo whose home has
    // fetched it: a `check` in this process, whose cargo home has not, must
    // fail rather than fetch. The control is the same project asked of the
    // child's cargo home with `--offline`, which succeeds, so the missing fetch
    // is the only thing wrong. Allowing the network would make the read succeed,
    // and this test red.
    let fixture = Fixture::new()?;
    let url = git::commit_repository(&fixture.house_rules_directory(), &fixture.home())?;
    fixture.write_member(
        "tools",
        &format!("rules = {{ package = \"house-rules\", git = \"{url}\" }}\n"),
        &wearing_table("rules"),
    )?;
    fixture.cargo(&["generate-lockfile"])?;
    fixture.cargo(&[
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--all-features",
        "--offline",
    ])?;

    let Err(error) = skeletons_library::check(fixture.project()) else {
        return Err("a skeleton nobody had fetched was read".into());
    };

    assert!(error.is_cargo_metadata_failed(), "{error:?}");
    let detail = error.detail().ok_or("cargo failed with nothing to say")?;
    assert!(detail.contains(&url), "cargo said: {detail}");
    Ok(())
}
