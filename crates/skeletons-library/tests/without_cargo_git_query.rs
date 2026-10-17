//! A git URL with a query of its own, read without Cargo. Cargo keeps the query in the
//! lockfile's source and reads it back as the same one, and this reader compares a source's
//! query only as one `branch`, `tag` or `rev`, so it refuses the shape by name: in a manifest,
//! and in a lockfile, whichever is read.

mod support;

use skeletons_library::{PackageLocations, check_without_cargo};
use support::{Fixture, TestOutcome};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A lockfile of the member `tools` depending on `plain`, a git package whose `source` is as
/// given.
fn lockfile_of_tools_and_plain(source: &str) -> String {
    format!(
        "version = 4\n\n\
         [[package]]\nname = \"plain\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n\n\
         [[package]]\nname = \"tools\"\nversion = \"0.1.0\"\ndependencies = [\"plain\"]\n"
    )
}

#[test]
fn a_git_url_with_a_query_in_a_manifest_is_unsupported_and_names_the_url() -> TestOutcome {
    // Given a member that depends on a git URL written with `?x=1`, the read is refused as
    // unsupported and not as a lockfile out of date: the detail names the manifest, the table,
    // the dependency key and the URL.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "plain = { git = \"https://registry.example.test/plain?x=1\" }\n",
        "",
    )?;
    fixture.write(
        "Cargo.lock",
        lockfile_of_tools_and_plain(&format!(
            "git+https://registry.example.test/plain?x=1#{COMMIT}"
        ))
        .as_bytes(),
    )?;

    let Err(error) = check_without_cargo(fixture.project(), &PackageLocations::new()) else {
        return Err("expected a refusal, got a report".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert!(!error.is_lockfile_not_current(), "{error:?}");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("tools/Cargo.toml: "), "{detail}");
    assert!(
        detail.ends_with(
            "[dependencies] `plain`: `git = \"https://registry.example.test/plain?x=1\"` has a \
             query of its own, which Cargo keeps in the lockfile's source and this reader does \
             not follow"
        ),
        "{detail}"
    );
    Ok(())
}

#[test]
fn a_git_source_with_a_query_in_a_lockfile_is_unsupported_and_names_the_package() -> TestOutcome {
    // Given a manifest that writes the URL plain and a lockfile whose source for it carries
    // `?x=1`, the read is refused as unsupported and not as a lockfile out of date: the detail
    // names the lockfile, the package, its version and the source.
    let fixture = Fixture::new()?;
    fixture.write_member(
        "tools",
        "plain = { git = \"https://registry.example.test/plain\" }\n",
        "",
    )?;
    let source = format!("git+https://registry.example.test/plain?x=1#{COMMIT}");
    fixture.write(
        "Cargo.lock",
        lockfile_of_tools_and_plain(&source).as_bytes(),
    )?;

    let Err(error) = check_without_cargo(fixture.project(), &PackageLocations::new()) else {
        return Err("expected a refusal, got a report".into());
    };

    assert!(error.is_unsupported_without_cargo(), "{error:?}");
    assert!(!error.is_lockfile_not_current(), "{error:?}");
    let detail = error.detail().ok_or("no detail")?;
    assert!(detail.contains("Cargo.lock: "), "{detail}");
    assert!(
        detail.ends_with(&format!(
            "package `plain` 1.0.0 is locked from `{source}`, whose query is not one `branch`, \
             `tag` or `rev`; Cargo keeps a query of the URL's own in the source, which this \
             reader does not follow"
        )),
        "{detail}"
    );
    Ok(())
}
