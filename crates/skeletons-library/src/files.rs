//! A worn skeleton's files, and whether they could all be read.

use crate::claimed_file::ClaimedFile;
use crate::refusal::Refusal;

/// The files a worn skeleton claims, read by matching.
///
/// `Checked` is the only variant that holds every file the skeleton claims, so
/// "no drifted file" can be concluded from `Checked` and from nothing else. In
/// `Refused`, `compared` is a part of the files and never a verdict on the
/// skeleton.
///
/// A variant this version of a caller does not know is to be treated as
/// `Refused` with nothing known: it never means "nothing to report".
///
/// # Examples
///
/// ```no_run
/// use skeletons_library::Files;
///
/// let report = skeletons_library::check(".")?;
/// for skeleton in report.skeletons() {
///     match skeleton.files() {
///         Files::Checked(files) => {
///             let drifted = files.iter().filter(|file| file.state().is_drifted());
///             println!("{}: {} drifted", skeleton.package(), drifted.count());
///         }
///         Files::Refused { refusals, .. } => {
///             println!("{}: refused {} times", skeleton.package(), refusals.len());
///         }
///         // A state a later version adds: not checked, never "nothing drifted".
///         _ => println!("{}: could not be checked", skeleton.package()),
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Files<'report> {
    /// Every file the skeleton claims, in path order. No refusal names this
    /// wearing. The slice is empty only when the skeleton claims no file.
    Checked(&'report [ClaimedFile]),
    /// The skeleton could not be checked as a whole.
    #[non_exhaustive]
    Refused {
        /// Every refusal that names this wearing. Never empty.
        refusals: &'report [Refusal],
        /// The files that were compared all the same, in path order: not every
        /// file the skeleton claims, and none at all unless every refusal is an
        /// [`Overlap`](crate::RefusalKind::Overlap).
        compared: &'report [ClaimedFile],
    },
}
