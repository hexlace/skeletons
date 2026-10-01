//! The fixtures that wear the checked-in `passthrough-plain` test skeleton,
//! which several acceptance files start from: one file, no options, whose
//! `files/plain.yml` holds `name: unchanged\nvalue: 42\n` verbatim.

use std::error::Error;

use super::{Fixture, path_dependency_on_test_skeleton, wearing_table, write_package_manifest};

/// The bytes `passthrough-plain`'s only file, `files/plain.yml`, renders as:
/// copied through unchanged, since the skeleton has no placeholders and no
/// directives.
pub(crate) const PASSTHROUGH_PLAIN_RENDER: &[u8] = b"name: unchanged\nvalue: 42\n";

/// Builds a fixture wearing `passthrough-plain` under the dependency key
/// `passthrough-plain`, with no recorded options (the skeleton declares
/// none), its manifest and lockfile written but `plain.yml` not yet created.
///
/// Because `plain.yml` is absent it reads `drifted (missing)`, so `sync`
/// always has something to write.
pub(crate) fn fixture_wearing_passthrough_plain() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let extra = format!(
        "[dependencies]\n{}\n{}",
        path_dependency_on_test_skeleton("passthrough-plain", "passthrough-plain"),
        wearing_table("passthrough-plain", ""),
    );
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

/// The same fixture as [`fixture_wearing_passthrough_plain`], plus a real
/// `git init` and one commit of everything present so far (the manifest and
/// the lockfile; `plain.yml` is still absent), giving a scenario a clean
/// baseline to introduce exactly one kind of dirt on top of.
pub(crate) fn fixture_wearing_passthrough_plain_with_clean_baseline()
-> Result<Fixture, Box<dyn Error>> {
    let fixture = fixture_wearing_passthrough_plain()?;
    fixture.init_git_repository()?;
    Ok(fixture)
}
