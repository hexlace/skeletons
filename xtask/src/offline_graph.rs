//! The standing check that the library side of `skeletons-engine` stays free
//! of the commands' dependencies.
//!
//! The engine reads a workspace and surveys it with nothing else compiled in;
//! its `commands` feature adds the command-line framework, the network client
//! and the manifest editor. A project that depends on the engine for the first
//! half must not pay for the second, and nothing in the engine's own tests
//! can say so, because they build the engine with whatever features the test
//! run unifies. So the question goes to Cargo itself: `cargo tree` for one
//! package at a time, normal edges only, against the lockfile as it stands.
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

use crate::process::{self, Program};

/// The packages that only the commands bring in: the command-line framework
/// and its composition crate, the argument parser, and the network client
/// with the TLS stack under it.
const COMMANDS_ONLY: [&str; 6] = [
    "rituals",
    "rituals-compose",
    "clap",
    "ureq",
    "rustls",
    "ring",
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

/// The names of the packages in `root`'s normal dependency graph, `root`
/// itself included, built with `features`.
fn normal_graph(root: &str, features: Features) -> Vec<String> {
    let mut arguments = vec![
        "tree",
        "--locked",
        "--package",
        root,
        "--edges",
        "normal",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ];
    match features {
        Features::Default => {}
        Features::Commands => arguments.extend(["--features", "commands"]),
    }
    let tree = process::query(Program::Cargo, &crate::workspace::root(), &arguments)
        .unwrap_or_else(|error| panic!("the graph of {root} could not be read: {error}"));
    package_names(&tree)
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

#[cfg(test)]
mod tests {
    use super::{COMMANDS_ONLY, Features, WITHOUT_COMMANDS, normal_graph, package_names};

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

    /// Asks Cargo for the default build of each root in `WITHOUT_COMMANDS`
    /// and requires that it holds the root itself, so an empty or unparsed
    /// answer cannot pass, and none of the packages only the commands bring.
    #[test]
    fn a_build_without_the_commands_holds_none_of_their_dependencies() {
        for root in WITHOUT_COMMANDS {
            let graph = normal_graph(root, Features::Default);

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
        let graph = normal_graph("skeletons-engine", Features::Commands);

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
        let graph = normal_graph("skeletons", Features::Default);

        for package in COMMANDS_ONLY {
            assert!(
                graph.iter().any(|found| found == package),
                "skeletons is missing {package} from its graph, so the query does not follow a \
                 dependent's feature edge into skeletons-engine"
            );
        }
    }
}
