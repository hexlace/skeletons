//! Which lock entries the workspace's members reach.

use std::collections::BTreeSet;

use crate::workspace::without_cargo::lockfile::Lockfile;

/// Whether every entry of `lockfile` that is not in `set_aside` is reachable from the entries at
/// `roots`, following dependencies and skipping the entries set aside.
///
/// Cargo writes a lockfile of only the entries the members reach, so an entry nobody depends on,
/// and one only a set-aside entry depends on, make the lockfile differ from the one it would
/// write.
pub(super) fn every_entry_is_reached(
    lockfile: &Lockfile,
    set_aside: &BTreeSet<usize>,
    roots: &[usize],
) -> bool {
    let entry_count = lockfile.entries().len();
    let mut reached = vec![false; entry_count];
    let mut pending: Vec<usize> = Vec::with_capacity(entry_count);

    // Each entry is pushed at most once, when it is first reached, a root included however many
    // times `roots` names it, so the stack holds at most `entry_count` entries and the loop runs
    // at most `entry_count` times.
    let mut reached_count: usize = 0;
    for &root in roots {
        assert!(root < entry_count, "a root is the position of an entry");
        if !reached[root] {
            reached[root] = true;
            reached_count += 1;
            pending.push(root);
        }
    }
    while let Some(index) = pending.pop() {
        assert!(
            index < entry_count,
            "only the position of an entry was ever pushed"
        );
        for &dependency in lockfile.entries()[index].dependency_indexes() {
            assert!(
                dependency < entry_count,
                "the lockfile resolves a dependency to the position of an entry"
            );
            if !set_aside.contains(&dependency) && !reached[dependency] {
                reached[dependency] = true;
                reached_count += 1;
                pending.push(dependency);
            }
        }
        assert!(
            reached_count <= entry_count,
            "an entry is marked reached once, so no more entries are reached than exist"
        );
        assert!(
            pending.len() <= reached_count,
            "an entry is pushed only as it is marked reached"
        );
    }

    (0..entry_count).all(|index| reached[index] || set_aside.contains(&index))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::every_entry_is_reached;
    use crate::workspace::without_cargo::lockfile::Lockfile;

    /// `a` depends on `b`, which depends on `c`; `d` stands alone.
    const CHAIN_AND_A_LONER: &str = r#"
version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b"]

[[package]]
name = "b"
version = "0.1.0"
dependencies = ["c"]

[[package]]
name = "c"
version = "0.1.0"

[[package]]
name = "d"
version = "0.1.0"
"#;

    fn chain() -> Lockfile {
        Lockfile::parse(CHAIN_AND_A_LONER).expect("the lockfile in this test is well formed")
    }

    #[test]
    fn an_entry_nobody_depends_on_is_not_reached() {
        assert!(!every_entry_is_reached(&chain(), &BTreeSet::new(), &[0]));
    }

    #[test]
    fn the_entries_of_a_chain_are_reached_through_their_dependencies() {
        let set_aside = BTreeSet::from([3]);

        assert!(every_entry_is_reached(&chain(), &set_aside, &[0]));
    }

    #[test]
    fn an_entry_only_a_set_aside_entry_depends_on_is_not_reached() {
        // `b` is set aside, so `c`, which only it depends on, has nothing reaching it.
        let set_aside = BTreeSet::from([1, 3]);

        assert!(!every_entry_is_reached(&chain(), &set_aside, &[0]));
    }

    #[test]
    fn a_second_root_reaches_the_entry_the_first_does_not() {
        assert!(every_entry_is_reached(&chain(), &BTreeSet::new(), &[0, 3]));
    }
}
