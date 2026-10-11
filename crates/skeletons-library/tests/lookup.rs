//! What `skeletons_named` answers beside a wearing that resolved to no
//! skeleton: the lookup is unsettled only by a refusal that could be the
//! package asked for, because its wearing names that package or names none.

mod support;

use skeletons_library::{Refusal, RefusalKind, SkeletonsNamed};
use support::{
    Fixture, TestOutcome, path_dependency, undetermined, wearing_table, wearings_of, write_file,
};

#[test]
fn a_refusal_naming_another_package_leaves_the_lookup_not_worn() -> TestOutcome {
    // Given `plain-crate`, a package with no `skeletons` metadata, declared
    // under the key `plain` and given a wearing table, the wearing is refused
    // as `NotASkeleton` and names the package `plain-crate`. Asking for that
    // package is `Undetermined` with the refusal, whose wearing names the
    // manifest, the key and the package and has no path. Asking for any other
    // package, `house-rules` and the misspelling `plain_crate` included, is
    // `NotWorn`: the refusal cannot be them, and names are compared exactly.
    let fixture = Fixture::new()?;
    let plain = fixture.write_crate("plain-crate", "0.1.0", "")?;
    fixture.write_member(
        "tools",
        &path_dependency("plain", "plain-crate", &plain),
        &wearing_table("plain"),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let refusals = undetermined(&report, "plain-crate")?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::NotASkeleton);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("plain"), Some("plain-crate"))]
    );
    assert!(refusals[0].paths().is_empty(), "{:?}", refusals[0]);
    assert!(report.skeletons().is_empty(), "{:?}", report.skeletons());
    assert_eq!(report.refusals(), refusals);
    for other in ["house-rules", "plain_crate", "plain"] {
        assert!(
            matches!(report.skeletons_named(other), SkeletonsNamed::NotWorn),
            "{other}: {:?}",
            report.skeletons_named(other)
        );
    }
    Ok(())
}

#[test]
fn each_lookup_holds_the_refusals_naming_its_package_then_those_naming_none() -> TestOutcome {
    // Given three refused wearings in one member: a table under the misspelt
    // key `rulez` (names no package), a non-skeleton `plain-crate`, and a
    // binary-only crate `bin-only` that cargo resolves to no library. Asking
    // for `plain-crate` holds exactly its own refusal and then the misspelt
    // one, asking for `bin-only` holds its own and then the misspelt one, and
    // asking for a package none of them names holds the misspelt one alone.
    let fixture = Fixture::new()?;
    let plain = fixture.write_crate("plain-crate", "0.1.0", "")?;
    let binary = fixture.write_crate("bin-only", "0.1.0", "")?;
    std::fs::remove_file(binary.join("src/lib.rs"))?;
    write_file(&binary.join("src/main.rs"), b"fn main() {}\n")?;
    fixture.write_member(
        "tools",
        &format!(
            "{}{}",
            path_dependency("plain", "plain-crate", &plain),
            path_dependency("bin-only", "bin-only", &binary)
        ),
        &format!(
            "{}{}{}",
            wearing_table("plain"),
            wearing_table("bin-only"),
            wearing_table("rulez")
        ),
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let expected = [
        (
            "plain-crate",
            [RefusalKind::NotASkeleton, RefusalKind::NamesNoDependency],
        ),
        (
            "bin-only",
            [RefusalKind::Unresolved, RefusalKind::NamesNoDependency],
        ),
    ];
    for (package, kinds) in expected {
        let refusals = undetermined(&report, package)?;
        let found: Vec<_> = refusals.iter().map(Refusal::kind).collect();
        assert_eq!(found, kinds, "{package}: {refusals:?}");
        assert_eq!(wearings_of(&refusals[0])[0].2, Some(package), "{package}");
        assert_eq!(wearings_of(&refusals[1])[0].2, None, "{package}");
    }
    let unrelated = undetermined(&report, "nobody-wears-this")?;
    let found: Vec<_> = unrelated.iter().map(Refusal::kind).collect();
    assert_eq!(found, [RefusalKind::NamesNoDependency], "{unrelated:?}");
    assert_eq!(report.refusals().len(), 3, "{:?}", report.refusals());
    Ok(())
}

#[test]
fn a_non_table_under_a_declared_key_names_that_dependencys_package() -> TestOutcome {
    // Given `plain = "text"` in the `skeletons` table beside a dependency
    // `plain` that is the package `plain-crate`, the refusal is `NotATable` for
    // the key `plain`, and its wearing names the package behind the key rather
    // than the key itself. The lookup for that package is `Undetermined`, and
    // for any other it is `NotWorn`.
    let fixture = Fixture::new()?;
    let plain = fixture.write_crate("plain-crate", "0.1.0", "")?;
    fixture.write_member(
        "tools",
        &path_dependency("plain", "plain-crate", &plain),
        "[package.metadata.skeletons]\nplain = \"text\"\n",
    )?;
    fixture.generate_lockfile()?;

    let report = skeletons_library::check(fixture.project())?;

    let refusals = undetermined(&report, "plain-crate")?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::NotATable);
    assert_eq!(
        wearings_of(&refusals[0]),
        [("tools/Cargo.toml", Some("plain"), Some("plain-crate"))]
    );
    assert!(
        matches!(report.skeletons_named("plain"), SkeletonsNamed::NotWorn),
        "{:?}",
        report.skeletons_named("plain")
    );
    Ok(())
}
