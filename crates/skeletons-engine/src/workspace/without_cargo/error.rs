//! Why a workspace could not be read without Cargo.

use std::fmt;
use std::path::Path;

use super::cargo_cannot_load::CargoCannotLoad;
use super::text_error::{TextError, TextErrorKind};

/// Why a workspace could not be read from its manifests and lockfile.
///
/// This is the whole-workspace failure, as opposed to a single wearing that is refused and leaves
/// every other one reported. The variants are disjoint: each failure is exactly one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadWithoutCargoError {
    /// `Cargo.lock` is missing or out of date: the abort `cargo metadata --locked` gives for it.
    Lockfile,
    /// The directory asked about cannot be read: nothing is there, it is not a directory, or it
    /// is not readable.
    DirectoryNotFound {
        /// The directory and what the operating system said.
        detail: String,
    },
    /// A manifest, the lockfile or a configuration file cannot be read, or Cargo would refuse it
    /// too.
    ManifestUnreadable {
        /// The file and what was wrong with it.
        detail: String,
    },
    /// Cargo reads the workspace and this reader does not follow something in it, in a manifest
    /// or in a configuration file.
    Unsupported {
        /// The file and the feature.
        detail: String,
    },
}

impl ReadWithoutCargoError {
    /// An unreadable manifest or lockfile, or one Cargo refuses.
    pub(super) fn unreadable(detail: impl Into<String>) -> Self {
        Self::ManifestUnreadable {
            detail: detail.into(),
        }
    }

    /// A workspace that uses something only Cargo reads.
    pub(super) fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported {
            detail: detail.into(),
        }
    }

    /// A file that could not be read from the file system.
    pub(super) fn file(path: &Path, error: &std::io::Error) -> Self {
        Self::unreadable(format!("{} could not be read: {error}", path.display()))
    }

    /// The error for a file whose text was refused, with the file named in front.
    pub(super) fn text(path: &Path, error: &TextError) -> Self {
        let detail = format!("{}: {}", path.display(), error.detail());
        match error.kind() {
            TextErrorKind::Unreadable => Self::unreadable(detail),
            TextErrorKind::Unsupported => Self::unsupported(detail),
        }
    }
}

/// A read of one manifest that failed: the error to report, and the shape Cargo fails on too when
/// the failure is one this reader names ([`CargoCannotLoad`]).
///
/// A caller that reports the failure turns it into the [`ReadWithoutCargoError`]. A caller that
/// may pass over a manifest Cargo cannot load matches on the shape, with no wildcard, and
/// propagates the error for any manifest without one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManifestFailure {
    error: ReadWithoutCargoError,
    cargo_cannot_load: Option<CargoCannotLoad>,
}

impl ManifestFailure {
    /// A failure that is no shape Cargo is known to fail on too.
    pub(super) const fn refused(error: ReadWithoutCargoError) -> Self {
        Self {
            error,
            cargo_cannot_load: None,
        }
    }

    /// A manifest that could not be read from the file system. One that is not there is
    /// [`CargoCannotLoad::Missing`]; any other failure to read it is not a shape.
    pub(super) fn file(path: &Path, error: &std::io::Error) -> Self {
        let cargo_cannot_load =
            (error.kind() == std::io::ErrorKind::NotFound).then_some(CargoCannotLoad::Missing);
        Self {
            error: ReadWithoutCargoError::file(path, error),
            cargo_cannot_load,
        }
    }

    /// A manifest whose text was refused, with the file named in front and the shape the text is
    /// when it is one.
    pub(super) fn text(path: &Path, error: &TextError) -> Self {
        assert_ne!(
            error.cargo_cannot_load(),
            Some(CargoCannotLoad::Missing),
            "a text error is never a missing file"
        );
        Self {
            error: ReadWithoutCargoError::text(path, error),
            cargo_cannot_load: error.cargo_cannot_load(),
        }
    }

    /// The shape Cargo fails on too that this failure is, or `None` for any other failure.
    #[must_use]
    pub(super) const fn cargo_cannot_load(&self) -> Option<CargoCannotLoad> {
        self.cargo_cannot_load
    }
}

impl From<ManifestFailure> for ReadWithoutCargoError {
    fn from(failure: ManifestFailure) -> Self {
        failure.error
    }
}

impl fmt::Display for ReadWithoutCargoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lockfile => formatter.write_str("Cargo.lock is missing or out of date"),
            Self::DirectoryNotFound { detail } => {
                write!(formatter, "the directory cannot be read: {detail}")
            }
            Self::ManifestUnreadable { detail } => {
                write!(
                    formatter,
                    "a manifest or the lockfile cannot be read: {detail}"
                )
            }
            Self::Unsupported { detail } => {
                write!(
                    formatter,
                    "the workspace uses something only cargo reads: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for ReadWithoutCargoError {}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ManifestFailure, ReadWithoutCargoError};
    use crate::workspace::without_cargo::cargo_cannot_load::CargoCannotLoad;
    use crate::workspace::without_cargo::text_error::TextError;

    #[test]
    fn a_refused_text_keeps_its_kind_and_names_the_file() {
        // An unreadable text must become the unreadable variant and an unsupported one the
        // unsupported variant, each with the file in front of the sentence.
        let path = Path::new("/w/Cargo.toml");

        assert_eq!(
            ReadWithoutCargoError::text(path, &TextError::unreadable("bad key")),
            ReadWithoutCargoError::ManifestUnreadable {
                detail: "/w/Cargo.toml: bad key".to_owned()
            }
        );
        assert_eq!(
            ReadWithoutCargoError::text(path, &TextError::unsupported("format 2")),
            ReadWithoutCargoError::Unsupported {
                detail: "/w/Cargo.toml: format 2".to_owned()
            }
        );
    }

    #[test]
    fn a_manifest_that_is_not_there_fails_as_missing_and_any_other_file_error_as_none() {
        // The shape is set from the kind of the file system's error: only `NotFound` says the
        // manifest is gone. A permission failure says nothing about Cargo, so it carries none.
        let path = Path::new("/w/Cargo.toml");
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        assert_eq!(
            ManifestFailure::file(path, &gone).cargo_cannot_load(),
            Some(CargoCannotLoad::Missing)
        );
        assert_eq!(
            ManifestFailure::file(path, &denied).cargo_cannot_load(),
            None
        );
    }

    #[test]
    fn a_failure_keeps_the_error_to_report_and_the_shape_of_its_text() {
        // The failure turns into the same error `ReadWithoutCargoError::text` gives, and a text
        // error's shape is kept beside it.
        let path = Path::new("/w/Cargo.toml");
        let shaped = TextError::unreadable_as(CargoCannotLoad::GitAndPath, "ambiguous");
        let plain = TextError::unsupported("format 2");

        let failure = ManifestFailure::text(path, &shaped);
        assert_eq!(
            failure.cargo_cannot_load(),
            Some(CargoCannotLoad::GitAndPath)
        );
        assert_eq!(
            ReadWithoutCargoError::from(failure),
            ReadWithoutCargoError::text(path, &shaped)
        );
        assert_eq!(
            ManifestFailure::text(path, &plain).cargo_cannot_load(),
            None
        );
        assert_eq!(
            ManifestFailure::refused(ReadWithoutCargoError::Lockfile).cargo_cannot_load(),
            None
        );
    }

    #[test]
    fn every_variant_displays_a_distinct_sentence() {
        let sentences = [
            ReadWithoutCargoError::Lockfile.to_string(),
            ReadWithoutCargoError::DirectoryNotFound {
                detail: "d".to_owned(),
            }
            .to_string(),
            ReadWithoutCargoError::unreadable("d").to_string(),
            ReadWithoutCargoError::unsupported("d").to_string(),
        ];

        assert_eq!(
            sentences,
            [
                "Cargo.lock is missing or out of date",
                "the directory cannot be read: d",
                "a manifest or the lockfile cannot be read: d",
                "the workspace uses something only cargo reads: d",
            ]
        );
    }
}
