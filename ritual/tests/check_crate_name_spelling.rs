//! Acceptance: a dependency is found however its crate name and its key are
//! spelt.
//!
//! Cargo takes `-` and `_` as one in a crate's extern name, so a skeleton
//! packaged as `a_skeleton` or as `a-skeleton` is the same crate to `rustc`,
//! and a dependency key may be spelt with either. `check` and `sync` must
//! find the dependency a wearing table names, and read the skeleton as worn,
//! whichever spelling the package and the key use.
//!
//! Cargo itself refuses a dependency, from a path, git or registry source,
//! whose crate name is spelt differently from its package's, so the
//! spellings that can differ inside one manifest are a package name against
//! the extern name `rustc` derives from it, and a rename key against that
//! same derived name. Each scenario below writes a skeleton under the package
//! name it is about, wears it under the key it is about, writes the one
//! claimed file matching its render (or, for `sync`, leaves it missing), and
//! reads the result back through the built command line.

mod support;

use support::{
    Fixture, TemporaryDirectory, path_dependency_on, wearing_table, write_minimal_skeleton,
    write_package_manifest,
};

const CLAIMED_FILE: &str = "x.yml";
const CLAIMED_RENDER: &str = "hello: world\n";

/// A `path = …` dependency under `key` on `path`, declaring `package` as the
/// package it names.
fn renamed_path_dependency(key: &str, package: &str, path: &std::path::Path) -> String {
    path_dependency_on(key, path).replace(" }\n", &format!(", package = \"{package}\" }}\n"))
}

/// A skeleton crate named `package_name`, claiming [`CLAIMED_FILE`], which
/// renders as `render`.
fn skeleton_rendering(
    package_name: &str,
    render: &str,
) -> Result<TemporaryDirectory, Box<dyn std::error::Error>> {
    let directory = TemporaryDirectory::new("crate-name-spelling-skeleton")?;
    write_minimal_skeleton(directory.path(), package_name, CLAIMED_FILE, render)?;
    Ok(directory)
}

/// As [`skeleton_rendering`], at `version`: two skeletons of one package name
/// can share a lockfile only at different versions.
fn skeleton_rendering_at(
    package_name: &str,
    version: &str,
    render: &str,
) -> Result<TemporaryDirectory, Box<dyn std::error::Error>> {
    let directory = skeleton_rendering(package_name, render)?;
    let manifest = String::from_utf8(support::read(directory.path(), "Cargo.toml")?)?;
    let versioned = manifest.replace("version = \"0.0.0\"", &format!("version = \"{version}\""));
    support::write(directory.path(), "Cargo.toml", versioned.as_bytes())?;
    Ok(directory)
}

/// A skeleton crate named `package_name`, claiming [`CLAIMED_FILE`].
fn skeleton_named(package_name: &str) -> Result<TemporaryDirectory, Box<dyn std::error::Error>> {
    skeleton_rendering(package_name, CLAIMED_RENDER)
}

/// A workspace whose one package depends on `dependency_lines` and wears
/// the dependency keys in `worn_keys`.
fn fixture_with(
    dependency_lines: &str,
    worn_keys: &[&str],
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut extra = format!("[dependencies]\n{dependency_lines}\n");
    for key in worn_keys {
        extra.push_str(&wearing_table(key, ""));
        extra.push('\n');
    }
    write_package_manifest(fixture.root(), "", "wearer", &extra)?;
    fixture.generate_lockfile()?;
    Ok(fixture)
}

/// Runs `check --json` against `fixture` (claimed file already matching its
/// render) and asserts the one dependency `expected_key` is read as worn,
/// with the one claimed file matching.
fn assert_worn_and_matching(
    fixture: &Fixture,
    expected_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_worn_and_matching_render(fixture, expected_key, CLAIMED_RENDER)
}

/// As [`assert_worn_and_matching`], with the claimed file written as `render`.
fn assert_worn_and_matching_render(
    fixture: &Fixture,
    expected_key: &str,
    render: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    fixture.write(CLAIMED_FILE, render.as_bytes())?;
    let report = fixture.run(&["skeletons", "check", "--json"])?;
    assert_eq!(
        report.exit_code, 0,
        "`{expected_key}` must be located and read as worn; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    let document = support::json::parse(&report.stdout)?;
    let refusals = support::json::refusals(&document)?;
    assert!(
        refusals.is_empty(),
        "nothing about `{expected_key}` may be refused; refusals were: {refusals:?}"
    );
    let bones = support::json::bones(&document)?;
    assert_eq!(
        bones.len(),
        1,
        "expected one bone row; bones were: {bones:?}"
    );
    assert_eq!(support::json::bone_dependency(&bones[0])?, expected_key);
    assert_eq!(support::json::bone_path(&bones[0])?, CLAIMED_FILE);
    assert_eq!(support::json::bone_drift_state(&bones[0])?, "matches");
    Ok(())
}

#[test]
fn a_skeleton_packaged_with_an_underscore_is_located_under_its_own_key() -> support::TestOutcome {
    // Package `a_skeleton`, depended on and worn as `a_skeleton`: the extern
    // name and the package name are spelt the same.
    let skeleton = skeleton_named("a_skeleton")?;
    let fixture = fixture_with(
        &path_dependency_on("a_skeleton", skeleton.path()),
        &["a_skeleton"],
    )?;
    assert_worn_and_matching(&fixture, "a_skeleton")?;
    Ok(())
}

#[test]
fn a_skeleton_packaged_with_a_hyphen_is_located_under_its_own_key() -> support::TestOutcome {
    // Package `a-skeleton`, depended on and worn as `a-skeleton`: Cargo
    // names the edge `a_skeleton`, so the package and the edge are spelt
    // differently.
    let skeleton = skeleton_named("a-skeleton")?;
    let fixture = fixture_with(
        &path_dependency_on("a-skeleton", skeleton.path()),
        &["a-skeleton"],
    )?;
    assert_worn_and_matching(&fixture, "a-skeleton")?;
    Ok(())
}

#[test]
fn a_hyphenated_rename_key_over_an_underscored_package_is_located() -> support::TestOutcome {
    // `my-key = { package = "a_skeleton" }`: the key is spelt with `-`, the
    // package with `_`, and Cargo names the edge `my_key`.
    let skeleton = skeleton_named("a_skeleton")?;
    let fixture = fixture_with(
        &renamed_path_dependency("my-key", "a_skeleton", skeleton.path()),
        &["my-key"],
    )?;
    assert_worn_and_matching(&fixture, "my-key")?;
    Ok(())
}

#[test]
fn an_underscored_rename_key_over_a_hyphenated_package_is_located() -> support::TestOutcome {
    // `my_key = { package = "a-skeleton" }`: the reverse of the case above.
    let skeleton = skeleton_named("a-skeleton")?;
    let fixture = fixture_with(
        &renamed_path_dependency("my_key", "a-skeleton", skeleton.path()),
        &["my_key"],
    )?;
    assert_worn_and_matching(&fixture, "my_key")?;
    Ok(())
}

#[test]
fn a_hyphenated_rename_key_over_a_hyphenated_package_is_located() -> support::TestOutcome {
    // `my-key = { package = "a-skeleton" }`: both spelt with `-`, both
    // differing from the edge Cargo names `my_key`.
    let skeleton = skeleton_named("a-skeleton")?;
    let fixture = fixture_with(
        &renamed_path_dependency("my-key", "a-skeleton", skeleton.path()),
        &["my-key"],
    )?;
    assert_worn_and_matching(&fixture, "my-key")?;
    Ok(())
}

#[test]
fn a_hyphenated_rename_is_found_beside_an_unrenamed_dependency_of_the_same_name()
-> support::TestOutcome {
    // Two different skeleton crates share the package name `a-skeleton` at
    // two versions,
    // one depended on unrenamed and one as `alias-key`; they render the
    // claimed file differently. Only `alias-key` is worn, so the file
    // matching its render (not the unrenamed one's) proves `check` resolved
    // it through its own edge, `alias_key`, and not through `a_skeleton`.
    let unrenamed = skeleton_rendering_at("a-skeleton", "1.0.0", "unrenamed: one\n")?;
    let renamed = skeleton_rendering_at("a-skeleton", "2.0.0", "renamed: two\n")?;
    let lines = format!(
        "{}{}",
        path_dependency_on("a-skeleton", unrenamed.path()),
        renamed_path_dependency("alias-key", "a-skeleton", renamed.path()),
    );
    let fixture = fixture_with(&lines, &["alias-key"])?;
    assert_worn_and_matching_render(&fixture, "alias-key", "renamed: two\n")?;
    Ok(())
}

#[test]
fn an_unrenamed_dependency_is_found_beside_a_hyphenated_rename_of_the_same_name()
-> support::TestOutcome {
    // The same pair, but only the unrenamed `a-skeleton` is worn: it must
    // resolve to its own skeleton, never to the renamed sibling's.
    let unrenamed = skeleton_rendering_at("a-skeleton", "1.0.0", "unrenamed: one\n")?;
    let renamed = skeleton_rendering_at("a-skeleton", "2.0.0", "renamed: two\n")?;
    let lines = format!(
        "{}{}",
        path_dependency_on("a-skeleton", unrenamed.path()),
        renamed_path_dependency("alias-key", "a-skeleton", renamed.path()),
    );
    let fixture = fixture_with(&lines, &["a-skeleton"])?;
    assert_worn_and_matching_render(&fixture, "a-skeleton", "unrenamed: one\n")?;
    Ok(())
}

#[test]
fn sync_writes_the_file_of_a_hyphenated_rename_key_over_an_underscored_package()
-> support::TestOutcome {
    // `sync` goes through the same lookup as `check`: for
    // `my-key = { package = "a_skeleton" }` it must find the skeleton and
    // create the missing claimed file, matching its render.
    let skeleton = skeleton_named("a_skeleton")?;
    let fixture = fixture_with(
        &renamed_path_dependency("my-key", "a_skeleton", skeleton.path()),
        &["my-key"],
    )?;
    fixture.init_git_repository()?;

    let report = fixture.run(&["skeletons", "sync"])?;

    assert_eq!(
        report.exit_code, 0,
        "sync must find `my-key`; stdout was: {}, stderr was: {}",
        report.stdout, report.stderr
    );
    assert_eq!(fixture.read(CLAIMED_FILE)?, CLAIMED_RENDER.as_bytes());
    Ok(())
}
