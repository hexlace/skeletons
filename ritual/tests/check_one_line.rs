//! Acceptance: every message `check` prints is one line, whatever the names
//! in it hold.
//!
//! A newline in a key, a directory name or a claimed file name is the wearer's
//! own text and legitimate on the filesystems this runs on. Printed raw it
//! would split a refusal across lines, and into `--json`'s `message`. Each
//! scenario here puts `first\nsecond` (a real newline between the words) into
//! the one place a refusal kind echoes a name from, and asserts two things
//! of the plain report and of `--json`: no message breaks at the newline, and
//! the newline shows as the two characters `\n`. The fields `--json` carries
//! as the exact string (`manifest`, `dependency`, `path`) are asserted to
//! keep the real newline.
//!
//! Where a name has to exist on disk, the test writes it and lets the
//! filesystem answer; a name the filesystem cannot hold ends the test with a
//! `skipped:` line on stderr, never a silent pass.
//!
//! Platforms: macOS and Linux.

mod support;

use serde_json::Value;

use support::one_line::{
    NAME, assert_field_is_one_line, assert_name_stays_on_one_line, filesystem_keeps_the_name,
    only_refusal, skip, toml_string,
};
use support::sync::fixture_wearing;
use support::{
    Fixture, Report, TestOutcome, path_dependency_on_test_skeleton, write_package_manifest,
};

const RENDER: &str = "rendered: bytes\n";

/// What `check` printed in plain form and in `--json`, for one fixture.
struct Reports {
    human: Report,
    json: Report,
}

fn run_check(fixture: &Fixture) -> Result<Reports, Box<dyn std::error::Error>> {
    let human = fixture.run(&["skeletons", "check"])?;
    let json = fixture.run(&["skeletons", "check", "--json"])?;
    for report in [&human, &json] {
        assert_eq!(
            report.exit_code, 1,
            "check must fail on a refusal, not panic; stdout was {:?}, stderr was {:?}",
            report.stdout, report.stderr
        );
    }
    Ok(Reports { human, json })
}

impl Reports {
    /// The plain report is one line per message and the one refusal `--json`
    /// reports has a one-line `message`; returns that refusal for the caller's
    /// own field checks.
    fn assert_one_line_everywhere(&self, what: &str) -> Result<Value, Box<dyn std::error::Error>> {
        assert_name_stays_on_one_line(&format!("{}{}", self.human.stdout, self.human.stderr), what);
        let document = support::json::parse(&self.json.stdout)?;
        let refusal = only_refusal(&document)?.clone();
        assert_field_is_one_line("message", support::json::refusal_message(&refusal)?);
        Ok(refusal)
    }
}

/// Writes a fixture whose one manifest holds `extra` after `[package]`.
fn fixture_with_manifest(extra: &str) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_package_manifest(fixture.root(), "", "wearer", extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

/// A workspace whose only member lives in a directory named [`NAME`], its
/// manifest holding `extra`.
fn fixture_with_member_in_newline_directory(
    extra: &str,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "Cargo.toml",
        format!(
            "[workspace]\nmembers = [{}]\nresolver = \"2\"\n",
            toml_string(NAME)
        )
        .as_bytes(),
    )?;
    write_package_manifest(fixture.root(), NAME, "member", extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

#[test]
fn a_wearing_table_key_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // A `[package.metadata.skeletons."first\nsecond"]` table names no
    // dependency. The refusal echoes the key; it must print the newline as
    // `\n`, and `--json` must keep the key's real newline in `dependency`
    // while `message` stays one line.
    let fixture = fixture_with_manifest(&format!(
        "[package.metadata.skeletons.{}]\n",
        toml_string(NAME)
    ))?;

    let reports = run_check(&fixture)?;

    let refusal = reports.assert_one_line_everywhere("names-no-dependency, from a table key")?;
    assert_eq!(
        support::json::refusal_kind(&refusal)?,
        "names-no-dependency"
    );
    assert_eq!(
        support::json::refusal_dependency(&refusal)?,
        Some(NAME),
        "`dependency` carries the exact key, newline included; refusal was {refusal}"
    );
    Ok(())
}

#[test]
fn a_manifest_under_a_directory_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // The member's manifest lives at `first\nsecond/Cargo.toml` and holds a
    // wearing table naming no dependency. The refusal echoes the manifest's
    // path; `--json` keeps the real newline in `manifest`.
    let fixture =
        fixture_with_member_in_newline_directory("[package.metadata.skeletons.nothing]\n")?;

    let reports = run_check(&fixture)?;

    let refusal = reports.assert_one_line_everywhere("names-no-dependency, from a manifest")?;
    assert_eq!(
        support::json::refusal_kind(&refusal)?,
        "names-no-dependency"
    );
    let manifest = refusal
        .get("manifest")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("expected a `manifest` string; refusal was {refusal}"))?;
    assert_eq!(
        manifest,
        format!("{NAME}/Cargo.toml"),
        "refusal was {refusal}"
    );
    Ok(())
}

#[test]
fn a_wearing_key_that_is_not_a_table_is_refused_on_one_line() -> TestOutcome {
    // `[package.metadata.skeletons]` holds `"first\nsecond" = "text"`, a
    // string where a table belongs. The refusal echoes the key.
    let fixture = fixture_with_manifest(&format!(
        "[package.metadata.skeletons]\n{} = \"text\"\n",
        toml_string(NAME)
    ))?;

    let reports = run_check(&fixture)?;

    let refusal = reports.assert_one_line_everywhere("not-a-table, with a key")?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "not-a-table");
    Ok(())
}

#[test]
fn a_wearing_table_that_is_not_a_table_in_a_newline_directory_is_refused_on_one_line() -> TestOutcome
{
    // `skeletons = "text"` where the whole table belongs, in a manifest
    // under `first\nsecond/`. With no key to name, the refusal echoes only
    // the manifest's path.
    let fixture =
        fixture_with_member_in_newline_directory("[package.metadata]\nskeletons = \"text\"\n")?;

    let reports = run_check(&fixture)?;

    let refusal = reports.assert_one_line_everywhere("not-a-table, with no key")?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "not-a-table");
    Ok(())
}

#[test]
fn an_option_key_of_the_wrong_shape_holding_a_newline_is_refused_on_one_line() -> TestOutcome {
    // The worn skeleton's table records `"first\nsecond" = 42`, an integer
    // where an option value is a string or an array of strings. The shape
    // refusal echoes the option's name in the message and in `--json`'s
    // `detail`; both must be one line.
    let fixture = fixture_with_manifest(&format!(
        "[dependencies]\n{}\n[package.metadata.skeletons.passthrough-plain]\n{} = 42\n",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        toml_string(NAME)
    ))?;

    let reports = run_check(&fixture)?;

    let refusal = reports.assert_one_line_everywhere("option-refused, the shape refusal")?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "option-refused");
    let detail = refusal
        .get("detail")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("expected a `detail` string; refusal was {refusal}"))?;
    assert_field_is_one_line("detail", detail);
    Ok(())
}

// ---------------------------------------------------------------------
// unsafe-path: the claimed file name holds the newline.
// ---------------------------------------------------------------------

/// The name the skeleton claims in every scenario below.
fn claimed_name() -> String {
    format!("{NAME}.yml")
}

/// A workspace wearing a skeleton that claims [`claimed_name`] (or `claimed`,
/// when a scenario claims another spelling), with no file written for it.
fn fixture_claiming(
    claimed: &str,
) -> Result<support::sync::WearingFixture, Box<dyn std::error::Error>> {
    fixture_wearing(&[("claims-newline", &[(claimed, RENDER)])])
}

/// Asserts the one refusal is an `unsafe-path` of `cause` about the claimed
/// path `claimed`, that both reports keep it on one line, and that `--json`
/// carries the path exactly.
fn assert_unsafe_path_on_one_line(reports: &Reports, cause: &str, claimed: &str) -> TestOutcome {
    let refusal = reports.assert_one_line_everywhere(&format!("unsafe-path, {cause}"))?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "unsafe-path");
    assert_eq!(
        support::json::refusal_cause(&refusal)?,
        Some(cause),
        "refusal was {refusal}"
    );
    assert_eq!(
        support::json::refusal_path(&refusal)?,
        Some(claimed),
        "`path` carries the exact claimed path, newline included; refusal was {refusal}"
    );
    Ok(())
}

#[test]
fn a_claimed_file_that_is_a_symbolic_link_is_refused_on_one_line() -> TestOutcome {
    if !filesystem_keeps_the_name(&claimed_name()) {
        skip("the filesystem cannot hold a file name with a newline in it");
        return Ok(());
    }
    // The claimed `first\nsecond.yml` is a symbolic link to a real file.
    let claimed = claimed_name();
    let wearing = fixture_claiming(&claimed)?;
    let fixture = &wearing.fixture;
    fixture.write("target.yml", RENDER.as_bytes())?;
    std::os::unix::fs::symlink("target.yml", fixture.root().join(&claimed))?;

    let reports = run_check(fixture)?;

    assert_unsafe_path_on_one_line(&reports, "symbolic-link", &claimed)
}

#[test]
fn a_claimed_file_that_is_a_directory_is_refused_on_one_line() -> TestOutcome {
    if !filesystem_keeps_the_name(&claimed_name()) {
        skip("the filesystem cannot hold a file name with a newline in it");
        return Ok(());
    }
    // The claimed `first\nsecond.yml` is a directory on disk.
    let claimed = claimed_name();
    let wearing = fixture_claiming(&claimed)?;
    let fixture = &wearing.fixture;
    std::fs::create_dir(fixture.root().join(&claimed))?;

    let reports = run_check(fixture)?;

    assert_unsafe_path_on_one_line(&reports, "not-a-file", &claimed)
}

#[test]
fn a_claimed_file_present_under_another_case_is_refused_on_one_line() -> TestOutcome {
    let on_disk = format!("{NAME}.YML");
    if !filesystem_keeps_the_name(&on_disk) {
        skip("the filesystem cannot hold a file name with a newline in it");
        return Ok(());
    }
    // The skeleton claims `first\nsecond.yml`; the directory holds only
    // `first\nsecond.YML`. The message names both spellings.
    let claimed = claimed_name();
    let wearing = fixture_claiming(&claimed)?;
    let fixture = &wearing.fixture;
    fixture.write(&on_disk, b"a sibling that is not the claim\n")?;

    let reports = run_check(fixture)?;

    assert_unsafe_path_on_one_line(&reports, "spelled-differently", &claimed)
}

#[test]
fn a_claimed_file_present_under_another_normalization_is_refused_on_one_line() -> TestOutcome {
    // The skeleton claims the precomposed `caf\u{e9}` after the newline; the
    // directory holds the decomposed `cafe\u{301}`. Canonically equivalent
    // names are the one case where the message shows both as code points, and
    // an ASCII newline in them must be escaped there too.
    let claimed = format!("{NAME}-caf\u{e9}.yml");
    let on_disk = format!("{NAME}-cafe\u{301}.yml");
    if !filesystem_keeps_the_name(&on_disk) {
        skip("the filesystem lists a decomposed name with a newline back exactly as written");
        return Ok(());
    }
    let wearing = fixture_claiming(&claimed)?;
    let fixture = &wearing.fixture;
    fixture.write(&on_disk, b"a sibling that is not the claim\n")?;

    let reports = run_check(fixture)?;

    assert_unsafe_path_on_one_line(&reports, "spelled-differently", &claimed)
}

// ---------------------------------------------------------------------
// overlap
// ---------------------------------------------------------------------

#[test]
fn two_skeletons_claiming_one_path_holding_a_newline_are_refused_on_one_line() -> TestOutcome {
    // Two worn skeletons each claim `first\nsecond.yml`. The overlap message
    // names the path.
    let claimed = claimed_name();
    let wearing = fixture_wearing(&[
        ("claims-one", &[(&claimed, RENDER)]),
        ("claims-two", &[(&claimed, RENDER)]),
    ])?;

    let reports = run_check(&wearing.fixture)?;

    let refusal = reports.assert_one_line_everywhere("overlap, one path")?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "overlap");
    let paths: Vec<&str> = refusal
        .get("paths")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("expected a `paths` array; refusal was {refusal}"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        paths,
        [claimed.as_str()],
        "`paths` carries the exact path, newline included; refusal was {refusal}"
    );
    Ok(())
}

#[test]
fn two_skeletons_claiming_one_name_in_two_cases_are_refused_on_one_line() -> TestOutcome {
    // `first\nsecond.yml` and `first\nsecond.YML` are one name to a filesystem
    // that ignores case, so the two claims overlap. The two-path message
    // names both spellings.
    let lower = claimed_name();
    let upper = format!("{NAME}.YML");
    let wearing = fixture_wearing(&[
        ("claims-one", &[(&lower, RENDER)]),
        ("claims-two", &[(&upper, RENDER)]),
    ])?;

    let reports = run_check(&wearing.fixture)?;

    let refusal = reports.assert_one_line_everywhere("overlap, two spellings")?;
    assert_eq!(support::json::refusal_kind(&refusal)?, "overlap");
    Ok(())
}

// ---------------------------------------------------------------------
// The lines `check` prints outside a refusal.
// ---------------------------------------------------------------------

#[test]
fn a_missing_claimed_file_holding_a_newline_is_listed_on_one_line() -> TestOutcome {
    // The skeleton claims `first\nsecond.yml` and the workspace has no such
    // file, so `check` lists it as `drifted (missing)`. The line must print
    // the path with the newline escaped; `--json`'s bone `path` keeps the
    // real newline.
    let claimed = claimed_name();
    let wearing = fixture_claiming(&claimed)?;

    let reports = run_check(&wearing.fixture)?;

    assert_name_stays_on_one_line(&reports.human.stdout, "drifted (missing) line");
    let document = support::json::parse(&reports.json.stdout)?;
    let bones = support::json::bones(&document)?;
    let [bone] = bones.as_slice() else {
        return Err(format!("expected exactly one bone; bones were {bones:?}").into());
    };
    assert_eq!(
        support::json::bone_path(bone)?,
        claimed,
        "`path` carries the exact claimed path, newline included; bone was {bone}"
    );
    Ok(())
}

#[test]
fn a_worn_skeleton_in_a_manifest_under_a_newline_directory_is_headed_on_one_line() -> TestOutcome {
    // The header line above each worn skeleton's files reads `<dependency> in
    // <manifest>: ...`. With the manifest under `first\nsecond/` the header
    // must not break at the newline; `--json`'s `manifest` keeps it.
    let extra = format!(
        "[dependencies]\n{}\n[package.metadata.skeletons.passthrough-plain]\n",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
    );
    let fixture = fixture_with_member_in_newline_directory(&extra)?;

    let reports = run_check(&fixture)?;

    assert_name_stays_on_one_line(&reports.human.stdout, "worn skeleton header");
    let document = support::json::parse(&reports.json.stdout)?;
    let skeletons = support::json::skeletons(&document)?;
    let [skeleton] = skeletons.as_slice() else {
        return Err(format!("expected one worn skeleton; rows were {skeletons:?}").into());
    };
    assert_eq!(
        support::json::skeleton_manifest(skeleton)?,
        format!("{NAME}/Cargo.toml"),
        "`manifest` carries the exact path, newline included; row was {skeleton}"
    );
    Ok(())
}
