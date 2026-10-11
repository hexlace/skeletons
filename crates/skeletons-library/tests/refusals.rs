//! What the library says about a wearing it could not check as a whole, and
//! what `files()` and `skeletons_named` answer beside each refusal: a refused
//! skeleton never reads as `Checked`, and a wearing that resolved to no
//! skeleton leaves the lookup undetermined for every package it could be.

mod support;

use std::fs;
use std::os::unix::fs::symlink;

use skeletons_library::{FileState, RefusalKind, Report, SkeletonsNamed};
use support::{
    DEPENDABOT_PATH, Fixture, HOUSE_NOTES_PATH, HOUSE_NOTES_RENDERED, OTHER_NOTES_PATH,
    OTHER_NOTES_RENDERED, TestOutcome, checked_files, path_dependency, paths_of, refused,
    undetermined, wearing_table, wearings_of, worn_once, write_file,
};

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
        [("tools/Cargo.toml", Some("rules"), Some("house-rules"))]
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
        [("tools/Cargo.toml", Some("rules"), Some("house-rules"))]
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
                    ("more-tools/Cargo.toml", Some("rules"), Some("house-rules")),
                    ("tools/Cargo.toml", Some("rules"), Some("house-rules"))
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
                ("tools/Cargo.toml", Some("more"), Some("other-rules")),
                ("tools/Cargo.toml", Some("rules"), Some("house-rules"))
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
        [("tools/Cargo.toml", Some("rulez"), None)]
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
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", None, None)]
    );
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

/// Asserts that `report` holds one refusal and no skeleton, that the refusal is
/// of `kind`, names the wearing of `key` in `tools/Cargo.toml` and the package
/// `package`, names no path, and that the lookup for `package` is
/// `Undetermined` with it while the lookup for any other package is `NotWorn`.
fn assert_only_unidentified_refusal(
    report: &Report,
    kind: RefusalKind,
    key: &str,
    package: &str,
) -> TestOutcome {
    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    let refusals = undetermined(report, package)?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), kind);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some(key), Some(package))]
    );
    assert!(refusals[0].paths().is_empty(), "{:?}", refusals[0]);
    assert_eq!(report.refusals(), refusals);
    assert!(
        matches!(
            report.skeletons_named("nobody-wears-this"),
            SkeletonsNamed::NotWorn
        ),
        "{:?}",
        report.skeletons_named("nobody-wears-this")
    );
    Ok(())
}

#[test]
fn a_dependency_under_a_reserved_key_is_refused_as_a_reserved_key() -> TestOutcome {
    // Given `house-rules` declared under `options`, and again under `verbatim`,
    // each with a wearing table (the second as the `verbatim` list of the
    // `skeletons` table), the wearing is refused as `ReservedKey` however the
    // key is written. It names the key and the package declared under it, the
    // report holds no skeleton, and only the package asked for is unsettled.
    for (key, metadata) in [
        ("options", wearing_table("options")),
        (
            "verbatim",
            "[package.metadata.skeletons]\nverbatim = [\"a\"]\n".to_owned(),
        ),
    ] {
        let fixture = Fixture::new()?;
        fixture.write_member(
            "tools",
            &path_dependency(key, "house-rules", &fixture.house_rules_directory()),
            &metadata,
        )?;
        fixture.generate_lockfile()?;

        let report = skeletons_library::check(fixture.project())?;

        assert_only_unidentified_refusal(&report, RefusalKind::ReservedKey, key, "house-rules")?;
    }
    Ok(())
}

#[test]
fn a_dependency_with_no_library_is_refused_as_unresolved() -> TestOutcome {
    // Given `bin-only`, a crate with a binary and no library, declared and given
    // a wearing table, cargo lists no edge to it, so the wearing resolves to no
    // package and is refused as `Unresolved`. It names the package declared
    // under the key.
    let fixture = Fixture::new()?;
    let binary = fixture.write_crate("bin-only", "0.1.0", "")?;
    std::fs::remove_file(binary.join("src/lib.rs"))?;
    write_file(&binary.join("src/main.rs"), b"fn main() {}\n")?;
    fixture.write_member(
        "tools",
        &path_dependency("bin-only", "bin-only", &binary),
        &wearing_table("bin-only"),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    assert_only_unidentified_refusal(&report, RefusalKind::Unresolved, "bin-only", "bin-only")
}

#[test]
fn a_dependency_that_resolves_to_two_versions_of_a_package_is_refused_as_ambiguous() -> TestOutcome
{
    // Given a vendored source holding `x` at two versions, one required as a
    // dependency and the other as a build dependency of the same member, and a
    // wearing table under `x`, cargo resolves the key to two packages of one
    // name. The wearing is refused as `Ambiguous` and names that package once.
    let fixture = Fixture::new()?;
    for version in ["1.0.0", "2.0.0"] {
        let directory = format!("vendor/x-{version}");
        fixture.write(
            &format!("{directory}/Cargo.toml"),
            format!("[package]\nname = \"x\"\nversion = \"{version}\"\nedition = \"2021\"\n")
                .as_bytes(),
        )?;
        fixture.write(&format!("{directory}/src/lib.rs"), b"")?;
        fixture.write(
            &format!("{directory}/.cargo-checksum.json"),
            b"{\"files\":{}}",
        )?;
    }
    fixture.write(
        ".cargo/config.toml",
        b"[source.crates-io]\nreplace-with = \"vendored\"\n\n\
          [source.vendored]\ndirectory = \"vendor\"\n",
    )?;
    fixture.write_member(
        "tools",
        "x = \"1\"\n",
        &format!("[build-dependencies]\nx = \"2\"\n\n{}", wearing_table("x")),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    assert_only_unidentified_refusal(&report, RefusalKind::Ambiguous, "x", "x")
}

#[test]
fn a_skeleton_with_a_file_that_is_not_text_is_refused_as_invalid_and_stays_worn() -> TestOutcome {
    // Given `broken-rules`, a skeleton whose `notes/broken.txt` is not UTF-8,
    // worn under `broken`, the skeleton is found and `Worn`, but its files are
    // `Refused` with one `SkeletonInvalid` naming the wearing and the package
    // and no path, and nothing is compared. The refusal is about a worn
    // skeleton, so the lookup for any other package is `NotWorn`.
    let fixture = Fixture::new()?;
    let broken = fixture.write_crate("broken-rules", "0.1.0", "[package.metadata.skeletons]\n")?;
    write_file(&broken.join("files/notes/broken.txt"), &[0xff, 0xfe, 0xfd])?;
    fixture.write_member(
        "tools",
        &path_dependency("broken", "broken-rules", &broken),
        &wearing_table("broken"),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let (refusals, compared) = refused(worn_once(&report, "broken-rules")?)?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::SkeletonInvalid);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("broken"), Some("broken-rules"))]
    );
    assert!(refusals[0].paths().is_empty(), "{:?}", refusals[0]);
    assert!(compared.is_empty(), "{compared:?}");
    assert_eq!(report.refusals(), refusals);
    assert!(
        matches!(
            report.skeletons_named("nobody-wears-this"),
            SkeletonsNamed::NotWorn
        ),
        "{:?}",
        report.skeletons_named("nobody-wears-this")
    );
    Ok(())
}
