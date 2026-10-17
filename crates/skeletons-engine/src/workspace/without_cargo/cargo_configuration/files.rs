//! Which configuration files Cargo reads for a directory, in the order it reads them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::workspace::without_cargo::file_system::{
    CARGO_DIRECTORY_NAME, CONFIGURATION_FILE_NAME, LEGACY_CONFIGURATION_FILE_NAME,
};

/// Every configuration file Cargo reads when it starts in `start`, nearest first.
///
/// For `start` and each of its ancestors up to the root of the file system, Cargo looks in
/// `.cargo` there, and then in the Cargo home `cargo_home`. In each directory it takes the file
/// `config` if there is one, and otherwise `config.toml`: when both exist it uses the first and
/// ignores the second altogether, which is a thing a reader that merged them would get wrong.
/// The home is left out when it is one of the `.cargo` directories the walk has been through,
/// since its files are read where that directory is. A directory below `start`, or beside it, is
/// never looked in, so a workspace member's own configuration is read only when the member is
/// `start`.
///
/// The walk has no ceiling short of the root of the file system: Cargo does not stop at a
/// workspace root, a manifest, a repository or the user's home.
///
/// `start` is a real directory, as `std::fs::canonicalize` gives one.
pub(super) fn configuration_files(start: &Path, cargo_home: Option<&Path>) -> Vec<PathBuf> {
    assert!(
        start.is_absolute(),
        "the start is the canonical form of a directory, which is absolute"
    );
    let mut files = Vec::new();
    let mut walked = Vec::with_capacity(start.ancestors().count());
    for directory in start.ancestors() {
        let cargo_directory = directory.join(CARGO_DIRECTORY_NAME);
        files.extend(configuration_file_in(&cargo_directory));
        walked.push(cargo_directory);
    }
    if let Some(home) = cargo_home {
        if !is_walked(home, &walked) {
            files.extend(configuration_file_in(home));
        }
    }

    assert!(
        files.iter().all(|file| is_configuration_file_name(file)),
        "only configuration files are listed"
    );
    let distinct: BTreeSet<&PathBuf> = files.iter().collect();
    assert_eq!(
        distinct.len(),
        files.len(),
        "the home is left out when the walk reached it, so no file is listed twice"
    );
    files
}

/// The configuration file Cargo takes in `directory`, if there is one: `config` when it exists,
/// and otherwise `config.toml`.
///
/// A path that cannot be looked at, because the directory above it cannot be entered, does not
/// exist as far as this goes, which is Cargo's own `Path::exists` and so the same answer.
fn configuration_file_in(directory: &Path) -> Option<PathBuf> {
    let legacy = directory.join(LEGACY_CONFIGURATION_FILE_NAME);
    if legacy.exists() {
        return Some(legacy);
    }
    let current = directory.join(CONFIGURATION_FILE_NAME);
    current.exists().then_some(current)
}

/// Whether the Cargo home `home` is one of the `.cargo` directories the walk went through,
/// spelled as it is or as the file system resolves it.
fn is_walked(home: &Path, walked: &[PathBuf]) -> bool {
    // A home that cannot be resolved does not exist, so it holds no file and is read as itself.
    let resolved = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    walked
        .iter()
        .any(|directory| *directory == resolved || directory == home)
}

/// Whether `file` is named as one of Cargo's two configuration files.
fn is_configuration_file_name(file: &Path) -> bool {
    file.file_name().is_some_and(|name| {
        name == LEGACY_CONFIGURATION_FILE_NAME || name == CONFIGURATION_FILE_NAME
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::configuration_files;
    use crate::workspace::without_cargo::fixture::Fixture;

    /// The files `start` walks to with `cargo_home`, less any the machine has in the
    /// directories above the fixture's temporary directory, which are no business of a test.
    fn walk(fixture: &Fixture, start: &Path, cargo_home: Option<&Path>) -> Vec<PathBuf> {
        configuration_files(start, cargo_home)
            .into_iter()
            .filter(|file| file.starts_with(&fixture.top))
            .collect()
    }

    #[test]
    fn the_start_then_each_ancestor_then_the_home_are_listed_nearest_first() {
        // A configuration file at the start, one at its parent and one at the temporary
        // directory above that, and one in the Cargo home: the walk gives them in that order,
        // the home last because it has the lowest precedence.
        let fixture = Fixture::new();
        let start = fixture.write("app/.cargo/config.toml", "");
        let parent = fixture.write(".cargo/config.toml", "");
        let top = fixture.write("../.cargo/config.toml", "");
        let home = fixture.write("../cargo-home/config.toml", "");

        let files = walk(
            &fixture,
            &fixture.root.join("app"),
            Some(&fixture.top.join("cargo-home")),
        );

        assert_eq!(files, [start, parent, top, home]);
    }

    #[test]
    fn a_directory_below_the_start_or_beside_it_is_never_looked_in() {
        // Cargo reads no configuration of a workspace member when it starts at the root, and
        // none of a sibling member or a directory under the start.
        let fixture = Fixture::new();
        let root = fixture.write(".cargo/config.toml", "");
        let app = fixture.write("app/.cargo/config.toml", "");
        fixture.write("other/.cargo/config.toml", "");
        fixture.write("app/src/.cargo/config.toml", "");

        assert_eq!(
            walk(&fixture, &fixture.root, None),
            std::slice::from_ref(&root)
        );
        assert_eq!(walk(&fixture, &fixture.root.join("app"), None), [app, root]);
    }

    #[test]
    fn the_file_without_the_extension_hides_the_one_with_it_beside_it() {
        // Where `config` and `config.toml` both exist Cargo uses `config` alone, in a `.cargo`
        // directory and in the Cargo home, and takes `config.toml` where it is the only one.
        let fixture = Fixture::new();
        let legacy = fixture.write(".cargo/config", "");
        fixture.write(".cargo/config.toml", "");
        let plain = fixture.write("../.cargo/config.toml", "");
        let home_legacy = fixture.write("../cargo-home/config", "");
        fixture.write("../cargo-home/config.toml", "");

        let files = walk(
            &fixture,
            &fixture.root,
            Some(&fixture.top.join("cargo-home")),
        );

        assert_eq!(files, [legacy, plain, home_legacy]);
    }

    #[test]
    fn a_directory_with_no_configuration_file_adds_nothing() {
        // A `.cargo` with no `config` in it, and a Cargo home that is not there at all.
        let fixture = Fixture::new();
        fixture.directory(".cargo");
        fixture.write(".cargo/credentials.toml", "");

        assert_eq!(
            walk(
                &fixture,
                &fixture.root,
                Some(&fixture.top.join("no-such-home"))
            ),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn a_home_that_is_an_ancestors_cargo_directory_is_listed_once_in_its_place() {
        // The Cargo home is the `.cargo` of the temporary directory above the start, which the
        // walk reaches anyway. Its file is read there, at that directory's precedence, and the
        // home does not add it again, however the home is spelled.
        let fixture = Fixture::new();
        let root = fixture.write(".cargo/config.toml", "");
        let top = fixture.write("../.cargo/config.toml", "");
        let spelled_with_a_parent = fixture.top.join("w/../.cargo");

        assert_eq!(
            walk(&fixture, &fixture.root, Some(&fixture.top.join(".cargo"))),
            [root.clone(), top.clone()]
        );
        assert_eq!(
            walk(&fixture, &fixture.root, Some(&spelled_with_a_parent)),
            [root, top]
        );
    }

    #[test]
    fn a_home_that_is_a_cargo_directory_the_walk_never_reaches_is_read_last() {
        // The home is another directory's `.cargo` entirely: one below the start, which the walk
        // does not enter, so it is listed after the walk.
        let fixture = Fixture::new();
        let root = fixture.write(".cargo/config.toml", "");
        let home = fixture.write("app/.cargo/config.toml", "");

        let files = walk(
            &fixture,
            &fixture.root,
            Some(&fixture.root.join("app/.cargo")),
        );

        assert_eq!(files, [root, home]);
    }
}
