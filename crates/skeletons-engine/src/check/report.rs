//! Everything `check` says, rolled up from a [`Survey`] and a behind map
//! into the shape both the human output and `--json` read from.

use std::collections::BTreeMap;

use crate::behind::Behind;
use crate::claim::Drift;
use crate::survey::{Refusal, Row, Survey};
use crate::workspace::{WornDependency, WornId};

/// The `--drifted`/`--behind` flags that narrow which rows are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) drifted: bool,
    pub(crate) behind: bool,
}

impl Filter {
    /// Whether this filter narrows anything at all.
    pub(crate) const fn is_narrowed(self) -> bool {
        self.drifted || self.behind
    }

    /// Whether a bone's own row is shown under this filter, given
    /// the behind fact of the skeleton that claims it.
    pub(crate) const fn shows(self, row: &Row, behind: &Behind) -> bool {
        if !self.is_narrowed() {
            return true;
        }
        let drifted_match = self.drifted && matches!(row.drift, Drift::Drifted(_));
        let behind_match =
            self.behind && matches!(behind, Behind::Behind(_) | Behind::Undetermined(_));
        drifted_match || behind_match
    }
}

/// Every closed count `--json`'s `summary` carries.
pub(crate) struct Summary {
    pub(crate) bones: usize,
    pub(crate) matches: usize,
    pub(crate) drifted: usize,
    pub(crate) skeletons: usize,
    pub(crate) current: usize,
    pub(crate) behind: usize,
    pub(crate) pinned: usize,
    pub(crate) undetermined: usize,
    pub(crate) refusals: usize,
    pub(crate) failed: bool,
}

/// Everything `check` reports: the survey it ran and the behind fact for
/// every worn dependency in it, presented as the counts and groupings both
/// outputs read from.
pub(crate) struct Report<'workspace> {
    survey: Survey<'workspace>,
    behind: BTreeMap<WornId, Behind>,
}

impl<'workspace> Report<'workspace> {
    pub(crate) const fn new(survey: Survey<'workspace>, behind: BTreeMap<WornId, Behind>) -> Self {
        Self { survey, behind }
    }

    pub(crate) fn root(&self) -> &std::path::Path {
        self.survey.root.as_path()
    }

    /// The behind fact for `worn` — every worn dependency `behind::determine`
    /// was given has exactly one.
    ///
    /// # Panics
    ///
    /// If `worn` was not one of the dependencies this report's behind map
    /// was built from — every worn dependency this crate ever hands to
    /// [`Report::new`] was also handed to `behind::determine`, so this never
    /// happens in practice.
    pub(crate) fn behind_for(&self, worn: &WornDependency) -> &Behind {
        let Some(behind) = self.behind.get(&worn.id()) else {
            unreachable!("behind::determine returns exactly one result per worn dependency")
        };
        behind
    }

    /// Every worn dependency, in manifest-then-dependency order — the order
    /// both `skeletons[]` and the human output's blocks are shown in.
    pub(crate) fn skeletons(&self) -> Vec<&'workspace WornDependency> {
        let mut skeletons = self.survey.worn.clone();
        skeletons
            .sort_by(|left, right| (&left.manifest, &left.key).cmp(&(&right.manifest, &right.key)));
        skeletons
    }

    /// Every row claimed by `worn`, in path order (already the survey's own
    /// sort order).
    pub(crate) fn rows_for(&self, worn: &WornDependency) -> Vec<&Row> {
        self.survey
            .rows
            .iter()
            .filter(|row| {
                row.claimant.manifest == worn.manifest && row.claimant.dependency == worn.key
            })
            .collect()
    }

    /// Every refusal about `worn` as a whole, in path order — more than one
    /// only when every one of them is `unsafe-path`; every other kind is
    /// alone, since a skeleton whose own render is refused (`option-refused`,
    /// `skeleton-invalid`) never has claims to resolve any further unsafe
    /// paths from.
    ///
    /// # Panics
    ///
    /// If `worn`'s own refusals mix an `unsafe-path` refusal with one of
    /// another kind — the survey this report was built from never produces
    /// that mix, and asserts as much where it builds one and again here
    /// where it is read.
    pub(crate) fn refusals_for(&self, worn: &WornDependency) -> Vec<&Refusal> {
        let id = worn.id();
        let mut refusals: Vec<&Refusal> = self
            .survey
            .refusals
            .iter()
            .filter(|refusal| refusal.about_one_skeleton() == Some(&id))
            .collect();
        assert!(
            refusals
                .iter()
                .all(|refusal| refusal.unsafe_path().is_some())
                || refusals.len() <= 1,
            "a worn skeleton's refusals are all unsafe-path, or there is exactly one of another \
             kind"
        );
        refusals.sort_by_key(|refusal| refusal.unsafe_path());
        refusals
    }

    /// Every refusal about no single worn skeleton — the closing `refused:`
    /// list.
    pub(crate) fn closing_refusals(&self) -> Vec<&Refusal> {
        let mut refusals: Vec<&Refusal> = self
            .survey
            .refusals
            .iter()
            .filter(|refusal| refusal.about_one_skeleton().is_none())
            .collect();
        refusals.sort_by_key(|refusal| (refusal.kind(), refusal.message()));
        refusals
    }

    /// Every refusal at all, closing or inside a block.
    pub(crate) fn all_refusals(&self) -> &[Refusal] {
        &self.survey.refusals
    }

    /// Rolls up every count `--json`'s `summary` and the human output's
    /// count line report, always over every bone and skeleton, whatever the
    /// display filter narrows away. `fail_behind` decides whether `failed`
    /// (the actual exit status) also counts a behind or undetermined
    /// skeleton.
    pub(crate) fn summary(&self, fail_behind: bool) -> Summary {
        let bones = self.survey.rows.len();
        let matches = self
            .survey
            .rows
            .iter()
            .filter(|row| matches!(row.drift, Drift::Matches))
            .count();
        let drifted = bones - matches;
        let skeletons = self.survey.worn.len();
        let refusals = self.survey.refusals.len();

        let mut current = 0;
        let mut behind = 0;
        let mut pinned = 0;
        let mut undetermined = 0;
        for fact in self.behind.values() {
            match fact {
                Behind::Current => current += 1,
                Behind::Behind(_) => behind += 1,
                Behind::Pinned => pinned += 1,
                Behind::Undetermined(_) => undetermined += 1,
            }
        }
        assert_eq!(
            current + behind + pinned + undetermined,
            skeletons,
            "every worn skeleton reads as exactly one of current, behind, pinned or undetermined"
        );

        let failed =
            drifted > 0 || refusals > 0 || (fail_behind && (behind > 0 || undetermined > 0));
        Summary {
            bones,
            matches,
            drifted,
            skeletons,
            current,
            behind,
            pinned,
            undetermined,
            refusals,
            failed,
        }
    }
}
