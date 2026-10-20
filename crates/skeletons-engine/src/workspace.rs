//! What a repository wears, read fresh on every run — nothing here is ever
//! stored and read back.
//!
//! There are three readers. [`read`] asks `cargo metadata`. [`read_without_cargo`]
//! reads the manifests and `Cargo.lock` as files, and takes from its caller the
//! directory of each locked package that is not a path on disk.
//! [`read_without_cargo_configuration`] is the second with no Cargo
//! configuration file read. All three build one document in the shape `cargo
//! metadata` prints, and everything after it is shared, so they give the same
//! workspace for the same files, or, in the one stale shape the lockfile can be
//! in, the workspace the first gives once Cargo brings the lock up to date. The
//! one thing only the last two can say is a wearing whose package has no
//! location, which is [`WearingRefusal::Unlocated`].

mod cargo_metadata;
mod crate_name;
mod crates_io_source;
mod git_query;
mod locate;
mod locked_package_id;
#[cfg(feature = "commands")]
mod pin;
#[cfg(feature = "commands")]
mod prospect;
mod schema;
mod wearing_table;
mod without_cargo;
mod worn;

use std::path::{Path, PathBuf};

pub use cargo_metadata::ReadWorkspaceError;
#[cfg(feature = "commands")]
pub(crate) use crate_name::{same_crate, underscored};
#[cfg(feature = "commands")]
pub(crate) use crates_io_source::CRATES_IO_SOURCE;
pub(crate) use locate::PackageIdentity;
pub use locked_package_id::LockedPackageId;
#[cfg(feature = "commands")]
pub(crate) use pin::Pin;
#[cfg(feature = "commands")]
pub(crate) use prospect::{Declared, Member, Prospect, SkeletonsTable, read_prospect};
pub(crate) use wearing_table::OptionShapeRefusal;
#[cfg(feature = "commands")]
pub(crate) use wearing_table::RESERVED_WEARING_KEYS;
#[cfg(test)]
pub(crate) use wearing_table::ShapeFound;
pub use without_cargo::{
    ReadWithoutCargoError, read_without_cargo, read_without_cargo_configuration,
};
pub use worn::WornDependency;
pub(crate) use worn::WornId;

use locate::{LocateRefusal, Target};
use schema::{Document, Package};
use wearing_table::{TableEntryRefusal, WholeTableRefusal};

/// What a repository wears: its root, and one outcome per
/// `[package.metadata.skeletons.<key>]` table found on any member.
pub struct Workspace {
    pub(crate) root: PathBuf,
    pub(crate) wearing: Vec<Wearing>,
}

/// One wearing table's outcome: a real worn skeleton, or a refusal that names no
/// single worn skeleton (a malformed table, an unresolved or ambiguous
/// dependency, a dependency that is not itself a skeleton).
pub(crate) enum Wearing {
    Worn(WornDependency),
    Refused(WearingRefusal),
}

/// Why a manifest's own wearing table could not name a worn skeleton at all —
/// as opposed to [`OptionShapeRefusal`], which still wears the skeleton and
/// refuses one of its option values instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WearingRefusal {
    /// `[package.metadata.skeletons]` itself, or one of its keys, is present but
    /// not a table. `key` is `None` for the whole table. `package` is the
    /// package the dependency declared under `key` names, and `None` for the
    /// whole table or when no dependency is declared under `key`.
    NotATable {
        manifest: String,
        key: Option<String>,
        package: Option<String>,
    },
    /// A real dependency is declared under a reserved key, `options` or
    /// `verbatim`. The JSON `dependency` field this refusal reports is that
    /// `key`; `crate_name` is the actual crate declared there, which the
    /// message needs to name the fix (`crate_name = { package =
    /// "crate_name", … }`) — tested by `crates/skeletons-engine/src/check/json.rs` →
    /// `reserved_key_reports_the_key_it_was_as_the_dependency`.
    ReservedKey {
        manifest: String,
        key: String,
        crate_name: String,
    },
    /// The key names no dependency of this manifest.
    NamesNoDependency {
        manifest: String,
        dependency: String,
    },
    /// The declared dependency resolved to no locked package. `package` is the
    /// package the declared dependency names.
    Unresolved {
        manifest: String,
        dependency: String,
        package: String,
    },
    /// The declared dependency resolved to more than one locked package.
    Ambiguous {
        manifest: String,
        dependency: String,
        packages: Vec<PackageIdentity>,
    },
    /// The resolved package is not itself a skeleton: its own manifest has no
    /// `[package.metadata.skeletons]` table.
    NotASkeleton {
        manifest: String,
        dependency: String,
        package: PackageIdentity,
    },
    /// The declared dependency resolved to one locked package whose files
    /// are not available, so whether it is a skeleton is unknown.
    Unlocated {
        manifest: String,
        dependency: String,
        package: LockedPackageId,
    },
}

/// Why a command meets no [`WearingRefusal::Unlocated`], for the arms that
/// state it cannot happen: only a reader that does not run cargo makes one,
/// and no command reads that way.
#[cfg(feature = "commands")]
pub(crate) const UNLOCATED_NEVER_MET: &str =
    "only a read without cargo makes an Unlocated refusal, and no command reads without cargo";

/// Whether [`read`] may let `cargo metadata` reach the network for a locked
/// source it does not already have.
///
/// `Allowed` is cargo's own normal fetch of exactly what the lockfile pins, so
/// a fresh clone whose members were never built still reads. `Refused` runs
/// cargo with `--offline`: a clone whose sources were never fetched fails with
/// cargo's own offline error rather than fetching them. `skeletons check`,
/// `skeletons wear` and `skeletons remove` read with `Allowed`; `skeletons sync`
/// reads with `Refused`, because writing requires nothing beyond what is
/// already locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// Cargo may fetch a locked source it does not already have.
    Allowed,
    /// Cargo runs offline and fetches nothing.
    Refused,
}

/// Reads `directory`'s workspace (cargo walks up from it to the workspace
/// root itself) and turns it into every wearing table any member declares.
pub fn read(directory: &Path, network: Network) -> Result<Workspace, ReadWorkspaceError> {
    Ok(from_document(&cargo_metadata::fetch(directory, network)?))
}

/// Brings `directory`'s `Cargo.lock` current, which `cargo metadata` does when
/// it is not told `--locked`, and reads nothing else from it.
///
/// This is how a lockfile loses the packages only a removed dependency needed.
/// It may write `Cargo.lock`, which is why [`read`] and `read_prospect`, which
/// never do, are asked afterwards to prove the result.
///
/// # Errors
///
/// Returns [`ReadWorkspaceError`] when `cargo metadata` could not be run or
/// refused.
#[cfg(feature = "commands")]
pub(crate) fn relock(directory: &Path) -> Result<(), ReadWorkspaceError> {
    cargo_metadata::relock(directory)
}

/// Turns one `cargo metadata` document into every wearing table any member
/// declares.
///
/// Shared by [`read`] and `read_prospect`, so the two see a workspace
/// identically and one `cargo metadata` run answers both.
fn from_document(document: &Document) -> Workspace {
    let mut members: Vec<&Package> = document
        .packages
        .iter()
        .filter(|package| document.workspace_members.contains(&package.id))
        .collect();
    // Visited in package-name order, so which refusal is reported first
    // never depends on `cargo metadata`'s own (unspecified) package order.
    members.sort_by(|left, right| left.name.cmp(&right.name));

    let mut wearing = Vec::new();
    for member in members {
        wearing.extend(read_member(document, member));
    }

    Workspace {
        root: document.workspace_root.clone(),
        wearing,
    }
}

/// Every wearing outcome (worn, or refused) declared on one workspace
/// member.
fn read_member(document: &Document, member: &Package) -> Vec<Wearing> {
    let manifest = relative_to_root(&document.workspace_root, &member.manifest_path);

    let (tables, entry_refusals) = match wearing_table::wearing_tables(&member.metadata, |key| {
        locate::declares_dependency_key(&member.dependencies, key)
    }) {
        Ok(read) => read,
        Err(WholeTableRefusal::NotATable) => {
            return vec![Wearing::Refused(WearingRefusal::NotATable {
                manifest,
                key: None,
                package: None,
            })];
        }
    };

    let mut outcomes: Vec<Wearing> = entry_refusals
        .into_iter()
        .map(|refusal| Wearing::Refused(entry_refusal(&manifest, &member.dependencies, refusal)))
        .collect();

    for (key, table) in tables {
        outcomes.push(read_one_wearing(
            document,
            member,
            &manifest,
            key.as_str(),
            &table,
        ));
    }
    outcomes
}

/// Converts one [`TableEntryRefusal`] into the [`WearingRefusal`] every
/// output actually reports, attaching the manifest name every message
/// carries.
fn entry_refusal(
    manifest: &str,
    dependencies: &[schema::Dependency],
    refusal: TableEntryRefusal,
) -> WearingRefusal {
    match refusal {
        TableEntryRefusal::NotATable { key } => WearingRefusal::NotATable {
            manifest: manifest.to_owned(),
            package: locate::find_declared(dependencies, &key)
                .map(|declared| declared.name.clone()),
            key: Some(key),
        },
        TableEntryRefusal::ReservedKey { dependency } => {
            // Guaranteed present: the reader only refuses a reserved key when
            // `locate::declares_dependency_key` was true for it over this very
            // `dependencies` slice, and that and this lookup
            // (`locate::find_declared`) share the one predicate, so `.any()`
            // having been true means `.find()` cannot come back empty.
            let Some(declared) = locate::find_declared(dependencies, &dependency) else {
                unreachable!("a reserved-key refusal only fires when the key is declared")
            };
            WearingRefusal::ReservedKey {
                manifest: manifest.to_owned(),
                key: dependency,
                crate_name: declared.name.clone(),
            }
        }
    }
}

/// Resolves one wearing table's own key to a worn dependency, or the
/// refusal that names why it could not be.
fn read_one_wearing(
    document: &Document,
    member: &Package,
    manifest: &str,
    key: &str,
    table: &wearing_table::WearingTable,
) -> Wearing {
    let Some(declared) = locate::find_declared(&member.dependencies, key) else {
        return Wearing::Refused(WearingRefusal::NamesNoDependency {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
        });
    };

    let skeleton = match locate::locate(document, member, declared) {
        Ok(Target::Package(skeleton)) => skeleton,
        // Checked before `declares_skeleton`: with no manifest to read, whether
        // the package is a skeleton is unknown, not "no".
        Ok(Target::Unlocated(unlocated)) => {
            return Wearing::Refused(WearingRefusal::Unlocated {
                manifest: manifest.to_owned(),
                dependency: key.to_owned(),
                package: LockedPackageId {
                    name: unlocated.name.clone(),
                    version: unlocated.version.clone(),
                    source: unlocated.source.clone(),
                },
            });
        }
        Err(LocateRefusal::Unresolved) => {
            return Wearing::Refused(WearingRefusal::Unresolved {
                manifest: manifest.to_owned(),
                dependency: key.to_owned(),
                package: declared.name.clone(),
            });
        }
        Err(LocateRefusal::Ambiguous { packages }) => {
            return Wearing::Refused(WearingRefusal::Ambiguous {
                manifest: manifest.to_owned(),
                dependency: key.to_owned(),
                packages,
            });
        }
    };

    if !declares_skeleton(&skeleton.metadata) {
        return Wearing::Refused(WearingRefusal::NotASkeleton {
            manifest: manifest.to_owned(),
            dependency: key.to_owned(),
            package: (skeleton.name.clone(), skeleton.version.clone()),
        });
    }

    // Postcondition of `cargo metadata`: every package's own version parses
    // as semver, and every manifest path has a parent directory —
    // asserted rather than surfaced as a refusal, since neither is a
    // condition a wearer's own manifest could cause.
    let version = semver::Version::parse(&skeleton.version).unwrap_or_else(|error| {
        unreachable!("cargo guarantees a parseable package version: {error}")
    });
    let Some(skeleton_directory) = skeleton.manifest_path.parent() else {
        unreachable!("a manifest path always has a parent directory")
    };

    Wearing::Worn(WornDependency {
        manifest: manifest.to_owned(),
        key: key.to_owned(),
        package: skeleton.name.clone(),
        version,
        skeleton_directory: skeleton_directory.to_owned(),
        #[cfg(feature = "commands")]
        pin: Pin::from_source(skeleton.source.as_deref(), skeleton_directory),
        choices: table.choices(),
    })
}

/// Whether `metadata`, a package's `[package.metadata]` as raw JSON, holds a
/// `skeletons` table: the declaration that makes a package a skeleton. A TOML
/// datetime there is not a table.
fn declares_skeleton(metadata: &serde_json::Value) -> bool {
    metadata
        .get("skeletons")
        .is_some_and(|skeletons| schema::as_toml_table(skeletons).is_some())
}

/// Renders `path` (always absolute, as `cargo metadata` gives every path
/// this crate reads) relative to `root`, purely lexically — comparing path
/// components as strings, never touching the filesystem or resolving a
/// symlink. `root` and `path` need not share an ancestor at any depth: a
/// `path =` dependency's own directory may sit anywhere on disk, and the
/// result may begin with any number of `..` components.
///
/// Used both eagerly, for a wearing member's own manifest path (this
/// module), and lazily, for a `path` pin's own directory at report-rendering
/// time (`check/pin_words.rs`, which the JSON and table output both go
/// through) — the same rule either way, so a wearing and a pin name a
/// directory identically.
pub(crate) fn relative_to_root(root: &Path, path: &Path) -> String {
    let root_components: Vec<_> = root.components().collect();
    let path_components: Vec<_> = path.components().collect();
    let shared = root_components
        .iter()
        .zip(path_components.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut parts: Vec<String> = Vec::new();
    for _ in shared..root_components.len() {
        parts.push("..".to_owned());
    }
    for component in &path_components[shared..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }

    // Postcondition: a relative path this crate reports never begins with
    // `/` and is never empty — `root` itself relativises to `.`, which no
    // real manifest or pin path ever equals.
    if parts.is_empty() {
        ".".to_owned()
    } else {
        let joined = parts.join("/");
        assert!(
            !joined.starts_with('/'),
            "a relative path never begins with `/`: {joined}"
        );
        joined
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::schema::{
        Dependency, Document, Node, NodeDependency, Package, Resolve, UnlocatedPackage,
    };
    use super::wearing_table::TableEntryRefusal;
    use super::{
        LockedPackageId, Wearing, WearingRefusal, declares_skeleton, entry_refusal, from_document,
        relative_to_root,
    };

    /// A document with one member wearing `house-rules`, whose only resolve
    /// edge points at a package that only the `unlocated` list holds.
    fn wearing_an_unlocated_package() -> Document {
        let member = Package {
            id: "id-wearer".to_owned(),
            name: "wearer".to_owned(),
            version: "0.1.0".to_owned(),
            #[cfg(feature = "commands")]
            source: None,
            manifest_path: "/WORKSPACE/Cargo.toml".into(),
            dependencies: vec![Dependency {
                name: "house-rules".to_owned(),
                rename: None,
            }],
            metadata: json!({"skeletons": {"house-rules": {}}}),
        };
        Document {
            version: 1,
            workspace_root: "/WORKSPACE".into(),
            workspace_members: vec!["id-wearer".to_owned()],
            packages: vec![member],
            resolve: Resolve {
                nodes: vec![Node {
                    id: "id-wearer".to_owned(),
                    deps: vec![NodeDependency {
                        name: "house_rules".to_owned(),
                        pkg: "id-house-rules".to_owned(),
                    }],
                }],
            },
            unlocated: vec![UnlocatedPackage {
                id: "id-house-rules".to_owned(),
                name: "house-rules".to_owned(),
                version: "4.5.6".to_owned(),
                source: "registry+https://example.test/index".to_owned(),
            }],
        }
    }

    #[test]
    fn a_wearing_of_an_unlocated_package_is_refused_as_unlocated() {
        // The package has no manifest, so whether it is a skeleton is
        // unknown: the wearing must be refused as unlocated, naming the exact
        // lockfile identity, and not as `not-a-skeleton`, which would claim
        // its manifest was read and held no skeletons table.
        let workspace = from_document(&wearing_an_unlocated_package());

        let [Wearing::Refused(refusal)] = workspace.wearing.as_slice() else {
            panic!("expected exactly one refused wearing");
        };
        assert_eq!(
            *refusal,
            WearingRefusal::Unlocated {
                manifest: "Cargo.toml".to_owned(),
                dependency: "house-rules".to_owned(),
                package: LockedPackageId {
                    name: "house-rules".to_owned(),
                    version: "4.5.6".to_owned(),
                    source: "registry+https://example.test/index".to_owned(),
                },
            }
        );
    }

    #[test]
    fn a_package_with_a_skeletons_table_declares_a_skeleton() {
        assert!(declares_skeleton(&json!({"skeletons": {}})));
        assert!(declares_skeleton(&json!({"skeletons": {"options": {}}})));
    }

    #[test]
    fn a_package_without_a_skeletons_table_declares_no_skeleton() {
        assert!(!declares_skeleton(&json!(null)));
        assert!(!declares_skeleton(&json!({})));
        assert!(!declares_skeleton(&json!({"skeletons": "text"})));
    }

    #[test]
    fn a_skeletons_that_is_a_datetime_declares_no_skeleton() {
        let metadata = json!({"skeletons": {"$__toml_private_datetime": "1979-05-27"}});

        assert!(!declares_skeleton(&metadata));
    }

    #[test]
    fn a_path_directly_under_the_root_relativises_with_no_leading_dots() {
        assert_eq!(
            relative_to_root(
                std::path::Path::new("/w"),
                std::path::Path::new("/w/Cargo.toml")
            ),
            "Cargo.toml"
        );
    }

    #[test]
    fn a_nested_path_keeps_every_intermediate_component() {
        assert_eq!(
            relative_to_root(
                std::path::Path::new("/w"),
                std::path::Path::new("/w/tools/Cargo.toml")
            ),
            "tools/Cargo.toml"
        );
    }

    #[test]
    fn a_path_outside_the_root_climbs_with_dot_dot() {
        assert_eq!(
            relative_to_root(
                std::path::Path::new("/w/root"),
                std::path::Path::new("/w/ci-skeleton")
            ),
            "../ci-skeleton"
        );
    }

    #[test]
    fn the_root_itself_relativises_to_a_single_dot() {
        assert_eq!(
            relative_to_root(std::path::Path::new("/w"), std::path::Path::new("/w")),
            "."
        );
    }

    #[test]
    fn a_deeply_unrelated_path_climbs_once_per_root_component_beyond_the_shared_prefix() {
        assert_eq!(
            relative_to_root(
                std::path::Path::new("/w/a/b"),
                std::path::Path::new("/w/x/y")
            ),
            "../../x/y"
        );
    }

    #[test]
    fn a_table_that_is_not_a_table_names_the_package_of_the_dependency_under_its_key() {
        // The dependency `tidy` is a rename of the package `house-rules`, so
        // the refusal for the key `tidy` must name the package, never the key.
        let dependencies = [Dependency {
            name: "house-rules".to_owned(),
            rename: Some("tidy".to_owned()),
        }];

        let refusal = entry_refusal(
            "Cargo.toml",
            &dependencies,
            TableEntryRefusal::NotATable {
                key: "tidy".to_owned(),
            },
        );

        assert_eq!(
            refusal,
            WearingRefusal::NotATable {
                manifest: "Cargo.toml".to_owned(),
                key: Some("tidy".to_owned()),
                package: Some("house-rules".to_owned()),
            }
        );
    }

    #[test]
    fn a_table_that_is_not_a_table_under_an_undeclared_key_names_no_package() {
        // Only an unrelated dependency is declared, so nothing says which
        // package the key `tidy` was meant for.
        let dependencies = [Dependency {
            name: "other".to_owned(),
            rename: None,
        }];

        let refusal = entry_refusal(
            "Cargo.toml",
            &dependencies,
            TableEntryRefusal::NotATable {
                key: "tidy".to_owned(),
            },
        );

        assert_eq!(
            refusal,
            WearingRefusal::NotATable {
                manifest: "Cargo.toml".to_owned(),
                key: Some("tidy".to_owned()),
                package: None,
            }
        );
    }
}
