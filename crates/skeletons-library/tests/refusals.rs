//! What the library says about a wearing it could not check as a whole, and
//! what `files()` and `skeletons_named` answer beside each refusal: a refused
//! skeleton never reads as `Checked`, and a wearing that resolved to no
//! skeleton never reads as `NotWorn`.

mod support;

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;

use skeletons_library::{
    ClaimedFile, FileState, Files, Refusal, RefusalKind, Report, SkeletonsNamed, WornSkeleton,
};
use support::{
    DEPENDABOT_PATH, Fixture, HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED, OTHER_NOTES_PATH,
    OTHER_NOTES_RENDERED, TestOutcome, checked_files, wearing_table, worn_once,
};

/// The manifest and dependency key of each wearing a refusal names, in order.
fn wearings_of(refusal: &Refusal) -> Vec<(&str, Option<&str>)> {
    refusal
        .wearings()
        .iter()
        .map(|wearing| {
            (
                wearing.manifest().to_str().unwrap_or("?"),
                wearing.dependency_key(),
            )
        })
        .collect()
}

/// The paths a refusal names, as text.
fn paths_of(refusal: &Refusal) -> Vec<&str> {
    refusal
        .paths()
        .iter()
        .map(|path| path.to_str().unwrap_or("?"))
        .collect()
}

/// The refusals and the compared files of a skeleton whose files were refused.
type RefusedFiles<'report> = (&'report [Refusal], &'report [ClaimedFile]);

/// The refusals and compared files of a skeleton whose files were refused, or
/// an error naming what `files()` answered instead.
fn refused(skeleton: &WornSkeleton) -> Result<RefusedFiles<'_>, Box<dyn Error>> {
    match skeleton.files() {
        Files::Refused {
            refusals, compared, ..
        } => Ok((refusals, compared)),
        other => Err(format!(
            "expected {} to be Refused, files() answered {other:?}",
            skeleton.dependency_key()
        )
        .into()),
    }
}

/// The refusals the lookup for `package` answered as `Undetermined`, or an
/// error naming what it answered instead.
fn undetermined<'report>(
    report: &'report Report,
    package: &str,
) -> Result<&'report [Refusal], Box<dyn Error>> {
    match report.skeletons_named(package) {
        SkeletonsNamed::Undetermined(refusals) => Ok(refusals),
        other => Err(format!("expected {package} to be Undetermined, found {other:?}").into()),
    }
}

#[test]
fn an_option_the_skeleton_does_not_declare_refuses_the_skeleton() -> TestOutcome {
    // Given a wearing that records an option `house-rules` does not declare,
    // the skeleton is still found, but its files are `Refused` with one
    // `OptionRefused` naming the wearing and no path, nothing was compared, and
    // the same refusal is in the report's own list.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &fixture.rules_dependency(),
        &format!("{}nonesuch = \"x\"\n", wearing_table("rules")),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let skeleton = worn_once(&report, "house-rules")?;
    let (refusals, compared) = refused(skeleton)?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::OptionRefused);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("rules"))]
    );
    assert!(refusals[0].paths().is_empty(), "{:?}", refusals[0]);
    assert!(compared.is_empty(), "{compared:?}");
    assert_eq!(report.refusals(), refusals);
    Ok(())
}

#[test]
fn a_symbolic_link_at_a_claimed_path_refuses_the_skeleton_and_compares_nothing() -> TestOutcome {
    // Given a symbolic link where `house-rules` claims `.github/dependabot.yml`
    // and a matching second file, the skeleton is `Refused` with one
    // `UnsafePath` naming that path. Nothing is compared, though the second
    // file is in order: a skeleton with a refusal of its own has no compared
    // file.
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write("elsewhere.yml", b"interval: \"weekly\"\n")?;
    fs::create_dir_all(fixture.project().join(".github"))?;
    symlink(
        fixture.project().join("elsewhere.yml"),
        fixture.project().join(DEPENDABOT_PATH),
    )?;
    fixture.write(HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED.as_bytes())?;

    let report = skeletons_library::check(fixture.project())?;

    let (refusals, compared) = refused(worn_once(&report, "house-rules")?)?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::UnsafePath);
    assert_eq!(paths_of(&refusals[0]), [DEPENDABOT_PATH]);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("rules"))]
    );
    assert!(compared.is_empty(), "{compared:?}");
    Ok(())
}

#[test]
fn a_skeleton_worn_by_two_members_is_refused_in_both_with_an_overlap_per_path() -> TestOutcome {
    // Given `house-rules` worn by `tools` and by `more-tools`, both claim the
    // same two paths. The lookup finds two skeletons, ordered by manifest, each
    // `Refused` with one `Overlap` per colliding path, each naming both
    // wearings, and nothing compared because every path collides.
    let fixture = Fixture::new()?;
    fixture.write_workspace_manifest(&["more-tools", "tools"])?;
    for member in ["tools", "more-tools"] {
        fixture.write_member(member, &fixture.rules_dependency(), &wearing_table("rules"))?;
    }
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let SkeletonsNamed::Worn(skeletons) = report.skeletons_named("house-rules") else {
        return Err("house-rules was not found".into());
    };
    let manifests: Vec<_> = skeletons.iter().map(|s| s.manifest().to_str()).collect();
    assert_eq!(
        manifests,
        [Some("more-tools/Cargo.toml"), Some("tools/Cargo.toml")]
    );
    for skeleton in skeletons {
        let (refusals, compared) = refused(skeleton)?;
        assert_eq!(refusals.len(), 2, "{refusals:?}");
        assert!(compared.is_empty(), "{compared:?}");
        let mut paths = Vec::new();
        for refusal in refusals {
            assert_eq!(refusal.kind(), RefusalKind::Overlap);
            let mut wearings = wearings_of(refusal);
            wearings.sort_unstable();
            assert_eq!(
                wearings,
                [
                    ("more-tools/Cargo.toml", Some("rules")),
                    ("tools/Cargo.toml", Some("rules"))
                ]
            );
            paths.extend(paths_of(refusal));
        }
        paths.sort_unstable();
        assert_eq!(paths, [DEPENDABOT_PATH, HOUSE_NOTES_PATH]);
    }
    assert_eq!(report.refusals().len(), 2, "{:?}", report.refusals());
    Ok(())
}

#[test]
fn two_skeletons_claiming_one_path_are_refused_but_keep_the_files_that_collide_with_nothing()
-> TestOutcome {
    // Given `house-rules` and `other-rules` worn by one member, they claim
    // `notes/house.txt` between them. Each is `Refused` with the one `Overlap`
    // naming that path and both wearings, and each keeps exactly one compared
    // file, its own, in the state it has on disk: `dependabot.yml` is absent,
    // and `other.txt` holds what `other-rules` renders.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            fixture.rules_dependency(),
            fixture.more_dependency()
        ),
        &format!("{}{}", wearing_table("rules"), wearing_table("more")),
    )?;
    fixture.generate_lockfile()?;
    fixture.write(OTHER_NOTES_PATH, OTHER_NOTES_RENDERED.as_bytes())?;

    let report = skeletons_library::check(fixture.project())?;

    let expected = [
        ("house-rules", DEPENDABOT_PATH, FileState::Missing),
        ("other-rules", OTHER_NOTES_PATH, FileState::Matches),
    ];
    for (package, own_path, own_state) in expected {
        let (refusals, compared) = refused(worn_once(&report, package)?)?;
        assert_eq!(refusals.len(), 1, "{package}: {refusals:?}");
        assert_eq!(refusals[0].kind(), RefusalKind::Overlap);
        assert_eq!(paths_of(&refusals[0]), [HOUSE_NOTES_PATH]);
        let mut wearings = wearings_of(&refusals[0]);
        wearings.sort_unstable();
        assert_eq!(
            wearings,
            [
                ("tools/Cargo.toml", Some("more")),
                ("tools/Cargo.toml", Some("rules"))
            ]
        );
        let seen: Vec<_> = compared
            .iter()
            .map(|file| (file.path().to_str(), file.state()))
            .collect();
        assert_eq!(seen, [(Some(own_path), own_state)], "{package}");
    }
    assert_eq!(report.refusals().len(), 1, "{:?}", report.refusals());
    Ok(())
}

#[test]
fn a_wearing_table_under_a_key_no_dependency_has_makes_the_lookup_undetermined() -> TestOutcome {
    // Given a dependency `rules` with no table and a table under the misspelt
    // key `rulez`: no wearing resolved to a skeleton, so there is none, but the
    // lookup for `house-rules` says `Undetermined`, never `NotWorn`, because
    // the misspelt table may have been meant for it. The refusal names the
    // manifest and the misspelt key.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &fixture.rules_dependency(),
        &wearing_table("rulez"),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    let refusals = undetermined(&report, "house-rules")?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::NamesNoDependency);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("rulez"))]
    );
    assert!(refusals[0].paths().is_empty(), "{:?}", refusals[0]);
    assert_eq!(report.refusals(), refusals);
    Ok(())
}

#[test]
fn a_skeletons_table_that_is_not_a_table_makes_the_lookup_undetermined() -> TestOutcome {
    // Given `[package.metadata] skeletons = "text"`, the refusal is about the
    // manifest's whole table, so its wearing has no dependency key, and a
    // lookup is `Undetermined`.
    let fixture = Fixture::new()?;
    fixture.write_member("tools", "", "[package.metadata]\nskeletons = \"text\"\n")?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let refusals = undetermined(&report, "house-rules")?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::NotATable);
    assert_eq!(wearings_of(&refusals[0]), [("tools/Cargo.toml", None)]);
    Ok(())
}

#[test]
fn a_misspelt_table_beside_a_good_wearing_does_not_unsettle_the_good_one() -> TestOutcome {
    // Given a good wearing of `rules` and a misspelt `rulez` beside it, the
    // skeleton that was read in full is `Worn` and `Checked`, and the
    // unidentified wearing is in the report's refusals rather than turning the
    // found skeleton into `Undetermined`.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &fixture.rules_dependency(),
        &format!("{}{}", wearing_table("rules"), wearing_table("rulez")),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let files = checked_files(worn_once(&report, "house-rules")?)?;
    assert_eq!(files.len(), 2, "{files:?}");
    assert_eq!(report.refusals().len(), 1, "{:?}", report.refusals());
    assert_eq!(report.refusals()[0].kind(), RefusalKind::NamesNoDependency);
    // A package nobody wears is not `NotWorn` while a wearing is unidentified.
    assert!(matches!(
        report.skeletons_named("nobody-wears-this"),
        SkeletonsNamed::Undetermined(_)
    ));
    Ok(())
}
