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
    /// Refusals about a wearing that resolved to no skeleton come first, those
    /// whose wearing names no package before those that name one, so that
    /// [`SkeletonsNamed::Undetermined`] can borrow the first group as one slice.
    refusals: Vec<Refusal>,
    unnamed_count: usize,
    /// For each package an unidentified wearing names, the refusals that could
    /// be that package: the ones naming it, then every one naming no package.
    undetermined: BTreeMap<String, Vec<Refusal>>,
}

/// Where a refusal sits in [`Report::refusals`], in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// About a wearing that resolved to no skeleton, naming no package.
    UnidentifiedNamingNone,
    /// About a wearing that resolved to no skeleton, naming a package.
    UnidentifiedNamingOne,
    /// About worn skeletons.
    Identified,
}

impl Group {
    fn of(refusal: &Refusal) -> Self {
        if refusal.kind().concerns_unidentified_wearing() {
            if refusal.package_named().is_some() {
                Self::UnidentifiedNamingOne
            } else {
                Self::UnidentifiedNamingNone
            }
        } else {
            Self::Identified
        }
    }
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
        assert!(
            refusals
                .iter()
                .filter(|refusal| refusal.kind().concerns_unidentified_wearing())
                .all(|refusal| refusal.wearings().len() == 1),
            "a refusal about a wearing that resolved to no skeleton concerns exactly one wearing"
        );
        refusals.sort_by_key(Group::of);
        let unnamed_count = refusals
            .iter()
            .take_while(|refusal| Group::of(refusal) == Group::UnidentifiedNamingNone)
            .count();
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
        let undetermined = undetermined_by_package(&refusals, unnamed_count, unidentified_count);

        Self {
            root: survey.root.clone(),
            skeletons,
            refusals,
            unnamed_count,
            undetermined,
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
    /// which belongs to the wearer. A wearing that was refused before its
    /// skeleton was identified makes the answer unsettled only when it names
    /// `package` or names no package at all. See [`SkeletonsNamed`] for what
    /// each answer does and does not say.
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
        if let Some(refusals) = self.undetermined.get(package) {
            return SkeletonsNamed::Undetermined(refusals);
        }
        let unnamed = &self.refusals[..self.unnamed_count];
        if unnamed.is_empty() {
            SkeletonsNamed::NotWorn
        } else {
            SkeletonsNamed::Undetermined(unnamed)
        }
    }

    /// Every refusal in the workspace, in the same order on every run.
    ///
    /// Refusals about a wearing that resolved to no skeleton come first: those
    /// whose [`Wearing`](crate::Wearing) names no package, then those whose
    /// wearing names one, then the rest. A refusal that names worn skeletons is
    /// also reached through each one's [`files`](WornSkeleton::files).
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }
}

/// For each package a refused unidentified wearing names, the refusals that
/// could be that package: those naming it, in report order, then clones of the
/// ones naming none.
///
/// `refusals` is sorted by [`Group`]: the first `unnamed_count` name no package
/// and the next ones, up to `unidentified_count`, each name one.
fn undetermined_by_package(
    refusals: &[Refusal],
    unnamed_count: usize,
    unidentified_count: usize,
) -> BTreeMap<String, Vec<Refusal>> {
    let mut undetermined: BTreeMap<String, Vec<Refusal>> = BTreeMap::new();
    for refusal in &refusals[unnamed_count..unidentified_count] {
        let Some(package) = refusal.package_named() else {
            unreachable!("the sort put every refusal naming no package before this range")
        };
        undetermined
            .entry(package.to_owned())
            .or_default()
            .push(refusal.clone());
    }
    for list in undetermined.values_mut() {
        list.extend(refusals[..unnamed_count].iter().cloned());
    }
    undetermined
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
