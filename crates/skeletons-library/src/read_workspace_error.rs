//! Why a workspace could not be read at all.

use std::backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use std::path::Path;

/// Which failure it was. Private, so a new failure is not a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    LockfileNotCurrent,
    CargoMetadataFailed,
    CargoNotStarted,
    DirectoryNotFound,
    MetadataUnreadable,
}

/// Why the workspace could not be read at all, as opposed to a wearing being
/// refused, which still leaves every other wearing reported.
///
/// `Display` is a fixed phrase for each failure, with no remedy, so it can be
/// compared exactly; everything that came from outside is in
/// [`detail`](Self::detail). The five `is_*` predicates are disjoint: no two
/// are true of one error. A later version may add a failure of which none is
/// true, so a caller's last arm handles any other failure.
///
/// Each predicate names the `aborted.kind` word that `check --json` reports
/// for the same failure; two of them, a cargo that could not be started and a
/// directory that was not found, share the word `cargo-unavailable`.
///
/// # Examples
///
/// ```no_run
/// let report = match skeletons_library::check(".") {
///     Ok(report) => report,
///     Err(error) if error.is_lockfile_not_current() => {
///         return Err("make or update Cargo.lock, then ask again".into());
///     }
///     // Cargo failed or could not be started, there was no directory to start it in,
///     // or its output could not be used: `detail` is what cargo or the operating
///     // system said, when either said anything.
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
    pub(crate) fn from_engine(
        error: skeletons_engine::ReadWorkspaceError,
        directory: &Path,
    ) -> Self {
        use skeletons_engine::ReadWorkspaceError as Engine;
        let (kind, detail) = match error {
            Engine::Lockfile => (ErrorKind::LockfileNotCurrent, None),
            Engine::CargoMetadataFailed { stderr } => {
                (ErrorKind::CargoMetadataFailed, Some(stderr))
            }
            Engine::CargoUnavailable { detail } => (not_started_kind(directory), Some(detail)),
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

    /// Whether cargo could not be started although the `directory` given to
    /// `check` was a directory when looked at afterwards: there is no `cargo`
    /// on `PATH` and none named by `$CARGO`, a program that cannot be run, or a
    /// directory that cannot be entered. [`detail`](Self::detail) holds
    /// `failed to run: ` followed by the operating system's words, which do not
    /// say which. This is the
    /// `cargo-unavailable` word.
    #[must_use]
    pub fn is_cargo_not_started(&self) -> bool {
        matches!(self.kind, ErrorKind::CargoNotStarted)
    }

    /// Whether cargo could not be started and no directory was found at the
    /// `directory` given to `check`: nothing exists there, something other than
    /// a directory does, or the path could not be looked at. The look is made
    /// after the failure and describes the path as it was then. This is the
    /// `cargo-unavailable` word, as `check --json` reports it.
    #[must_use]
    pub fn is_directory_not_found(&self) -> bool {
        matches!(self.kind, ErrorKind::DirectoryNotFound)
    }

    /// Whether cargo's output could not be used: it was too large, was not in
    /// the expected shape, or named a format version this library does not
    /// read. This is the `metadata-unreadable` word.
    #[must_use]
    pub fn is_metadata_unreadable(&self) -> bool {
        matches!(self.kind, ErrorKind::MetadataUnreadable)
    }

    /// The text that came with the failure, whoever wrote it: cargo's standard
    /// error when cargo ran and failed, `failed to run: ` followed by the
    /// operating system's words when cargo could not be started or the
    /// directory was not found, what was wrong when the output could not be
    /// used. `None` when nothing came, which is always
    /// so for a lockfile that is not current.
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
            ErrorKind::DirectoryNotFound => "the directory to read was not found",
            ErrorKind::MetadataUnreadable => "cargo metadata's output could not be read",
        };
        formatter.write_str(phrase)
    }
}

impl Error for ReadWorkspaceError {}

/// Which of the two failures a cargo that could not be started is, decided by
/// one look at `directory`.
///
/// The look comes after cargo failed to start. It is for the report and
/// guards nothing: the operating system gives the same error for a program it
/// cannot find as for a directory it cannot enter or does not have, so the
/// error alone cannot tell them apart. A directory that exists but cannot be
/// entered reads as a cargo that could not be started, because nothing in the
/// standard library tells it from a program that may not be run.
fn not_started_kind(directory: &Path) -> ErrorKind {
    if directory.is_dir() {
        ErrorKind::CargoNotStarted
    } else {
        ErrorKind::DirectoryNotFound
    }
}
