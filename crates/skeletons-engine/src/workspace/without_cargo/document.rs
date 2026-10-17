//! The [`Document`] a workspace read without Cargo amounts to: the one `cargo metadata` would
//! have printed, as far as the readers of a workspace go.
//!
//! The member packages, the path packages, and each located package are `packages`; each
//! member's resolve edges come from the lockfile, named as Cargo names them; a locked package
//! whose files are not given is in `unlocated`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::error::ReadWithoutCargoError;
use super::layout::Layout;
use super::located::{self, LocatedPackage};
use super::lock_entries::{path_entry_index, set_aside_entries};
use super::lockfile::{LockEntry, Lockfile};
use super::package_manifest::{Declaration, DeclaredSource};
use super::path_package::PathPackage;
use crate::workspace::LockedPackageId;
use crate::workspace::crate_name::underscored;
use crate::workspace::schema::{
    Dependency, Document, Node, NodeDependency, Package, Resolve, UnlocatedPackage,
};

/// The `cargo metadata` format version the document stands in for.
const FORMAT_VERSION: u64 = 1;

/// What a lock entry resolved to for the edge that names it.
#[derive(Debug, Clone)]
enum Resolution {
    /// A package whose files are known, with the name of its library, if it has one.
    Located { id: String, library: Option<String> },
    /// A package whose files are not known.
    Unlocated { id: String },
}

/// The pieces of a [`Document`] as they are collected from the lockfile.
struct Collector<'input> {
    layout: &'input Layout,
    lockfile: &'input Lockfile,
    locations: &'input BTreeMap<LockedPackageId, PathBuf>,
    /// Entries that Cargo drops on loading the lockfile.
    set_aside: std::collections::BTreeSet<usize>,
    /// What each entry with a source resolved to, so a package two members depend on is read
    /// once.
    resolved: BTreeMap<usize, Resolution>,
    /// Packages read from the locations given, which a member's lock entry depends on.
    located: Vec<Package>,
    unlocated: Vec<UnlocatedPackage>,
}

/// Builds the document for `layout`, a lockfile that is current for it, and the directories the
/// caller gave for locked packages with a source.
///
/// # Errors
///
/// [`ReadWithoutCargoError::Lockfile`] when a member or a dependency it keeps is locked in a way
/// no package on disk matches; unsupported when a declaration resolves to a package with no
/// source without writing a `path`; unreadable when a located package has no target.
pub(super) fn build(
    layout: &Layout,
    lockfile: &Lockfile,
    locations: &BTreeMap<LockedPackageId, PathBuf>,
) -> Result<Document, ReadWithoutCargoError> {
    let mut collector = Collector {
        layout,
        lockfile,
        locations,
        set_aside: set_aside_entries(lockfile, &layout.packages),
        resolved: BTreeMap::new(),
        located: Vec::new(),
        unlocated: Vec::new(),
    };

    let mut nodes = Vec::new();
    for member in layout.packages.iter().filter(|package| package.is_member) {
        nodes.push(collector.node(member)?);
    }

    let workspace_members: Vec<String> = layout
        .packages
        .iter()
        .filter(|package| package.is_member)
        .map(path_id)
        .collect();
    assert_eq!(
        workspace_members.len(),
        nodes.len(),
        "every member has exactly one resolve node"
    );
    let mut packages: Vec<Package> = layout.packages.iter().map(path_record).collect();
    packages.append(&mut collector.located);

    Ok(Document {
        version: FORMAT_VERSION,
        workspace_root: layout.root_directory.clone(),
        workspace_members,
        packages,
        resolve: Resolve { nodes },
        unlocated: collector.unlocated,
    })
}

impl Collector<'_> {
    /// The resolve node for `member`: one edge per lock dependency it declares, as Cargo writes
    /// them.
    fn node(&mut self, member: &PathPackage) -> Result<Node, ReadWithoutCargoError> {
        let lockfile = self.lockfile;
        let Some(entry_index) = path_entry_index(lockfile, &member.name, &member.version) else {
            return Err(ReadWithoutCargoError::Lockfile);
        };

        let mut deps = Vec::new();
        for &dependency_index in lockfile.entries()[entry_index].dependency_indexes() {
            if self.set_aside.contains(&dependency_index) {
                continue;
            }
            if let Some(edge) = self.edge(member, dependency_index)? {
                deps.push(edge);
            }
        }
        Ok(Node {
            id: path_id(member),
            deps,
        })
    }

    /// The edge from `member` to the lock entry at `dependency_index`, or `None` when Cargo
    /// writes none: the entry is a package that `member` does not declare, or one with no
    /// library.
    fn edge(
        &mut self,
        member: &PathPackage,
        dependency_index: usize,
    ) -> Result<Option<NodeDependency>, ReadWithoutCargoError> {
        let lockfile = self.lockfile;
        let dependency = &lockfile.entries()[dependency_index];
        let Some(declaration) = member
            .declarations
            .iter()
            .find(|declaration| declaration.package == dependency.name())
        else {
            return Ok(None);
        };
        refuse_a_path_resolution_without_a_path(member, dependency, declaration)?;

        let (id, library) = match self.resolution(dependency_index)? {
            Resolution::Located { id, library } => (id, library),
            // An unlocated package has no manifest to say what its library is called, so the
            // edge is named for the package, which is what a library with no `[lib] name` is.
            Resolution::Unlocated { id } => (id, Some(underscored(dependency.name()))),
        };
        let Some(library) = library else {
            return Ok(None);
        };
        let name = declaration.rename.as_deref().map_or(library, underscored);
        Ok(Some(NodeDependency { name, pkg: id }))
    }

    /// What the lock entry at `index` resolved to, read once.
    fn resolution(&mut self, index: usize) -> Result<Resolution, ReadWithoutCargoError> {
        if let Some(known) = self.resolved.get(&index) {
            return Ok(known.clone());
        }
        let lockfile = self.lockfile;
        let entry = &lockfile.entries()[index];
        let resolution = match entry.source() {
            None => self.path_resolution(entry)?,
            Some(source) => self.sourced_resolution(entry, source)?,
        };
        self.resolved.insert(index, resolution.clone());
        Ok(resolution)
    }

    /// The package on disk that a lock entry with no source names.
    fn path_resolution(&self, entry: &LockEntry) -> Result<Resolution, ReadWithoutCargoError> {
        let Some(package) = self
            .layout
            .packages
            .iter()
            .find(|package| package.name == entry.name() && package.version == entry.version())
        else {
            // A path package locked at a version no manifest has: the lockfile is out of date.
            return Err(ReadWithoutCargoError::Lockfile);
        };
        Ok(Resolution::Located {
            id: path_id(package),
            library: package.library.clone(),
        })
    }

    /// The package a lock entry with a `source` names: read from the directory the caller gave,
    /// or unlocated.
    fn sourced_resolution(
        &mut self,
        entry: &LockEntry,
        source: &str,
    ) -> Result<Resolution, ReadWithoutCargoError> {
        let identity = LockedPackageId {
            name: entry.name().to_owned(),
            version: entry.version().to_owned(),
            source: source.to_owned(),
        };
        let id = sourced_id(&identity);
        let Some(package) = located::read(self.locations, &identity)? else {
            self.unlocated.push(UnlocatedPackage {
                id: id.clone(),
                name: identity.name,
                version: identity.version,
                source: identity.source,
            });
            return Ok(Resolution::Unlocated { id });
        };
        let library = package.library.clone();
        self.located.push(located_record(&id, &identity, package));
        Ok(Resolution::Located { id, library })
    }
}

/// Refuses a declaration that is locked as a package with no source without writing a `path`:
/// only a patch or a replacement does that, and this reader does not follow either.
fn refuse_a_path_resolution_without_a_path(
    member: &PathPackage,
    dependency: &LockEntry,
    declaration: &Declaration,
) -> Result<(), ReadWithoutCargoError> {
    let is_path_package = dependency.source().is_none();
    let writes_a_path = matches!(declaration.source, DeclaredSource::Path { .. });
    if is_path_package && !writes_a_path {
        return Err(ReadWithoutCargoError::unsupported(format!(
            "{}: `{}` is locked as a package on disk but declared without a `path`",
            member.manifest_path.display(),
            declaration.key
        )));
    }
    Ok(())
}

/// The id a package on disk goes by in the document.
fn path_id(package: &PathPackage) -> String {
    format!(
        "{} {} (path+{})",
        package.name,
        package.version,
        package.directory.display()
    )
}

/// The id a locked package with a source goes by in the document.
fn sourced_id(identity: &LockedPackageId) -> String {
    format!(
        "{} {} ({})",
        identity.name, identity.version, identity.source
    )
}

/// The `(name, rename)` pairs `cargo metadata` lists for a package's dependencies.
fn dependencies_of(declarations: &[Declaration]) -> Vec<Dependency> {
    declarations
        .iter()
        .map(|declaration| Dependency {
            name: declaration.package.clone(),
            rename: declaration.rename.clone(),
        })
        .collect()
}

/// The document's record of a package on disk.
fn path_record(package: &PathPackage) -> Package {
    Package {
        id: path_id(package),
        name: package.name.clone(),
        version: package.version.clone(),
        #[cfg(feature = "commands")]
        source: None,
        manifest_path: package.manifest_path.clone(),
        dependencies: dependencies_of(&package.declarations),
        metadata: package.metadata.clone(),
    }
}

/// The document's record of a located package.
fn located_record(id: &str, identity: &LockedPackageId, package: LocatedPackage) -> Package {
    Package {
        id: id.to_owned(),
        name: identity.name.clone(),
        version: identity.version.clone(),
        #[cfg(feature = "commands")]
        source: Some(identity.source.clone()),
        manifest_path: package.manifest_path,
        dependencies: dependencies_of(&package.declarations),
        metadata: package.metadata,
    }
}
