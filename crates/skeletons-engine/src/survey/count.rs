//! The words in `check`'s and `sync`'s messages that agree with a count: a
//! noun with the number it counts, a verb or pronoun with the number that is
//! its subject.
//!
//! Every function reads one count and answers for exactly one, so a message
//! that names two counts asks each of its words about the count it belongs
//! to. In `<n> of <total> <noun> <verb>` the noun agrees with `total`, the
//! group the phrase names, and the verb with `n`, the number the phrase is
//! about: `1 of 2 bones has drifted`, `2 of 2 bones have drifted`,
//! `1 of 1 bone has drifted`. Zero is plural everywhere, as in `0 bones` and
//! `0 of 1 bone have drifted`.

/// `singular` for exactly one, `plural` for anything else.
const fn agreeing(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

/// `bone` for exactly one, `bones` for anything else.
pub(crate) const fn bone(count: usize) -> &'static str {
    agreeing(count, "bone", "bones")
}

/// `worn skeleton` for exactly one, `worn skeletons` for anything else.
pub(crate) const fn worn_skeleton(count: usize) -> &'static str {
    agreeing(count, "worn skeleton", "worn skeletons")
}

/// `refusal` for exactly one, `refusals` for anything else.
pub(crate) const fn refusal(count: usize) -> &'static str {
    agreeing(count, "refusal", "refusals")
}

/// `file` for exactly one, `files` for anything else.
pub(crate) const fn file(count: usize) -> &'static str {
    agreeing(count, "file", "files")
}

/// `change` for exactly one, `changes` for anything else.
pub(crate) const fn change(count: usize) -> &'static str {
    agreeing(count, "change", "changes")
}

/// `is` for exactly one, `are` for anything else.
pub(crate) const fn is_or_are(count: usize) -> &'static str {
    agreeing(count, "is", "are")
}

/// `was` for exactly one, `were` for anything else.
pub(crate) const fn was_or_were(count: usize) -> &'static str {
    agreeing(count, "was", "were")
}

/// `has` for exactly one, `have` for anything else.
pub(crate) const fn has_or_have(count: usize) -> &'static str {
    agreeing(count, "has", "have")
}

/// `matches` for exactly one, `match` for anything else: a present-tense
/// verb agreeing with its count (`2 bones match`, `1 bone matches`), not a
/// noun being pluralised.
pub(crate) const fn matches_or_match(count: usize) -> &'static str {
    agreeing(count, "matches", "match")
}

/// `it` for exactly one, `them` for anything else.
pub(crate) const fn it_or_them(count: usize) -> &'static str {
    agreeing(count, "it", "them")
}

/// `it` for exactly one, `each` for anything else: the pronoun that stands
/// for a group's members one at a time (`what it holds now`, `what each holds
/// now`).
pub(crate) const fn it_or_each(count: usize) -> &'static str {
    agreeing(count, "it", "each")
}

/// `line above says` for exactly one, `lines above say` for anything else:
/// the noun and its verb, agreeing together.
pub(crate) const fn line_above_says_or_lines_above_say(count: usize) -> &'static str {
    agreeing(count, "line above says", "lines above say")
}

/// `it points` for exactly one, `they point` for anything else.
pub(crate) const fn it_points_or_they_point(count: usize) -> &'static str {
    agreeing(count, "it points", "they point")
}

/// `<count> of <total> <noun>`, the noun agreeing with `total`: the words
/// before the verb of a phrase like `1 of 2 bones has drifted`. The verb is
/// the caller's, agreeing with `count`.
pub(crate) fn of_total(count: usize, total: usize, noun: fn(usize) -> &'static str) -> String {
    assert!(
        count <= total,
        "{count} of {total} names more than the whole"
    );
    format!("{count} of {total} {}", noun(total))
}

/// `there is 1 refusal` for exactly one, `there are <count> refusals` for
/// anything else.
pub(crate) fn there_are_refusals(count: usize) -> String {
    format!("there {} {count} {}", is_or_are(count), refusal(count))
}

#[cfg(test)]
mod tests {
    use super::{
        bone, change, file, has_or_have, is_or_are, it_or_each, it_or_them,
        it_points_or_they_point, line_above_says_or_lines_above_say, matches_or_match, of_total,
        refusal, there_are_refusals, was_or_were, worn_skeleton,
    };

    // Each word is singular for exactly one and plural for zero and for
    // several, so the pair pins the one boundary a count can get wrong.
    #[test]
    fn nouns_agree_with_their_count() {
        for (word, one, several) in [
            (bone as fn(usize) -> &'static str, "bone", "bones"),
            (worn_skeleton, "worn skeleton", "worn skeletons"),
            (refusal, "refusal", "refusals"),
            (file, "file", "files"),
            (change, "change", "changes"),
        ] {
            assert_eq!(word(1), one);
            assert_eq!(word(0), several);
            assert_eq!(word(2), several);
        }
    }

    #[test]
    fn verbs_and_pronouns_agree_with_their_count() {
        for (word, one, several) in [
            (is_or_are as fn(usize) -> &'static str, "is", "are"),
            (was_or_were, "was", "were"),
            (has_or_have, "has", "have"),
            (matches_or_match, "matches", "match"),
            (it_or_them, "it", "them"),
            (it_or_each, "it", "each"),
            (
                line_above_says_or_lines_above_say,
                "line above says",
                "lines above say",
            ),
            (it_points_or_they_point, "it points", "they point"),
        ] {
            assert_eq!(word(1), one);
            assert_eq!(word(0), several);
            assert_eq!(word(2), several);
        }
    }

    // The noun follows the total and never the count, in every corner of
    // the two-by-two of one/several against one/several.
    #[test]
    fn of_total_agrees_its_noun_with_the_total() {
        assert_eq!(of_total(0, 1, bone), "0 of 1 bone");
        assert_eq!(of_total(1, 1, bone), "1 of 1 bone");
        assert_eq!(of_total(1, 2, bone), "1 of 2 bones");
        assert_eq!(of_total(2, 2, bone), "2 of 2 bones");
        assert_eq!(of_total(0, 2, bone), "0 of 2 bones");
    }

    #[test]
    #[should_panic(expected = "2 of 1 names more than the whole")]
    fn of_total_refuses_a_count_above_its_total() {
        let _ = of_total(2, 1, bone);
    }

    #[test]
    fn a_phrase_built_from_of_total_agrees_its_verb_with_the_count() {
        let phrase = |count, total| {
            format!(
                "{} {} drifted",
                of_total(count, total, bone),
                has_or_have(count)
            )
        };
        assert_eq!(phrase(0, 1), "0 of 1 bone have drifted");
        assert_eq!(phrase(1, 1), "1 of 1 bone has drifted");
        assert_eq!(phrase(1, 2), "1 of 2 bones has drifted");
        assert_eq!(phrase(2, 2), "2 of 2 bones have drifted");
    }

    #[test]
    fn there_are_refusals_agrees_verb_and_noun_with_the_count() {
        assert_eq!(there_are_refusals(1), "there is 1 refusal");
        assert_eq!(there_are_refusals(2), "there are 2 refusals");
        assert_eq!(there_are_refusals(0), "there are 0 refusals");
    }
}
