//! The lock entries a wearing can resolve to: the only packages whose files are worth reading.

use std::collections::BTreeSet;

use super::layout::Layout;
use super::lock_entries::path_entry_index;
use super::lockfile::Lockfile;
use super::package_manifest::Declaration;
use super::path_package::PathPackage;
use crate::workspace::wearing_table::wearing_tables;

/// The positions, in [`Lockfile::entries`], of the entries a wearing table of some member can
/// resolve to: those its lock entry lists under the name of a package that the member declares
/// under a key the table names. Entries in `set_aside` are never among them.
///
/// A caller may give the location of every locked package with a source, and a package that
/// nothing wears has to be able to sit at a location Cargo would refuse: nothing reads it, so
/// nothing is wrong with it.
pub(super) fn worn_entries(
    layout: &Layout,
    lockfile: &Lockfile,
    set_aside: &BTreeSet<usize>,
) -> BTreeSet<usize> {
    let mut entries = BTreeSet::new();
    for member in layout.packages.iter().filter(|package| package.is_member) {
        let Some(entry_index) = path_entry_index(lockfile, &member.name, &member.version) else {
            // The lockfile does not hold the member, which the document refuses on its own.
            continue;
        };
        let worn_packages: BTreeSet<&str> = worn_declarations(member)
            .into_iter()
            .map(|declaration| declaration.package.as_str())
            .collect();
        for &dependency_index in lockfile.entries()[entry_index].dependency_indexes() {
            let dependency = &lockfile.entries()[dependency_index];
            if !set_aside.contains(&dependency_index) && worn_packages.contains(dependency.name()) {
                entries.insert(dependency_index);
            }
        }
    }
    entries
}

/// The declarations of `member` that its wearing tables name: those written under a key some
/// `[package.metadata.skeletons.<key>]` table has.
///
/// This is the same reading `from_document` makes of the tables, so the two agree on which keys
/// are worn. A table that is not one refuses the whole member there, so nothing is worn.
pub(super) fn worn_declarations(member: &PathPackage) -> Vec<&Declaration> {
    let Ok((tables, _refusals)) = wearing_tables(&member.metadata, |key| {
        member
            .declarations
            .iter()
            .any(|declaration| declaration.key == key)
    }) else {
        return Vec::new();
    };
    member
        .declarations
        .iter()
        .filter(|declaration| {
            tables
                .keys()
                .any(|worn_key| worn_key.as_str() == declaration.key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::worn_entries;
    use crate::workspace::without_cargo::fixture::Fixture;
    use crate::workspace::without_cargo::layout;
    use crate::workspace::without_cargo::lock_entries::set_aside_entries;
    use crate::workspace::without_cargo::lockfile::Lockfile;

    const LOCK: &str = "version = 4\n\n\
        [[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\
        dependencies = [\"house-rules\", \"plain\", \"tests-only\"]\n\n\
        [[package]]\nname = \"house-rules\"\nversion = \"1.0.0\"\n\
        source = \"git+https://example.test/house-rules#0123456789abcdef\"\n\n\
        [[package]]\nname = \"plain\"\nversion = \"2.0.0\"\n\
        source = \"git+https://example.test/plain#0123456789abcdef\"\n\n\
        [[package]]\nname = \"tests-only\"\nversion = \"3.0.0\"\n\
        source = \"git+https://example.test/tests-only#0123456789abcdef\"\n";

    #[test]
    fn only_the_package_under_a_worn_key_is_worn_even_when_that_key_renames_it() {
        // Reads the lock entries of a member that depends on three packages with a source: one
        // under a renamed key it wears, one it does not wear, and a dev dependency that shares
        // no key with a wearing table. Only the renamed, worn one is in the set, named by its
        // package and not by the key it is written under.
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package(
            "app",
            "app",
            "version = \"0.1.0\"\n\
             [dependencies]\n\
             worn-rules = { package = \"house-rules\", \
             git = \"https://example.test/house-rules\" }\n\
             plain = { git = \"https://example.test/plain\" }\n\
             [dev-dependencies]\n\
             tests-only = { git = \"https://example.test/tests-only\" }\n\
             [package.metadata.skeletons.worn-rules]\n",
        );
        let layout = layout::discover(&fixture.root).expect("a layout");
        let lockfile = Lockfile::parse(LOCK).expect("a lockfile");
        let set_aside = set_aside_entries(&lockfile, &layout.packages);

        let worn = worn_entries(&layout, &lockfile, &set_aside);

        let names: Vec<&str> = worn
            .iter()
            .map(|&index| lockfile.entries()[index].name())
            .collect();
        assert_eq!(names, ["house-rules"]);
    }

    #[test]
    fn a_member_whose_metadata_table_is_not_a_table_wears_nothing() {
        // The member writes `skeletons` as a string, which refuses its whole wearing, so no
        // package it depends on is worn.
        let fixture = Fixture::new();
        fixture.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        fixture.package(
            "app",
            "app",
            "version = \"0.1.0\"\n\
             [dependencies]\n\
             house-rules = { git = \"https://example.test/house-rules\" }\n\
             [package.metadata]\n\
             skeletons = \"house-rules\"\n",
        );
        let layout = layout::discover(&fixture.root).expect("a layout");
        let lockfile = Lockfile::parse(LOCK).expect("a lockfile");
        let set_aside = set_aside_entries(&lockfile, &layout.packages);

        let worn = worn_entries(&layout, &lockfile, &set_aside);

        assert!(worn.is_empty(), "{worn:?}");
    }
}
