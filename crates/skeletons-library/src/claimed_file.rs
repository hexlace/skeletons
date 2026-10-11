//! One file a worn skeleton claims, and whether it matches.

use std::fmt;
use std::path::{Path, PathBuf};

use skeletons_engine::{Drift, DriftReason, Row};

/// Whether a claimed file on disk matches what its skeleton renders.
///
/// The comparison is byte for byte. Two files that mean the same thing but are
/// spelled differently are `Changed`, and permissions and modification times
/// are never consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileState {
    /// The file holds exactly the bytes the skeleton renders.
    Matches,
    /// The file does not exist.
    Missing,
    /// The file exists and holds other bytes.
    Changed,
}

impl FileState {
    /// Whether the file has drifted from its skeleton: it is `Missing` or
    /// `Changed`.
    ///
    /// Asking this rather than matching on the state also answers for a state
    /// a later version adds, so a caller needs no wildcard arm to ask.
    ///
    /// # Examples
    ///
    /// ```
    /// use skeletons_library::FileState;
    ///
    /// assert!(FileState::Missing.is_drifted());
    /// assert!(FileState::Changed.is_drifted());
    /// assert!(!FileState::Matches.is_drifted());
    /// ```
    #[must_use]
    pub fn is_drifted(self) -> bool {
        match self {
            Self::Matches => false,
            Self::Missing | Self::Changed => true,
        }
    }
}

impl From<Drift> for FileState {
    fn from(drift: Drift) -> Self {
        match drift {
            Drift::Matches => Self::Matches,
            Drift::Drifted(DriftReason::Missing) => Self::Missing,
            Drift::Drifted(DriftReason::Changed) => Self::Changed,
        }
    }
}

/// One file a worn skeleton claims: where it lives, whether it matches, and
/// the bytes it should hold.
///
/// This is what the command line calls a bone.
#[derive(Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ClaimedFile {
    path: PathBuf,
    state: FileState,
    rendered: Vec<u8>,
}

impl ClaimedFile {
    pub(crate) fn from_row(row: &Row) -> Self {
        Self {
            path: PathBuf::from(row.path.as_str()),
            state: FileState::from(row.drift),
            rendered: row.rendered.clone(),
        }
    }

    /// Where the file lives, relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the file on disk matches.
    #[must_use]
    pub fn state(&self) -> FileState {
        self.state
    }

    /// The bytes the skeleton renders for this path: writing them to the path
    /// makes the file match, whatever it held before.
    ///
    /// The bytes are present whatever the [`state`](Self::state) is, for a
    /// [`Missing`](FileState::Missing) file too. A caller that decides to write
    /// them to [`path`](Self::path) under the report's
    /// [`root`](crate::Report::root) finds that a second
    /// [`check`](crate::check) answers [`Matches`](FileState::Matches). The
    /// crate documentation shows the round trip.
    #[must_use]
    pub fn rendered(&self) -> &[u8] {
        &self.rendered
    }
}

/// Prints how many bytes `rendered` holds rather than the bytes, which would
/// make a report unreadable.
impl fmt::Debug for ClaimedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaimedFile")
            .field("path", &self.path)
            .field("state", &self.state)
            .field("rendered_bytes", &self.rendered.len())
            .finish()
    }
}
