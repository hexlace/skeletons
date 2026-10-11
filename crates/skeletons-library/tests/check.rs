//! A workspace that wears nothing, one that wears a skeleton whose files match,
//! are missing or have changed, the round trip from one to the other, and the
//! order the worn skeletons come in.

mod support;

use std::fs;

use skeletons_library::{ClaimedFile, FileState, SkeletonsNamed, WornSkeleton};
use support::{
    DEPENDABOT_PATH, DEPENDABOT_RENDERED, Fixture, HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED,
    TestOutcome, checked_files, wearing_table, worn_once, write_file,
};

#[test]
fn a_workspace_that_wears_nothing_reports_no_skeleton_and_no_refusal() -> TestOutcome {
    // Given a workspace whose member declares no wearing table, asked from the
    // member's own directory: the report holds no skeleton and no refusal, the
    // lookup for a package says it is not worn, and `root()` is the workspace
    // root as the filesystem names it, not the directory the call was given.
    let fixture = Fixture::new()?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project().join("tools"))?;

    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    assert!(report.refusals().is_empty(), "{:?}", report.refusals());
    assert_eq!(
        report.skeletons_named("house-rules"),
        SkeletonsNamed::NotWorn
    );
    assert_eq!(report.root(), fs::canonicalize(fixture.project())?);
    Ok(())
}

#[test]
fn a_worn_skeleton_whose_files_match_says_so() -> TestOutcome {
    // Given both files on disk holding what the skeleton renders, the skeleton
    // is found by its package name, not by its dependency key, with its locked
    // version, the manifest that wears it and the key it is worn under, and
    // both files are `Matches`, in path order, with the bytes they should hold.
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, DEPENDABOT_RENDERED.as_bytes())?;
    fixture.write(HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED.as_bytes())?;

    let report = skeletons_library::check(fixture.project())?;

    let skeleton = worn_once(&report, "house-rules")?;
    assert_eq!(skeleton.package(), "house-rules");
    assert_eq!(skeleton.dependency_key(), "rules");
    assert_eq!(skeleton.manifest().to_str(), Some("tools/Cargo.toml"));
    let version = skeleton.version();
    assert_eq!(
        (version.major(), version.minor(), version.patch()),
        (4, 5, 6)
    );
    assert_eq!(version.pre_release(), None);
    assert_eq!(version.build_metadata(), None);
    assert_eq!(version.to_string(), "4.5.6");
    let files = checked_files(skeleton)?;
    let seen: Vec<_> = files
        .iter()
        .map(|file| (file.path().to_str(), file.state(), file.rendered()))
        .collect();
    assert_eq!(
        seen,
        [
            (
                Some(DEPENDABOT_PATH),
                FileState::Matches,
                DEPENDABOT_RENDERED.as_bytes()
            ),
            (
                Some(HOUSE_NOTES_PATH),
                FileState::Matches,
                HOUSE_NOTES_RENDERED.as_bytes()
            ),
        ]
    );
    assert!(report.refusals().is_empty(), "{:?}", report.refusals());
    // The dependency key is the wearer's, so it finds nothing.
    assert_eq!(report.skeletons_named("rules"), SkeletonsNamed::NotWorn);
    Ok(())
}

#[test]
fn a_file_that_is_not_there_is_missing_and_still_carries_its_bytes() -> TestOutcome {
    // Given a project with neither claimed file on disk, the skeleton is still
    // `Checked`, and both files are `Missing`, drifted, with the bytes that
    // would put them right.
    let fixture = Fixture::wearing_house_rules()?;

    let report = skeletons_library::check(fixture.project())?;

    let files = checked_files(worn_once(&report, "house-rules")?)?;
    assert_eq!(files.len(), 2, "{files:?}");
    for file in files {
        assert_eq!(file.state(), FileState::Missing, "{file:?}");
        assert!(file.state().is_drifted(), "{file:?}");
    }
    assert_eq!(files[0].rendered(), DEPENDABOT_RENDERED.as_bytes());
    assert_eq!(files[1].rendered(), HOUSE_NOTES_RENDERED.as_bytes());
    Ok(())
}

#[test]
fn a_file_holding_other_bytes_is_changed_and_its_neighbour_still_matches() -> TestOutcome {
    // Given one file overwritten with other bytes and the other left as the
    // skeleton renders it, only the overwritten one is `Changed`.
    let fixture = Fixture::wearing_house_rules()?;
    fixture.write(DEPENDABOT_PATH, b"interval: \"daily\"\n")?;
    fixture.write(HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED.as_bytes())?;

    let report = skeletons_library::check(fixture.project())?;

    let files = checked_files(worn_once(&report, "house-rules")?)?;
    let states: Vec<_> = files.iter().map(ClaimedFile::state).collect();
    assert_eq!(states, [FileState::Changed, FileState::Matches]);
    assert!(states[0].is_drifted());
    assert!(!states[1].is_drifted());
    Ok(())
}

#[test]
fn writing_what_is_rendered_to_each_path_makes_every_file_match() -> TestOutcome {
    // The promise `rendered` documents: given a fresh project, every file is
    // `Missing`; the test writes each `rendered()` to its `path()` under the
    // root with `std::fs` alone (no git is run, and the directory is not a
    // repository); the next check finds `Checked` with every file `Matches`
    // and no refusal.
    let fixture = Fixture::wearing_house_rules()?;
    let before = skeletons_library::check(fixture.project())?;
    let files = checked_files(worn_once(&before, "house-rules")?)?;
    assert!(files.iter().all(|file| file.state() == FileState::Missing));

    for file in files {
        write_file(&before.root().join(file.path()), file.rendered())?;
    }

    let after = skeletons_library::check(fixture.project())?;
    let files = checked_files(worn_once(&after, "house-rules")?)?;
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(
        files.iter().all(|file| file.state() == FileState::Matches),
        "{files:?}"
    );
    assert!(after.refusals().is_empty(), "{:?}", after.refusals());
    Ok(())
}

#[test]
fn skeletons_are_ordered_by_package_name_and_each_lookup_finds_its_own() -> TestOutcome {
    // Given a member wearing `other-rules` under the key `more` and
    // `house-rules` under the key `rules`: the key sorts `more` first, the
    // package name sorts `house-rules` first, and `skeletons()` follows the
    // package name. Each package's lookup finds exactly its own skeleton. The
    // two skeletons overlap on one path, which `refusals.rs` reads; here only
    // the order and the lookups are asserted.
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

    let report = skeletons_library::check(fixture.project())?;

    let packages: Vec<_> = report
        .skeletons()
        .iter()
        .map(WornSkeleton::package)
        .collect();
    assert_eq!(packages, ["house-rules", "other-rules"]);
    for package in ["house-rules", "other-rules"] {
        let SkeletonsNamed::Worn(found) = report.skeletons_named(package) else {
            return Err(format!("{package} was not found").into());
        };
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].package(), package);
    }
    assert_eq!(
        worn_once(&report, "other-rules")?.version().to_string(),
        "1.2.3"
    );
    Ok(())
}

#[test]
fn a_version_with_a_pre_release_and_build_metadata_gives_each_part_and_orders() -> TestOutcome {
    // Given `house-rules` at 4.5.6-rc.1+build.7 and `other-rules` at 1.2.3 in
    // one workspace, the parts come apart, absence is `None` where there is
    // none, `Display` is the whole text, and `Ord` puts the older version
    // first. Both are worn by one member, which refuses them over one shared
    // path; the version is still read.
    let fixture = Fixture::new()?;
    fixture.write_house_rules("4.5.6-rc.1+build.7")?;
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

    let report = skeletons_library::check(fixture.project())?;

    let newer = worn_once(&report, "house-rules")?.version();
    let older = worn_once(&report, "other-rules")?.version();
    assert_eq!((newer.major(), newer.minor(), newer.patch()), (4, 5, 6));
    assert_eq!(newer.pre_release(), Some("rc.1"));
    assert_eq!(newer.build_metadata(), Some("build.7"));
    assert_eq!(newer.to_string(), "4.5.6-rc.1+build.7");
    assert_eq!(older.pre_release(), None);
    assert_eq!(older.build_metadata(), None);
    assert!(older < newer, "{older} should sort before {newer}");
    assert!(
        (older.major(), older.minor()) < (4, 5) && (newer.major(), newer.minor()) >= (4, 5),
        "the parts answer `at least 4.5`"
    );
    Ok(())
}
