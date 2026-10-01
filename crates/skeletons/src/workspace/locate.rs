//! Dependency key -> locked package: matching a wearing table's key against
//! a member's own declared `[dependencies]`, then against the resolved
//! graph, to find the one locked package a wearing table names.

use std::collections::BTreeSet;

use super::schema::{Dependency, Document, Package};

/// Whether `dependencies` (a member's own `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]`, or a target-specific table — `cargo metadata`
/// reports all of them in the one list) declares a dependency under exactly
/// `key`: its `rename`, or its own crate name when it has none.
pub(crate) fn declares_dependency_key(dependencies: &[Dependency], key: &str) -> bool {
    dependencies
        .iter()
        .any(|dependency| declared_key(dependency) == key)
}

/// Finds the one `[dependencies]` entry declared under `key`, if any.
pub(crate) fn find_declared<'a>(
    dependencies: &'a [Dependency],
    key: &str,
) -> Option<&'a Dependency> {
    dependencies
        .iter()
        .find(|dependency| declared_key(dependency) == key)
}

fn declared_key(dependency: &Dependency) -> &str {
    dependency.rename.as_deref().unwrap_or(&dependency.name)
}

/// One resolved package's identity, for naming in an `ambiguous` refusal.
pub(crate) type PackageIdentity = (String, String);

/// Why a declared dependency could not be resolved to exactly one locked
/// package.
#[derive(Debug)]
pub(crate) enum LocateRefusal {
    /// Cargo resolved no package for it at all.
    Unresolved,
    /// Cargo resolved more than one distinct package for it.
    Ambiguous { packages: Vec<PackageIdentity> },
}

/// Resolves `declared` — found under workspace member `member`'s own
/// `[dependencies]` — to the one locked package it names.
///
/// Matched entirely by cargo's own resolved graph edges
/// (`resolve.nodes[].deps[]`), never by a package's Cargo target kind
/// (`lib`, `rlib`, `dylib`, `proc-macro`, `cdylib`, `staticlib`, and cargo's
/// own unstable `sdylib`). Identifying a library by reading one such kind
/// out of several would refuse a skeleton whose crate declared a different
/// one with a message claiming cargo resolved nothing, when it plainly had;
/// there is no closed list of kinds to read, so this reads none. What
/// cargo *does* state, unconditionally, is which package each edge resolves
/// to (`deps[].pkg`) and which extern-crate name that edge carries
/// (`deps[].name`) — a renamed declaration's edge is named for the name it
/// is declared under (`y` in `y = { package = "x" }`, underscored), and
/// every other edge to a same-named package is the declaration's, except one
/// a renamed sibling already claims (see [`names_taken_by_renames`]).
///
/// # Panics
///
/// Panics if `member` has no resolve node, or if a resolve node names a
/// package id absent from `document.packages` — both contradict cargo's own
/// documented `--format-version 1` shape (`resolve.nodes` carries exactly
/// one entry per package, and every edge's `pkg` is a package cargo also
/// lists), so either is a defect in cargo's own output, not a case this
/// function can name a sensible refusal for.
pub(crate) fn locate<'a>(
    document: &'a Document,
    member: &Package,
    declared: &Dependency,
) -> Result<&'a Package, LocateRefusal> {
    let Some(node) = document
        .resolve
        .nodes
        .iter()
        .find(|node| node.id == member.id)
    else {
        unreachable!(
            "cargo metadata lists a resolve node for every workspace member, including {}",
            member.id
        )
    };

    let claimed_by_a_rename = names_taken_by_renames(&member.dependencies, &declared.name);
    let mut distinct_ids: Vec<&str> = Vec::new();
    for edge in &node.deps {
        let Some(candidate) = document
            .packages
            .iter()
            .find(|package| package.id == edge.pkg)
        else {
            unreachable!(
                "cargo metadata lists every package a resolve node names, including {}",
                edge.pkg
            )
        };
        if candidate.name != declared.name {
            continue;
        }
        let is_declareds_own_edge = declared.rename.as_deref().map_or_else(
            || !claimed_by_a_rename.contains(&edge.name),
            |rename| edge.name == rename.replace('-', "_"),
        );
        if is_declareds_own_edge && !distinct_ids.contains(&edge.pkg.as_str()) {
            distinct_ids.push(&edge.pkg);
        }
    }

    match distinct_ids.as_slice() {
        [] => Err(LocateRefusal::Unresolved),
        [only] => {
            let Some(package) = document.packages.iter().find(|package| package.id == *only) else {
                unreachable!("distinct_ids only ever holds an id already found in packages")
            };
            Ok(package)
        }
        many => Err(LocateRefusal::Ambiguous {
            packages: many
                .iter()
                .filter_map(|id| document.packages.iter().find(|package| package.id == *id))
                .map(|package| (package.name.clone(), package.version.clone()))
                .collect(),
        }),
    }
}

/// Every underscored extern-crate name some *other* declared dependency of
/// `member_dependencies` claims for a package named `package_name`, by its
/// own rename (`y = { package = "x" }` claims `y` for the package `x`).
///
/// An unrenamed declaration for the same package must not match one of
/// these edges: it is already the renamed sibling's own edge, reached
/// through a different dependency entry, not a second edge the unrenamed
/// declaration also owns
/// (`an_unrenamed_declaration_ignores_the_edge_its_renamed_sibling_owns`).
fn names_taken_by_renames(
    member_dependencies: &[Dependency],
    package_name: &str,
) -> BTreeSet<String> {
    member_dependencies
        .iter()
        .filter(|dependency| dependency.name == package_name)
        .filter_map(|dependency| dependency.rename.as_deref())
        .map(|rename| rename.replace('-', "_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{LocateRefusal, declares_dependency_key, find_declared, locate};
    use crate::workspace::schema::{Dependency, Document, Node, NodeDependency, Package, Resolve};

    /// Builds a resolved package with no declared dependencies of its own —
    /// every test skeleton this file locates.
    fn package(id: &str, name: &str, version: &str) -> Package {
        Package {
            id: id.to_owned(),
            name: name.to_owned(),
            version: version.to_owned(),
            source: None,
            manifest_path: format!("/WORKSPACE/{name}/Cargo.toml").into(),
            dependencies: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Builds the workspace member whose own `[dependencies]` `dependencies`
    /// mirrors, and whose resolve node is `id` — the second argument
    /// [`locate`] itself reads both from.
    fn member(id: &str, dependencies: Vec<Dependency>) -> Package {
        Package {
            id: id.to_owned(),
            name: "wearer".to_owned(),
            version: "0.1.0".to_owned(),
            source: None,
            manifest_path: "/WORKSPACE/wearer/Cargo.toml".into(),
            dependencies,
            metadata: serde_json::Value::Null,
        }
    }

    fn document(packages: Vec<Package>, nodes: Vec<Node>) -> Document {
        Document {
            version: 1,
            workspace_root: "/WORKSPACE".into(),
            workspace_members: Vec::new(),
            packages,
            resolve: Resolve { nodes },
        }
    }

    /// Builds a declared `[dependencies]` entry — `Dependency` derives
    /// neither `Clone` nor `Copy`, so tests that need the same declaration
    /// both inside a member's own dependency list and standing alone as the
    /// one being resolved build it twice, from the same two arguments,
    /// rather than sharing one value.
    fn dependency(name: &str, rename: Option<&str>) -> Dependency {
        Dependency {
            name: name.to_owned(),
            rename: rename.map(str::to_owned),
        }
    }

    #[test]
    fn declares_dependency_key_matches_a_rename_over_the_crate_name() {
        let dependencies = vec![Dependency {
            name: "a-skeleton".to_owned(),
            rename: Some("renamed".to_owned()),
        }];
        assert!(declares_dependency_key(&dependencies, "renamed"));
        assert!(!declares_dependency_key(&dependencies, "a-skeleton"));
    }

    #[test]
    fn declares_dependency_key_matches_the_crate_name_with_no_rename() {
        let dependencies = vec![Dependency {
            name: "a-skeleton".to_owned(),
            rename: None,
        }];
        assert!(declares_dependency_key(&dependencies, "a-skeleton"));
    }

    #[test]
    fn find_declared_returns_none_for_an_unknown_key() {
        assert!(find_declared(&[], "nope").is_none());
    }

    #[test]
    fn an_rlib_skeleton_is_located() {
        // `crate-type = ["rlib"]`: cargo's own target kind is `["rlib"]`,
        // never `["lib"]`, but the edge to it is exactly as ordinary as any
        // other.
        let skeleton = package("id-skeleton", "rlib-skeleton", "0.1.0");
        let declared = Dependency {
            name: "rlib-skeleton".to_owned(),
            rename: None,
        };
        let document = document(
            vec![skeleton],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![NodeDependency {
                    name: "rlib_skeleton".to_owned(),
                    pkg: "id-skeleton".to_owned(),
                }],
            }],
        );
        let member = member("id-wearer", vec![dependency("rlib-skeleton", None)]);
        let located = locate(&document, &member, &declared).expect("must resolve");
        assert_eq!(located.id, "id-skeleton");
    }

    #[test]
    fn a_proc_macro_skeleton_is_located() {
        // `proc-macro = true`: cargo's own target kind is `["proc-macro"]`,
        // which a match on the `lib` kind alone would refuse.
        let skeleton = package("id-skeleton", "proc-macro-skeleton", "0.1.0");
        let declared = Dependency {
            name: "proc-macro-skeleton".to_owned(),
            rename: None,
        };
        let document = document(
            vec![skeleton],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![NodeDependency {
                    name: "proc_macro_skeleton".to_owned(),
                    pkg: "id-skeleton".to_owned(),
                }],
            }],
        );
        let member = member("id-wearer", vec![dependency("proc-macro-skeleton", None)]);
        let located = locate(&document, &member, &declared).expect("must resolve");
        assert_eq!(located.id, "id-skeleton");
    }

    #[test]
    fn a_skeleton_whose_lib_is_renamed_is_located_by_its_edge_name() {
        // `[lib] name = "other_name"`: the crate's own extern-crate name is
        // whatever the skeleton's manifest gives its library, not any
        // function of the package's own name or the dependency's key. An
        // unrenamed declaration still resolves, because matching reads only
        // the edge's target package identity, never a derived name.
        let skeleton = package("id-skeleton", "sk-named", "0.1.0");
        let declared = Dependency {
            name: "sk-named".to_owned(),
            rename: None,
        };
        let document = document(
            vec![skeleton],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![NodeDependency {
                    name: "other_name".to_owned(),
                    pkg: "id-skeleton".to_owned(),
                }],
            }],
        );
        let member = member("id-wearer", vec![dependency("sk-named", None)]);
        let located = locate(&document, &member, &declared).expect("must resolve");
        assert_eq!(located.id, "id-skeleton");
    }

    #[test]
    fn a_renamed_declaration_is_located_by_its_underscored_rename() {
        let skeleton = package("id-skeleton", "a-skeleton", "0.1.0");
        let declared = Dependency {
            name: "a-skeleton".to_owned(),
            rename: Some("renamed".to_owned()),
        };
        let document = document(
            vec![skeleton],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![NodeDependency {
                    name: "renamed".to_owned(),
                    pkg: "id-skeleton".to_owned(),
                }],
            }],
        );
        let member = member("id-wearer", vec![dependency("a-skeleton", Some("renamed"))]);
        let located = locate(&document, &member, &declared).expect("must resolve");
        assert_eq!(located.id, "id-skeleton");
    }

    #[test]
    fn an_unrenamed_declaration_ignores_the_edge_its_renamed_sibling_owns() {
        // `x = "1"` and `y = { package = "x", version = "2" }`: two distinct
        // packages both named `x`, reached through two edges on the same
        // member — one named `x` (the unrenamed declaration's own), one
        // named `y` (the renamed sibling's own). Without the exclusion, the
        // unrenamed declaration would match both and read `x` as ambiguous.
        let x1 = package("id-x1", "x", "1.0.0");
        let x2 = package("id-x2", "x", "2.0.0");
        let unrenamed = Dependency {
            name: "x".to_owned(),
            rename: None,
        };
        let renamed_sibling = Dependency {
            name: "x".to_owned(),
            rename: Some("y".to_owned()),
        };
        let document = document(
            vec![x1, x2],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![
                    NodeDependency {
                        name: "x".to_owned(),
                        pkg: "id-x1".to_owned(),
                    },
                    NodeDependency {
                        name: "y".to_owned(),
                        pkg: "id-x2".to_owned(),
                    },
                ],
            }],
        );
        let member = member(
            "id-wearer",
            vec![dependency("x", None), dependency("x", Some("y"))],
        );
        let located = locate(&document, &member, &unrenamed).expect("must resolve to x@1");
        assert_eq!(located.id, "id-x1");
        let located_sibling =
            locate(&document, &member, &renamed_sibling).expect("must resolve to x@2");
        assert_eq!(located_sibling.id, "id-x2");
    }

    #[test]
    fn no_matching_candidate_is_unresolved() {
        let document = document(
            vec![],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![],
            }],
        );
        let declared = Dependency {
            name: "a-skeleton".to_owned(),
            rename: None,
        };
        let member = member("id-wearer", vec![dependency("a-skeleton", None)]);
        let error =
            locate(&document, &member, &declared).expect_err("no candidate must be unresolved");
        assert!(matches!(error, LocateRefusal::Unresolved));
    }

    #[test]
    fn two_distinct_packages_matching_the_same_key_is_ambiguous() {
        let one = package("id-one", "a-skeleton", "0.1.0");
        let two = package("id-two", "a-skeleton", "0.2.0");
        let document = document(
            vec![one, two],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![
                    NodeDependency {
                        name: "a_skeleton".to_owned(),
                        pkg: "id-one".to_owned(),
                    },
                    NodeDependency {
                        name: "a_skeleton".to_owned(),
                        pkg: "id-two".to_owned(),
                    },
                ],
            }],
        );
        let declared = Dependency {
            name: "a-skeleton".to_owned(),
            rename: None,
        };
        let member = member("id-wearer", vec![dependency("a-skeleton", None)]);
        let error = locate(&document, &member, &declared)
            .expect_err("two distinct packages must be ambiguous");
        let LocateRefusal::Ambiguous { packages } = error else {
            panic!("expected Ambiguous")
        };
        assert_eq!(packages.len(), 2);
    }

    #[test]
    fn the_same_package_reached_through_two_edges_is_not_ambiguous() {
        let skeleton = package("id-skeleton", "a-skeleton", "0.1.0");
        let document = document(
            vec![skeleton],
            vec![Node {
                id: "id-wearer".to_owned(),
                deps: vec![
                    NodeDependency {
                        name: "a_skeleton".to_owned(),
                        pkg: "id-skeleton".to_owned(),
                    },
                    NodeDependency {
                        name: "a_skeleton".to_owned(),
                        pkg: "id-skeleton".to_owned(),
                    },
                ],
            }],
        );
        let declared = Dependency {
            name: "a-skeleton".to_owned(),
            rename: None,
        };
        let member = member("id-wearer", vec![dependency("a-skeleton", None)]);
        let located = locate(&document, &member, &declared)
            .expect("the same pkg id twice must resolve, not be ambiguous");
        assert_eq!(located.id, "id-skeleton");
    }
}
