//! Binaries, examples, tests and benches: which of them a package has, as Cargo decides.
//!
//! A package has the tables its manifest writes and, unless Cargo is told or defaults not to, the
//! ones it finds in the files. A table with a `path` is a target whether or not the file is
//! there. A table without one is the one file that matches its name. A binary that finds none is
//! refused; an example, test or bench that finds none is dropped, and so is not a target.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::super::package_manifest::{Edition, TargetKind, Targets};
use super::super::text_error::TextError;
use super::inferred::{Inferred, inferred};

/// The kinds counted here, in the order Cargo reads them.
const KINDS: [TargetKind; 4] = [
    TargetKind::Binary,
    TargetKind::Example,
    TargetKind::Test,
    TargetKind::Benchmark,
];

/// A binary cannot be named for a directory Cargo builds into.
const FORBIDDEN_BINARY_NAMES: [&str; 4] = ["deps", "examples", "build", "incremental"];

/// The words Cargo uses for a kind in its sentences.
struct Words {
    /// The word in `<human> target names cannot be empty`.
    human: &'static str,
    /// The key in `<human> target <key>.name is required`, which is the table's name.
    key: &'static str,
    /// The word in `found duplicate <duplicate> name <name>`.
    duplicate: &'static str,
}

fn words(kind: TargetKind) -> Words {
    match kind {
        TargetKind::Binary => Words {
            human: "binary",
            key: "bin",
            duplicate: "binary",
        },
        TargetKind::Example => Words {
            human: "example",
            key: "example",
            duplicate: "example",
        },
        TargetKind::Test => Words {
            human: "test",
            key: "test",
            duplicate: "test",
        },
        TargetKind::Benchmark => Words {
            human: "benchmark",
            key: "bench",
            duplicate: "bench",
        },
        TargetKind::Library => unreachable!("a library is resolved in `library`"),
    }
}

/// A target to resolve: a table the manifest writes, or a file Cargo found.
struct Candidate {
    name: Option<String>,
    path: Option<PathBuf>,
}

/// Whether Cargo finds a file for a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Located {
    Found,
    NotFound,
    /// Two files match the name.
    Ambiguous,
}

/// How many binaries, examples, tests and benches the package in `directory` has.
///
/// `has_library` is whether the package has a library, which the 2015 fallback for a binary asks.
///
/// # Errors
///
/// Unreadable for the manifests Cargo refuses: a table with no name or an empty one, a binary
/// named for a build directory, a binary that finds no file or two, and two targets of one kind
/// with one name.
pub(super) fn count(
    directory: &Path,
    package_name: &str,
    targets: &Targets,
    has_library: bool,
) -> Result<usize, TextError> {
    let mut resolved = Vec::with_capacity(KINDS.len());
    for kind in KINDS {
        let names = resolve(directory, package_name, targets, kind, has_library)?;
        resolved.push((kind, names));
    }
    for (kind, names) in &resolved {
        refuse_duplicates(*kind, names)?;
    }
    Ok(resolved.iter().map(|(_, names)| names.len()).sum())
}

/// The names of the targets of `kind` that Cargo finds a file for, in name order.
fn resolve(
    directory: &Path,
    package_name: &str,
    targets: &Targets,
    kind: TargetKind,
    has_library: bool,
) -> Result<Vec<String>, TextError> {
    let found = inferred(directory, kind, package_name);
    let candidates = candidates(directory, targets, kind, &found);
    let mut names = Vec::with_capacity(candidates.len());
    for candidate in &candidates {
        let name = checked_name(kind, candidate.name.as_deref())?;
        let located = locate(
            directory,
            targets.edition(),
            kind,
            has_library,
            candidate,
            &found,
        );
        match (located, kind) {
            (Located::Found, _) => names.push(name.to_owned()),
            (Located::NotFound, TargetKind::Binary) => {
                return Err(TextError::unreadable(format!(
                    "can't find `{name}` bin at `src/bin/{name}.rs` or `src/bin/{name}/main.rs`. \
                     Please specify bin.path if you want to use a non-default path."
                )));
            }
            (Located::Ambiguous, TargetKind::Binary) => {
                return Err(TextError::unreadable(format!(
                    "cannot infer path for `{name}` bin"
                )));
            }
            (
                Located::NotFound | Located::Ambiguous,
                TargetKind::Example | TargetKind::Test | TargetKind::Benchmark,
            ) => {}
            (_, TargetKind::Library) => unreachable!("a library is resolved in `library`"),
        }
    }
    assert!(
        names.len() <= candidates.len(),
        "a target is dropped or kept, never added"
    );
    Ok(names)
}

/// The targets of `kind` to resolve, in name order: every table, and, when Cargo discovers
/// this kind, each found file that no table names or points at.
fn candidates(
    directory: &Path,
    targets: &Targets,
    kind: TargetKind,
    found: &[Inferred],
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = targets
        .declared(kind)
        .map(|table| Candidate {
            name: table.name.clone(),
            path: table.path.as_ref().map(PathBuf::from),
        })
        .collect();

    if targets.discovers(kind) {
        let written_names: BTreeSet<Option<String>> = candidates
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        let paths: BTreeSet<PathBuf> = candidates
            .iter()
            .filter_map(|candidate| candidate.path.as_ref())
            .map(|path| directory.join(path))
            .collect();
        for target in found {
            let named = written_names.contains(&Some(target.name.clone()));
            let pointed_at = paths.contains(&directory.join(&target.path));
            if !named && !pointed_at {
                candidates.push(Candidate {
                    name: Some(target.name.clone()),
                    path: Some(target.path.clone()),
                });
            }
        }
    }
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    candidates
}

/// `name`, when Cargo takes it as the name of a target of `kind`.
fn checked_name(kind: TargetKind, name: Option<&str>) -> Result<&str, TextError> {
    let words = words(kind);
    let Some(name) = name else {
        return Err(TextError::unreadable(format!(
            "{} target {}.name is required",
            words.human, words.key
        )));
    };
    if name.trim().is_empty() {
        return Err(TextError::unreadable(format!(
            "{} target names cannot be empty",
            words.human
        )));
    }
    if kind == TargetKind::Binary && FORBIDDEN_BINARY_NAMES.contains(&name) {
        return Err(TextError::unreadable(format!(
            "the binary target name `{name}` is forbidden, it conflicts with cargo's build \
             directory names"
        )));
    }
    Ok(name)
}

/// Whether Cargo finds the file of `candidate`: its written `path`, the one file found under its
/// name, or in the 2015 edition a fallback path.
fn locate(
    directory: &Path,
    edition: Edition,
    kind: TargetKind,
    has_library: bool,
    candidate: &Candidate,
    found: &[Inferred],
) -> Located {
    if candidate.path.is_some() {
        return Located::Found;
    }
    let name = candidate.name.as_deref();
    let matching = found
        .iter()
        .filter(|target| Some(target.name.as_str()) == name)
        .count();
    if matching == 1 {
        return Located::Found;
    }
    let fallback = edition == Edition::Edition2015
        && name.is_some_and(|name| fallback_exists(directory, kind, name, has_library));
    match (fallback, matching) {
        (true, _) => Located::Found,
        (false, 0) => Located::NotFound,
        (false, _) => Located::Ambiguous,
    }
}

/// Whether one of the paths Cargo accepts in the 2015 edition, and warns about, is there.
///
/// A binary may be `src/<name>.rs` (with no library), `src/main.rs` or `src/bin/main.rs`, and a
/// bench named `bench` may be `src/bench.rs`.
fn fallback_exists(directory: &Path, kind: TargetKind, name: &str, has_library: bool) -> bool {
    let exists = |relative: &str| directory.join(relative).exists();
    match kind {
        TargetKind::Binary => {
            (!has_library && exists(&format!("src/{name}.rs")))
                || exists("src/main.rs")
                || exists("src/bin/main.rs")
        }
        TargetKind::Benchmark => name == "bench" && exists("src/bench.rs"),
        TargetKind::Example | TargetKind::Test => false,
        TargetKind::Library => unreachable!("a library is resolved in `library`"),
    }
}

/// Refuses two targets of `kind` with one name.
fn refuse_duplicates(kind: TargetKind, names: &[String]) -> Result<(), TextError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            let word = words(kind).duplicate;
            return Err(TextError::unreadable(format!(
                "found duplicate {word} name {name}, but all {word} targets must have a unique \
                 name"
            )));
        }
    }
    Ok(())
}
