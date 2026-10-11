//! The answer to asking which worn skeletons have a given package name.

use crate::refusal::Refusal;
use crate::worn_skeleton::WornSkeleton;

/// The worn skeletons of one package, or why there are none.
///
/// `NotWorn` is the only answer that means the workspace does not wear the
/// package. `Undetermined` is not `NotWorn`: the refusals it holds are about
/// wearings whose skeleton was never identified, and the package asked for may
/// be one of them. A refusal whose [`Wearing`](crate::Wearing) names another
/// package cannot be it, so it never makes the answer `Undetermined`. `Worn`
/// says nothing of the rest of the workspace: another wearing may have been
/// refused, and [`Report::refusals`](crate::Report::refusals) holds it.
///
/// A variant this version of a caller does not know is to be treated as
/// `Undetermined`.
///
/// # Examples
///
/// ```no_run
/// use skeletons_library::SkeletonsNamed;
///
/// let report = skeletons_library::check(".")?;
/// match report.skeletons_named("house-rules") {
///     SkeletonsNamed::Worn(skeletons) => println!("worn {} times", skeletons.len()),
///     SkeletonsNamed::NotWorn => println!("house-rules is not worn"),
///     SkeletonsNamed::Undetermined(refusals) => {
///         for wearing in refusals.iter().flat_map(|refusal| refusal.wearings()) {
///             println!("{} may wear it", wearing.manifest().display());
///         }
///     }
///     // An answer a later version adds: not known, never "not worn".
///     _ => println!("whether house-rules is worn could not be determined"),
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SkeletonsNamed<'report> {
    /// One skeleton per wearing of the package. Never empty, and a sub-slice of
    /// [`Report::skeletons`](crate::Report::skeletons).
    Worn(&'report [WornSkeleton]),
    /// No wearing resolved to the package, and no refused wearing could be it:
    /// each one that resolved to no skeleton names another package.
    NotWorn,
    /// No wearing resolved to the package, and these refusals are each about a
    /// wearing that resolved to no skeleton and may be the package: its
    /// [`Wearing`](crate::Wearing) names the package, or names none. Those
    /// naming the package come first. Never empty.
    Undetermined(&'report [Refusal]),
}
