//! Reading a skeleton's `Cargo.toml`: its `package.name`, and the raw
//! `[package.metadata.skeletons]` table [`super::declarations`] parses further.

use std::path::Path;

use super::error::{Reason, RenderError, SkeletonIdentity, one_based_line};
use super::limits::ByteBudget;

/// The file name Cargo reads a package's manifest from: a skeleton's own, and
/// the name nothing under `files/` or `partials/` may have.
pub(crate) const MANIFEST_NAME: &str = "Cargo.toml";

/// A skeleton's manifest, read as far as `render` needs it: its name, and the
/// metadata table that makes it a skeleton at all.
#[derive(Debug)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    pub(crate) metadata: toml::Table,
}

/// Reads and parses `skeleton_directory/Cargo.toml`.
///
/// Refuses, in order, a `Cargo.toml` that cannot be read (including one that
/// is a symbolic link, or not a regular file at all), that is not valid
/// TOML, that has no string `package.name`, that has an empty
/// `package.name`, and one without a `[package.metadata.skeletons]` table — the
/// last of which is what makes a crate a skeleton at all, so nothing before it
/// can yet name the skeleton by name.
pub(crate) fn read(
    skeleton_directory: &Path,
    byte_budget: &mut ByteBudget,
) -> Result<Manifest, RenderError> {
    let unnamed = SkeletonIdentity::Directory(skeleton_directory.to_path_buf());
    let path = skeleton_directory.join(MANIFEST_NAME);

    let text = super::limits::read_utf8(&path, byte_budget)
        .map_err(|reason| RenderError::about_file(unnamed.clone(), MANIFEST_NAME, reason))?;

    let table: toml::Table = text
        .parse()
        .map_err(|error: toml::de::Error| not_toml(&unnamed, &text, &error))?;

    let package = table.get("package").and_then(toml::Value::as_table);
    let name = package
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            RenderError::about_file(unnamed.clone(), MANIFEST_NAME, Reason::PackageNameMissing)
        })?
        .to_owned();
    if name.is_empty() {
        return Err(RenderError::about_file(
            unnamed,
            MANIFEST_NAME,
            Reason::PackageNameEmpty,
        ));
    }

    let named = SkeletonIdentity::Named(name.clone());
    let metadata = table
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("metadata"))
        .and_then(toml::Value::as_table)
        .and_then(|metadata| metadata.get("skeletons"))
        .and_then(toml::Value::as_table)
        .cloned()
        .ok_or_else(|| {
            RenderError::about_file(named.clone(), MANIFEST_NAME, Reason::NotASkeleton)
        })?;

    // Postcondition: a `Manifest` always carries a non-empty name — the
    // string we just extracted from `package.name` and nothing else.
    assert!(
        !name.is_empty(),
        "package.name was checked to be a string, but an empty one is still a name"
    );
    Ok(Manifest { name, metadata })
}

/// The refusal for a `Cargo.toml` that does not parse: the parser's own
/// one-line message, at the 1-based line its span starts on, or at the file
/// alone when the parser names no span. The parser's `Display` is a multi-line
/// snippet with a caret, which no one-line report can hold.
fn not_toml(unnamed: &SkeletonIdentity, text: &str, error: &toml::de::Error) -> RenderError {
    let reason = Reason::ManifestNotToml {
        message: error.message().to_owned(),
    };
    let line = error
        .span()
        .and_then(|span| text.get(..span.start))
        .map(|before| one_based_line(before.bytes().filter(|byte| *byte == b'\n').count()));
    match line {
        Some(line) => RenderError::about_line(unnamed.clone(), MANIFEST_NAME, line, reason),
        None => RenderError::about_file(unnamed.clone(), MANIFEST_NAME, reason),
    }
}

#[cfg(test)]
mod tests {
    use super::read;
    use crate::skeleton::error::Reason;
    use crate::skeleton::limits::ByteBudget;

    /// A scratch directory holding just a `Cargo.toml` with `text` as its
    /// content, for the manifest-reading edge cases no committed test skeleton
    /// carries (each committed skeleton is a real, complete crate; these
    /// fixtures are deliberately incomplete, which is the point).
    fn manifest_only(text: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "skeletons-manifest-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        std::fs::write(directory.join("Cargo.toml"), text).expect("write Cargo.toml");
        directory
    }

    #[test]
    fn a_manifest_with_no_skeletons_table_is_not_a_skeleton() {
        let directory = manifest_only("[package]\nname = \"plain\"\n");
        let mut budget = ByteBudget::new();
        let error = read(&directory, &mut budget)
            .expect_err("a crate with no `skeletons` table is not a skeleton");
        assert!(matches!(error.reason(), Reason::NotASkeleton));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_manifest_with_no_package_name_is_refused() {
        let directory = manifest_only("[package]\nversion = \"0.0.0\"\n");
        let mut budget = ByteBudget::new();
        let error =
            read(&directory, &mut budget).expect_err("a missing package.name must be refused");
        assert!(matches!(error.reason(), Reason::PackageNameMissing));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_manifest_with_an_empty_package_name_is_refused() {
        let directory = manifest_only("[package]\nname = \"\"\n\n[package.metadata.skeletons]\n");
        let mut budget = ByteBudget::new();
        let error =
            read(&directory, &mut budget).expect_err("an empty package.name must be refused");
        assert!(matches!(error.reason(), Reason::PackageNameEmpty));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_symlinked_manifest_is_refused_naming_cargo_toml() {
        let directory = manifest_only("[package]\nname = \"plain\"\n");
        let real = directory.join("Cargo.toml");
        let renamed = directory.join("real-cargo.toml");
        std::fs::rename(&real, &renamed).expect("move the manifest aside");
        std::os::unix::fs::symlink(&renamed, &real).expect("symlink Cargo.toml to it");

        let mut budget = ByteBudget::new();
        let error =
            read(&directory, &mut budget).expect_err("a symlinked Cargo.toml must be refused");
        assert_eq!(error.file(), Some("Cargo.toml"));
        assert!(matches!(error.reason(), Reason::SymbolicLink));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_refused() {
        let directory = manifest_only("this is not { toml at all");
        let mut budget = ByteBudget::new();
        let error = read(&directory, &mut budget).expect_err("invalid TOML must be refused");
        assert!(matches!(error.reason(), Reason::ManifestNotToml { .. }));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_refused_at_its_line_on_one_line() {
        // Line 7 is the malformed one; the refusal must name it, and its
        // message must hold no line break, which the parser's own multi-line
        // snippet would.
        let directory = manifest_only(concat!(
            "[package]\nname = \"plain\"\nversion = \"0.0.0\"\n\n",
            "[package.metadata.skeletons]\n\nbroken = = 1\n",
        ));
        let mut budget = ByteBudget::new();
        let error = read(&directory, &mut budget).expect_err("invalid TOML must be refused");
        assert_eq!(error.file(), Some("Cargo.toml"));
        assert_eq!(error.line(), Some(7));
        let Reason::ManifestNotToml { message } = error.reason() else {
            panic!("expected ManifestNotToml, got {:?}", error.reason());
        };
        assert!(
            !message.contains('\n'),
            "message must be one line: {message:?}"
        );
        assert!(
            !error.to_string().contains('\n'),
            "display must be one line: {error}"
        );
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_missing_manifest_is_refused_as_unreadable() {
        let directory = manifest_only("[package]\nname = \"plain\"\n");
        std::fs::remove_file(directory.join("Cargo.toml")).expect("remove the manifest");
        let mut budget = ByteBudget::new();
        let error = read(&directory, &mut budget).expect_err("a missing manifest must be refused");
        assert!(matches!(error.reason(), Reason::Unreadable { .. }));
        std::fs::remove_dir_all(&directory).expect("clean up");
    }

    #[test]
    fn a_well_formed_skeleton_manifest_is_read() {
        let directory =
            manifest_only("[package]\nname = \"dependabot\"\n\n[package.metadata.skeletons]\n");
        let mut budget = ByteBudget::new();
        let manifest = read(&directory, &mut budget).expect("a well-formed manifest must be read");
        assert_eq!(manifest.name, "dependabot");
        assert!(manifest.metadata.is_empty());
        std::fs::remove_dir_all(&directory).expect("clean up");
    }
}
