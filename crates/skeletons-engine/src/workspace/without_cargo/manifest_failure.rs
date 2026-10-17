//! A read of one manifest that failed, with the shape Cargo fails on too when it is one.

use std::path::Path;

use super::cargo_cannot_load::CargoCannotLoad;
use super::error::ReadWithoutCargoError;
use super::text_error::TextError;

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
        Self {
            error: ReadWithoutCargoError::text(path, error),
            cargo_cannot_load: error.refused_shape().map(CargoCannotLoad::from),
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CargoCannotLoad, ManifestFailure, ReadWithoutCargoError};
    use crate::workspace::without_cargo::text_error::{RefusedShape, TextError};

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
        let shaped = TextError::unreadable_as(RefusedShape::GitAndPath, "ambiguous");
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
}
