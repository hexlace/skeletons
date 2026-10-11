//! Reading the workspace back after `remove` has edited the manifest and
//! brought `Cargo.lock` current, to confirm the wearing is gone.
//!
//! The question is put to the reader `check` and `sync` use, and answered
//! twice. The wearing reader must no longer find a table at the key, worn or
//! refused; and Cargo must no longer report a dependency declared under the
//! key anywhere in the manifest, which is how a dependency `remove` does not
//! edit would show. Nothing the edit did is kept unless both hold.

use super::names::{ManifestShown, WearingKey};
use super::refusal::RemoveRefusal;
use super::resolve::candidates_in;
use crate::workspace::{Declared, Workspace};

/// Confirms that `workspace` and `declared`, read after the edit, show no
/// wearing under `key` in the manifest shown as `manifest` and no dependency
/// declared under it.
///
/// `declared` is every dependency that manifest's package declares, as the
/// read back reports them.
///
/// # Errors
///
/// Returns [`RemoveRefusal::StillWorn`] when the table at the key is still
/// there, and [`RemoveRefusal::DependencyRemains`] when it is gone and a
/// dependency under the key is not.
pub(crate) fn confirm(
    workspace: &Workspace,
    declared: &[Declared],
    manifest: &ManifestShown,
    key: &WearingKey,
) -> Result<(), RemoveRefusal> {
    let table_remains = candidates_in(workspace, manifest, declared)
        .iter()
        .any(|candidate| candidate.key() == key);
    if table_remains {
        return Err(RemoveRefusal::StillWorn {
            manifest: manifest.clone(),
            key: key.clone(),
        });
    }
    if declared.iter().any(|entry| entry.key == key.as_str()) {
        return Err(RemoveRefusal::DependencyRemains {
            manifest: manifest.clone(),
            key: key.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::confirm;
    use crate::remove::names::{ManifestShown, WearingKey};
    use crate::remove::refusal::RemoveRefusal;
    use crate::skeleton::Choices;
    use crate::workspace::{Declared, Pin, Wearing, WearingRefusal, Workspace, WornDependency};

    const MANIFEST: &str = "ritual/Cargo.toml";

    fn manifest() -> ManifestShown {
        ManifestShown::new(MANIFEST)
    }

    fn key(text: &str) -> WearingKey {
        WearingKey::new(text)
    }

    fn worn(manifest: &str, key: &str) -> Wearing {
        Wearing::Worn(WornDependency {
            manifest: manifest.to_owned(),
            key: key.to_owned(),
            package: "a-skeleton".to_owned(),
            version: semver::Version::new(0, 1, 0),
            pin: Pin::from_source(None, Path::new("/skeleton")),
            skeleton_directory: PathBuf::from("/skeleton"),
            choices: Ok(Choices::default()),
        })
    }

    fn dangling(manifest: &str, key: &str) -> Wearing {
        Wearing::Refused(WearingRefusal::NamesNoDependency {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
        })
    }

    fn workspace_of(wearing: Vec<Wearing>) -> Workspace {
        Workspace {
            root: PathBuf::from("/workspace"),
            wearing,
        }
    }

    fn declared(key: &str) -> Declared {
        Declared {
            key: key.to_owned(),
            crate_name: "a-skeleton".to_owned(),
        }
    }

    #[test]
    fn a_workspace_with_neither_the_table_nor_the_dependency_is_confirmed() {
        // Other wearings, other keys and a dependency under another key are
        // all there, and none of them is the key: the control for the
        // refusals below, which differ from this by exactly one thing.
        let workspace = workspace_of(vec![worn(MANIFEST, "lint"), worn("tools/Cargo.toml", "ci")]);

        assert_eq!(
            confirm(&workspace, &[declared("lint")], &manifest(), &key("ci")),
            Ok(())
        );
    }

    #[test]
    fn a_worn_table_at_the_key_is_still_worn() {
        let workspace = workspace_of(vec![worn(MANIFEST, "ci")]);

        assert_eq!(
            confirm(&workspace, &[], &manifest(), &key("ci")),
            Err(RemoveRefusal::StillWorn {
                manifest: manifest(),
                key: key("ci"),
            })
        );
    }

    #[test]
    fn a_table_at_the_key_that_check_refuses_is_still_worn() {
        // The reader reports the table whether or not it makes a worn
        // skeleton of it.
        let workspace = workspace_of(vec![dangling(MANIFEST, "ci")]);

        assert_eq!(
            confirm(&workspace, &[], &manifest(), &key("ci")),
            Err(RemoveRefusal::StillWorn {
                manifest: manifest(),
                key: key("ci"),
            })
        );
    }

    #[test]
    fn a_dependency_under_the_key_with_no_table_remains() {
        let workspace = workspace_of(vec![worn(MANIFEST, "lint")]);

        assert_eq!(
            confirm(
                &workspace,
                &[declared("ci"), declared("lint")],
                &manifest(),
                &key("ci")
            ),
            Err(RemoveRefusal::DependencyRemains {
                manifest: manifest(),
                key: key("ci"),
            })
        );
    }

    #[test]
    fn a_table_and_a_dependency_still_there_are_reported_as_the_table() {
        // The table is the first thing `remove` took out, so it is the first
        // thing it says is still there.
        let workspace = workspace_of(vec![worn(MANIFEST, "ci")]);

        assert_eq!(
            confirm(&workspace, &[declared("ci")], &manifest(), &key("ci")),
            Err(RemoveRefusal::StillWorn {
                manifest: manifest(),
                key: key("ci"),
            })
        );
    }

    #[test]
    fn the_same_key_in_another_manifest_is_not_the_wearing() {
        let workspace = workspace_of(vec![
            worn("tools/Cargo.toml", "ci"),
            dangling("tools/Cargo.toml", "ci-next"),
        ]);

        assert_eq!(confirm(&workspace, &[], &manifest(), &key("ci")), Ok(()));
    }

    #[test]
    fn the_empty_key_is_a_key_like_any_other_to_confirm() {
        // A table under the empty key is still there until it is gone, and a
        // table under another key is not it.
        let still_there = workspace_of(vec![dangling(MANIFEST, "")]);
        let other = workspace_of(vec![dangling(MANIFEST, "ci")]);

        assert_eq!(
            confirm(&still_there, &[], &manifest(), &key("")),
            Err(RemoveRefusal::StillWorn {
                manifest: manifest(),
                key: key(""),
            })
        );
        assert_eq!(confirm(&other, &[], &manifest(), &key("")), Ok(()));
    }

    #[test]
    fn a_key_is_compared_exactly_and_not_as_a_prefix() {
        let workspace = workspace_of(vec![worn(MANIFEST, "ci-next")]);

        assert_eq!(
            confirm(&workspace, &[declared("ci-next")], &manifest(), &key("ci")),
            Ok(())
        );
    }
}
