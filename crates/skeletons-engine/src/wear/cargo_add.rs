//! Adding the skeleton as a dev-dependency by running `cargo add`.
//!
//! Cargo is the one that knows how to resolve a version, read a git
//! repository or a directory, and write a dependency into a manifest and a
//! lockfile, so `wear` asks it to rather than doing any of that itself. What
//! `wear` owns is the command it builds and what it makes of cargo's answer.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use rituals::Outcome;

use super::refusal::WearRefusal;
use super::request::Request;
use super::source::{GitReference, Source};
use crate::cargo;
use crate::subprocess::{self, Finished, Limits};

/// The most of cargo's stdout or stderr `wear` reads, in bytes: one mebibyte.
/// `cargo add` prints a few lines, so this is far more than any real run
/// needs, and it bounds what a misbehaving cargo can make this process hold.
const CARGO_ADD_OUTPUT_BYTES_MAX: u64 = 1024 * 1024;

/// The arguments that make `cargo add` add `request`'s skeleton to the
/// package `package` as a dev-dependency.
///
/// Every flag that takes a value is given as `--flag=value`, one argument, so
/// a value that begins with `-` (a url, a branch or a directory a wearer
/// typed) can never be read as an option of its own. `--rename` is given only
/// when the key differs from the crate's name: cargo writes a redundant
/// `package = "<name>"` for a rename equal to the name. The source flags
/// follow, as typed.
pub(crate) fn arguments(package: &str, request: &Request) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("add"),
        OsString::from("--dev"),
        flag_with("--package", package),
        crate_spec(request).into(),
    ];
    if request.key().as_str() != request.crate_name().as_str() {
        arguments.push(flag_with("--rename", request.key().as_str()));
    }
    arguments.extend(source_flags(request.source()));
    arguments
}

/// `<crate>`, or `<crate>@<version>` when a version was asked for.
fn crate_spec(request: &Request) -> String {
    let name = request.crate_name().as_str();
    request
        .version()
        .map_or_else(|| name.to_owned(), |version| format!("{name}@{version}"))
}

/// `flag` and `value` as the one argument `flag=value`.
fn flag_with(flag: &str, value: impl AsRef<OsStr>) -> OsString {
    let mut argument = OsString::from(flag);
    argument.push("=");
    argument.push(value);
    argument
}

/// The flags that name where `cargo add` finds the skeleton, none for the
/// registry.
fn source_flags(source: &Source) -> Vec<OsString> {
    match source {
        Source::Registry => Vec::new(),
        Source::Git { url, reference } => {
            let mut flags = vec![flag_with("--git", url)];
            match reference {
                GitReference::DefaultBranch => {}
                GitReference::Branch(branch) => flags.push(flag_with("--branch", branch)),
                GitReference::Tag(tag) => flags.push(flag_with("--tag", tag)),
                GitReference::Rev(rev) => flags.push(flag_with("--rev", rev)),
            }
            flags
        }
        Source::Path(path) => vec![flag_with("--path", path)],
    }
}

/// Runs `cargo add` for `request` in `directory`, which is the directory
/// `wear` was run in.
///
/// It runs there, rather than in the package's own directory, so a relative
/// `--path` means what the wearer typed: cargo rewrites it relative to the
/// manifest it writes. There is no timeout, as for `cargo metadata`: cargo
/// may be fetching over the network, which has no bound that would not
/// sometimes kill a healthy slow fetch. What cargo prints on success is not
/// shown.
///
/// # Errors
///
/// Returns a failure saying cargo could not be run, or what cargo said when
/// it refused.
pub(crate) fn run(directory: &Path, package: &str, request: &Request) -> Outcome {
    let mut command = cargo::command();
    command
        .current_dir(directory)
        .args(arguments(package, request));
    let limits = Limits {
        timeout: None,
        stdout_bytes_max: CARGO_ADD_OUTPUT_BYTES_MAX,
        stderr_bytes_max: CARGO_ADD_OUTPUT_BYTES_MAX,
    };
    let finished = subprocess::run(command, &limits).map_err(|error| {
        WearRefusal::CargoAddUnavailable {
            detail: error.to_string(),
        }
        .into_failure()
    })?;
    classify(&finished).map_err(WearRefusal::into_failure)
}

/// What `cargo add`'s exit says: nothing to report when it succeeded, and
/// cargo's own words when it did not.
fn classify(finished: &Finished) -> Result<(), WearRefusal> {
    if finished.success() {
        Ok(())
    } else {
        Err(WearRefusal::CargoAddFailed {
            stderr: String::from_utf8_lossy(finished.stderr_head())
                .trim_end()
                .to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{arguments, classify};
    use crate::subprocess::Finished;
    use crate::wear::refusal::WearRefusal;
    use crate::wear::request::Request;
    use crate::wear::source::{GitReference, Source};

    fn request(skeleton: &str, key: Option<&str>, source: Source) -> Request {
        Request::new(skeleton, key, source).expect("a test passes only requests it knows are valid")
    }

    fn vector(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn git(reference: GitReference) -> Source {
        Source::Git {
            url: "file:///repository".to_owned(),
            reference,
        }
    }

    #[test]
    fn a_registry_crate_with_no_version_adds_by_name() {
        assert_eq!(
            arguments("cli", &request("tidy", None, Source::Registry)),
            vector(&["add", "--dev", "--package=cli", "tidy"])
        );
    }

    #[test]
    fn a_registry_crate_with_a_version_adds_by_name_and_version() {
        assert_eq!(
            arguments("cli", &request("tidy@1.2", None, Source::Registry)),
            vector(&["add", "--dev", "--package=cli", "tidy@1.2"])
        );
    }

    #[test]
    fn a_key_other_than_the_crates_name_is_a_rename() {
        assert_eq!(
            arguments("cli", &request("tidy", Some("neat"), Source::Registry)),
            vector(&["add", "--dev", "--package=cli", "tidy", "--rename=neat"])
        );
    }

    #[test]
    fn a_key_equal_to_the_crates_name_is_not_a_rename() {
        // Cargo writes a redundant `package = "tidy"` for `--rename=tidy`.
        assert_eq!(
            arguments("cli", &request("tidy", Some("tidy"), Source::Registry)),
            arguments("cli", &request("tidy", None, Source::Registry)),
        );
    }

    #[test]
    fn a_key_that_differs_only_by_hyphen_and_underscore_is_still_a_rename() {
        // Cargo treats `foo_bar` and `foo-bar` as one name when resolving, but
        // they are two spellings in a manifest, and the one asked for is the
        // one the dependency has to be written under.
        assert_eq!(
            arguments(
                "cli",
                &request("foo-bar", Some("foo_bar"), Source::Registry)
            ),
            vector(&[
                "add",
                "--dev",
                "--package=cli",
                "foo-bar",
                "--rename=foo_bar"
            ])
        );
    }

    #[test]
    fn a_git_repository_with_no_reference_adds_by_url_alone() {
        assert_eq!(
            arguments(
                "cli",
                &request("tidy", None, git(GitReference::DefaultBranch))
            ),
            vector(&[
                "add",
                "--dev",
                "--package=cli",
                "tidy",
                "--git=file:///repository"
            ])
        );
    }

    #[test]
    fn a_git_branch_tag_and_rev_each_follow_the_url_under_their_own_flag() {
        for (reference, flag) in [
            (
                GitReference::Branch("feature".to_owned()),
                "--branch=feature",
            ),
            (GitReference::Tag("1.0.0".to_owned()), "--tag=1.0.0"),
            (GitReference::Rev("abc123".to_owned()), "--rev=abc123"),
        ] {
            assert_eq!(
                arguments("cli", &request("tidy", None, git(reference))),
                vector(&[
                    "add",
                    "--dev",
                    "--package=cli",
                    "tidy",
                    "--git=file:///repository",
                    flag
                ])
            );
        }
    }

    #[test]
    fn a_path_is_passed_as_typed() {
        let source = Source::Path(PathBuf::from("../skeletons/tidy"));

        assert_eq!(
            arguments("cli", &request("tidy", None, source)),
            vector(&[
                "add",
                "--dev",
                "--package=cli",
                "tidy",
                "--path=../skeletons/tidy"
            ])
        );
    }

    #[test]
    fn a_rename_comes_before_the_source_flags() {
        let source = Source::Path(PathBuf::from("p"));

        assert_eq!(
            arguments("cli", &request("tidy", Some("neat"), source)),
            vector(&[
                "add",
                "--dev",
                "--package=cli",
                "tidy",
                "--rename=neat",
                "--path=p"
            ])
        );
    }

    #[test]
    fn a_value_that_begins_with_a_hyphen_stays_inside_its_own_flag() {
        // As two arguments, a url, a reference or a directory typed with a
        // leading `-` would be read by cargo as an option. Joined to its flag
        // it cannot be, whichever flag it follows.
        assert_eq!(
            arguments(
                "cli",
                &request(
                    "tidy",
                    None,
                    Source::Git {
                        url: "--upload-pack=x".to_owned(),
                        reference: GitReference::Branch("-b".to_owned()),
                    }
                )
            ),
            vector(&[
                "add",
                "--dev",
                "--package=cli",
                "tidy",
                "--git=--upload-pack=x",
                "--branch=-b"
            ])
        );
        assert_eq!(
            arguments(
                "cli",
                &request("tidy", None, Source::Path(PathBuf::from("-dir")))
            ),
            vector(&["add", "--dev", "--package=cli", "tidy", "--path=-dir"])
        );
    }

    #[test]
    fn a_successful_cargo_add_has_nothing_to_report_whatever_it_printed() {
        let finished = Finished::for_test(0, b"    Adding tidy to dev-dependencies\n");

        assert_eq!(classify(&finished), Ok(()));
    }

    #[test]
    fn a_refused_cargo_add_carries_every_line_cargo_said() {
        let finished = Finished::for_test(
            101,
            b"error: could not find `x`\n\nCaused by:\n  no such file\n",
        );

        assert_eq!(
            classify(&finished),
            Err(WearRefusal::CargoAddFailed {
                stderr: "error: could not find `x`\n\nCaused by:\n  no such file".to_owned()
            })
        );
    }

    #[test]
    fn cargo_words_that_are_not_utf8_are_kept_lossily_rather_than_refused() {
        let finished = Finished::for_test(101, b"bad \xff byte");

        let Err(WearRefusal::CargoAddFailed { stderr }) = classify(&finished) else {
            panic!("a failed cargo add must be refused as CargoAddFailed");
        };

        assert_eq!(stderr, "bad \u{fffd} byte");
    }

    #[test]
    fn a_multi_line_failure_reads_as_one_line_in_the_message() {
        let finished = Finished::for_test(101, b"first\nsecond");

        let Err(refusal) = classify(&finished) else {
            panic!("a failed cargo add must be refused");
        };

        assert_eq!(refusal.message(), "cargo add failed: first\\nsecond");
    }
}
