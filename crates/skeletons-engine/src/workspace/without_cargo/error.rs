//! Why a workspace could not be read without Cargo.

use std::fmt;
use std::path::Path;

use super::text_error::TextError;

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
    /// A manifest or the lockfile cannot be read, or Cargo would refuse it too.
    ManifestUnreadable {
        /// The file and what was wrong with it.
        detail: String,
    },
    /// Cargo reads the workspace and this reader does not follow something in it.
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
        match (error.is_unreadable(), error.is_unsupported()) {
            (true, false) => Self::unreadable(detail),
            (false, true) => Self::unsupported(detail),
            (true, true) | (false, false) => {
                unreachable!("a text error is exactly one of unreadable and unsupported")
            }
        }
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

    use super::ReadWithoutCargoError;
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
