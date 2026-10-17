//! Why a workspace could not be read at all.

use std::backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use std::path::Path;

use skeletons_engine::ReadWithoutCargoError;

/// Which failure it was. Private, so a new failure is not a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    LockfileNotCurrent,
    CargoMetadataFailed,
    CargoNotStarted,
    DirectoryNotFound,
    MetadataUnreadable,
    ManifestOrLockfileUnreadable,
    UnsupportedWithoutCargo,
}

/// Why the workspace could not be read at all, as opposed to a wearing being
/// refused, which still leaves every other wearing reported.
///
/// Both [`check`](crate::check) and
/// [`check_without_cargo`](crate::check_without_cargo) return it. `Display` is
/// a fixed phrase for each failure, with no remedy, so it can be compared
/// exactly; everything that came from outside is in [`detail`](Self::detail).
/// The seven `is_*` predicates are disjoint: no two are true of one error. A
/// later version may add a failure of which none is true, so a caller's last
/// arm handles any other failure.
///
/// Five of the predicates name the `aborted.kind` word that `check --json`
/// reports for the same failure; two of them, a cargo that could not be
/// started and a directory that was not found, share the word
/// `cargo-unavailable`. The other two,
/// [`is_manifest_or_lockfile_unreadable`](Self::is_manifest_or_lockfile_unreadable)
/// and [`is_unsupported_without_cargo`](Self::is_unsupported_without_cargo),
/// are failures only `check_without_cargo` has, and the command line reads
/// through cargo, so it has no word for them. Of the other five,
/// `check_without_cargo` makes true only
/// [`is_lockfile_not_current`](Self::is_lockfile_not_current) and
/// [`is_directory_not_found`](Self::is_directory_not_found), since it runs no
/// cargo.
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
        Self::new(kind, detail)
    }

    pub(crate) fn from_engine_without_cargo(error: ReadWithoutCargoError) -> Self {
        let (kind, detail) = match error {
            ReadWithoutCargoError::Lockfile => (ErrorKind::LockfileNotCurrent, None),
            ReadWithoutCargoError::DirectoryNotFound { detail } => {
                (ErrorKind::DirectoryNotFound, Some(detail))
            }
            ReadWithoutCargoError::ManifestUnreadable { detail } => {
                (ErrorKind::ManifestOrLockfileUnreadable, Some(detail))
            }
            ReadWithoutCargoError::Unsupported { detail } => {
                (ErrorKind::UnsupportedWithoutCargo, Some(detail))
            }
        };
        Self::new(kind, detail)
    }

    fn new(kind: ErrorKind, detail: Option<String>) -> Self {
        Self {
            kind,
            detail: detail.filter(|text| !text.trim().is_empty()),
            backtrace: Backtrace::capture(),
        }
    }

    /// Whether `Cargo.lock` is missing or out of date. Neither `check` nor
    /// `check_without_cargo` writes one. This is the `lockfile` word.
    ///
    /// For `check_without_cargo` it is also true when the manifests have
    /// outgrown the lock in a way a reader of the files can see, such as a
    /// dependency added and not locked.
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
    /// say which. This is the `cargo-unavailable` word.
    #[must_use]
    pub fn is_cargo_not_started(&self) -> bool {
        matches!(self.kind, ErrorKind::CargoNotStarted)
    }

    /// Whether no directory was found at the `directory` given to `check` or
    /// `check_without_cargo`: nothing exists there, something other than a
    /// directory does, or the path could not be looked at.
    ///
    /// For `check` this is cargo that could not be started, and the look is
    /// made after the failure and describes the path as it was then; it is the
    /// `cargo-unavailable` word, as `check --json` reports it. For
    /// `check_without_cargo` the directory is looked at first, and
    /// [`detail`](Self::detail) holds the directory and the operating system's
    /// words.
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

    /// Whether `check_without_cargo` could not read a manifest, the lockfile
    /// or a cargo configuration file, or found one cargo would refuse too: a
    /// file that is missing, is not TOML or not in cargo's shape, a workspace
    /// member or path dependency with no `Cargo.toml`, a lock entry that names
    /// no package, a package with no target, no manifest at or above the
    /// directory, a configuration file whose `paths` is not an array or
    /// whose `patch` is not a table, or a `git` dependency whose URL is not
    /// one cargo reads, such as `git@github.com:foo/bar.git`.
    /// [`detail`](Self::detail) names the file and what was wrong. `check`
    /// never makes this true. The command line has no word for it.
    #[must_use]
    pub fn is_manifest_or_lockfile_unreadable(&self) -> bool {
        matches!(self.kind, ErrorKind::ManifestOrLockfileUnreadable)
    }

    /// Whether the workspace uses something only cargo can read, so
    /// `check_without_cargo` did not guess: `[patch]`, `[replace]`,
    /// `package.workspace`, one package under two dependency keys, two path
    /// packages with one name and version, a lockfile in a format other than 3
    /// or 4, an optional dependency of a package outside the workspace, or of a
    /// path package it reaches, that asks a worn package for a version its
    /// locked one does not meet and cargo cannot add beside it (for a registry
    /// package, one its compatible range could meet; for a path or git package,
    /// any), and in a cargo configuration file a non-empty `paths`, a `[patch]`
    /// with an entry or any `include`. [`detail`](Self::detail) names the
    /// manifest, lockfile or configuration file and what it uses. `check` reads
    /// such a workspace where cargo is available, except for that optional
    /// dependency once a feature enables it, where cargo cannot resolve the
    /// workspace and `check` fails with
    /// [`is_cargo_metadata_failed`](Self::is_cargo_metadata_failed). The
    /// function does not resolve features, so it refuses that dependency
    /// whenever a feature could enable it, and `check` reads the workspace
    /// where nothing does. This never becomes true for `check`. The command
    /// line has no word for it.
    #[must_use]
    pub fn is_unsupported_without_cargo(&self) -> bool {
        matches!(self.kind, ErrorKind::UnsupportedWithoutCargo)
    }

    /// The text that came with the failure, whoever wrote it: cargo's standard
    /// error when cargo ran and failed, `failed to run: ` followed by the
    /// operating system's words when cargo could not be started or the
    /// directory was not found, what was wrong when the output could not be
    /// used, and for `check_without_cargo` the file and what was wrong with
    /// it. `None` when nothing came, which is always so for a lockfile that is
    /// not current.
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
            ErrorKind::ManifestOrLockfileUnreadable => {
                "a manifest or the lockfile could not be read"
            }
            ErrorKind::UnsupportedWithoutCargo => {
                "the workspace uses something only cargo can read"
            }
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
