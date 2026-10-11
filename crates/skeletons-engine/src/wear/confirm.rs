//! Reading the workspace back after `wear` has written, to confirm that what
//! it wrote is what `check` and `sync` will read as a worn skeleton.
//!
//! The question is put to the same reader they use, which finds the
//! dependency declared under the key, locates the skeleton it resolves to and
//! applies the test for a skeleton, for a registry, a git or a path source
//! alike. Nothing `wear` wrote is kept unless that reader agrees.

use super::refusal::WearRefusal;
use super::request::Key;
use crate::survey::Refusal;
use crate::workspace::{Wearing, WearingRefusal, Workspace, WornDependency, same_crate};

/// The skeleton that is now worn, as the success message names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Added {
    pub(crate) package: String,
    pub(crate) version: semver::Version,
    pub(crate) manifest: String,
    pub(crate) key: String,
}

/// Finds the wearing `(manifest, key)` in `workspace`, which was read after
/// `wear` wrote it, and says whether it is a worn skeleton.
///
/// A worn skeleton that another entry also wears under its package name is
/// refused: Cargo may have written the dependency under a spelling of the
/// name that the typed one did not match, which no check before the write
/// could see, and one skeleton worn twice would claim every file twice.
///
/// # Errors
///
/// Returns [`WearRefusal`] when the crate is not a skeleton, when another
/// entry already wears the same skeleton, when the reader refused the wearing
/// for any other reason, or when it did not report the wearing at all.
pub(crate) fn confirm(
    workspace: &Workspace,
    manifest: &str,
    key: &Key,
) -> Result<Added, WearRefusal> {
    let Some(wearing) = workspace
        .wearing
        .iter()
        .find(|wearing| is_at(wearing, manifest, key.as_str()))
    else {
        return Err(WearRefusal::NotReported {
            manifest: manifest.to_owned(),
            key: key.as_str().to_owned(),
        });
    };
    match wearing {
        Wearing::Worn(worn) => worn_elsewhere(workspace, worn).map_or_else(
            || Ok(added(worn)),
            |other| {
                Err(WearRefusal::AlreadyWorn {
                    package: other.package.clone(),
                    key: other.key.clone(),
                    manifest: other.manifest.clone(),
                })
            },
        ),
        Wearing::Refused(WearingRefusal::NotASkeleton {
            package: (package, version),
            ..
        }) => Err(WearRefusal::NotASkeleton {
            package: package.clone(),
            version: version.clone(),
        }),
        Wearing::Refused(refusal) => Err(WearRefusal::NotConfirmed {
            manifest: manifest.to_owned(),
            key: key.as_str().to_owned(),
            reason: Refusal::Wearing(refusal.clone()).message(),
        }),
    }
}

fn added(worn: &WornDependency) -> Added {
    Added {
        package: worn.package.clone(),
        version: worn.version.clone(),
        manifest: worn.manifest.clone(),
        key: worn.key.clone(),
    }
}

/// Another worn dependency, at a different manifest or key, of the same
/// package as `worn`.
fn worn_elsewhere<'workspace>(
    workspace: &'workspace Workspace,
    worn: &WornDependency,
) -> Option<&'workspace WornDependency> {
    workspace
        .wearing
        .iter()
        .filter_map(|wearing| match wearing {
            Wearing::Worn(other) => Some(other),
            Wearing::Refused(_) => None,
        })
        .filter(|other| same_crate(&other.package, &worn.package))
        .find(|other| other.id() != worn.id())
}

/// Whether `wearing` is the outcome for the table at `(manifest, key)`: a
/// worn skeleton or a refusal about that key, or a refusal about the whole of
/// the manifest's `[package.metadata.skeletons]`, which stands in the way of
/// every key in it.
fn is_at(wearing: &Wearing, manifest: &str, key: &str) -> bool {
    let (at_manifest, at_key) = match wearing {
        Wearing::Worn(worn) => (worn.manifest.as_str(), Some(worn.key.as_str())),
        Wearing::Refused(refusal) => refusal_location(refusal),
    };
    match (at_manifest == manifest, at_key) {
        (true, Some(at_key)) => at_key == key,
        (true, None) => true,
        (false, _) => false,
    }
}

/// The manifest a wearing refusal is about, and the key it is about, which is
/// `None` for the refusal about the whole table.
fn refusal_location(refusal: &WearingRefusal) -> (&str, Option<&str>) {
    match refusal {
        WearingRefusal::NotATable { manifest, key } => (manifest, key.as_deref()),
        WearingRefusal::ReservedKey { manifest, key, .. } => (manifest, Some(key)),
        WearingRefusal::NamesNoDependency {
            manifest,
            dependency,
        }
        | WearingRefusal::Unresolved {
            manifest,
            dependency,
        }
        | WearingRefusal::Ambiguous {
            manifest,
            dependency,
            ..
        }
        | WearingRefusal::NotASkeleton {
            manifest,
            dependency,
            ..
        } => (manifest, Some(dependency)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Added, confirm};
    use crate::wear::refusal::WearRefusal;
    use crate::wear::request::Key;
    use crate::wear::test_workspace::{not_a_skeleton, workspace_of, worn};
    use crate::workspace::{Wearing, WearingRefusal};

    fn key(text: &str) -> Key {
        Key::new(text).expect("a test passes only keys it knows are valid")
    }

    #[test]
    fn a_worn_skeleton_under_the_key_is_confirmed_with_what_it_is() {
        // The arm that keeps what `wear` wrote: the reader found the
        // dependency, located its skeleton and accepted it.
        let workspace = workspace_of(vec![worn("Cargo.toml", "tidy", "tidy")]);

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("tidy")),
            Ok(Added {
                package: "tidy".to_owned(),
                version: semver::Version::new(1, 2, 3),
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            })
        );
    }

    #[test]
    fn the_wearing_is_found_among_others_by_manifest_and_key() {
        // Another key in the same manifest, and the same key in another
        // manifest, are different wearings and must not be mistaken for it.
        let workspace = workspace_of(vec![
            worn("Cargo.toml", "other", "other"),
            worn("tools/Cargo.toml", "tidy", "elsewhere"),
            worn("Cargo.toml", "tidy", "tidy"),
        ]);

        let added = confirm(&workspace, "Cargo.toml", &key("tidy")).expect("it is worn");

        assert_eq!(added.package, "tidy");
    }

    #[test]
    fn a_skeleton_another_entry_already_wears_is_refused_naming_that_entry() {
        // Cargo writing `foo_bar` as `foo-bar` is how a package can be worn
        // twice without the typed name matching a prior entry. The refusal
        // names the entry that was there first, not the one just written.
        let workspace = workspace_of(vec![
            worn("Cargo.toml", "tidy", "tidy"),
            worn("Cargo.toml", "neat", "tidy"),
        ]);

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("neat")),
            Err(WearRefusal::AlreadyWorn {
                package: "tidy".to_owned(),
                key: "tidy".to_owned(),
                manifest: "Cargo.toml".to_owned(),
            })
        );
    }

    #[test]
    fn a_skeleton_another_entry_wears_under_the_other_spelling_is_refused_too() {
        // The two entries carry the package as `foo-bar` and `foo_bar`, which
        // crates.io holds as one crate, so the later one wears what the
        // earlier already does.
        for (earlier, later) in [("foo-bar", "foo_bar"), ("foo_bar", "foo-bar")] {
            let workspace = workspace_of(vec![
                worn("Cargo.toml", "tidy", earlier),
                worn("Cargo.toml", "neat", later),
            ]);

            assert_eq!(
                confirm(&workspace, "Cargo.toml", &key("neat")),
                Err(WearRefusal::AlreadyWorn {
                    package: earlier.to_owned(),
                    key: "tidy".to_owned(),
                    manifest: "Cargo.toml".to_owned(),
                }),
                "earlier {earlier}, later {later}"
            );
        }
    }

    #[test]
    fn a_crate_that_is_not_a_skeleton_is_refused_as_such() {
        let workspace = workspace_of(vec![not_a_skeleton("Cargo.toml", "plain", "plain-crate")]);

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("plain")),
            Err(WearRefusal::NotASkeleton {
                package: "plain-crate".to_owned(),
                version: "0.1.0".to_owned(),
            })
        );
    }

    #[test]
    fn any_other_refusal_carries_the_readers_own_message() {
        let unresolved = WearingRefusal::Unresolved {
            manifest: "Cargo.toml".to_owned(),
            dependency: "tidy".to_owned(),
        };
        let workspace = workspace_of(vec![Wearing::Refused(unresolved)]);

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("tidy")),
            Err(WearRefusal::NotConfirmed {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
                reason: "tidy in Cargo.toml is declared, but cargo resolved no package for it, \
                         so there is no locked skeleton to compare against"
                    .to_owned(),
            })
        );
    }

    #[test]
    fn a_refusal_about_the_whole_skeletons_table_stands_in_the_way_of_the_key() {
        // The reader could not read `[package.metadata.skeletons]` at all, so
        // it reports one refusal for the manifest and none for any key in it.
        let refusal = WearingRefusal::NotATable {
            manifest: "Cargo.toml".to_owned(),
            key: None,
        };
        let workspace = workspace_of(vec![Wearing::Refused(refusal)]);

        let Err(WearRefusal::NotConfirmed { key, .. }) =
            confirm(&workspace, "Cargo.toml", &key("tidy"))
        else {
            panic!("the whole-table refusal must be reported for the key");
        };

        assert_eq!(key, "tidy");
    }

    #[test]
    fn a_refusal_about_another_manifest_or_key_is_not_this_wearings() {
        let elsewhere = WearingRefusal::Unresolved {
            manifest: "tools/Cargo.toml".to_owned(),
            dependency: "tidy".to_owned(),
        };
        let other_key = WearingRefusal::Unresolved {
            manifest: "Cargo.toml".to_owned(),
            dependency: "other".to_owned(),
        };
        let workspace = workspace_of(vec![
            Wearing::Refused(elsewhere),
            Wearing::Refused(other_key),
        ]);

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("tidy")),
            Err(WearRefusal::NotReported {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            })
        );
    }

    #[test]
    fn a_workspace_that_does_not_report_the_wearing_at_all_is_a_defect() {
        let workspace = workspace_of(Vec::new());

        assert_eq!(
            confirm(&workspace, "Cargo.toml", &key("tidy")),
            Err(WearRefusal::NotReported {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            })
        );
    }
}
