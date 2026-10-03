//! What a repository wears, read fresh from `cargo metadata` on every run —
//! nothing here is ever stored and read back.

mod cargo_metadata;
mod crate_name;
mod locate;
mod pin;
mod prospect;
mod schema;
mod wearing_table;
mod worn;

use std::path::{Path, PathBuf};

pub(crate) use cargo_metadata::ReadWorkspaceError;
pub(crate) use crate_name::{same_crate, underscored};
pub(crate) use locate::PackageIdentity;
pub(crate) use pin::{CRATES_IO_SOURCE, Pin};
pub(crate) use prospect::{Declared, Member, Prospect, SkeletonsTable, read_prospect};
pub(crate) use wearing_table::{OptionShapeRefusal, RESERVED_WEARING_KEYS};
pub(crate) use worn::{WornDependency, WornId};

use locate::LocateRefusal;
use schema::{Document, Package};
use wearing_table::{TableEntryRefusal, WholeTableRefusal};

/// What a repository wears: its root, and one outcome per
/// `[package.metadata.skeletons.<key>]` table found on any member.
pub(crate) struct Workspace {
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
    /// not a table. `key` is `None` for the whole table.
    NotATable {
        manifest: String,
        key: Option<String>,
    },
    /// A real dependency is declared under a reserved key, `options` or
    /// `verbatim`. The JSON `dependency` field this refusal reports is that
    /// `key`; `crate_name` is the actual crate declared there, which the
    /// message needs to name the fix (`crate_name = { package =
    /// "crate_name", … }`) — tested by `crates/skeletons/src/check/json.rs` →
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
    /// The declared dependency resolved to no locked package.
    Unresolved {
        manifest: String,
        dependency: String,
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
}

/// Whether `read` may let `cargo metadata` reach the network for a locked
/// source it does not already have.
///
/// `check` always passes `Allowed`, so a fresh clone whose members were
/// never built still works: cargo fetches exactly what the lockfile pins,
/// which is cargo's own normal fetch, not a question `skeletons` asks. `wear`
/// reads the same way, through `read_prospect` and through its read back,
/// since `cargo add` may itself be fetching. `sync` always passes `Refused`
/// (`--offline`), because its own contract is that writing requires nothing
/// beyond what is already locked — a clone whose sources were never fetched
/// aborts with cargo's own offline error rather than fetching them. Each
/// command's own `run` makes that choice and none takes it from a caller —
/// there is nowhere else in the crate a different choice could come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Network {
    Allowed,
    Refused,
}

/// Reads `directory`'s workspace (cargo walks up from it to the workspace
/// root itself) and turns it into every wearing table any member declares.
pub(crate) fn read(directory: &Path, network: Network) -> Result<Workspace, ReadWorkspaceError> {
    Ok(from_document(&cargo_metadata::fetch(directory, network)?))
}

/// Turns one `cargo metadata` document into every wearing table any member
/// declares.
///
/// Shared by [`read`] and [`read_prospect`], so the two see a workspace
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
        Ok(skeleton) => skeleton,
        Err(LocateRefusal::Unresolved) => {
            return Wearing::Refused(WearingRefusal::Unresolved {
                manifest: manifest.to_owned(),
                dependency: key.to_owned(),
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

    if !skeleton
        .metadata
        .get("skeletons")
        .is_some_and(serde_json::Value::is_object)
    {
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
        pin: Pin::from_source(skeleton.source.as_deref(), skeleton_directory),
        choices: table.choices(),
    })
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
    use super::relative_to_root;

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
}
