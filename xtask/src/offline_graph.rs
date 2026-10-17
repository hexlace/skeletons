//! The standing check that the library side of `skeletons-engine` stays free
//! of the commands' dependencies.
//!
//! The engine reads a workspace and surveys it with nothing else compiled in;
//! its `commands` feature adds the command-line framework, the network client
//! and the manifest editor. A project that depends on the engine for the first
//! half must not pay for the second, and nothing in the engine's own tests
//! can say so, because they build the engine with whatever features the test
//! run unifies. So the question goes to Cargo itself: `cargo tree` for one
//! package at a time, over normal and build edges, against the lockfile as it
//! stands.
//!
//! `--package` resolves that package alone, so another workspace member that
//! turns `commands` on cannot turn it on for the package asked about, and the
//! answer does not depend on how this test binary was built. `--locked` pins
//! the graph to `Cargo.lock`, so the answer does not depend on the network
//! either. The query is not run offline: reading a package's dependencies
//! takes its source, and a cargo home that has built only `xtask` does not
//! hold the engine's. Cargo fetches what the lockfile names, as any build of
//! the workspace does, so a cold cargo home is never what fails this check.
//!
//! Each answer is asked of the same instrument twice over. The packages that
//! must be absent are looked for in the build without the commands; the same
//! packages must all be present when the commands are asked for, directly and
//! through a dependent that asks for them. A query that could not see the
//! graph would find nothing in the first and fail the second.
//!
//! The same graphs are also held to one version of each package. A project that
//! lints for duplicate crate versions is affected by whatever the library drags
//! in, so for each package in the first list no package may appear at two
//! different versions. The control for that is a parsed sample whose text the
//! test states, not the live graph with the commands on. That graph does hold
//! two versions of some packages today, but only because of what third-party
//! crates currently depend on, so an ordinary dependency update could remove
//! them and turn a control red for a reason that has nothing to do with this
//! workspace.

use std::collections::{BTreeMap, BTreeSet};

use crate::process::{self, Program};

/// The packages that only the commands bring in: the command-line framework
/// and its composition crate, the argument parser, the network client with the
/// TLS stack under it, and the manifest editor.
const COMMANDS_ONLY: [&str; 7] = [
    "rituals",
    "rituals-compose",
    "clap",
    "ureq",
    "rustls",
    "ring",
    "toml_edit",
];

/// The packages whose default build must hold none of [`COMMANDS_ONLY`]. A
/// package that depends on the engine for its offline half is added here.
const WITHOUT_COMMANDS: [&str; 2] = ["skeletons-engine", "skeletons-library"];

/// Which features a package is asked about with.
#[derive(Clone, Copy, Debug)]
enum Features {
    /// The package's default features and nothing else.
    Default,
    /// The package's default features and `commands`.
    Commands,
}

/// What `cargo tree` prints for `root`'s dependency graph over normal and build
/// edges, built with `features`: one line per package, `root` first.
fn dependency_tree(root: &str, features: Features) -> String {
    let mut arguments = vec![
        "tree",
        "--locked",
        "--package",
        root,
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ];
    match features {
        Features::Default => {}
        Features::Commands => arguments.extend(["--features", "commands"]),
    }
    process::query(Program::Cargo, &crate::workspace::root(), &arguments)
        .unwrap_or_else(|error| panic!("the graph of {root} could not be read: {error}"))
}

/// The names of the packages in `root`'s dependency graph over normal and
/// build edges, `root` itself included, built with `features`.
fn dependency_graph(root: &str, features: Features) -> Vec<String> {
    package_names(&dependency_tree(root, features))
}

/// The package name on each line of `cargo tree --prefix none --format {p}`:
/// the first word, before the version, the source and any `(*)` that marks a
/// repeated subtree. Blank lines are skipped.
fn package_names(tree: &str) -> Vec<String> {
    tree.lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// A package at one version, as `cargo tree` prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Package {
    name: String,
    version: String,
}

/// The name and version on each line of `cargo tree --prefix none --format
/// {p}`: the first two words. A line with no second word names no version and
/// is skipped, as are blank lines; Cargo prints a version for every package.
fn packages(tree: &str) -> Vec<Package> {
    tree.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let version = words.next()?;
            Some(Package {
                name: name.to_owned(),
                version: version.to_owned(),
            })
        })
        .collect()
}

/// A package name that appears at more than one version.
#[derive(Debug, PartialEq, Eq)]
struct Duplicate {
    name: String,
    /// Every version the name appears at, each once, in order.
    versions: Vec<String>,
}

/// The package names in `packages` that appear at two or more different
/// versions, in name order. The same name and version on several lines, which
/// is how a repeated subtree and a path package print, is one package.
fn duplicates(packages: &[Package]) -> Vec<Duplicate> {
    let mut versions_by_name: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for package in packages {
        versions_by_name
            .entry(&package.name)
            .or_default()
            .insert(&package.version);
    }
    versions_by_name
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
        .map(|(name, versions)| Duplicate {
            name: name.to_owned(),
            versions: versions.into_iter().map(str::to_owned).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        COMMANDS_ONLY, Duplicate, Features, WITHOUT_COMMANDS, dependency_graph, dependency_tree,
        duplicates, package_names, packages,
    };

    /// A line is read by its first word, whether it carries a version alone,
    /// a version and a path, a trailing repeat marker, or only a name.
    #[test]
    fn a_package_is_the_first_word_of_its_line() {
        let tree = "skeletons-engine v1.2.3 (/some/path)\n\
                    \n\
                    serde v1.0.0\n\
                    semver v1.0.0 (*)\n\
                    toml_edit v0.22.0 (/some/path) (*)\n\
                    bare\n";

        assert_eq!(
            package_names(tree),
            ["skeletons-engine", "serde", "semver", "toml_edit", "bare"]
        );
    }

    /// Nothing in the answer is a package when Cargo printed nothing.
    #[test]
    fn an_empty_tree_names_no_package() {
        assert_eq!(package_names(""), Vec::<String>::new());
        assert_eq!(package_names("\n  \n"), Vec::<String>::new());
    }

    /// The positive control for the duplicate finder: a sample holding one
    /// name at two versions reports that name with both, and only it.
    #[test]
    fn a_name_at_two_versions_is_a_duplicate() {
        let tree = "root v0.1.0 (/some/path)\n\
                    foo v1.0.0\n\
                    bar v3.0.0\n\
                    foo v2.1.0\n";

        assert_eq!(
            duplicates(&packages(tree)),
            [Duplicate {
                name: "foo".to_owned(),
                versions: vec!["v1.0.0".to_owned(), "v2.1.0".to_owned()],
            }]
        );
    }

    /// The same name at the same version on several lines is one package: a
    /// repeated subtree ends in `(*)`, and a package from a path carries it in
    /// parentheses.
    #[test]
    fn a_name_at_one_version_repeated_is_not_a_duplicate() {
        let tree = "root v0.1.0 (/some/path)\n\
                    foo v1.0.0\n\
                    foo v1.0.0 (*)\n\
                    member v0.1.0 (/some/path)\n\
                    member v0.1.0 (/some/path) (*)\n\
                    foo v1.0.0\n";

        assert_eq!(duplicates(&packages(tree)), Vec::<Duplicate>::new());
    }

    /// Nothing is a duplicate when Cargo printed nothing.
    #[test]
    fn an_empty_tree_has_no_duplicate() {
        assert_eq!(duplicates(&packages("")), Vec::<Duplicate>::new());
        assert_eq!(duplicates(&packages("\n  \n")), Vec::<Duplicate>::new());
    }

    /// A line is read as its name and its version, whatever follows them.
    #[test]
    fn a_package_line_holds_a_name_and_a_version() {
        let tree = "skeletons-engine v1.2.3 (/some/path)\n\
                    \n\
                    serde v1.0.0\n\
                    semver v1.0.0 (*)\n";

        let found: Vec<(String, String)> = packages(tree)
            .into_iter()
            .map(|package| (package.name, package.version))
            .collect();

        assert_eq!(
            found,
            [
                ("skeletons-engine".to_owned(), "v1.2.3".to_owned()),
                ("serde".to_owned(), "v1.0.0".to_owned()),
                ("semver".to_owned(), "v1.0.0".to_owned()),
            ]
        );
    }

    /// Asks Cargo for the default build of each root in `WITHOUT_COMMANDS`
    /// and requires that the root is in the answer with a version, so an empty
    /// or unparsed answer cannot pass, and that no package appears at two
    /// versions.
    #[test]
    fn a_build_without_the_commands_holds_one_version_of_each_package() {
        for root in WITHOUT_COMMANDS {
            let found = packages(&dependency_tree(root, Features::Default));

            assert!(
                found.iter().any(|package| package.name == root),
                "{root} built without the commands is missing from its own graph, with a \
                 version: the query answered {found:?}"
            );
            let repeated = duplicates(&found);
            assert!(
                repeated.is_empty(),
                "{root} built without the commands holds a package at more than one version: \
                 {repeated:?}"
            );
        }
    }

    /// Asks Cargo for the default build of each root in `WITHOUT_COMMANDS`
    /// and requires that it holds the root itself, so an empty or unparsed
    /// answer cannot pass, and none of the packages only the commands bring.
    #[test]
    fn a_build_without_the_commands_holds_none_of_their_dependencies() {
        for root in WITHOUT_COMMANDS {
            let graph = dependency_graph(root, Features::Default);

            assert!(
                graph.iter().any(|package| package == root),
                "{root} built without the commands is missing from its own graph: the query \
                 answered {graph:?}"
            );
            let found: Vec<&str> = COMMANDS_ONLY
                .into_iter()
                .filter(|package| graph.iter().any(|name| name == package))
                .collect();
            assert!(
                found.is_empty(),
                "{root} built without the commands depends on {found:?}, which only the commands \
                 may bring in"
            );
        }
    }

    /// The positive control for the test above: the same query, asked of the
    /// engine with `commands` on, must see every one of the packages, or the
    /// absence reported there means nothing.
    #[test]
    fn the_engine_with_the_commands_holds_all_of_their_dependencies() {
        let graph = dependency_graph("skeletons-engine", Features::Commands);

        for package in COMMANDS_ONLY {
            assert!(
                graph.iter().any(|found| found == package),
                "skeletons-engine built with the commands is missing {package} from its graph, \
                 so the query cannot see what it is looking for"
            );
        }
    }

    /// The same control through a dependent: `skeletons` asks the engine for
    /// `commands` in its own manifest, and the query must follow that edge.
    #[test]
    fn a_dependent_that_asks_for_the_commands_holds_all_of_their_dependencies() {
        let graph = dependency_graph("skeletons", Features::Default);

        for package in COMMANDS_ONLY {
            assert!(
                graph.iter().any(|found| found == package),
                "skeletons is missing {package} from its graph, so the query does not follow a \
                 dependent's feature edge into skeletons-engine"
            );
        }
    }
}
