//! A package's library: its name, checked as Cargo checks it, and the path Cargo finds for it.

use std::path::Path;

use super::super::package_manifest::{Edition, TargetKind, Targets};
use super::super::text_error::TextError;
use crate::workspace::crate_name::underscored;

/// Where Cargo looks for a library when `[lib]` writes no `path`.
const DEFAULT_LIBRARY_PATH: &str = "src/lib.rs";

/// The name of the library the package in `directory` has, or `None` when it has no library.
///
/// The name is `[lib] name` when written and the package's name with its hyphens as underscores
/// otherwise, and it is checked before the library's path is looked for. A library with a written
/// `path` is a library whether or not its file exists. With no `path` it is `src/lib.rs`, and in
/// the 2015 edition alone `src/<name>.rs` too.
///
/// # Errors
///
/// Unreadable for a name Cargo refuses, and for a `[lib]` table that finds no file.
pub(super) fn resolve(
    directory: &Path,
    package_name: &str,
    targets: &Targets,
) -> Result<Option<String>, TextError> {
    let library = find(directory, package_name, targets)?;
    if let Some(name) = &library {
        // Every name `find` returns went through `checked` first, so a manifest cannot make
        // either of these fire.
        assert!(
            !name.trim().is_empty(),
            "a library name is never blank: it was checked before its path"
        );
        assert!(
            !name.contains('-'),
            "a library name never holds a hyphen: {name}"
        );
    }
    Ok(library)
}

/// The library `resolve` describes, before its name is asserted on.
fn find(
    directory: &Path,
    package_name: &str,
    targets: &Targets,
) -> Result<Option<String>, TextError> {
    let default_path_exists = directory.join(DEFAULT_LIBRARY_PATH).is_file();
    let Some(table) = &targets.library else {
        // With no `[lib]`, Cargo discovers the library from `src/lib.rs` unless `autolib` is off.
        if !targets.discovers(TargetKind::Library) || !default_path_exists {
            return Ok(None);
        }
        let name = underscored(package_name);
        checked(&name)?;
        return Ok(Some(name));
    };

    let name = table
        .name
        .clone()
        .unwrap_or_else(|| underscored(package_name));
    checked(&name)?;
    let has_file = table.path.is_some()
        || default_path_exists
        || (targets.edition() == Edition::Edition2015
            && directory.join("src").join(format!("{name}.rs")).exists());
    if has_file {
        Ok(Some(name))
    } else {
        Err(TextError::unreadable(format!(
            "can't find library `{name}`, rename file to `src/lib.rs` or specify lib.path"
        )))
    }
}

/// `name`, when Cargo takes it as the name of a library: not blank, and with no hyphen.
fn checked(name: &str) -> Result<(), TextError> {
    if name.trim().is_empty() {
        return Err(TextError::unreadable(
            "library target names cannot be empty",
        ));
    }
    if name.contains('-') {
        return Err(TextError::unreadable(format!(
            "library target names cannot contain hyphens: {name}"
        )));
    }
    Ok(())
}
