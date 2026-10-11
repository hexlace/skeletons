//! `check`'s human output (pure): one block per worn skeleton, a closing
//! `refused:` list, and a count line.

use std::path::Path;

use super::behind_words::{newer_detail, undetermined_detail};
use super::pin_words::pin_detail;
use super::report::{Filter, Report, Summary};
use crate::behind::Behind;
use crate::claim::{Drift, DriftReason};
use crate::skeleton::Escaped;
use crate::survey::count;
use crate::workspace::{Pin, WornDependency};

/// The drift word's own widest rendering (`drifted (changed)`), which every
/// line's word is padded out to with trailing spaces, so the paths that
/// follow line up in a column.
const DRIFT_WORD_WIDTH: usize = "drifted (changed)".len();

/// Every line `check`'s human output prints for `report`, narrowed by
/// `filter`. `fail_behind` is threaded through only because
/// [`Report::summary`] needs it; the count line itself never shows the exit
/// status.
pub(crate) fn lines(report: &Report<'_>, filter: Filter, fail_behind: bool) -> Vec<String> {
    let root = report.root();
    let mut lines = Vec::new();
    let mut first_block = true;

    for worn in report.skeletons() {
        let Some(block) = block_lines(report, worn, root, filter) else {
            continue;
        };
        if !first_block {
            lines.push(String::new());
        }
        first_block = false;
        lines.extend(block);
    }

    let closing = report.closing_refusals();
    if !closing.is_empty() {
        if !first_block {
            lines.push(String::new());
        }
        for refusal in &closing {
            lines.push(format!("refused: {}", refusal.message()));
        }
    }

    if !first_block || !closing.is_empty() {
        lines.push(String::new());
    }
    lines.push(count_line(&report.summary(fail_behind)));
    lines
}

/// One worn skeleton's own block (header, then its rows or its own
/// refusal), or `None` when `filter` narrows away every line it would
/// otherwise show.
fn block_lines(
    report: &Report<'_>,
    worn: &WornDependency,
    root: &Path,
    filter: Filter,
) -> Option<Vec<String>> {
    let behind = report.behind_for(worn);
    let refusals = report.refusals_for(worn);

    if !refusals.is_empty() {
        // Refusals are always shown, whatever the filter: hiding one would
        // hide why the exit was non-zero. One line per refusal, in path
        // order — only `unsafe-path` ever has more than one.
        let mut lines = vec![header(worn, root, behind)];
        lines.extend(
            refusals
                .iter()
                .map(|refusal| format!("  refused: {}", refusal.text_in_block())),
        );
        return Some(lines);
    }

    let rows: Vec<_> = report
        .rows_for(worn)
        .into_iter()
        .filter(|row| filter.shows(row, behind))
        .collect();
    if rows.is_empty() {
        return None;
    }

    let mut lines = vec![header(worn, root, behind)];
    for row in rows {
        lines.push(row_line(row.drift, row.path.as_str()));
    }
    Some(lines)
}

/// One bone's line in its skeleton's block: the drift word, padded so the
/// paths line up, then the path.
fn row_line(drift: Drift, path: &str) -> String {
    let path = Escaped(path);
    format!(
        "  {:<width$}  {path}",
        drift_word(drift),
        width = DRIFT_WORD_WIDTH
    )
}

/// `<dependency> in <manifest>: <skeleton> <version> from <pin>, <behind>`.
fn header(worn: &WornDependency, root: &Path, behind: &Behind) -> String {
    let (key, manifest, package) = (
        Escaped(&worn.key),
        Escaped(&worn.manifest),
        Escaped(&worn.package),
    );
    format!(
        "{key} in {manifest}: {package} {} from {}, {}",
        worn.version,
        pin_detail(&worn.pin, root),
        behind_word(&worn.pin, behind)
    )
}

/// The state word, with the JSON `detail` in parentheses where one exists:
/// `current`, `behind (0.2.0 is available)`, `pinned`, `undetermined
/// (<detail>)`.
fn behind_word(pin: &Pin, behind: &Behind) -> String {
    match behind {
        Behind::Current => "current".to_owned(),
        Behind::Behind(newer) => format!("behind ({})", newer_detail(pin, newer)),
        Behind::Pinned => "pinned".to_owned(),
        Behind::Undetermined(undetermined) => {
            format!("undetermined ({})", undetermined_detail(undetermined))
        }
    }
}

const fn drift_word(drift: Drift) -> &'static str {
    match drift {
        Drift::Matches => "matches",
        Drift::Drifted(DriftReason::Missing) => "drifted (missing)",
        Drift::Drifted(DriftReason::Changed) => "drifted (changed)",
    }
}

/// The closing count line — always over every bone and skeleton, whatever
/// the filter: the bones sentence, the skeletons sentence, and, only when
/// there is at least one, the refusals sentence.
fn count_line(summary: &Summary) -> String {
    let mut sentences = vec![bones_sentence(summary), skeletons_sentence(summary)];
    if summary.refusals > 0 {
        sentences.push(refusals_sentence(summary.refusals));
    }
    sentences.join(" ")
}

/// `<m> bones: <d> drifted, <x> match.`
fn bones_sentence(summary: &Summary) -> String {
    let mut clauses = Vec::new();
    if summary.drifted > 0 {
        clauses.push(format!("{} drifted", summary.drifted));
    }
    if summary.matches > 0 {
        clauses.push(format!(
            "{} {}",
            summary.matches,
            count::matches_or_match(summary.matches)
        ));
    }
    sentence(summary.bones, count::bone(summary.bones), &clauses)
}

/// `<n> worn skeletons: <b> behind, <c> current, <p> pinned, <u> undetermined.`
fn skeletons_sentence(summary: &Summary) -> String {
    let mut clauses = Vec::new();
    if summary.behind > 0 {
        clauses.push(format!("{} behind", summary.behind));
    }
    if summary.current > 0 {
        clauses.push(format!("{} current", summary.current));
    }
    if summary.pinned > 0 {
        clauses.push(format!("{} pinned", summary.pinned));
    }
    if summary.undetermined > 0 {
        clauses.push(format!("{} undetermined", summary.undetermined));
    }
    sentence(
        summary.skeletons,
        count::worn_skeleton(summary.skeletons),
        &clauses,
    )
}

/// `<k> refusal(s).`
fn refusals_sentence(refusals: usize) -> String {
    format!("{refusals} {}.", count::refusal(refusals))
}

/// `<total> <noun>[: <clauses joined by ", ">].` — the one shape both the
/// bones and the skeletons sentence share: a total that is never omitted,
/// and a breakdown that is, when every one of its own counts is zero.
fn sentence(total: usize, noun: &str, clauses: &[String]) -> String {
    let mut text = format!("{total} {noun}");
    if !clauses.is_empty() {
        text.push_str(": ");
        text.push_str(&clauses.join(", "));
    }
    text.push('.');
    text
}

#[cfg(test)]
mod tests {
    use super::{behind_word, count_line, header, row_line};
    use crate::behind::{Behind, Newer, Undetermined, UndeterminedReason};
    use crate::check::report::Summary;
    use crate::claim::{Drift, DriftReason};
    use crate::skeleton::Choices;
    use crate::survey::poison::{POISON, POISON_ESCAPED, assert_escaped_once};
    use crate::workspace::{Pin, WornDependency};

    #[test]
    fn behind_word_reads_the_bare_state_for_current_and_pinned() {
        let pin = Pin::CratesIo;
        assert_eq!(behind_word(&pin, &Behind::Current), "current");
        assert_eq!(behind_word(&pin, &Behind::Pinned), "pinned");
    }

    #[test]
    fn behind_word_parenthesises_the_detail_for_behind_and_undetermined() {
        let pin = Pin::CratesIo;
        let behind = Behind::Behind(Newer::Version(semver::Version::new(0, 2, 0)));
        assert_eq!(behind_word(&pin, &behind), "behind (0.2.0 is available)");

        let undetermined = Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::OtherRegistry,
            detail: "`skeletons` asks only crates.io whether a skeleton is behind".to_owned(),
        });
        assert_eq!(
            behind_word(&pin, &undetermined),
            "undetermined (`skeletons` asks only crates.io whether a skeleton is behind)"
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a test fixture builder, not an API"
    )]
    fn summary(
        bones: usize,
        matches: usize,
        drifted: usize,
        skeletons: usize,
        current: usize,
        behind: usize,
        pinned: usize,
        undetermined: usize,
        refusals: usize,
    ) -> Summary {
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
            failed: drifted > 0 || refusals > 0,
        }
    }

    #[test]
    fn all_matching_omits_the_drifted_clause() {
        assert_eq!(
            count_line(&summary(4, 4, 0, 4, 4, 0, 0, 0, 0)),
            "4 bones: 4 match. 4 worn skeletons: 4 current."
        );
    }

    #[test]
    fn all_drifted_omits_the_match_clause() {
        assert_eq!(
            count_line(&summary(3, 0, 3, 1, 1, 0, 0, 0, 0)),
            "3 bones: 3 drifted. 1 worn skeleton: 1 current."
        );
    }

    #[test]
    fn a_mix_shows_both_file_clauses_drifted_first() {
        assert_eq!(
            count_line(&summary(4, 2, 2, 1, 1, 0, 0, 0, 0)),
            "4 bones: 2 drifted, 2 match. 1 worn skeleton: 1 current."
        );
    }

    #[test]
    fn one_bone_is_singular() {
        assert_eq!(
            count_line(&summary(1, 1, 0, 1, 1, 0, 0, 0, 0)),
            "1 bone: 1 matches. 1 worn skeleton: 1 current."
        );
    }

    #[test]
    fn zero_bones_still_names_the_total() {
        assert_eq!(
            count_line(&summary(0, 0, 0, 0, 0, 0, 0, 0, 0)),
            "0 bones. 0 worn skeletons."
        );
    }

    #[test]
    fn the_skeletons_sentence_lists_every_nonzero_state_in_a_fixed_order() {
        assert_eq!(
            count_line(&summary(4, 2, 2, 4, 1, 1, 1, 1, 0)),
            "4 bones: 2 drifted, 2 match. \
             4 worn skeletons: 1 behind, 1 current, 1 pinned, 1 undetermined."
        );
    }

    #[test]
    fn one_worn_skeleton_is_singular() {
        assert_eq!(
            count_line(&summary(1, 1, 0, 1, 0, 1, 0, 0, 0)),
            "1 bone: 1 matches. 1 worn skeleton: 1 behind."
        );
    }

    #[test]
    fn a_nonzero_refusal_count_appends_its_own_sentence() {
        assert_eq!(
            count_line(&summary(4, 4, 0, 3, 3, 0, 0, 0, 2)),
            "4 bones: 4 match. 3 worn skeletons: 3 current. 2 refusals."
        );
    }

    #[test]
    fn a_single_refusal_is_singular() {
        assert_eq!(
            count_line(&summary(4, 4, 0, 3, 3, 0, 0, 0, 1)),
            "4 bones: 4 match. 3 worn skeletons: 3 current. 1 refusal."
        );
    }

    #[test]
    fn zero_refusals_omits_the_refusals_sentence_entirely() {
        let line = count_line(&summary(4, 4, 0, 3, 3, 0, 0, 0, 0));
        assert!(!line.contains("refusal"));
    }

    #[test]
    fn a_block_header_prints_its_outside_text_escaped_once() {
        // The key, the manifest, the skeleton's package name, and the detail
        // of a pin and of a behind fact all come from the repository or a
        // remote. Each holds the poison, and the header stays one line.
        let worn = WornDependency {
            manifest: POISON.to_owned(),
            key: POISON.to_owned(),
            package: POISON.to_owned(),
            version: semver::Version::new(0, 1, 0),
            skeleton_directory: "/w/skeleton".into(),
            pin: Pin::Unrecognised {
                source: POISON.to_owned(),
            },
            choices: Ok(Choices::new()),
        };
        let behind = Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::Unreachable,
            detail: format!("could not reach {POISON}"),
        });

        let line = header(&worn, std::path::Path::new("/w"), &behind);

        assert_escaped_once(&line, "the header");
        assert_eq!(
            line.matches(POISON_ESCAPED).count(),
            5,
            "key, manifest, package, pin and behind detail must each show it: {line:?}"
        );
    }

    #[test]
    fn a_bone_line_prints_its_path_escaped_once() {
        for drift in [
            Drift::Matches,
            Drift::Drifted(DriftReason::Missing),
            Drift::Drifted(DriftReason::Changed),
        ] {
            assert_escaped_once(&row_line(drift, POISON), &format!("{drift:?}"));
        }
    }

    #[test]
    fn behind_word_escapes_the_detail_it_parenthesises() {
        let behind = Behind::Behind(Newer::Tag(POISON.to_owned()));
        assert_escaped_once(&behind_word(&Pin::CratesIo, &behind), "the behind word");
    }
}
