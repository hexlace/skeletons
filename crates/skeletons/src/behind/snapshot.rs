//! Fetching one remote's own snapshot at one commit — the head side of a
//! branch or default-branch pin's own directory-scoped `behind` comparison.
//! `behind` creates a temporary bare repository of its own to fetch into,
//! reads every requested directory's own tree back out of it, then removes
//! it. The wearer's own repository and Cargo's own checkout are never
//! touched by anything in this module; nothing fetched here is kept between
//! runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::git::{self, Locale, ObjectId, RepositoryPrefix};
use crate::subprocess::Finished;

use super::temporary_directory::TemporaryDirectory;

/// How long the one network call this module makes — fetching exactly one
/// commit's own snapshot, `--depth=1 --filter=blob:none` — waits before
/// being treated as unreachable. Larger than [`crate::git::LOCAL_GIT_TIMEOUT`]
/// on purpose: a depth-1 fetch transfers whole trees (and, where the server
/// cannot filter blobs, their blobs too), which is more than a bare ref
/// list ever is.
const SNAPSHOT_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Every requested directory's own tree at one remote's own head, once
/// fetched — `None` for a directory that does not exist there at all — or
/// why they could not be read.
pub(crate) enum SnapshotAnswer {
    Trees(BTreeMap<RepositoryPrefix, Option<ObjectId>>),
    /// A local operation of this crate's own — creating or initializing the
    /// temporary repository itself — failed, before any network request was
    /// even attempted.
    LocalFailure {
        detail: String,
    },
    Unreachable {
        detail: String,
    },
    Malformed {
        detail: String,
    },
}

/// Fetches `head` alone from `url` into a fresh temporary bare repository
/// created under `parent` (production always passes [`std::env::temp_dir`];
/// `slot` names it so two snapshot queries running concurrently never
/// collide), reads every one of `prefixes`' own tree at that commit out of
/// it, then removes the temporary repository.
pub(crate) fn directory_trees(
    url: &str,
    head: &ObjectId,
    prefixes: &BTreeSet<RepositoryPrefix>,
    parent: &Path,
    slot: usize,
) -> SnapshotAnswer {
    let temporary = match TemporaryDirectory::create(parent, slot) {
        Ok(temporary) => temporary,
        Err(error) => {
            return SnapshotAnswer::LocalFailure {
                detail: temporary_directory_failure_detail(url, &error.to_string()),
            };
        }
    };
    if let Err(detail) = init_bare_repository(temporary.path()) {
        return SnapshotAnswer::LocalFailure {
            detail: temporary_directory_failure_detail(url, &detail),
        };
    }
    if let Err(detail) = fetch_head(temporary.path(), url, head) {
        return SnapshotAnswer::Unreachable {
            detail: format!("could not reach {url}: {detail}"),
        };
    }

    let mut trees = BTreeMap::new();
    for prefix in prefixes {
        match rev_parse_tree(temporary.path(), head, prefix) {
            Ok(tree) => {
                trees.insert(prefix.clone(), tree);
            }
            Err(detail) => {
                return SnapshotAnswer::Malformed {
                    detail: format!("unexpected answer from {url}: {detail}"),
                };
            }
        }
    }
    SnapshotAnswer::Trees(trees)
}

fn temporary_directory_failure_detail(url: &str, detail: &str) -> String {
    format!("`skeletons` could not create a temporary directory to fetch {url} into: {detail}")
}

/// `git init --bare --quiet --template= <repository>`: a fresh, empty
/// repository with no hooks or templates copied in, and — through
/// [`git::command_for_new_repository`] — none of a wearer's own
/// `GIT_DEFAULT_HASH`/`GIT_DEFAULT_REF_FORMAT` deciding its object format or
/// ref layout.
fn init_bare_repository(repository: &Path) -> Result<(), String> {
    let mut command = git::command_for_new_repository(Locale::Fixed);
    let repository_argument = repository.to_string_lossy().into_owned();
    command.args([
        "init",
        "--bare",
        "--quiet",
        "--template=",
        &repository_argument,
    ]);
    let finished = run_local(command)?;
    if finished.success() {
        Ok(())
    } else {
        Err(git::diagnostic(finished.stderr_head()))
    }
}

/// `git fetch --quiet --no-tags --depth=1 --filter=blob:none <url> <head>`,
/// with hooks and automatic maintenance turned off (`core.hooksPath=/dev/null
/// -c maintenance.auto=false -c gc.auto=0`) so nothing runs in the
/// background against a repository this function is about to remove.
fn fetch_head(repository: &Path, url: &str, head: &ObjectId) -> Result<(), String> {
    let mut command = git::command_for_new_repository(Locale::Fixed);
    let head_argument = head.to_string();
    command.current_dir(repository).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "maintenance.auto=false",
        "-c",
        "gc.auto=0",
        "fetch",
        "--quiet",
        "--no-tags",
        "--depth=1",
        "--filter=blob:none",
        url,
        &head_argument,
    ]);
    let finished = run_remote(command)?;
    if finished.success() {
        Ok(())
    } else {
        Err(git::diagnostic(finished.stderr_head()))
    }
}

/// `git rev-parse --verify --quiet <head>:<prefix>`: the tree `prefix` names
/// at `head`, or `None` when nothing exists there — `--quiet` turns that
/// case into a bare exit 1 with no output, rather than a diagnostic this
/// function would otherwise have to tell apart from a genuine failure.
fn rev_parse_tree(
    repository: &Path,
    head: &ObjectId,
    prefix: &RepositoryPrefix,
) -> Result<Option<ObjectId>, String> {
    let mut command = git::command_for_new_repository(Locale::Fixed);
    let spec = format!("{head}:{}", prefix.as_str());
    command
        .current_dir(repository)
        .args(["rev-parse", "--verify", "--quiet", &spec]);
    let finished = run_local(command)?;
    classify_rev_parse_tree(&finished)
}

/// The classifier behind [`rev_parse_tree`]'s own reading of `rev-parse
/// --verify --quiet`'s answer.
fn classify_rev_parse_tree(finished: &Finished) -> Result<Option<ObjectId>, String> {
    let Ok(stdout) = finished.stdout() else {
        return Err("printed more than 16 MiB, the most `skeletons` reads".to_owned());
    };
    match finished.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(stdout);
            let Some(first_line) = text.lines().next() else {
                return Err("printed no answer at all".to_owned());
            };
            ObjectId::parse(first_line)
                .map(Some)
                .ok_or_else(|| format!("printed a tree id `skeletons` cannot read: {first_line}"))
        }
        Some(1) if stdout.iter().all(u8::is_ascii_whitespace) => Ok(None),
        _ => Err(git::diagnostic(finished.stderr_head())),
    }
}

fn run_local(command: Command) -> Result<Finished, String> {
    git::run_local(command).map_err(|error| error.to_string())
}

fn run_remote(command: Command) -> Result<Finished, String> {
    git::run_bounded(command, SNAPSHOT_FETCH_TIMEOUT).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::classify_rev_parse_tree;
    use crate::subprocess::{Limits, run};
    use std::process::Command;

    /// Runs `sh -c script`, under no bound at all, for a hand-built
    /// [`Finished`](crate::subprocess::Finished) this module's own
    /// classifier can be driven against without spawning `git` at all.
    fn finished_from_shell(script: &str) -> crate::subprocess::Finished {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        run(
            command,
            &Limits {
                timeout: None,
                stdout_bytes_max: u64::MAX,
                stderr_bytes_max: u64::MAX,
            },
        )
        .expect("sh must run")
    }

    #[test]
    fn exit_zero_with_one_object_id_line_reads_the_tree() {
        let finished = finished_from_shell("printf '8a9a2221e545e17fae7676301faca75dd3d411f1\\n'");
        let tree = classify_rev_parse_tree(&finished)
            .expect("must parse")
            .expect("must be present");
        assert_eq!(tree.to_string(), "8a9a2221e545e17fae7676301faca75dd3d411f1");
    }

    #[test]
    fn exit_one_with_empty_output_reads_as_absent() {
        let finished = finished_from_shell("exit 1");
        let tree = classify_rev_parse_tree(&finished).expect("must classify");
        assert_eq!(tree, None);
    }

    #[test]
    fn exit_one_with_whitespace_only_output_still_reads_as_absent() {
        let finished = finished_from_shell("printf '\\n'; exit 1");
        let tree = classify_rev_parse_tree(&finished).expect("must classify");
        assert_eq!(tree, None);
    }

    #[test]
    fn exit_one_with_real_output_is_malformed() {
        let finished = finished_from_shell("printf 'unexpected'; exit 1");
        assert!(classify_rev_parse_tree(&finished).is_err());
    }

    #[test]
    fn exit_zero_with_unparseable_output_is_malformed() {
        let finished = finished_from_shell("printf 'not-an-object-id\\n'");
        assert!(classify_rev_parse_tree(&finished).is_err());
    }

    #[test]
    fn any_other_exit_code_is_malformed() {
        let finished = finished_from_shell("printf 'fatal: broken\\n' 1>&2; exit 128");
        assert!(classify_rev_parse_tree(&finished).is_err());
    }
}
