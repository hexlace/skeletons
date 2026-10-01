//! The drift fact: whether a claimed file's bytes on disk match what the
//! skeleton renders, and if not, whether it is missing or merely changed.

use super::location::OnDisk;

/// Why a claimed file is drifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriftReason {
    /// The file does not exist at all.
    Missing,
    /// The file exists, but its bytes differ from the render.
    Changed,
}

/// Whether a claimed file matches its render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drift {
    Matches,
    Drifted(DriftReason),
}

/// Compares `rendered` against what is really on disk at the claim's path.
///
/// Byte-for-byte only: two files that mean the same thing but are spelled
/// differently (reformatted, reordered) still compare `Changed`. Permissions
/// and modification time are never consulted — not merely unused, but
/// unreachable: [`OnDisk`] carries either nothing or a file's bytes
/// ([`OnDisk::Missing`] or [`OnDisk::File(Vec<u8>)`](OnDisk::File)), so there
/// is no permission or timestamp this function could read even if it tried.
/// The skeleton claims bytes only, and the type this compares against says
/// so.
pub(crate) fn compare(rendered: &[u8], on_disk: &OnDisk) -> Drift {
    match on_disk {
        OnDisk::Missing => Drift::Drifted(DriftReason::Missing),
        OnDisk::File(bytes) => {
            if bytes.as_slice() == rendered {
                Drift::Matches
            } else {
                Drift::Drifted(DriftReason::Changed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Drift, DriftReason, OnDisk, compare};

    #[test]
    fn identical_bytes_match() {
        assert_eq!(
            compare(b"same", &OnDisk::File(b"same".to_vec())),
            Drift::Matches
        );
    }

    #[test]
    fn different_bytes_of_the_same_length_are_changed() {
        assert_eq!(
            compare(b"aaaa", &OnDisk::File(b"bbbb".to_vec())),
            Drift::Drifted(DriftReason::Changed)
        );
    }

    #[test]
    fn different_lengths_are_changed() {
        assert_eq!(
            compare(b"short", &OnDisk::File(b"a much longer file".to_vec())),
            Drift::Drifted(DriftReason::Changed)
        );
    }

    #[test]
    fn a_missing_file_is_drifted_as_missing() {
        assert_eq!(
            compare(b"anything", &OnDisk::Missing),
            Drift::Drifted(DriftReason::Missing)
        );
    }

    #[test]
    fn empty_rendered_bytes_match_an_empty_file() {
        assert_eq!(compare(b"", &OnDisk::File(Vec::new())), Drift::Matches);
    }
}
