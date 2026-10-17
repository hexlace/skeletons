//! A dependency declared with `registry =`, read without Cargo. Only Cargo's
//! configuration says which index a registry name stands for, so a wearing
//! whose own dependency is declared that way is refused by name, and a
//! dependency nothing wears is read as the lockfile has it.

mod support;

use skeletons_library::{PackageLocations, check_without_cargo};
use support::{Fixture, TestOutcome, wearing_table, worn_once};

/// A lockfile for the workspace the tests write: the member `tools`, depending
/// on the packages named in `tools_dependencies` (a quoted, comma-separated
/// list), then the lock `entries` of those packages.
fn lockfile(tools_dependencies: &str, entries: &str) -> String {
    format!(
        "version = 4\n\n\
         [[package]]\nname = \"tools\"\nversion = \"0.1.0\"\n\
         dependencies = [{tools_dependencies}]\n\n{entries}"
    )
}

/// The lock entry for `name` at `version`, taken from the registry at
/// `https://registry.example.test/index`.
fn registry_entry(name: &str, version: &str) -> String {
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
         source = \"registry+https://registry.example.test/index\"\n\
         checksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n\n"
    )
}

#[test]
fn a_worn_dependency_declared_with_a_registry_name_is_refused_by_name() -> TestOutcome {
    // Given a member that wears `rules`, a dependency declared with
    // `registry = "internal"`, and a lockfile that holds a package for it from
    // some registry, the read is refused as unsupported: the detail names the
    // manifest, the dependency key and the registry name, because the lock
    // entry could name another index than the one the name stands for.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "rules = { package = \"house-rules\", version = \"4\", registry = \"internal\" }\n",
        &wearing_table("rules"),
    )?;
    fixture.write(
        "Cargo.lock",
        lockfile("\"house-rules\"", &registry_entry("house-rules", "4.5.6")).as_bytes(),
    )?;

    let Err(error) = check_without_cargo(fixture.project(), &PackageLocations::new()) else {
        return Err("expected a refusal, got a report".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("tools/Cargo.toml: "), "{detail}");
    assert!(
        detail.ends_with(
            "the worn dependency `rules` is declared with `registry = \"internal\"`, and only \
             cargo's configuration says which index that names"
        ),
        "{detail}"
    );
    Ok(())
}

#[test]
fn a_dependency_nothing_wears_declared_with_a_registry_name_is_read_as_locked() -> TestOutcome {
    // Given a member that wears `rules` by path and also declares `plain` with
    // `registry = "internal"`, which no wearing table names, the report is the
    // one for the path skeleton: `plain` is read as the lockfile has it, so it
    // neither refuses the read nor appears in the report.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        &format!(
            "{}plain = {{ version = \"1\", registry = \"internal\" }}\n",
            fixture.rules_dependency()
        ),
        &wearing_table("rules"),
    )?;
    let entries = format!(
        "[[package]]\nname = \"house-rules\"\nversion = \"4.5.6\"\n\n{}",
        registry_entry("plain", "1.0.0")
    );
    fixture.write(
        "Cargo.lock",
        lockfile("\"house-rules\", \"plain\"", &entries).as_bytes(),
    )?;

    let report = check_without_cargo(fixture.project(), &PackageLocations::new())?;

    assert!(report.refusals().is_empty(), "{:?}", report.refusals());
    let skeleton = worn_once(&report, "house-rules")?;
    assert_eq!(skeleton.dependency_key(), "rules");
    assert_eq!(skeleton.version().to_string(), "4.5.6");
    Ok(())
}
