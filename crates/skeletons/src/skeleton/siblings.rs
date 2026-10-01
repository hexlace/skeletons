//! One directory's entry names, admitted one at a time in ascending order,
//! each checked against the names before it for one that
//! [`super::folding`] says is the same name.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use super::folding::FoldedName;

/// The names of one directory's entries admitted so far, keyed by their
/// folded names.
#[derive(Debug)]
pub(crate) struct Siblings<'listing> {
    by_folded_name: BTreeMap<FoldedName, &'listing str>,
    last_admitted: Option<&'listing str>,
}

impl<'listing> Siblings<'listing> {
    pub(crate) const fn new() -> Self {
        Self {
            by_folded_name: BTreeMap::new(),
            last_admitted: None,
        }
    }

    /// Admits `name`, or returns the earlier sibling it is the same name
    /// as.
    ///
    /// Names are admitted in ascending byte order, so the sibling returned
    /// is always the earlier of the two in path order, and the first
    /// collision found is the one at the first name that collides with any
    /// name before it — whatever the filesystem's own listing order was.
    ///
    /// # Panics
    ///
    /// If `name` is not strictly greater than the name admitted before it:
    /// a caller admitting a listing out of order, or twice, is a bug here
    /// rather than a skeleton to refuse.
    pub(crate) fn admit(&mut self, name: &'listing str) -> Result<(), &'listing str> {
        if let Some(previous) = self.last_admitted {
            assert!(
                previous < name,
                "siblings are admitted in strictly ascending order: {previous:?} then {name:?}"
            );
        }
        self.last_admitted = Some(name);

        match self.by_folded_name.entry(FoldedName::of(name)) {
            Entry::Occupied(earlier) => {
                let earlier = *earlier.get();
                // Postcondition: the sibling named first is the earlier one
                // in path order, as the refusal that reports it promises.
                assert!(earlier < name, "an earlier sibling sorts before {name:?}");
                Err(earlier)
            }
            Entry::Vacant(slot) => {
                slot.insert(name);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Siblings;

    /// The first collision `names`, admitted in order, runs into, as
    /// `(earlier, later)`.
    fn first_collision<'listing>(
        names: &[&'listing str],
    ) -> Option<(&'listing str, &'listing str)> {
        let mut siblings = Siblings::new();
        for &name in names {
            if let Err(earlier) = siblings.admit(name) {
                return Some((earlier, name));
            }
        }
        None
    }

    #[test]
    fn two_file_names_differing_only_in_case_collide_naming_the_earlier_first() {
        // `D` (0x44) sorts before `d` (0x64).
        assert_eq!(
            first_collision(&["Dependabot.yml", "dependabot.yml"]),
            Some(("Dependabot.yml", "dependabot.yml"))
        );
    }

    #[test]
    fn two_directory_names_differing_only_in_case_collide_as_names() {
        // A directory is an entry of its parent like a file is: `A` and `a`
        // collide as names, whatever each one holds.
        assert_eq!(first_collision(&["A", "a"]), Some(("A", "a")));
    }

    #[test]
    fn nfc_and_nfd_spellings_collide_naming_the_earlier_first() {
        // NFD `e` + U+0301 begins with `e` (0x65); NFC `é` begins 0xC3.
        assert_eq!(
            first_collision(&["e\u{301}.txt", "\u{e9}.txt"]),
            Some(("e\u{301}.txt", "\u{e9}.txt"))
        );
    }

    #[test]
    fn names_that_do_not_collide_are_all_admitted() {
        assert_eq!(
            first_collision(&[
                "a.yml",
                "a.yml.bak",
                "b.yml",
                "dependabot.yaml",
                "dependabot.yml"
            ]),
            None
        );
    }

    #[test]
    fn the_first_collision_is_at_the_first_name_colliding_with_any_before_it() {
        // `P` collides with `p` and `Q` with `q`. Admitted in byte order —
        // `P`, `Q`, `p`, `q` — the first name to meet an earlier sibling is
        // `p`, so the collision reported is `P` with `p`, never `Q` with
        // `q`.
        assert_eq!(first_collision(&["P", "Q", "p", "q"]), Some(("P", "p")));
        // With `p` absent, nothing collides until `q` meets `Q`.
        assert_eq!(first_collision(&["P", "Q", "R", "q"]), Some(("Q", "q")));
    }

    #[test]
    #[should_panic(expected = "strictly ascending order")]
    fn admitting_out_of_order_panics() {
        let mut siblings = Siblings::new();
        let _admitted = siblings.admit("b");
        let _never_reached = siblings.admit("a");
    }
}
