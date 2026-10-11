//! Asking a git remote, read-only, through `git ls-remote` — always the real
//! subprocess: unlike crates.io, no test-only seam exists here, because
//! every git remote a test needs is a real, local repository reached over a
//! `file://` URL (see `ritual/tests/support/git.rs`), so the real code path
//! is exactly what a test already exercises.

use std::process::Command;
use std::time::Duration;

use crate::git::{self, Locale, ObjectId};
use crate::subprocess::Finished;

/// How long a `behind` git query waits before it is treated as unreachable.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);

/// Every tag on a remote, or why they could not be listed.
pub(crate) enum TagsAnswer {
    Tags(Vec<String>),
    Unreachable { detail: String },
    Malformed { detail: String },
}

/// Lists every tag `git ls-remote --tags --refs <url>` reports.
pub(crate) fn list_tags(url: &str) -> TagsAnswer {
    let mut command = ls_remote_command();
    command.args(["--tags", "--refs", url]);
    match run(command) {
        Err(detail) => TagsAnswer::Unreachable { detail },
        Ok(finished) => {
            if !finished.success() {
                return TagsAnswer::Unreachable {
                    detail: git::diagnostic(finished.stderr_head()),
                };
            }
            match finished.stdout() {
                Err(_truncated) => TagsAnswer::Malformed {
                    detail: "the tag list was too large to read in full".to_owned(),
                },
                Ok(bytes) => {
                    let stdout = String::from_utf8_lossy(bytes);
                    TagsAnswer::Tags(parse_refs(&stdout))
                }
            }
        }
    }
}

/// A branch's own remote head, or why it could not be read.
pub(crate) enum BranchAnswer {
    Head(ObjectId),
    /// `git ls-remote --exit-code` exited 2: the branch no longer exists.
    Missing,
    Unreachable {
        detail: String,
    },
    Malformed {
        detail: String,
    },
}

/// Asks `branch`'s own current head on `url`'s remote.
pub(crate) fn branch_head(url: &str, branch: &str) -> BranchAnswer {
    let reference = format!("refs/heads/{branch}");
    let mut command = ls_remote_command();
    command.args(["--exit-code", url, &reference]);
    match run(command) {
        Err(detail) => BranchAnswer::Unreachable { detail },
        Ok(finished) => {
            if finished.success() {
                match finished.stdout() {
                    Err(_truncated) => BranchAnswer::Malformed {
                        detail: "the branch's own remote head was too large to read in full"
                            .to_owned(),
                    },
                    Ok(bytes) => {
                        let stdout = String::from_utf8_lossy(bytes);
                        parse_single_ref_sha(&stdout).map_or_else(
                            || BranchAnswer::Malformed {
                                detail: "the branch's own remote head could not be read".to_owned(),
                            },
                            BranchAnswer::Head,
                        )
                    }
                }
            } else if finished.code() == Some(2) {
                BranchAnswer::Missing
            } else {
                BranchAnswer::Unreachable {
                    detail: git::diagnostic(finished.stderr_head()),
                }
            }
        }
    }
}

/// The remote's own default branch, its own name (when the server names it)
/// and its current head — or why it could not be read.
pub(crate) enum DefaultBranchAnswer {
    Head {
        branch: Option<String>,
        sha: ObjectId,
    },
    Unreachable {
        detail: String,
    },
    Malformed {
        detail: String,
    },
}

/// Asks `url`'s remote which branch `HEAD` points at, and that branch's own
/// current head, via `git ls-remote --symref <url> HEAD`.
pub(crate) fn default_branch_head(url: &str) -> DefaultBranchAnswer {
    let mut command = ls_remote_command();
    command.args(["--symref", url, "HEAD"]);
    match run(command) {
        Err(detail) => DefaultBranchAnswer::Unreachable { detail },
        Ok(finished) => {
            if !finished.success() {
                return DefaultBranchAnswer::Unreachable {
                    detail: git::diagnostic(finished.stderr_head()),
                };
            }
            match finished.stdout() {
                Err(_truncated) => DefaultBranchAnswer::Malformed {
                    detail: "the default branch's own answer was too large to read in full"
                        .to_owned(),
                },
                Ok(bytes) => {
                    let stdout = String::from_utf8_lossy(bytes);
                    let (branch, sha) = parse_symref_head(&stdout);
                    sha.map_or_else(
                        || DefaultBranchAnswer::Malformed {
                            detail: "the default branch's own remote head could not be read"
                                .to_owned(),
                        },
                        |sha| DefaultBranchAnswer::Head { branch, sha },
                    )
                }
            }
        }
    }
}

/// A bare `git ls-remote`, with no credential prompt possible, the
/// repository-redirecting and pathspec variables removed (`git::command`),
/// and the running user's own git configuration otherwise honoured —
/// credentials and `insteadOf` rewrites live there, and this crate never
/// overrides them.
fn ls_remote_command() -> Command {
    let mut command = git::command(Locale::Fixed);
    command.arg("ls-remote");
    command
}

/// Runs `command` under this module's bounded limits, turning a failure to
/// run it at all into the one detail string every "unreachable" answer
/// carries.
fn run(command: Command) -> Result<Finished, String> {
    git::run_bounded(command, REMOTE_TIMEOUT).map_err(|error| error.to_string())
}

/// Parses `--tags --refs`'s own output (`<sha>\trefs/tags/<name>` per line,
/// no `^{}` peel lines) into the tag names alone — `behind` never needs a
/// tag's own sha, only whether its name parses as a newer version. A line
/// that does not match the expected shape is skipped, never treated as a
/// whole-response failure: a git server's own well-formed output is trusted
/// further than a third-party HTTP body is.
fn parse_refs(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let (sha, reference) = line.split_once('\t')?;
            ObjectId::parse(sha)?;
            reference.strip_prefix("refs/tags/").map(str::to_owned)
        })
        .collect()
}

/// Parses a single-ref `ls-remote` answer (`<sha>\t<ref>`) into its own sha.
fn parse_single_ref_sha(stdout: &str) -> Option<ObjectId> {
    stdout.lines().find_map(|line| {
        let (sha, _reference) = line.split_once('\t')?;
        ObjectId::parse(sha)
    })
}

/// Parses `--symref <url> HEAD`'s own two-line answer
/// (`ref: refs/heads/<branch>\tHEAD` then `<sha>\tHEAD`) into the branch name
/// the server named, when it did, and the head's own sha.
fn parse_symref_head(stdout: &str) -> (Option<String>, Option<ObjectId>) {
    let mut branch = None;
    let mut sha = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("ref: ") {
            if let Some((reference, _head)) = rest.split_once('\t') {
                branch = reference.strip_prefix("refs/heads/").map(str::to_owned);
            }
        } else if let Some((candidate, _head)) = line.split_once('\t') {
            if let Some(object_id) = ObjectId::parse(candidate) {
                sha = Some(object_id);
            }
        }
    }
    (branch, sha)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{parse_refs, parse_single_ref_sha, parse_symref_head};
    use crate::git::ObjectId;

    const SHA_A: &str = "3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39";
    const SHA_B: &str = "8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f";

    /// `text` parsed as an [`ObjectId`], for a test's own expected value.
    fn oid(text: &str) -> ObjectId {
        ObjectId::parse(text).expect("a well-formed test object id")
    }

    #[test]
    fn parse_refs_reads_every_tag_name() {
        let stdout = format!("{SHA_A}\trefs/tags/v1.0.0\n{SHA_B}\trefs/tags/v1.1.0\n");
        assert_eq!(
            parse_refs(&stdout),
            vec!["v1.0.0".to_owned(), "v1.1.0".to_owned()]
        );
    }

    #[test]
    fn parse_refs_skips_a_line_that_is_not_a_tag_ref() {
        let stdout = format!("{SHA_A}\trefs/heads/main\n{SHA_B}\trefs/tags/v1.0.0\n");
        assert_eq!(parse_refs(&stdout), vec!["v1.0.0".to_owned()]);
    }

    #[test]
    fn parse_refs_skips_a_line_with_a_malformed_sha() {
        let stdout = "not-a-sha\trefs/tags/v1.0.0\n";
        assert_eq!(parse_refs(stdout), Vec::<String>::new());
    }

    #[test]
    fn parse_refs_on_empty_output_is_an_empty_list() {
        assert_eq!(parse_refs(""), Vec::<String>::new());
    }

    #[test]
    fn parse_single_ref_sha_reads_the_first_matching_line() {
        let stdout = format!("{SHA_A}\trefs/heads/main\n");
        assert_eq!(parse_single_ref_sha(&stdout), Some(oid(SHA_A)));
    }

    #[test]
    fn parse_single_ref_sha_on_malformed_output_is_none() {
        assert_eq!(parse_single_ref_sha("garbage, no tab"), None);
        assert_eq!(parse_single_ref_sha(""), None);
    }

    #[test]
    fn parse_symref_head_reads_the_branch_name_and_sha() {
        let stdout = format!("ref: refs/heads/main\tHEAD\n{SHA_A}\tHEAD\n");
        assert_eq!(
            parse_symref_head(&stdout),
            (Some("main".to_owned()), Some(oid(SHA_A)))
        );
    }

    #[test]
    fn parse_symref_head_without_a_ref_line_still_reads_the_sha() {
        // A server that does not send `ref:` at all (`--symref` unsupported)
        // still lets the sha through; only the branch name is unknown.
        let stdout = format!("{SHA_A}\tHEAD\n");
        assert_eq!(parse_symref_head(&stdout), (None, Some(oid(SHA_A))));
    }

    #[test]
    fn parse_symref_head_on_malformed_output_finds_neither() {
        assert_eq!(parse_symref_head("garbage"), (None, None));
        assert_eq!(parse_symref_head(""), (None, None));
    }

    proptest! {
        /// None of these parsers ever panic, whatever bytes a remote
        /// (adversarial, truncated, or simply a different program entirely)
        /// sends back.
        #[test]
        fn parsers_never_panic_on_arbitrary_input(stdout in ".*") {
            let _refs = parse_refs(&stdout);
            let _sha = parse_single_ref_sha(&stdout);
            let _symref = parse_symref_head(&stdout);
        }
    }
}
