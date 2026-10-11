//! Everything `check` found in a workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skeletons_engine::{RefusalRecord, Survey, WornDependency};

use crate::claimed_file::ClaimedFile;
use crate::refusal::Refusal;
use crate::skeletons_named::SkeletonsNamed;
use crate::worn_skeleton::WornSkeleton;

/// What `check` found in a workspace: each wearing that resolved to a skeleton,
/// and every refusal.
///
/// There are two complete ways to read a report. For one package, use
/// [`skeletons_named`](Self::skeletons_named). For the whole workspace, use
/// [`skeletons`](Self::skeletons) together with [`refusals`](Self::refusals):
/// every refusal is in `refusals()`, including those of wearings that resolved
/// to no skeleton, which no [`WornSkeleton`] carries. The workspace is as the
/// command line's `check` would pass it only when `refusals()` is empty and
/// every file of every skeleton is [`Matches`](crate::FileState::Matches).
///
/// A report never changes after `check` returns it.
///
/// # Examples
///
/// ```no_run
/// let report = skeletons_library::check(".")?;
/// let all_clear = report.refusals().is_empty()
///     && report.skeletons().iter().all(|skeleton| match skeleton.files() {
///         skeletons_library::Files::Checked(files) => {
///             files.iter().all(|file| !file.state().is_drifted())
///         }
///         _ => false,
///     });
/// println!("{all_clear}");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Report {
    root: PathBuf,
    skeletons: Vec<WornSkeleton>,
    /// Refusals about a wearing that resolved to no skeleton come first, so
    /// that [`SkeletonsNamed::Undetermined`] can borrow them as one slice.
    refusals: Vec<Refusal>,
    unidentified_count: usize,
}

impl Report {
    /// Joins the survey's flat rows and refusal records to the worn skeletons
    /// they belong to, once, so no caller ever does.
    pub(crate) fn from_survey(survey: &Survey<'_>) -> Self {
        let mut worn: Vec<&WornDependency> = survey.worn.clone();
        worn.sort_by(|left, right| {
            (&left.package, &left.manifest, &left.key).cmp(&(
                &right.package,
                &right.manifest,
                &right.key,
            ))
        });
        let mut skeletons: Vec<WornSkeleton> = worn
            .iter()
            .map(|worn| WornSkeleton::from_worn(worn))
            .collect();
        let index: BTreeMap<(&str, &str), usize> = worn
            .iter()
            .enumerate()
            .map(|(position, worn)| ((worn.manifest.as_str(), worn.key.as_str()), position))
            .collect();
        assert_eq!(
            index.len(),
            skeletons.len(),
            "a manifest and a dependency key name one worn dependency"
        );

        for row in &survey.rows {
            let key = (
                row.claimant.manifest.as_str(),
                row.claimant.dependency.as_str(),
            );
            let Some(&position) = index.get(&key) else {
                unreachable!("a file is claimed by a worn dependency, which the survey lists")
            };
            skeletons[position].push_file(ClaimedFile::from_row(row));
        }

        let records = survey.refusal_records();
        let mut refusals = Vec::with_capacity(records.len());
        for record in &records {
            let refusal = Refusal::from_record(record);
            hand_to_skeletons(&mut skeletons, &index, record, &refusal);
            refusals.push(refusal);
        }
        refusals.sort_by_key(|refusal| !refusal.kind().concerns_unidentified_wearing());
        let unidentified_count = refusals
            .iter()
            .take_while(|refusal| refusal.kind().concerns_unidentified_wearing())
            .count();
        assert!(
            refusals[unidentified_count..]
                .iter()
                .all(|refusal| !refusal.kind().concerns_unidentified_wearing()),
            "the sort put every refusal about an unidentified wearing first"
        );

        Self {
            root: survey.root.clone(),
            skeletons,
            refusals,
            unidentified_count,
        }
    }

    /// The workspace root as cargo reports it: absolute, with symbolic links
    /// resolved. Every other path in a report is relative to it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every wearing that resolved to a skeleton, refused or not, ordered by
    /// package name, then manifest, then dependency key.
    #[must_use]
    pub fn skeletons(&self) -> &[WornSkeleton] {
        &self.skeletons
    }

    /// The worn skeletons whose package name is `package`, compared exactly.
    ///
    /// The name is the skeleton's package name and never the dependency key,
    /// which belongs to the wearer. See [`SkeletonsNamed`] for what each answer
    /// does and does not say.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use skeletons_library::SkeletonsNamed;
    ///
    /// let report = skeletons_library::check(".")?;
    /// if let SkeletonsNamed::Worn(skeletons) = report.skeletons_named("house-rules") {
    ///     println!("{} wearings", skeletons.len());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn skeletons_named(&self, package: &str) -> SkeletonsNamed<'_> {
        let start = self
            .skeletons
            .partition_point(|skeleton| skeleton.package() < package);
        let end = self
            .skeletons
            .partition_point(|skeleton| skeleton.package() <= package);
        if start < end {
            return SkeletonsNamed::Worn(&self.skeletons[start..end]);
        }
        let unidentified = &self.refusals[..self.unidentified_count];
        if unidentified.is_empty() {
            SkeletonsNamed::NotWorn
        } else {
            SkeletonsNamed::Undetermined(unidentified)
        }
    }

    /// Every refusal in the workspace, in the same order on every run.
    ///
    /// Refusals about a wearing that resolved to no skeleton come first. A
    /// refusal that names worn skeletons is also reached through each one's
    /// [`files`](WornSkeleton::files).
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }
}

/// Gives `refusal` to each worn skeleton it names, which only a refusal that
/// concerns an identified wearing does.
fn hand_to_skeletons(
    skeletons: &mut [WornSkeleton],
    index: &BTreeMap<(&str, &str), usize>,
    record: &RefusalRecord,
    refusal: &Refusal,
) {
    for wearing in &record.wearings {
        let position = wearing
            .dependency
            .as_deref()
            .and_then(|key| index.get(&(wearing.manifest.as_str(), key)).copied());
        match (refusal.kind().concerns_unidentified_wearing(), position) {
            (true, None) => {}
            (true, Some(_)) => {
                unreachable!("a wearing that resolved to no skeleton is not a worn dependency")
            }
            (false, Some(position)) => skeletons[position].push_refusal(refusal.clone()),
            (false, None) => {
                unreachable!("a refusal about a skeleton names only worn dependencies")
            }
        }
    }
}
