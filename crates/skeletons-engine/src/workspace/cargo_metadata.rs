//! The one `cargo metadata` invocation this crate ever makes, and turning
//! its output into a [`Document`] or a named abort.

use std::path::Path;

use super::Network;
use super::schema::Document;
use crate::cargo;
use crate::subprocess::{self, Limits};

/// The largest `cargo metadata` output this crate ever reads, in mebibytes —
/// the abort message states the cap in this same unit, read from this one
/// constant. A workspace whose output is larger is refused outright — a
/// truncated JSON document cannot be parsed at all, so there is nothing to
/// gain from reading part of one this large.
const METADATA_OUTPUT_MEBIBYTES_MAX: u64 = 256;

/// [`METADATA_OUTPUT_MEBIBYTES_MAX`], in bytes — the only unit
/// [`Limits`] takes.
const METADATA_OUTPUT_BYTES_MAX: u64 = METADATA_OUTPUT_MEBIBYTES_MAX * 1024 * 1024;

/// The schema version this crate reads. Asked for explicitly on every call,
/// and checked against the response, so a future cargo defaulting to a new
/// schema fails loudly here rather than silently misreading a field.
const SUPPORTED_FORMAT_VERSION: u64 = 1;

/// The phrase cargo's own stderr carries when `--locked` refused to write a
/// lockfile that was missing or out of date. Recognising it can only fail
/// towards the verbatim [`ReadWorkspaceError::CargoMetadataFailed`] message,
/// never towards a wrong one: [`fetch`]'s own
/// `if stderr.contains(CARGO_LOCKED_REFUSAL)` is the only place this
/// constant is read, and its `else` arm is the generic, verbatim case — a
/// cargo that rewords this phrase falls out of the `if` and into that
/// `else`, never into a misclassification (tested by
/// `the_locked_refusal_phrase_matches_every_captured_cargo`, which checks
/// the phrase against four real captures, below).
///
/// Cargo has worded this two different ways across the toolchains this
/// crate supports. Captured 2026-09-27 with `cargo +<version> metadata
/// --format-version 1 --locked --offline` against a fresh crate, once with
/// no `Cargo.lock` and once with one made stale by an added dependency (see
/// `fixtures/cargo-1-85-missing-lockfile.stderr`,
/// `fixtures/cargo-1-85-stale-lockfile.stderr`,
/// `fixtures/cargo-1-95-missing-lockfile.stderr`, and
/// `fixtures/cargo-1-95-stale-lockfile.stderr`, each with the capturing
/// machine's path replaced by `<workspace-root>`):
///
/// - cargo 1.85.0 (this project's minimum): "the lock file `<path>` needs to
///   be updated but --locked was passed to prevent this", for both a
///   missing and a stale lockfile alike.
/// - cargo 1.95.0: "cannot create the lock file `<path>` because --locked was
///   passed to prevent this" when missing, "cannot update the lock file
///   `<path>` because --locked was passed to prevent this" when stale.
///
/// `--locked was passed to prevent this` is the phrase all four share.
const CARGO_LOCKED_REFUSAL: &str = "--locked was passed to prevent this";

/// Why the workspace could not be read at all — as opposed to a single worn
/// dependency being refused, which still leaves every other one reported.
#[derive(Debug)]
pub(crate) enum ReadWorkspaceError {
    /// `Cargo.lock` is missing or out of date, and `--locked` refused to
    /// write one.
    Lockfile,
    /// `cargo metadata` ran and exited unsuccessfully, for any other reason.
    CargoMetadataFailed { stderr: String },
    /// `cargo` (or whatever `$CARGO` names) could not be run at all.
    CargoUnavailable { detail: String },
    /// The output was too large, was not valid JSON in the expected shape,
    /// or named a format version this crate does not understand.
    MetadataUnreadable { detail: String },
}

/// Runs `$CARGO metadata --format-version 1 --locked --all-features` (with
/// `--offline` too, when `network` is [`Network::Refused`]) in `directory`,
/// and parses its output.
///
/// `directory` is used as-is — cargo itself walks up from it to find the
/// workspace root, exactly as it does for any other cargo command run from a
/// subdirectory.
pub(crate) fn fetch(directory: &Path, network: Network) -> Result<Document, ReadWorkspaceError> {
    let mut command = cargo::command();
    command.current_dir(directory).args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--all-features",
    ]);
    if network == Network::Refused {
        command.arg("--offline");
    }

    let limits = Limits {
        // No timeout: cargo may be downloading the locked sources over the
        // network, which has no fixed bound this crate could pick without
        // sometimes killing a perfectly healthy, slow fetch.
        timeout: None,
        stdout_bytes_max: METADATA_OUTPUT_BYTES_MAX,
        stderr_bytes_max: METADATA_OUTPUT_BYTES_MAX,
    };
    let finished = subprocess::run(command, &limits).map_err(|error| {
        ReadWorkspaceError::CargoUnavailable {
            detail: error.to_string(),
        }
    })?;

    if !finished.success() {
        let stderr = String::from_utf8_lossy(finished.stderr_head())
            .trim_end()
            .to_owned();
        return Err(if stderr.contains(CARGO_LOCKED_REFUSAL) {
            ReadWorkspaceError::Lockfile
        } else {
            ReadWorkspaceError::CargoMetadataFailed { stderr }
        });
    }

    match finished.stdout() {
        Err(_truncated) => Err(ReadWorkspaceError::MetadataUnreadable {
            detail: oversized_output_detail(),
        }),
        Ok(bytes) => parse(bytes),
    }
}

/// The message [`ReadWorkspaceError::MetadataUnreadable`] carries when
/// `cargo metadata`'s own output was truncated at
/// [`METADATA_OUTPUT_BYTES_MAX`] — a pure function, so its exact words are
/// pinned by a unit test with no process to spawn.
fn oversized_output_detail() -> String {
    format!(
        "cargo metadata printed more than {METADATA_OUTPUT_MEBIBYTES_MAX} MiB, the most \
         `skeletons` reads"
    )
}

/// Parses `cargo metadata --format-version 1`'s JSON output.
///
/// Split from [`fetch`] so a unit test can exercise parsing directly against
/// a committed fixture, without spawning `cargo`.
fn parse(bytes: &[u8]) -> Result<Document, ReadWorkspaceError> {
    let document: Document =
        serde_json::from_slice(bytes).map_err(|error| ReadWorkspaceError::MetadataUnreadable {
            detail: format!("parsing cargo metadata output failed: {error}"),
        })?;
    if document.version != SUPPORTED_FORMAT_VERSION {
        return Err(ReadWorkspaceError::MetadataUnreadable {
            detail: format!(
                "cargo metadata returned format version {}, but `skeletons` understands only \
                 version {SUPPORTED_FORMAT_VERSION}",
                document.version
            ),
        });
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::{CARGO_LOCKED_REFUSAL, ReadWorkspaceError, oversized_output_detail, parse};

    #[cfg(skeletons_checkout)]
    const DEMO_WORKSPACE: &str = include_str!("fixtures/demo-workspace.json");

    /// Real `cargo metadata --locked` stderr, captured from both a missing
    /// and a stale lockfile on both supported toolchains — see
    /// [`CARGO_LOCKED_REFUSAL`]'s own doc comment for how and when.
    #[cfg(skeletons_checkout)]
    const CARGO_1_85_MISSING_LOCKFILE: &str =
        include_str!("fixtures/cargo-1-85-missing-lockfile.stderr");
    #[cfg(skeletons_checkout)]
    const CARGO_1_85_STALE_LOCKFILE: &str =
        include_str!("fixtures/cargo-1-85-stale-lockfile.stderr");
    #[cfg(skeletons_checkout)]
    const CARGO_1_95_MISSING_LOCKFILE: &str =
        include_str!("fixtures/cargo-1-95-missing-lockfile.stderr");
    #[cfg(skeletons_checkout)]
    const CARGO_1_95_STALE_LOCKFILE: &str =
        include_str!("fixtures/cargo-1-95-stale-lockfile.stderr");

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_well_formed_document_parses() {
        let document = parse(DEMO_WORKSPACE.as_bytes()).expect("the fixture must parse");
        assert_eq!(document.version, 1);
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn a_future_format_version_is_refused_by_name() {
        let future = DEMO_WORKSPACE.replacen("\"version\": 1,", "\"version\": 2,", 1);
        let error = parse(future.as_bytes()).expect_err("an unsupported version must be refused");
        let ReadWorkspaceError::MetadataUnreadable { detail } = error else {
            panic!("expected MetadataUnreadable");
        };
        assert!(detail.contains('2'));
    }

    #[test]
    fn invalid_json_is_refused_as_unreadable() {
        let error = parse(b"not json at all").expect_err("invalid JSON must be refused");
        assert!(matches!(
            error,
            ReadWorkspaceError::MetadataUnreadable { .. }
        ));
    }

    #[test]
    fn the_locked_refusal_phrase_is_recognised_verbatim() {
        // Guards the constant itself against a typo: if this phrase ever
        // stops matching cargo's own wording, `fetch`'s recognition falls
        // back to the verbatim `CargoMetadataFailed` message, never to a
        // wrong one — this test only pins what the constant says.
        assert_eq!(CARGO_LOCKED_REFUSAL, "--locked was passed to prevent this");
    }

    // A missing lockfile and a stale one word cargo's refusal differently on
    // cargo 1.95.0, and cargo 1.85.0 words both the same way it words either
    // — but all four real captures share the phrase `CARGO_LOCKED_REFUSAL`
    // recognises, so every one of them must be caught.
    #[cfg(skeletons_checkout)]
    #[test]
    fn the_locked_refusal_phrase_matches_every_captured_cargo() {
        for stderr in [
            CARGO_1_85_MISSING_LOCKFILE,
            CARGO_1_85_STALE_LOCKFILE,
            CARGO_1_95_MISSING_LOCKFILE,
            CARGO_1_95_STALE_LOCKFILE,
        ] {
            assert!(
                stderr.contains(CARGO_LOCKED_REFUSAL),
                "expected {CARGO_LOCKED_REFUSAL:?} in captured stderr: {stderr}"
            );
        }
    }

    // The message states the cap in MiB, not bytes; this pins the exact
    // words a pure function builds, with no process spawned to reach them.
    #[test]
    fn the_oversized_output_message_states_the_cap_in_mebibytes() {
        assert_eq!(
            oversized_output_detail(),
            "cargo metadata printed more than 256 MiB, the most `skeletons` reads"
        );
    }
}
