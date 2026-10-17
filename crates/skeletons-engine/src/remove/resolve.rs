//! Which wearing the name the wearer typed means.
//!
//! The name is a dependency key or a skeleton's crate. The candidates are every
//! key under `[package.metadata.skeletons]` in the command line's own manifest,
//! whether its dependency resolved to a skeleton or the workspace reader
//! refuses the wearing: a table with a dependency that is no skeleton, or with
//! none, is something `wear` could have half written and a person wants gone,
//! so calling its key "not a key" would be false. The two keys a skeleton keeps
//! for its own declaration are never candidates.

use super::names::{ManifestShown, WearingKey};
use super::non_empty::NonEmpty;
use crate::workspace::{Declared, UNLOCATED_NEVER_MET, Wearing, WearingRefusal, Workspace};

/// What the workspace reader made of a wearing, as far as the success message
/// needs it: whether its dependency resolved to a skeleton it can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Standing {
    /// The dependency resolved to a skeleton: this crate at this locked
    /// version. Whether anything else about the wearing is refused, its files
    /// included, does not change that.
    Worn {
        crate_name: String,
        version: semver::Version,
    },
    /// The workspace reader refuses the wearing, so there is no skeleton at a
    /// locked version to name, and a dependency on this crate is declared under
    /// the key.
    RefusedWithDependency { crate_name: String },
    /// The workspace reader refuses the wearing, so there is no skeleton to
    /// name, and no dependency is declared under the key.
    RefusedWithoutDependency,
}

/// One wearing the name might mean: the key it sits under and whether its
/// dependency resolved to a skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    key: WearingKey,
    standing: Standing,
}

impl Candidate {
    /// The wearing under `key`, whose dependency resolved as `standing` says.
    ///
    /// Nothing about the key is checked: it is read from the wearer's own
    /// manifest, where any key a table can have, the empty one included, names
    /// a wearing a person may want gone. What the constructor cannot check is
    /// that `standing` is true of the key; [`candidates_in`] is what reads it
    /// from the workspace, and the one place the command makes a candidate.
    pub(crate) const fn new(key: WearingKey, standing: Standing) -> Self {
        Self { key, standing }
    }

    /// The key the wearing's table sits under.
    pub(crate) const fn key(&self) -> &WearingKey {
        &self.key
    }

    /// Whether the wearing's dependency resolved to a skeleton, and to which.
    pub(crate) const fn standing(&self) -> &Standing {
        &self.standing
    }

    /// The crate the key's dependency is on, when one is declared.
    pub(crate) fn crate_name(&self) -> Option<&str> {
        match &self.standing {
            Standing::Worn { crate_name, .. } | Standing::RefusedWithDependency { crate_name } => {
                Some(crate_name)
            }
            Standing::RefusedWithoutDependency => None,
        }
    }
}

/// Why no one wearing can be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PickRefusal {
    /// The name is neither a candidate's key nor a candidate's crate. `keys`
    /// are the candidates' keys, in key order.
    Neither { keys: Vec<WearingKey> },
    /// The name is the crate of several candidates and the key of none.
    /// `keys` are theirs, in key order.
    SeveralKeys { keys: NonEmpty<WearingKey> },
}

/// A wearing in a manifest other than the command line's own.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Elsewhere {
    /// The manifest, relative to the workspace root.
    pub(crate) manifest: String,
    /// The key the wearing's table sits under there.
    pub(crate) key: String,
}

/// Every candidate in the manifest shown as `manifest`, in key order.
///
/// `declared` is every dependency that manifest declares, which is how a
/// wearing the workspace reader refuses is told to have a dependency or not,
/// and which crate it is on.
pub(crate) fn candidates_in(
    workspace: &Workspace,
    manifest: &ManifestShown,
    declared: &[Declared],
) -> Vec<Candidate> {
    let dependency_of = |key: &str| declared.iter().find(|entry| entry.key == key);
    let mut candidates: Vec<Candidate> = workspace
        .wearing
        .iter()
        .filter_map(|wearing| match wearing {
            Wearing::Worn(worn) if worn.manifest == manifest.as_str() => Some(Candidate::new(
                WearingKey::new(&worn.key),
                Standing::Worn {
                    crate_name: worn.package.clone(),
                    version: worn.version.clone(),
                },
            )),
            Wearing::Worn(_) => None,
            Wearing::Refused(refusal) => {
                let (refused_manifest, key) = refused_key(refusal)?;
                if refused_manifest != manifest.as_str() {
                    return None;
                }
                let standing =
                    dependency_of(key).map_or(Standing::RefusedWithoutDependency, |entry| {
                        Standing::RefusedWithDependency {
                            crate_name: entry.crate_name.clone(),
                        }
                    });
                Some(Candidate::new(WearingKey::new(key), standing))
            }
        })
        .collect();
    candidates.sort_by(|left, right| left.key.cmp(&right.key));
    assert!(
        candidates.windows(2).all(|pair| pair[0].key <= pair[1].key),
        "`pick` names keys in key order, so the candidates are sorted by key"
    );
    candidates
}

/// Picks the one wearing `name` means from `candidates`.
///
/// A key equal to the name wins over a crate of the same name: without a way
/// to say which is meant, refusing the name as ambiguous would make that key
/// impossible to remove, while the other wearing is always reachable by its own
/// key. Otherwise the name is a crate, compared exactly: one candidate on that
/// crate is the wearing, several are refused, none is refused.
///
/// # Errors
///
/// Returns [`PickRefusal`] when the name is neither a key nor the crate of
/// exactly one candidate.
pub(crate) fn pick<'candidates>(
    name: &str,
    candidates: &'candidates [Candidate],
) -> Result<&'candidates Candidate, PickRefusal> {
    if let Some(keyed) = candidates
        .iter()
        .find(|candidate| candidate.key.as_str() == name)
    {
        return Ok(keyed);
    }
    let on_the_crate: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.crate_name() == Some(name))
        .collect();
    match on_the_crate.as_slice() {
        [only] => Ok(only),
        [] => Err(PickRefusal::Neither {
            keys: candidates
                .iter()
                .map(|candidate| candidate.key.clone())
                .collect(),
        }),
        [first, rest @ ..] => Err(PickRefusal::SeveralKeys {
            keys: NonEmpty::new(
                first.key.clone(),
                rest.iter().map(|candidate| candidate.key.clone()).collect(),
            ),
        }),
    }
}

/// Every wearing outside the manifest shown as `manifest` whose key is `name`
/// or whose crate is, in manifest then key order.
///
/// A wearing's crate is known only where `check` reads it from the lock: a
/// worn skeleton's, and a dependency that is no skeleton. A wearing `check`
/// refuses for another reason is found by its key alone.
pub(crate) fn worn_elsewhere(
    workspace: &Workspace,
    manifest: &ManifestShown,
    name: &str,
) -> Vec<Elsewhere> {
    let mut found: Vec<Elsewhere> = workspace
        .wearing
        .iter()
        .filter_map(|wearing| {
            let (wearing_manifest, key, crate_name) = match wearing {
                Wearing::Worn(worn) => (&*worn.manifest, &*worn.key, Some(&*worn.package)),
                Wearing::Refused(refusal) => {
                    let (wearing_manifest, key) = refused_key(refusal)?;
                    (wearing_manifest, key, refused_crate(refusal))
                }
            };
            let means_it = key == name || crate_name == Some(name);
            (wearing_manifest != manifest.as_str() && means_it).then(|| Elsewhere {
                manifest: wearing_manifest.to_owned(),
                key: key.to_owned(),
            })
        })
        .collect();
    found.sort();
    assert!(
        found.windows(2).all(|pair| pair[0] <= pair[1]),
        "the refusal groups a manifest's wearings by looking at neighbours, so they are sorted"
    );
    found
}

/// The manifest and key of a refused wearing, or `None` for a refusal that is
/// about no key a person could name: the whole table not being a table, and a
/// key a skeleton keeps for its own declaration.
fn refused_key(refusal: &WearingRefusal) -> Option<(&str, &str)> {
    match refusal {
        WearingRefusal::NotATable { manifest, key, .. } => {
            key.as_deref().map(|key| (manifest.as_str(), key))
        }
        WearingRefusal::ReservedKey { .. } => None,
        WearingRefusal::NamesNoDependency {
            manifest,
            dependency,
        }
        | WearingRefusal::Unresolved {
            manifest,
            dependency,
            ..
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
        } => Some((manifest.as_str(), dependency.as_str())),
        WearingRefusal::Unlocated { .. } => unreachable!("{UNLOCATED_NEVER_MET}"),
    }
}

/// The crate a refused wearing's dependency is on, where the refusal says.
fn refused_crate(refusal: &WearingRefusal) -> Option<&str> {
    match refusal {
        WearingRefusal::NotASkeleton { package, .. } => Some(package.0.as_str()),
        WearingRefusal::Unresolved { package, .. } => Some(package.as_str()),
        WearingRefusal::NotATable { package, .. } => package.as_deref(),
        WearingRefusal::ReservedKey { .. }
        | WearingRefusal::NamesNoDependency { .. }
        | WearingRefusal::Ambiguous { .. } => None,
        WearingRefusal::Unlocated { .. } => unreachable!("{UNLOCATED_NEVER_MET}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Candidate, Elsewhere, PickRefusal, Standing, candidates_in, pick, worn_elsewhere};
    use crate::remove::names::{ManifestShown, WearingKey};
    use crate::remove::non_empty::NonEmpty;
    use crate::skeleton::Choices;
    use crate::workspace::{Declared, Pin, Wearing, WearingRefusal, Workspace, WornDependency};

    fn key(text: &str) -> WearingKey {
        WearingKey::new(text)
    }

    fn keys_of(texts: &[&str]) -> Vec<WearingKey> {
        texts.iter().map(|text| key(text)).collect()
    }

    fn several_keys(first: &str, rest: &[&str]) -> NonEmpty<WearingKey> {
        NonEmpty::new(key(first), keys_of(rest))
    }

    fn manifest(text: &str) -> ManifestShown {
        ManifestShown::new(text)
    }

    fn worn(name: &str, crate_name: &str) -> Candidate {
        Candidate::new(
            key(name),
            Standing::Worn {
                crate_name: crate_name.to_owned(),
                version: semver::Version::new(0, 1, 0),
            },
        )
    }

    fn dangling(name: &str) -> Candidate {
        Candidate::new(key(name), Standing::RefusedWithoutDependency)
    }

    fn keys(picked: Result<&Candidate, PickRefusal>) -> Result<&str, PickRefusal> {
        picked.map(|candidate| candidate.key().as_str())
    }

    #[test]
    fn a_key_picks_its_own_wearing() {
        let candidates = [worn("ci", "a-skeleton"), worn("lint", "b-skeleton")];

        assert_eq!(keys(pick("lint", &candidates)), Ok("lint"));
    }

    #[test]
    fn a_crate_picks_the_one_key_that_wears_it() {
        let candidates = [worn("ci", "a-skeleton"), worn("lint", "b-skeleton")];

        assert_eq!(keys(pick("b-skeleton", &candidates)), Ok("lint"));
    }

    #[test]
    fn a_key_wins_over_a_crate_of_the_same_name() {
        // `a-skeleton` is a key, and also the crate that `other` wears: the
        // key is meant, and `other` stays reachable by its own key.
        let candidates = [
            worn("a-skeleton", "b-skeleton"),
            worn("other", "a-skeleton"),
        ];

        assert_eq!(keys(pick("a-skeleton", &candidates)), Ok("a-skeleton"));
        assert_eq!(keys(pick("other", &candidates)), Ok("other"));
    }

    #[test]
    fn a_crate_is_compared_exactly_and_not_with_hyphens_and_underscores_folded() {
        let candidates = [worn("ci", "a-skeleton")];

        assert_eq!(
            pick("a_skeleton", &candidates),
            Err(PickRefusal::Neither {
                keys: keys_of(&["ci"])
            })
        );
    }

    #[test]
    fn a_wearing_with_no_dependency_is_still_a_key() {
        let candidates = [dangling("ci")];

        assert_eq!(keys(pick("ci", &candidates)), Ok("ci"));
    }

    #[test]
    fn a_crate_several_keys_wear_is_refused_naming_each_in_key_order() {
        let candidates = [
            worn("ci", "a-skeleton"),
            worn("ci-next", "a-skeleton"),
            worn("lint", "b-skeleton"),
        ];

        assert_eq!(
            pick("a-skeleton", &candidates),
            Err(PickRefusal::SeveralKeys {
                keys: several_keys("ci", &["ci-next"])
            })
        );
    }

    #[test]
    fn a_name_that_is_neither_is_refused_with_the_keys_there_are() {
        let candidates = [worn("ci", "a-skeleton"), dangling("lint")];

        assert_eq!(
            pick("nope", &candidates),
            Err(PickRefusal::Neither {
                keys: keys_of(&["ci", "lint"])
            })
        );
    }

    #[test]
    fn a_name_is_refused_plainly_when_there_are_no_wearings() {
        assert_eq!(
            pick("nope", &[]),
            Err(PickRefusal::Neither { keys: Vec::new() })
        );
    }

    #[test]
    fn a_dependency_less_wearing_has_no_crate_for_a_name_to_mean() {
        assert_eq!(dangling("ci").crate_name(), None);
        assert_eq!(worn("ci", "a-skeleton").crate_name(), Some("a-skeleton"));
        assert_eq!(
            Candidate::new(
                key("ci"),
                Standing::RefusedWithDependency {
                    crate_name: "not-a-skeleton".to_owned()
                }
            )
            .crate_name(),
            Some("not-a-skeleton")
        );
    }

    fn worn_dependency(manifest: &str, key: &str, package: &str) -> WornDependency {
        WornDependency {
            manifest: manifest.to_owned(),
            key: key.to_owned(),
            package: package.to_owned(),
            version: semver::Version::new(0, 1, 0),
            pin: Pin::from_source(None, std::path::Path::new("/skeleton")),
            skeleton_directory: PathBuf::from("/skeleton"),
            choices: Ok(Choices::default()),
        }
    }

    fn refused(refusal: WearingRefusal) -> Wearing {
        Wearing::Refused(refusal)
    }

    fn declared(key: &str, crate_name: &str) -> Declared {
        Declared {
            key: key.to_owned(),
            crate_name: crate_name.to_owned(),
        }
    }

    /// A workspace of three members' wearings: `ritual` wears `lint` and has
    /// `ci` refused with a dependency, `dangling` with none and `bad` as no
    /// table, and a key a skeleton keeps; `tools` wears `ci`.
    fn workspace() -> Workspace {
        let manifest = "ritual/Cargo.toml".to_owned();
        Workspace {
            root: PathBuf::from("/workspace"),
            wearing: vec![
                refused(WearingRefusal::NotATable {
                    manifest: "tools/Cargo.toml".to_owned(),
                    key: None,
                    package: None,
                }),
                Wearing::Worn(worn_dependency("ritual/Cargo.toml", "lint", "b-skeleton")),
                refused(WearingRefusal::Unresolved {
                    manifest: manifest.clone(),
                    dependency: "ci".to_owned(),
                    package: "a-skeleton".to_owned(),
                }),
                refused(WearingRefusal::NamesNoDependency {
                    manifest: manifest.clone(),
                    dependency: "dangling".to_owned(),
                }),
                refused(WearingRefusal::NotATable {
                    manifest: manifest.clone(),
                    key: Some("bad".to_owned()),
                    package: None,
                }),
                refused(WearingRefusal::ReservedKey {
                    manifest,
                    key: "options".to_owned(),
                    crate_name: "x".to_owned(),
                }),
                Wearing::Worn(worn_dependency("tools/Cargo.toml", "ci", "a-skeleton")),
                refused(WearingRefusal::NotASkeleton {
                    manifest: "tools/Cargo.toml".to_owned(),
                    dependency: "plain".to_owned(),
                    package: ("plain-crate".to_owned(), "1.0.0".to_owned()),
                }),
                refused(WearingRefusal::Unresolved {
                    manifest: "tools/Cargo.toml".to_owned(),
                    dependency: "moved".to_owned(),
                    package: "moved-crate".to_owned(),
                }),
            ],
        }
    }

    #[test]
    fn the_candidates_are_every_key_of_the_manifest_but_the_reserved_ones_in_key_order() {
        // `ci` is refused with a dependency the manifest declares, `dangling`
        // names none, `bad` is not a table, `lint` is worn; `options` is a
        // skeleton's own declaration, and `tools` is another manifest.
        let declared = [declared("ci", "a-skeleton"), declared("lint", "b-skeleton")];

        assert_eq!(
            candidates_in(&workspace(), &manifest("ritual/Cargo.toml"), &declared),
            [
                Candidate::new(key("bad"), Standing::RefusedWithoutDependency),
                Candidate::new(
                    key("ci"),
                    Standing::RefusedWithDependency {
                        crate_name: "a-skeleton".to_owned()
                    }
                ),
                dangling("dangling"),
                worn("lint", "b-skeleton"),
            ]
        );
    }

    #[test]
    fn a_dependency_that_resolved_to_a_skeleton_is_worn_at_its_crate_and_locked_version() {
        // The standing carries the package and version the workspace reader
        // locked for the dependency, whatever else is later found about the
        // wearing.
        let mut dependency = worn_dependency("ritual/Cargo.toml", "lint", "b-skeleton");
        dependency.version = semver::Version::new(2, 3, 4);
        let workspace = Workspace {
            root: PathBuf::from("/workspace"),
            wearing: vec![Wearing::Worn(dependency)],
        };

        assert_eq!(
            candidates_in(&workspace, &manifest("ritual/Cargo.toml"), &[]),
            [Candidate::new(
                key("lint"),
                Standing::Worn {
                    crate_name: "b-skeleton".to_owned(),
                    version: semver::Version::new(2, 3, 4),
                }
            )]
        );
    }

    #[test]
    fn a_wearing_under_an_empty_key_is_a_candidate_like_any_other() {
        let workspace = Workspace {
            root: PathBuf::from("/workspace"),
            wearing: vec![refused(WearingRefusal::NamesNoDependency {
                manifest: "ritual/Cargo.toml".to_owned(),
                dependency: String::new(),
            })],
        };

        let candidates = candidates_in(&workspace, &manifest("ritual/Cargo.toml"), &[]);

        assert_eq!(candidates, [dangling("")]);
        assert_eq!(keys(pick("", &candidates)), Ok(""));
    }

    #[test]
    fn a_manifest_that_wears_nothing_has_no_candidates() {
        assert_eq!(
            candidates_in(&workspace(), &manifest("none/Cargo.toml"), &[]),
            []
        );
    }

    #[test]
    fn a_whole_skeletons_table_that_is_not_a_table_gives_no_candidate() {
        let mut only = workspace();
        only.wearing.retain(|wearing| {
            matches!(
                wearing,
                Wearing::Refused(WearingRefusal::NotATable { key: None, .. })
            )
        });

        assert_eq!(candidates_in(&only, &manifest("tools/Cargo.toml"), &[]), []);
    }

    #[test]
    fn a_wearing_elsewhere_is_found_by_its_key_and_by_its_crate() {
        let expected = vec![Elsewhere {
            manifest: "tools/Cargo.toml".to_owned(),
            key: "ci".to_owned(),
        }];

        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "ci"),
            expected
        );
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "a-skeleton"),
            expected
        );
    }

    #[test]
    fn a_dependency_that_is_no_skeleton_is_found_elsewhere_by_its_crate() {
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "plain-crate"),
            [Elsewhere {
                manifest: "tools/Cargo.toml".to_owned(),
                key: "plain".to_owned()
            }]
        );
    }

    #[test]
    fn a_dependency_that_resolved_to_nothing_is_found_elsewhere_by_its_crate() {
        // The refusal names the package the dependency is on, so the crate's
        // name reaches the wearing though nothing was locked for it.
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "moved-crate"),
            [Elsewhere {
                manifest: "tools/Cargo.toml".to_owned(),
                key: "moved".to_owned(),
            }]
        );
    }

    #[test]
    fn a_wearing_in_the_manifest_itself_is_not_elsewhere() {
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "lint"),
            []
        );
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("ritual/Cargo.toml"), "b-skeleton"),
            []
        );
    }

    #[test]
    fn a_reserved_key_and_a_whole_table_that_is_not_a_table_are_never_elsewhere() {
        assert_eq!(
            worn_elsewhere(&workspace(), &manifest("tools/Cargo.toml"), "options"),
            []
        );
    }

    #[test]
    fn wearings_elsewhere_come_in_manifest_then_key_order() {
        let mut several = workspace();
        several.wearing.push(Wearing::Worn(worn_dependency(
            "a/Cargo.toml",
            "z",
            "a-skeleton",
        )));
        several.wearing.push(Wearing::Worn(worn_dependency(
            "a/Cargo.toml",
            "b",
            "a-skeleton",
        )));

        let found = worn_elsewhere(&several, &manifest("ritual/Cargo.toml"), "a-skeleton");

        let order: Vec<(&str, &str)> = found
            .iter()
            .map(|entry| (entry.manifest.as_str(), entry.key.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("a/Cargo.toml", "b"),
                ("a/Cargo.toml", "z"),
                ("tools/Cargo.toml", "ci")
            ]
        );
    }
}
