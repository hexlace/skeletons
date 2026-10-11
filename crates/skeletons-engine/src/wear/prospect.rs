//! The refusals `wear` makes before it writes anything, from what the
//! workspace already holds.
//!
//! `cargo add` would quietly rewrite a dependency that is already there, so
//! each of these has to be said first, in `wear`'s own words and with nothing
//! yet changed. They are checked in the order a wearer would want to hear
//! them: the biggest misunderstanding first.

use super::refusal::{Parent, WearRefusal};
use super::request::Request;
use crate::workspace::{
    Declared, Member, Prospect, SkeletonsTable, Wearing, Workspace, same_crate, underscored,
};

/// Says whether `wear` may add `request`'s skeleton to the package `package`,
/// the one the command line was built from, in `prospect`'s workspace, and
/// hands back the member it was found to be.
///
/// The member returned is the one place that decides the package is in the
/// workspace: nothing after this asks again.
///
/// In order, it refuses:
///
/// 1. a package that is no member of the workspace;
/// 2. a skeleton any member already wears;
/// 3. a `[package.metadata]` or a `[package.metadata.skeletons]` that is not a
///    table;
/// 4. a dependency on this skeleton's crate under any key, which is not worn,
///    in any dependency table, with a wearing table at its key or without;
/// 5. a key a dependency on another crate already holds;
/// 6. a wearing table at the key with no dependency under it, or one at
///    another spelling of the key.
///
/// A dependency's key, and the crate it is on, are compared as `rustc` names
/// them, with `-` and `_` the same, because two dependencies that differ only
/// there are one name to the compiler and to the reader that finds a
/// skeleton's files.
///
/// # Errors
///
/// Returns the [`WearRefusal`] for the first of those that holds.
pub(crate) fn check<'prospect>(
    request: &Request,
    prospect: &'prospect Prospect,
    package: &str,
) -> Result<&'prospect Member, WearRefusal> {
    let Some(member) = &prospect.member else {
        return Err(WearRefusal::OutsideItsProject {
            package: package.to_owned(),
            root: prospect.workspace.root.display().to_string(),
        });
    };
    refuse_worn(request, &prospect.workspace)?;
    refuse_parent_not_a_table(member)?;
    refuse_depends_without_wearing(request, member)?;
    refuse_taken_key(request, member)?;
    refuse_table_without_dependency(request, member)?;
    Ok(member)
}

/// A skeleton some member of the workspace already wears, under any key.
///
/// A wearing that was refused wears nothing, so it does not count: it is a
/// problem of its own, which `check` reports, and not a reason to stop this.
fn refuse_worn(request: &Request, workspace: &Workspace) -> Result<(), WearRefusal> {
    let refusal = workspace.wearing.iter().find_map(|wearing| match wearing {
        Wearing::Worn(worn) if same_crate(&worn.package, request.crate_name().as_str()) => {
            Some(WearRefusal::AlreadyWorn {
                package: worn.package.clone(),
                key: worn.key.clone(),
                manifest: worn.manifest.clone(),
            })
        }
        Wearing::Worn(_) | Wearing::Refused(_) => None,
    });
    refusal.map_or(Ok(()), Err)
}

/// A table the wearing table goes under that is not a table.
///
/// A non-table `package` is not among them: Cargo refuses a manifest whose
/// `package` is not a table before `wear` can read it, so none reaches here.
/// `metadata` and `skeletons` are free-form to Cargo, so they do.
fn refuse_parent_not_a_table(member: &Member) -> Result<(), WearRefusal> {
    let parent = match member.skeletons {
        SkeletonsTable::MetadataNotATable => Parent::Metadata,
        SkeletonsTable::SkeletonsNotATable => Parent::Skeletons,
        SkeletonsTable::Absent | SkeletonsTable::Keys(_) => return Ok(()),
    };
    Err(WearRefusal::ParentNotATable {
        manifest: member.manifest.clone(),
        parent,
    })
}

/// A dependency on the skeleton's crate, under whatever key and in whatever
/// dependency table.
///
/// Adding it again under the key asked for would give the manifest two
/// dependencies on one crate, which Cargo refuses only once `cargo add` has
/// written it, and without saying what to do. The remedy is a wearing table at
/// the key it is declared under, unless the table is already there: then it
/// is not worn because its wearing is refused (a worn one was refused above),
/// and a second table is no remedy, so the refusal sends the wearer to `check`
/// for the reason.
fn refuse_depends_without_wearing(request: &Request, member: &Member) -> Result<(), WearRefusal> {
    let refusal = member
        .declared
        .iter()
        .find(|declared| is_the_skeleton(request, declared))
        .map(|holder| {
            let manifest = member.manifest.clone();
            let crate_name = holder.crate_name.clone();
            let key = holder.key.clone();
            match &member.skeletons {
                SkeletonsTable::Keys(keys) if keys.contains(&holder.key) => {
                    WearRefusal::DependsWithRefusedWearing {
                        manifest,
                        crate_name,
                        key,
                    }
                }
                SkeletonsTable::Keys(_)
                | SkeletonsTable::Absent
                | SkeletonsTable::MetadataNotATable
                | SkeletonsTable::SkeletonsNotATable => WearRefusal::DependsWithoutWearing {
                    manifest,
                    crate_name,
                    key,
                },
            }
        });
    refusal.map_or(Ok(()), Err)
}

/// A dependency already declared under the key, or under a spelling of it
/// `rustc` takes as the same.
///
/// It is on another crate: a dependency on the skeleton itself is refused
/// before this is asked.
fn refuse_taken_key(request: &Request, member: &Member) -> Result<(), WearRefusal> {
    let key = request.key().underscored();
    let refusal = member
        .declared
        .iter()
        .find(|declared| underscored(&declared.key) == key)
        .map(|holder| WearRefusal::KeyTaken {
            manifest: member.manifest.clone(),
            key: request.key().as_str().to_owned(),
            other: holder.crate_name.clone(),
            existing: holder.key.clone(),
        });
    refusal.map_or(Ok(()), Err)
}

/// Whether `declared` is a dependency on the crate `request` asks for, with
/// `-` and `_` the same as Cargo takes them to be in a crate's name.
fn is_the_skeleton(request: &Request, declared: &Declared) -> bool {
    same_crate(&declared.crate_name, request.crate_name().as_str())
}

/// A wearing table already at the key, which `wear` would have to overwrite or
/// adopt: it does neither. Or one at another spelling of the key, which
/// `rustc` takes as the same name: `wear` would write a second table beside
/// it, and `sync` would refuse the pair.
///
/// A table at exactly the key is said as it always was, with the choice of
/// another key. A table at another spelling is said with one remedy, removing
/// it: a different key would leave the stray table, which `check` refuses
/// anyway.
fn refuse_table_without_dependency(request: &Request, member: &Member) -> Result<(), WearRefusal> {
    match &member.skeletons {
        SkeletonsTable::Keys(keys) if keys.contains(request.key().as_str()) => {
            Err(WearRefusal::TableWithoutDependency {
                manifest: member.manifest.clone(),
                key: request.key().as_str().to_owned(),
            })
        }
        SkeletonsTable::Keys(keys) => {
            // One is enough to name: the keys come in sorted order, and
            // removing the one named and running again names the next.
            let other = keys
                .iter()
                .find(|table| underscored(table) == request.key().underscored());
            other.map_or(Ok(()), |table| {
                Err(WearRefusal::TableAtOtherSpelling {
                    manifest: member.manifest.clone(),
                    table: table.clone(),
                    key: request.key().as_str().to_owned(),
                })
            })
        }
        SkeletonsTable::Absent
        | SkeletonsTable::MetadataNotATable
        | SkeletonsTable::SkeletonsNotATable => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::wear::refusal::{Parent, WearRefusal};
    use crate::wear::request::Request;
    use crate::wear::source::Source;
    use crate::wear::test_workspace::{not_a_skeleton, workspace_of, worn};
    use crate::workspace::{Declared, Member, Prospect, SkeletonsTable, Wearing};

    /// What [`check`] refuses, leaving out the member it hands back.
    fn checked(request: &Request, prospect: &Prospect, package: &str) -> Result<(), WearRefusal> {
        check(request, prospect, package).map(|_| ())
    }

    fn request(skeleton: &str, key: Option<&str>) -> Request {
        Request::new(skeleton, key, Source::Registry)
            .expect("a test passes only requests it knows are valid")
    }

    fn declared(key: &str, crate_name: &str) -> Declared {
        Declared {
            key: key.to_owned(),
            crate_name: crate_name.to_owned(),
        }
    }

    fn keys(names: &[&str]) -> SkeletonsTable {
        SkeletonsTable::Keys(names.iter().map(|name| (*name).to_owned()).collect())
    }

    /// A prospect whose member holds `declared` and `skeletons`, in a
    /// workspace that wears `wearing`.
    fn prospect(
        wearing: Vec<Wearing>,
        declared: Vec<Declared>,
        skeletons: SkeletonsTable,
    ) -> Prospect {
        Prospect {
            workspace: workspace_of(wearing),
            member: Some(Member {
                manifest_path: "/workspace/Cargo.toml".into(),
                manifest: "Cargo.toml".to_owned(),
                declared,
                skeletons,
            }),
        }
    }

    fn empty() -> Prospect {
        prospect(Vec::new(), Vec::new(), SkeletonsTable::Absent)
    }

    #[test]
    fn a_workspace_with_nothing_in_the_way_is_clear() {
        assert_eq!(checked(&request("tidy", None), &empty(), "cli"), Ok(()));
    }

    #[test]
    fn a_clear_workspace_hands_back_the_member_it_found() {
        let prospect = empty();

        let member = check(&request("tidy", None), &prospect, "cli")
            .expect("nothing in this workspace is in the way");

        assert!(std::ptr::eq(
            member,
            prospect
                .member
                .as_ref()
                .expect("the prospect holds a member")
        ));
    }

    #[test]
    fn a_package_that_is_no_member_is_refused_naming_the_package_and_the_root() {
        // A command line run from a project it was not built from: there is no
        // manifest of its own to write into.
        let prospect = Prospect {
            workspace: workspace_of(Vec::new()),
            member: None,
        };

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::OutsideItsProject {
                package: "cli".to_owned(),
                root: "/workspace".to_owned(),
            })
        );
    }

    #[test]
    fn a_skeleton_already_worn_is_refused_naming_where() {
        // The pre-check names the entry that wears it, as the post-write one
        // does, so the wearer is told the same thing whichever finds it.
        let prospect = prospect(
            vec![worn("tools/Cargo.toml", "neat", "tidy")],
            Vec::new(),
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::AlreadyWorn {
                package: "tidy".to_owned(),
                key: "neat".to_owned(),
                manifest: "tools/Cargo.toml".to_owned(),
            })
        );
    }

    #[test]
    fn a_skeleton_worn_by_another_member_is_already_worn_too() {
        let prospect = prospect(
            vec![worn("other/Cargo.toml", "tidy", "tidy")],
            Vec::new(),
            SkeletonsTable::Absent,
        );

        assert!(matches!(
            checked(&request("tidy", Some("fresh")), &prospect, "cli"),
            Err(WearRefusal::AlreadyWorn { .. })
        ));
    }

    #[test]
    fn a_skeleton_worn_under_one_spelling_is_already_worn_under_the_other() {
        // crates.io holds `foo-bar` and `foo_bar` as one name, so asking for
        // either is asking for the skeleton that is already worn, whichever
        // spelling the package carries.
        for (worn_as, requested) in [("foo-bar", "foo_bar"), ("foo_bar", "foo-bar")] {
            let prospect = prospect(
                vec![worn("Cargo.toml", "neat", worn_as)],
                Vec::new(),
                SkeletonsTable::Absent,
            );

            assert_eq!(
                checked(&request(requested, Some("fresh")), &prospect, "cli"),
                Err(WearRefusal::AlreadyWorn {
                    package: worn_as.to_owned(),
                    key: "neat".to_owned(),
                    manifest: "Cargo.toml".to_owned(),
                }),
                "worn as {worn_as}, requested as {requested}"
            );
        }
    }

    #[test]
    fn a_different_skeleton_worn_is_no_obstacle() {
        let prospect = prospect(
            vec![worn("Cargo.toml", "other", "other")],
            Vec::new(),
            keys(&["other"]),
        );

        assert_eq!(checked(&request("tidy", None), &prospect, "cli"), Ok(()));
    }

    #[test]
    fn a_wearing_that_was_refused_does_not_block_the_key() {
        // A package of the same name whose wearing was refused is no skeleton
        // worn. That refusal is `check`'s to report, and is no reason to
        // refuse this.
        let prospect = prospect(
            vec![not_a_skeleton("Cargo.toml", "plain", "tidy")],
            Vec::new(),
            keys(&["plain"]),
        );

        assert_eq!(checked(&request("tidy", None), &prospect, "cli"), Ok(()));
    }

    #[test]
    fn a_skeletons_entry_that_is_not_a_table_is_refused_by_its_header() {
        let prospect = prospect(Vec::new(), Vec::new(), SkeletonsTable::SkeletonsNotATable);

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::ParentNotATable {
                manifest: "Cargo.toml".to_owned(),
                parent: Parent::Skeletons,
            })
        );
    }

    #[test]
    fn a_metadata_that_is_not_a_table_is_refused_by_its_header() {
        let prospect = prospect(Vec::new(), Vec::new(), SkeletonsTable::MetadataNotATable);

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::ParentNotATable {
                manifest: "Cargo.toml".to_owned(),
                parent: Parent::Metadata,
            })
        );
    }

    #[test]
    fn a_key_taken_by_another_crate_is_refused_naming_the_crate_and_its_key() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("taken", "enum-fill")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", Some("taken")), &prospect, "cli"),
            Err(WearRefusal::KeyTaken {
                manifest: "Cargo.toml".to_owned(),
                key: "taken".to_owned(),
                other: "enum-fill".to_owned(),
                existing: "taken".to_owned(),
            })
        );
    }

    #[test]
    fn a_default_key_taken_by_a_dependency_of_that_name_on_another_crate_is_taken() {
        // A rename can take the name a crate would default to.
        let prospect = prospect(
            Vec::new(),
            vec![declared("tidy", "enum-fill")],
            SkeletonsTable::Absent,
        );

        assert!(matches!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::KeyTaken { .. })
        ));
    }

    #[test]
    fn a_hyphen_and_an_underscore_collide_because_rustc_names_them_alike() {
        // `a_x` and `a-x` are the one extern-crate name `a_x`, so a dependency
        // under one takes the other, in either direction.
        for (held, asked) in [("a_x", "a-x"), ("a-x", "a_x")] {
            let prospect = prospect(
                Vec::new(),
                vec![declared(held, "enum-fill")],
                SkeletonsTable::Absent,
            );

            assert_eq!(
                checked(&request("tidy", Some(asked)), &prospect, "cli"),
                Err(WearRefusal::KeyTaken {
                    manifest: "Cargo.toml".to_owned(),
                    key: asked.to_owned(),
                    other: "enum-fill".to_owned(),
                    existing: held.to_owned(),
                }),
                "{held} against {asked}"
            );
        }
    }

    #[test]
    fn a_key_that_differs_by_more_than_the_hyphen_is_free() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("a_xy", "enum-fill"), declared("A_x", "enum-fill")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", Some("a-x")), &prospect, "cli"),
            Ok(())
        );
    }

    #[test]
    fn the_skeleton_itself_under_the_key_is_a_dependency_without_wearing_not_a_taken_key() {
        // The crate asked for is already declared under the key and nothing
        // wears it, so what is missing is the table, and the remedy is to add it.
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "tidy")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", Some("neat")), &prospect, "cli"),
            Err(WearRefusal::DependsWithoutWearing {
                manifest: "Cargo.toml".to_owned(),
                crate_name: "tidy".to_owned(),
                key: "neat".to_owned(),
            })
        );
    }

    #[test]
    fn the_skeleton_with_a_wearing_table_at_its_key_is_a_refused_wearing_not_a_missing_table() {
        // The table is there and the skeleton is not worn (a worn one is
        // refused earlier), so the wearing was refused. Adding a table is no
        // remedy, and the remedy that is, `check`, is the one to say. The
        // table is found at the key the dependency has, not the one typed.
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "tidy")],
            keys(&["neat", "other"]),
        );

        for asked in [None, Some("fresh"), Some("neat")] {
            assert_eq!(
                checked(&request("tidy", asked), &prospect, "cli"),
                Err(WearRefusal::DependsWithRefusedWearing {
                    manifest: "Cargo.toml".to_owned(),
                    crate_name: "tidy".to_owned(),
                    key: "neat".to_owned(),
                }),
                "{asked:?}"
            );
        }
    }

    #[test]
    fn a_wearing_table_under_another_key_leaves_the_dependency_without_wearing() {
        // Only a table at the dependency's own key is its wearing, matched
        // exactly as `sync` reads it.
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "tidy")],
            keys(&["other", "Neat"]),
        );

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            depends_without_wearing("tidy", "neat")
        );
    }

    #[test]
    fn the_skeleton_under_a_respelled_key_names_the_key_the_manifest_has() {
        // The table has to be named as the dependency is, so the remedy shows
        // the spelling the manifest holds, not the one that was typed.
        let prospect = prospect(
            Vec::new(),
            vec![declared("foo_bar", "foo-bar")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("foo_bar", Some("foo-bar")), &prospect, "cli"),
            Err(WearRefusal::DependsWithoutWearing {
                manifest: "Cargo.toml".to_owned(),
                crate_name: "foo-bar".to_owned(),
                key: "foo_bar".to_owned(),
            })
        );
    }

    #[test]
    fn a_target_specific_dependency_holds_its_key_like_any_other() {
        // `cargo metadata` lists a dependency under a `[target…]` table with
        // the rest (`workspace::prospect`'s captured-document test), so one
        // that holds the key on a platform nobody builds for still holds it.
        let prospect = prospect(
            Vec::new(),
            vec![declared("never", "enum-fill")],
            SkeletonsTable::Absent,
        );

        assert!(matches!(
            checked(&request("tidy", Some("never")), &prospect, "cli"),
            Err(WearRefusal::KeyTaken { .. })
        ));
    }

    /// What refusing a skeleton already depended on, under whatever key,
    /// looks like for the dependency `declared("neat", crate_name)`.
    fn depends_without_wearing(crate_name: &str, key: &str) -> Result<(), WearRefusal> {
        Err(WearRefusal::DependsWithoutWearing {
            manifest: "Cargo.toml".to_owned(),
            crate_name: crate_name.to_owned(),
            key: key.to_owned(),
        })
    }

    #[test]
    fn the_skeleton_under_another_key_is_a_dependency_without_wearing_naming_that_key() {
        // The key typed is free, but the crate is already a dependency under
        // `neat`. A second dependency on it would be refused by Cargo only
        // after being written, so the refusal comes first, and names the key
        // the wearing table has to be added at.
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "tidy")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            depends_without_wearing("tidy", "neat")
        );
        assert_eq!(
            checked(&request("tidy", Some("fresh")), &prospect, "cli"),
            depends_without_wearing("tidy", "neat")
        );
    }

    #[test]
    fn the_skeleton_as_a_dev_or_build_or_normal_dependency_is_one_all_the_same() {
        // `cargo metadata` lists a dependency whatever table declares it, so
        // the reading does not tell them apart and neither may the refusal:
        // every table's entry reaches `check` as one `Declared`, and each
        // is the skeleton depended on.
        for key in ["in-dev", "in-build", "in-normal"] {
            let prospect = prospect(
                Vec::new(),
                vec![declared("unrelated", "enum-fill"), declared(key, "tidy")],
                SkeletonsTable::Absent,
            );

            assert_eq!(
                checked(&request("tidy", None), &prospect, "cli"),
                depends_without_wearing("tidy", key),
                "{key}"
            );
        }
    }

    #[test]
    fn the_skeleton_under_another_key_in_a_target_specific_table_is_depended_on_too() {
        // A dependency under `[target.'cfg(any())'.dependencies]` is listed
        // like the rest (`workspace::prospect`'s captured-document test), and
        // Cargo counts it as a second dependency on the crate on every
        // platform, so it is refused as any other is.
        let prospect = prospect(
            Vec::new(),
            vec![declared("never", "tidy")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", Some("fresh")), &prospect, "cli"),
            depends_without_wearing("tidy", "never")
        );
    }

    #[test]
    fn a_hyphen_and_an_underscore_are_the_same_crate_under_another_key() {
        // `a_x` and `a-x` are one crate to Cargo, in either direction, so a
        // dependency spelled one way is the skeleton asked for the other way.
        for (held, asked) in [("a_x", "a-x"), ("a-x", "a_x")] {
            let prospect = prospect(
                Vec::new(),
                vec![declared("neat", held)],
                SkeletonsTable::Absent,
            );

            assert_eq!(
                checked(&request(asked, Some("fresh")), &prospect, "cli"),
                depends_without_wearing(held, "neat"),
                "{held} against {asked}"
            );
        }
    }

    #[test]
    fn a_dependency_on_the_skeleton_is_said_before_a_key_taken_by_another_crate() {
        // The key typed is held by another crate, and the skeleton is also
        // depended on under `neat`. Once the table is added at `neat` the
        // skeleton is worn and the run says so; a new key would not help, as
        // the crate is already a dependency. So the one remedy that works is
        // the one to say.
        let prospect = prospect(
            Vec::new(),
            vec![declared("taken", "enum-fill"), declared("neat", "tidy")],
            SkeletonsTable::Absent,
        );

        assert_eq!(
            checked(&request("tidy", Some("taken")), &prospect, "cli"),
            depends_without_wearing("tidy", "neat")
        );
    }

    #[test]
    fn a_wearing_table_with_no_dependency_is_refused() {
        let prospect = prospect(Vec::new(), Vec::new(), keys(&["tidy", "other"]));

        assert_eq!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::TableWithoutDependency {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            })
        );
    }

    #[test]
    fn a_wearing_table_under_another_key_is_no_obstacle() {
        let prospect = prospect(Vec::new(), Vec::new(), keys(&["other"]));

        assert_eq!(checked(&request("tidy", None), &prospect, "cli"), Ok(()));
    }

    #[test]
    fn a_wearing_table_at_the_other_spelling_of_the_key_is_refused_naming_it_as_written() {
        // `a_x` and `a-x` are one name to rustc, so a table at either is a
        // table for the key asked for at the other, in either direction.
        for (table, asked) in [("a_x", "a-x"), ("a-x", "a_x")] {
            let prospect = prospect(Vec::new(), Vec::new(), keys(&[table]));

            assert_eq!(
                checked(&request("tidy", Some(asked)), &prospect, "cli"),
                Err(WearRefusal::TableAtOtherSpelling {
                    manifest: "Cargo.toml".to_owned(),
                    table: table.to_owned(),
                    key: asked.to_owned(),
                }),
                "table {table} against {asked}"
            );
        }
    }

    #[test]
    fn a_table_at_exactly_the_key_is_still_the_table_without_a_dependency() {
        // The exact spelling keeps its own refusal, even when a table at
        // another spelling is there too.
        let prospect = prospect(Vec::new(), Vec::new(), keys(&["a_x", "a-x"]));

        assert_eq!(
            checked(&request("tidy", Some("a-x")), &prospect, "cli"),
            Err(WearRefusal::TableWithoutDependency {
                manifest: "Cargo.toml".to_owned(),
                key: "a-x".to_owned(),
            })
        );
    }

    #[test]
    fn the_first_of_several_other_spellings_is_the_one_named() {
        // `a_b-c` and `a-b_c` are both other spellings of `a-b-c`; the keys
        // are sorted, so `a-b_c` comes first.
        let prospect = prospect(Vec::new(), Vec::new(), keys(&["a_b-c", "a-b_c"]));

        assert_eq!(
            checked(&request("tidy", Some("a-b-c")), &prospect, "cli"),
            Err(WearRefusal::TableAtOtherSpelling {
                manifest: "Cargo.toml".to_owned(),
                table: "a-b_c".to_owned(),
                key: "a-b-c".to_owned(),
            })
        );
    }

    #[test]
    fn a_table_that_differs_by_more_than_the_hyphen_is_no_obstacle() {
        let prospect = prospect(Vec::new(), Vec::new(), keys(&["a_xy", "A_x"]));

        assert_eq!(
            checked(&request("tidy", Some("a-x")), &prospect, "cli"),
            Ok(())
        );
    }

    #[test]
    fn already_worn_is_said_before_a_taken_key() {
        // The skeleton is worn, and the key is also held by another crate:
        // the first is the bigger misunderstanding and the one to say.
        let prospect = prospect(
            vec![worn("Cargo.toml", "tidy", "tidy")],
            vec![declared("neat", "enum-fill")],
            keys(&["neat"]),
        );

        assert!(matches!(
            checked(&request("tidy", Some("neat")), &prospect, "cli"),
            Err(WearRefusal::AlreadyWorn { .. })
        ));
    }

    #[test]
    fn a_skeletons_entry_that_is_no_table_is_said_before_a_taken_key() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "enum-fill")],
            SkeletonsTable::SkeletonsNotATable,
        );

        assert!(matches!(
            checked(&request("tidy", Some("neat")), &prospect, "cli"),
            Err(WearRefusal::ParentNotATable { .. })
        ));
    }

    #[test]
    fn a_metadata_that_is_no_table_is_said_before_a_taken_key() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "enum-fill")],
            SkeletonsTable::MetadataNotATable,
        );

        assert!(matches!(
            checked(&request("tidy", Some("neat")), &prospect, "cli"),
            Err(WearRefusal::ParentNotATable {
                parent: Parent::Metadata,
                ..
            })
        ));
    }

    #[test]
    fn a_metadata_that_is_no_table_is_said_before_a_dependency_without_wearing() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "tidy")],
            SkeletonsTable::MetadataNotATable,
        );

        assert!(matches!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::ParentNotATable {
                parent: Parent::Metadata,
                ..
            })
        ));
    }

    #[test]
    fn a_taken_key_is_said_before_a_table_with_no_dependency() {
        let prospect = prospect(
            Vec::new(),
            vec![declared("neat", "enum-fill")],
            keys(&["neat"]),
        );

        assert!(matches!(
            checked(&request("tidy", Some("neat")), &prospect, "cli"),
            Err(WearRefusal::KeyTaken { .. })
        ));
    }

    #[test]
    fn a_package_outside_the_workspace_is_said_before_anything_else() {
        let prospect = Prospect {
            workspace: workspace_of(vec![worn("Cargo.toml", "tidy", "tidy")]),
            member: None,
        };

        assert!(matches!(
            checked(&request("tidy", None), &prospect, "cli"),
            Err(WearRefusal::OutsideItsProject { .. })
        ));
    }
}
