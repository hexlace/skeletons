//! Acceptance: a claim is accepted only under the exact spelling its skeleton
//! gives it — on a case-insensitive filesystem, `check` must never treat a
//! differently spelled file on disk as though it were the claimed one.
//!
//! `nested-dotfiles` (checked in under `crates/skeletons/test-skeletons/renders/`)
//! claims `.github/dependabot.yml` (rendering `dependabot: true\n`),
//! `a/b/deep.yml`, and `root.yml`. Every scenario here writes the other two
//! claimed files matching their renders exactly, so the only thing under
//! test is the spelling of the `.github` claim.
//!
//! Every scenario is written to expect the identical answer on a
//! case-insensitive filesystem (a default macOS volume) and a case-sensitive
//! one (Linux): the two platforms must agree here, so nothing in these
//! tests branches on which one is running.

mod support;

use support::sync::fixture_wearing;
use support::{Fixture, path_dependency_on_test_skeleton, wearing_table, write_package_manifest};

/// Builds a fixture wearing `nested-dotfiles`, with its manifest and
/// lockfile written but none of its three claimed files created yet.
fn fixture_wearing_nested_dotfiles() -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("nested-dotfiles", "nested-dotfiles"),
        wearing_table("nested-dotfiles", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

/// Builds the nested-dotfiles fixture for the scenario this file's first
/// test is about — `.github` present on disk only as `DEPENDABOT.YML`,
/// holding bytes the skeleton never rendered, and the exact claimed
/// spelling absent entirely — runs `check --json` against it, and returns
/// the fixture, the parsed document, and its one refusal.
fn fixture_and_refusal_for_dependabot_present_under_a_different_spelling()
-> Result<(Fixture, serde_json::Value, serde_json::Value), Box<dyn std::error::Error>> {
    let fixture = fixture_wearing_nested_dotfiles()?;
    fixture.write(".github/DEPENDABOT.YML", b"PRECIOUS")?;
    fixture.write("root.yml", b"root-file: yes\n")?;
    fixture.write("a/b/deep.yml", b"nested: yes\n")?;

    let json_report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        json_report.exit_code, 1,
        "a claimed file present only under a different spelling must fail check; stderr was: {}",
        json_report.stderr
    );

    let document = support::json::parse(&json_report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(
        refusals.len(),
        1,
        "expected exactly one refusal; refusals were: {refusals:?}"
    );
    let refusal = refusals[0].clone();
    Ok((fixture, document, refusal))
}

#[test]
fn a_claimed_file_present_only_under_a_different_spelling_is_refused_not_read()
-> support::TestOutcome {
    // `.github/DEPENDABOT.YML` exists, holding content the skeleton never
    // rendered (`PRECIOUS`), and `.github/dependabot.yml` — the exact
    // spelling the skeleton claims — does not exist at all. `check` must
    // refuse the skeleton as `unsafe-path`, cause `spelled-differently`, naming the
    // on-disk spelling it found — never read `DEPENDABOT.YML`'s bytes as
    // though they belonged to the claim.
    let (fixture, document, refusal) =
        fixture_and_refusal_for_dependabot_present_under_a_different_spelling()?;
    assert_eq!(
        support::json::refusal_kind(&refusal)?,
        "unsafe-path",
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_cause(&refusal)?,
        Some("spelled-differently"),
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_path(&refusal)?,
        Some(".github/dependabot.yml"),
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_at(&refusal)?,
        Some(".github/dependabot.yml"),
        "`at` must equal `path` when the differing component is the file itself; refusal was: \
         {refusal}"
    );
    assert_eq!(
        support::json::refusal_on_disk(&refusal)?,
        Some(vec![".github/DEPENDABOT.YML"]),
        "`on_disk` must name the file's on-disk spelling; refusal was: {refusal}"
    );

    let skeletons = support::json::skeletons(&document)?;
    let skeleton = skeletons
        .iter()
        .find(|row| support::json::skeleton_dependency(row).ok() == Some("nested-dotfiles"))
        .ok_or_else(|| {
            format!("expected a row for nested-dotfiles; skeletons were: {skeletons:?}")
        })?;
    assert!(
        support::json::skeleton_refused(skeleton)?,
        "the skeleton must be reported refused; skeleton row was: {skeleton}"
    );
    let bones = support::json::bones(&document)?;
    assert!(
        bones.is_empty(),
        "a refused skeleton's bones must be absent from `bones`; bones were: {bones:?}"
    );

    let human_report = fixture.run(&["skeletons", "check"])?;
    assert!(
        human_report
            .stdout
            .contains("refused: .github/dependabot.yml is spelled .github/DEPENDABOT.YML on disk"),
        "expected the refusal naming the on-disk spelling in the plain output; stdout was: {}",
        human_report.stdout
    );
    Ok(())
}

#[test]
fn a_claim_under_a_differently_spelled_intermediate_directory_is_refused_even_when_bytes_match()
-> support::TestOutcome {
    // `.GitHub/dependabot.yml` holds the render's exact bytes, but `check`
    // must never resolve the lookup through `.GitHub` on macOS, find the
    // bytes equal, and report `matches` for a file the claim never actually
    // names. It must instead refuse, naming the differing ancestor directory
    // (`.github`, on disk `.GitHub`), never report an exact answer for a
    // path it did not verify component by component.
    let fixture = fixture_wearing_nested_dotfiles()?;
    fixture.write(".GitHub/dependabot.yml", b"dependabot: true\n")?;
    fixture.write("root.yml", b"root-file: yes\n")?;
    fixture.write("a/b/deep.yml", b"nested: yes\n")?;

    let json_report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        json_report.exit_code, 1,
        "a claim under a differently spelled intermediate directory must fail check even though \
         the file's bytes match the render; stderr was: {}",
        json_report.stderr
    );

    let document = support::json::parse(&json_report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(
        refusals.len(),
        1,
        "expected exactly one refusal; refusals were: {refusals:?}"
    );
    let refusal = &refusals[0];
    assert_eq!(
        support::json::refusal_kind(refusal)?,
        "unsafe-path",
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_cause(refusal)?,
        Some("spelled-differently"),
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_at(refusal)?,
        Some(".github"),
        "`at` must name the differing ancestor directory, not the file; refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_on_disk(refusal)?,
        Some(vec![".GitHub"]),
        "`on_disk` must name the ancestor's on-disk spelling; refusal was: {refusal}"
    );

    let human_report = fixture.run(&["skeletons", "check"])?;
    assert!(
        human_report.stdout.contains(
            "refused: .github/dependabot.yml is under .GitHub on disk, which its skeleton spells \
             .github"
        ),
        "expected the refusal naming the differing ancestor in the plain output; stdout was: {}",
        human_report.stdout
    );
    Ok(())
}

#[test]
fn a_claim_beside_a_file_spelled_in_another_unicode_normalisation_is_refused_on_every_platform()
-> support::TestOutcome {
    // The skeleton claims `café.txt` spelled with the precomposed `é` (NFC,
    // U+00E9). On disk beside it sits `café.txt` spelled with `e` and a
    // combining acute accent (NFD, U+0065 U+0301), holding bytes the
    // skeleton never rendered. To a person the two names are one word; to a
    // filesystem they may or may not be one file, and which is not `check`'s
    // to guess. It must refuse, as `spelled-differently`, naming the NFD
    // spelling as the one on disk, and never read those bytes as the claim's.
    //
    // macOS (APFS keeps the names as written but looks either up) and Linux
    // (ext4 keeps them as two files) must give the same answer, so nothing
    // here branches on the platform.
    //
    // On Linux a `check` that looked only for the claim's own spelling
    // would find it missing, refuse nothing and exit 0. On macOS the
    // filesystem's own lookup already refuses it as `spelled-differently`,
    // but `on_disk` must still name the NFD spelling, which takes listing
    // the directory entry beside the claim.
    const CLAIMED_NFC: &str = "caf\u{e9}.txt";
    const ON_DISK_NFD: &str = "cafe\u{301}.txt";
    assert_ne!(
        CLAIMED_NFC, ON_DISK_NFD,
        "the two spellings must differ byte for byte"
    );

    let wearing = fixture_wearing(&[("normalisation", &[(CLAIMED_NFC, "rendered\n")])])?;
    let fixture = &wearing.fixture;
    fixture.write(ON_DISK_NFD, b"PRECIOUS")?;
    let listed: Vec<String> = std::fs::read_dir(fixture.root())?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    assert!(
        listed.iter().any(|name| name == ON_DISK_NFD),
        "precondition: the directory lists the NFD spelling as written; it lists {listed:?}"
    );

    let json_report = fixture.run(&["skeletons", "check", "--json"])?;

    assert_eq!(
        json_report.exit_code, 1,
        "a claim beside a differently normalised spelling must fail check; stdout was: {}, \
         stderr was: {}",
        json_report.stdout, json_report.stderr
    );
    let document = support::json::parse(&json_report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert_eq!(
        refusals.len(),
        1,
        "expected exactly one refusal; refusals were: {refusals:?}"
    );
    let refusal = &refusals[0];
    assert_eq!(
        support::json::refusal_kind(refusal)?,
        "unsafe-path",
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_cause(refusal)?,
        Some("spelled-differently"),
        "refusal was: {refusal}"
    );
    assert_eq!(
        support::json::refusal_on_disk(refusal)?,
        Some(vec![ON_DISK_NFD]),
        "`on_disk` must name the NFD spelling; refusal was: {refusal}"
    );
    Ok(())
}
