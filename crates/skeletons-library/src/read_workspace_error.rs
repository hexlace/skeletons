//! Why a workspace could not be read at all.

use std::backtrace::Backtrace;
use std::error::Error;
use std::fmt;

/// Which failure it was. Private, so a new failure is not a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    LockfileNotCurrent,
    CargoMetadataFailed,
    CargoNotStarted,
    MetadataUnreadable,
}

/// Why the workspace could not be read at all, as opposed to a wearing being
/// refused, which still leaves every other wearing reported.
///
/// `Display` is a fixed phrase for each failure, with no remedy, so it can be
/// compared exactly; everything that came from outside is in
/// [`detail`](Self::detail). The four `is_*` predicates are disjoint: no two
/// are true of one error. A later version may add a failure of which none is
/// true, so a caller's last arm handles any other failure.
///
/// Each predicate names the `aborted.kind` word that `check --json` reports
/// for the same failure.
///
/// # Examples
///
/// ```no_run
/// let report = match skeletons_library::check(".") {
///     Ok(report) => report,
///     Err(error) if error.is_lockfile_not_current() => {
///         return Err("make or update Cargo.lock, then ask again".into());
///     }
///     // Cargo failed or could not be started, or its output could not be used:
///     // `detail` is what cargo or the operating system said, when either said anything.
///     Err(error) => {
///         if let Some(detail) = error.detail() {
///             eprintln!("{detail}");
///         }
///         return Err(error.into());
///     }
/// };
/// # let _ = report;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
pub struct ReadWorkspaceError {
    kind: ErrorKind,
    detail: Option<String>,
    backtrace: Backtrace,
}

impl ReadWorkspaceError {
    pub(crate) fn from_engine(error: skeletons_engine::ReadWorkspaceError) -> Self {
        use skeletons_engine::ReadWorkspaceError as Engine;
        let (kind, detail) = match error {
            Engine::Lockfile => (ErrorKind::LockfileNotCurrent, None),
            Engine::CargoMetadataFailed { stderr } => {
                (ErrorKind::CargoMetadataFailed, Some(stderr))
            }
            Engine::CargoUnavailable { detail } => (ErrorKind::CargoNotStarted, Some(detail)),
            Engine::MetadataUnreadable { detail } => (ErrorKind::MetadataUnreadable, Some(detail)),
        };
        Self {
            kind,
            detail: detail.filter(|text| !text.trim().is_empty()),
            backtrace: Backtrace::capture(),
        }
    }

    /// Whether `Cargo.lock` is missing or out of date. `check` never writes
    /// one. This is the `lockfile` word.
    #[must_use]
    pub fn is_lockfile_not_current(&self) -> bool {
        matches!(self.kind, ErrorKind::LockfileNotCurrent)
    }

    /// Whether cargo ran and failed for another reason, such as a source that
    /// was never fetched, which `check` never downloads. This is the
    /// `cargo-metadata-failed` word.
    #[must_use]
    pub fn is_cargo_metadata_failed(&self) -> bool {
        matches!(self.kind, ErrorKind::CargoMetadataFailed)
    }

    /// Whether cargo could not be started: there is no `cargo` on `PATH` and
    /// none named by `$CARGO`, or the `directory` given to `check` cannot be
    /// entered. [`detail`](Self::detail) holds the operating system's words,
    /// which do not say which. This is the `cargo-unavailable` word.
    #[must_use]
    pub fn is_cargo_not_started(&self) -> bool {
        matches!(self.kind, ErrorKind::CargoNotStarted)
    }

    /// Whether cargo's output could not be used: it was too large, was not in
    /// the expected shape, or named a format version this library does not
    /// read. This is the `metadata-unreadable` word.
    #[must_use]
    pub fn is_metadata_unreadable(&self) -> bool {
        matches!(self.kind, ErrorKind::MetadataUnreadable)
    }

    /// The text that came with the failure, whoever wrote it: cargo's standard
    /// error when cargo ran and failed, the operating system's words when cargo
    /// could not be started, what was wrong when the output could not be used.
    /// `None` when nothing came, which is always so for a lockfile that is not
    /// current.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

/// Shows the failure, its detail and the backtrace captured when the error was
/// made, which is the part of the error a bug report needs.
impl fmt::Debug for ReadWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadWorkspaceError")
            .field("kind", &self.kind)
            .field("detail", &self.detail)
            .field("backtrace", &self.backtrace)
            .finish()
    }
}

impl fmt::Display for ReadWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phrase = match self.kind {
            ErrorKind::LockfileNotCurrent => "the lockfile is missing or out of date",
            ErrorKind::CargoMetadataFailed => "cargo metadata failed",
            ErrorKind::CargoNotStarted => "cargo metadata could not be started",
            ErrorKind::MetadataUnreadable => "cargo metadata's output could not be read",
        };
        formatter.write_str(phrase)
    }
}

impl Error for ReadWorkspaceError {}
