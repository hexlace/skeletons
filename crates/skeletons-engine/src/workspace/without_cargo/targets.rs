//! What a package's files make of its manifest's targets: whether it has a library, what that
//! library is called, and whether it has any target at all.

use std::path::Path;

use super::package_manifest::Targets;
use super::text_error::TextError;

mod inferred;
mod library;
mod written;

/// The name of the library a package has, or `None` when it has no library.
///
/// The name is `[lib] name` when written and the package's name with its hyphens as underscores
/// otherwise: the name `cargo metadata` gives a resolve edge to the package, and `rustc` the crate.
///
/// # Errors
///
/// Unreadable for every manifest Cargo refuses for its targets: a library name that is empty or
/// holds a hyphen, a `[lib]` table that finds no file, a binary, example, test or bench table that
/// is nameless, empty-named, duplicated or (for a binary) fileless, and a package with no target
/// at all. `directory` is where the package's manifest is.
pub(super) fn library_name(
    directory: &Path,
    package_name: &str,
    targets: &Targets,
) -> Result<Option<String>, TextError> {
    let library = library::resolve(directory, package_name, targets)?;
    let others = written::count(directory, package_name, targets, library.is_some())?;
    if library.is_none() && others == 0 {
        return Err(TextError::unreadable(
            "the package has no target: it needs a [lib], a [[bin]], or a source file Cargo \
             discovers as a target",
        ));
    }
    if let Some(name) = &library {
        assert!(
            !name.contains('-'),
            "a library name never holds a hyphen: {name}"
        );
        assert!(
            !name.trim().is_empty(),
            "a library name is never blank: it was checked before its path"
        );
    }
    Ok(library)
}

#[cfg(test)]
pub(super) mod matrix;

#[cfg(test)]
mod tests {
    use super::library_name;
    use crate::workspace::without_cargo::package_manifest::PackageManifest;

    /// Writes `files` (path, text) under a new directory, then reads the library of the package
    /// that `manifest` describes there. `Ok(None)` is a package with no library.
    pub(super) fn read(manifest: &str, files: &[&str]) -> Result<Option<String>, String> {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for file in files {
            let path = directory.path().join(file);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("a directory");
            std::fs::write(path, "").expect("a file");
        }
        let manifest = PackageManifest::parse(manifest, directory.path(), None)
            .expect("the manifest in this test is well formed");
        let package = manifest.package().expect("a package");
        library_name(directory.path(), &package.name, manifest.targets())
            .map_err(|error| error.detail().to_owned())
    }

    const PLAIN: &str = "[package]\nname = \"hy-phen\"\n";

    #[test]
    fn a_library_file_makes_a_library_named_for_the_package_with_underscores() {
        assert_eq!(read(PLAIN, &["src/lib.rs"]), Ok(Some("hy_phen".to_owned())));
    }

    #[test]
    fn lib_name_overrides_the_package_name() {
        let manifest = "[package]\nname = \"lib-a\"\n[lib]\nname = \"zed\"\n";

        assert_eq!(read(manifest, &["src/lib.rs"]), Ok(Some("zed".to_owned())));
    }

    #[test]
    fn lib_path_alone_names_the_library_for_the_package() {
        let manifest = "[package]\nname = \"custom-path\"\n[lib]\npath = \"src/other.rs\"\n";

        assert_eq!(
            read(manifest, &["src/other.rs"]),
            Ok(Some("custom_path".to_owned()))
        );
    }

    #[test]
    fn a_binary_only_package_has_no_library() {
        assert_eq!(read(PLAIN, &["src/main.rs"]), Ok(None));
    }

    #[test]
    fn autolib_false_turns_off_discovering_the_library() {
        let manifest = "[package]\nname = \"a\"\nautolib = false\n";

        assert_eq!(read(manifest, &["src/lib.rs", "src/main.rs"]), Ok(None));
    }

    #[test]
    fn a_package_whose_only_targets_are_tests_examples_or_bins_has_no_library() {
        assert_eq!(read(PLAIN, &["tests/t.rs"]), Ok(None));
        assert_eq!(read(PLAIN, &["examples/e.rs"]), Ok(None));
        assert_eq!(read(PLAIN, &["src/bin/x.rs"]), Ok(None));
        assert_eq!(read(PLAIN, &["src/bin/x/main.rs"]), Ok(None));
        assert_eq!(read(PLAIN, &["benches/b.rs"]), Ok(None));
        assert_eq!(read(PLAIN, &["benches/b/main.rs"]), Ok(None));
    }

    #[test]
    fn a_written_bin_table_is_a_target_without_a_file_to_discover() {
        let manifest = "[package]\nname = \"a\"\n[[bin]]\nname = \"a\"\npath = \"x.rs\"\n";

        assert_eq!(read(manifest, &[]), Ok(None));
    }

    #[test]
    fn a_package_with_files_and_a_manifest_only_has_no_target() {
        let refused = Err(
            "the package has no target: it needs a [lib], a [[bin]], or a source \
                            file Cargo discovers as a target"
                .to_owned(),
        );

        assert_eq!(read(PLAIN, &["readme.md"]), refused);
        assert_eq!(read(PLAIN, &[]), refused);
    }

    #[test]
    fn a_build_script_alone_is_no_target() {
        assert!(read(PLAIN, &["build.rs"]).is_err());
    }

    #[test]
    fn a_main_file_is_no_target_when_autobins_is_false() {
        let manifest = "[package]\nname = \"a\"\nautobins = false\n";

        assert!(read(manifest, &["src/main.rs"]).is_err());
    }

    #[test]
    fn a_target_directory_is_no_target_when_its_auto_key_is_false() {
        for (key, file) in [
            ("autotests", "tests/t.rs"),
            ("autoexamples", "examples/e.rs"),
            ("autobenches", "benches/b.rs"),
        ] {
            let manifest = format!("[package]\nname = \"a\"\n{key} = false\n");

            assert!(read(&manifest, &[file]).is_err(), "{key} = false");
        }
    }

    #[test]
    fn a_lib_table_with_no_file_is_refused() {
        let manifest = "[package]\nname = \"a\"\n[lib]\nname = \"zz\"\n";

        assert_eq!(
            read(manifest, &["src/main.rs"]),
            Err(
                "can't find library `zz`, rename file to `src/lib.rs` or specify lib.path"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_library_is_found_beside_other_targets() {
        assert_eq!(
            read(PLAIN, &["src/lib.rs", "src/main.rs", "tests/t.rs"]),
            Ok(Some("hy_phen".to_owned()))
        );
    }
}
