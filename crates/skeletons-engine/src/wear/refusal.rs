//! Every refusal `wear` itself words, and the one place each is worded.
//!
//! Text from outside, a name the wearer typed or what cargo said, goes
//! through [`Escaped`], so a message is one line however its words came.
//! Every message ends by saying what to do, or says that nothing is left to
//! do because the defect is in `skeletons`.

use rituals::Failure;

use super::request::KEY_BYTES_MAX;
use crate::skeleton::Escaped;

/// A table `wear` has to descend through to reach where a wearing table goes,
/// when that is not a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    Package,
    Metadata,
    Skeletons,
}

impl Parent {
    /// The table's name in a manifest, as a person writes its header.
    pub(crate) const fn header(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Metadata => "package.metadata",
            Self::Skeletons => "package.metadata.skeletons",
        }
    }

    /// The key the table has inside the table above it.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Metadata => "metadata",
            Self::Skeletons => "skeletons",
        }
    }
}

/// Why `wear` refused, with what its message names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WearRefusal {
    /// The crate named is not a name `wear` can hand to `cargo add`.
    CrateNameInvalid { crate_name: String },
    /// The key given is not a key `wear` can hand to `cargo add`.
    KeyInvalid { key: String },
    /// The key is one a skeleton keeps for its own declarations.
    KeyReserved { key: String },
    /// The key, as `rustc` names it, is a crate the compiler provides to every
    /// build, which a dependency under that key would shadow.
    KeyShadowsCompiler { key: String },
    /// The package the command line was built from is not a member of the
    /// workspace `wear` ran in.
    OutsideItsProject { package: String, root: String },
    /// A workspace member already wears this skeleton.
    AlreadyWorn {
        package: String,
        key: String,
        manifest: String,
    },
    /// The manifest already depends on the skeleton, under `key` and not
    /// necessarily the key asked for, without wearing it.
    DependsWithoutWearing {
        manifest: String,
        crate_name: String,
        key: String,
    },
    /// The manifest already depends on the skeleton and has a wearing table at
    /// that dependency's key, and the wearing is refused, so it is not worn.
    DependsWithRefusedWearing {
        manifest: String,
        crate_name: String,
        key: String,
    },
    /// The key is already the name of a dependency on another crate.
    KeyTaken {
        manifest: String,
        key: String,
        other: String,
        existing: String,
    },
    /// `cargo add` could not be run at all.
    CargoAddUnavailable { detail: String },
    /// `cargo add` ran and refused.
    CargoAddFailed { stderr: String },
    /// The crate `cargo add` added is not a skeleton.
    NotASkeleton { package: String, version: String },
    /// The manifest could not be read to add the wearing table to it.
    ManifestUnreadable { manifest: String, detail: String },
    /// A table the wearing table goes under is not a table.
    ParentNotATable { manifest: String, parent: Parent },
    /// The manifest already has a wearing table for the key.
    TableWithoutDependency { manifest: String, key: String },
    /// The manifest has a wearing table at another spelling of the key, which
    /// `rustc` takes as the same name.
    TableAtOtherSpelling {
        manifest: String,
        table: String,
        key: String,
    },
    /// `cargo add` declared the dependency under `spelled`, the spelling
    /// crates.io has for the crate, and not under the key it was given, which
    /// is one name to `rustc`.
    CrateSpelledDifferently { key: String, spelled: String },
    /// The wearing table written differs from the one meant, or changes
    /// something else in the manifest, for the reason `detail` gives.
    //
    // A defect in `skeletons` would ordinarily panic, and this is an `Err` on
    // purpose. `rollback` puts the manifest and lockfile back when a run
    // returns a failure and does not when it panics, and `cargo add` has
    // already changed both by the time this is found, so a panic would leave
    // the project half-written.
    TableDefect {
        manifest: String,
        key: String,
        detail: String,
    },
    /// Reading the workspace back refused the new wearing.
    NotConfirmed {
        manifest: String,
        key: String,
        reason: String,
    },
    /// Reading the workspace back does not report the new wearing at all.
    //
    // A defect in `skeletons` as well, and an `Err` for the reason
    // `TableDefect` is one: it is found after `cargo add` and the table write,
    // so only a returned failure makes `rollback` undo them.
    NotReported { manifest: String, key: String },
}

impl WearRefusal {
    /// The refusal's message: one line, ending in what to do.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::CrateNameInvalid { crate_name } => crate_name_invalid_message(crate_name),
            Self::KeyInvalid { key } => key_invalid_message(key),
            Self::KeyReserved { key } => key_reserved_message(key),
            Self::KeyShadowsCompiler { key } => key_shadows_compiler_message(key),
            Self::OutsideItsProject { package, root } => outside_its_project_message(package, root),
            Self::AlreadyWorn {
                package,
                key,
                manifest,
            } => already_worn_message(package, key, manifest),
            Self::DependsWithoutWearing {
                manifest,
                crate_name,
                key,
            } => depends_without_wearing_message(manifest, crate_name, key),
            Self::DependsWithRefusedWearing {
                manifest,
                crate_name,
                key,
            } => depends_with_refused_wearing_message(manifest, crate_name, key),
            Self::KeyTaken {
                manifest,
                key,
                other,
                existing,
            } => key_taken_message(manifest, key, other, existing),
            Self::CargoAddUnavailable { detail } => {
                format!("running `cargo add` failed: {}", Escaped(detail))
            }
            Self::CargoAddFailed { stderr } => format!("cargo add failed: {}", Escaped(stderr)),
            Self::NotASkeleton { package, version } => not_a_skeleton_message(package, version),
            Self::ManifestUnreadable { manifest, detail } => format!(
                "wear could not read {} to add the wearing table: {}",
                Escaped(manifest),
                Escaped(detail)
            ),
            Self::ParentNotATable { manifest, parent } => {
                parent_not_a_table_message(manifest, *parent)
            }
            Self::TableWithoutDependency { manifest, key } => {
                table_without_dependency_message(manifest, key)
            }
            Self::TableAtOtherSpelling {
                manifest,
                table,
                key,
            } => table_at_other_spelling_message(manifest, table, key),
            Self::CrateSpelledDifferently { key, spelled } => {
                crate_spelled_differently_message(key, spelled)
            }
            Self::TableDefect {
                manifest,
                key,
                detail,
            } => table_defect_message(manifest, key, detail),
            Self::NotConfirmed {
                manifest,
                key,
                reason,
            } => not_confirmed_message(manifest, key, reason),
            Self::NotReported { manifest, key } => not_reported_message(manifest, key),
        }
    }

    /// This refusal as the [`Failure`] a task returns.
    pub(crate) fn into_failure(self) -> Failure {
        Failure::new(self.message())
    }
}

fn crate_name_invalid_message(crate_name: &str) -> String {
    format!(
        "`{}` is not a crate name wear can add: a name starts with an ASCII letter or `_`, \
         continues with ASCII letters, digits, `-` and `_`, and is at most {KEY_BYTES_MAX} \
         bytes; give the `wear` task the crate's name as its manifest spells it",
        Escaped(crate_name)
    )
}

fn key_invalid_message(key: &str) -> String {
    format!(
        "`{}` cannot be a dependency key: a key starts with an ASCII letter or `_`, continues \
         with ASCII letters, digits, `-` and `_`, and is at most {KEY_BYTES_MAX} bytes; give \
         the `wear` task another key as its second argument",
        Escaped(key)
    )
}

fn key_reserved_message(key: &str) -> String {
    format!(
        "`{}` cannot be worn as a dependency key: `options` and `verbatim` under \
         [package.metadata.skeletons] belong to a skeleton's own declaration; give the `wear` \
         task another key as its second argument",
        Escaped(key)
    )
}

fn key_shadows_compiler_message(key: &str) -> String {
    format!(
        "`{}` cannot be worn as a dependency key: it names a crate the compiler provides; give \
         the `wear` task another key as its second argument",
        Escaped(key)
    )
}

fn outside_its_project_message(package: &str, root: &str) -> String {
    format!(
        "this command line is built from the package `{}`, which is not a member of the \
         workspace at {}, so wear wrote nothing: run the `wear` task from inside the project \
         this command line belongs to",
        Escaped(package),
        Escaped(root)
    )
}

fn already_worn_message(package: &str, key: &str, manifest: &str) -> String {
    let (package, key, manifest) = (Escaped(package), Escaped(key), Escaped(manifest));
    format!(
        "{package} is already worn, as `{key}` in {manifest}; a workspace wears a skeleton \
         once, so to change its options, edit [package.metadata.skeletons.{key}] there"
    )
}

fn depends_without_wearing_message(manifest: &str, crate_name: &str, key: &str) -> String {
    let (manifest, crate_name, key) = (Escaped(manifest), Escaped(crate_name), Escaped(key));
    format!(
        "{manifest} already depends on {crate_name} under the key `{key}`, without wearing it; \
         to wear it, add an empty [package.metadata.skeletons.{key}] table to {manifest}"
    )
}

fn depends_with_refused_wearing_message(manifest: &str, crate_name: &str, key: &str) -> String {
    let (manifest, crate_name, key) = (Escaped(manifest), Escaped(crate_name), Escaped(key));
    format!(
        "{manifest} already depends on {crate_name} under the key `{key}`, and has a \
         [package.metadata.skeletons.{key}] table for it, but that wearing is refused; the \
         `check` task says why, so run it and fix what it names in {manifest}"
    )
}

fn key_taken_message(manifest: &str, key: &str, other: &str, existing: &str) -> String {
    format!(
        "`{}` is taken in {}: its dependency on {} is declared under `{}`; give the `wear` task \
         another key as its second argument",
        Escaped(key),
        Escaped(manifest),
        Escaped(other),
        Escaped(existing)
    )
}

fn not_a_skeleton_message(package: &str, version: &str) -> String {
    format!(
        "{} {} is not a skeleton: its manifest has no [package.metadata.skeletons] table, so \
         it cannot be worn; wear a crate that is one",
        Escaped(package),
        Escaped(version)
    )
}

fn parent_not_a_table_message(manifest: &str, parent: Parent) -> String {
    format!(
        "[{}] in {} is not a table, so wear cannot add a wearing table under it; make it a \
         table, then run the `wear` task again",
        parent.header(),
        Escaped(manifest)
    )
}

fn table_without_dependency_message(manifest: &str, key: &str) -> String {
    let (manifest, key) = (Escaped(manifest), Escaped(key));
    format!(
        "{manifest} already has a [package.metadata.skeletons.{key}] table, with no dependency \
         declared under `{key}`; remove the table, or give the `wear` task another key as its \
         second argument"
    )
}

fn table_at_other_spelling_message(manifest: &str, table: &str, key: &str) -> String {
    let (manifest, table, key) = (Escaped(manifest), Escaped(table), Escaped(key));
    format!(
        "{manifest} already has a [package.metadata.skeletons.{table}] table, which is `{key}` \
         to rustc; remove [package.metadata.skeletons.{table}], which names no dependency, then \
         run the `wear` task again"
    )
}

fn crate_spelled_differently_message(key: &str, spelled: &str) -> String {
    let (key, spelled) = (Escaped(key), Escaped(spelled));
    format!(
        "crates.io spells the crate `{spelled}`, so Cargo added it under that key and not as \
         `{key}`; give the `wear` task `{spelled}`, as crates.io spells it"
    )
}

fn table_defect_message(manifest: &str, key: &str, detail: &str) -> String {
    let (manifest, key, detail) = (Escaped(manifest), Escaped(key), Escaped(detail));
    format!(
        "wear could not add [package.metadata.skeletons.{key}] to {manifest} without changing \
         anything else in it ({detail}); this is a defect in skeletons"
    )
}

/// `reason` is the message of the refusal reading the workspace back made,
/// which has already escaped what came from outside, so it is not escaped a
/// second time.
fn not_confirmed_message(manifest: &str, key: &str, reason: &str) -> String {
    format!(
        "wear added `{}` to {}, and reading it back refused it: {reason}",
        Escaped(key),
        Escaped(manifest)
    )
}

fn not_reported_message(manifest: &str, key: &str) -> String {
    format!(
        "wear added `{}` to {}, and cargo metadata does not report it; this is a defect in \
         skeletons",
        Escaped(key),
        Escaped(manifest)
    )
}

#[cfg(test)]
mod tests {
    use super::{Parent, WearRefusal};
    use crate::survey::poison::{
        POISON, assert_escaped_once, assert_every_kind, assert_one_line, poison,
    };

    /// How many kinds of [`WearRefusal`] there are. The match in [`kind`] has
    /// no wildcard, so a kind added without an arm does not compile, and
    /// [`samples`] must then cover it.
    const KINDS: usize = 20;

    const fn kind(refusal: &WearRefusal) -> usize {
        match refusal {
            WearRefusal::CrateNameInvalid { .. } => 0,
            WearRefusal::KeyInvalid { .. } => 1,
            WearRefusal::KeyReserved { .. } => 2,
            WearRefusal::KeyShadowsCompiler { .. } => 3,
            WearRefusal::OutsideItsProject { .. } => 4,
            WearRefusal::AlreadyWorn { .. } => 5,
            WearRefusal::DependsWithoutWearing { .. } => 6,
            WearRefusal::DependsWithRefusedWearing { .. } => 7,
            WearRefusal::KeyTaken { .. } => 8,
            WearRefusal::CargoAddUnavailable { .. } => 9,
            WearRefusal::CargoAddFailed { .. } => 10,
            WearRefusal::NotASkeleton { .. } => 11,
            WearRefusal::ManifestUnreadable { .. } => 12,
            WearRefusal::ParentNotATable { .. } => 13,
            WearRefusal::TableWithoutDependency { .. } => 14,
            WearRefusal::TableAtOtherSpelling { .. } => 15,
            WearRefusal::CrateSpelledDifferently { .. } => 16,
            WearRefusal::TableDefect { .. } => 17,
            WearRefusal::NotConfirmed { .. } => 18,
            WearRefusal::NotReported { .. } => 19,
        }
    }

    /// The refusals made before anything is written, poisoned as `samples` says.
    fn samples_before_cargo_add() -> Vec<WearRefusal> {
        vec![
            WearRefusal::CrateNameInvalid {
                crate_name: poison(),
            },
            WearRefusal::KeyInvalid { key: poison() },
            WearRefusal::KeyReserved { key: poison() },
            WearRefusal::KeyShadowsCompiler { key: poison() },
            WearRefusal::OutsideItsProject {
                package: poison(),
                root: poison(),
            },
            WearRefusal::AlreadyWorn {
                package: poison(),
                key: poison(),
                manifest: poison(),
            },
            WearRefusal::DependsWithoutWearing {
                manifest: poison(),
                crate_name: poison(),
                key: poison(),
            },
            WearRefusal::DependsWithRefusedWearing {
                manifest: poison(),
                crate_name: poison(),
                key: poison(),
            },
            WearRefusal::KeyTaken {
                manifest: poison(),
                key: poison(),
                other: poison(),
                existing: poison(),
            },
        ]
    }

    /// The refusals made from `cargo add` on, poisoned as `samples` says.
    fn samples_from_cargo_add() -> Vec<WearRefusal> {
        vec![
            WearRefusal::CargoAddUnavailable { detail: poison() },
            WearRefusal::CargoAddFailed { stderr: poison() },
            WearRefusal::NotASkeleton {
                package: poison(),
                version: poison(),
            },
            WearRefusal::ManifestUnreadable {
                manifest: poison(),
                detail: poison(),
            },
            WearRefusal::ParentNotATable {
                manifest: poison(),
                parent: Parent::Skeletons,
            },
            WearRefusal::TableWithoutDependency {
                manifest: poison(),
                key: poison(),
            },
            WearRefusal::TableAtOtherSpelling {
                manifest: poison(),
                table: poison(),
                key: poison(),
            },
            WearRefusal::CrateSpelledDifferently {
                key: poison(),
                spelled: poison(),
            },
            WearRefusal::TableDefect {
                manifest: poison(),
                key: poison(),
                detail: poison(),
            },
            WearRefusal::NotConfirmed {
                manifest: poison(),
                key: poison(),
                reason: POISON.replace('\n', "\\n"),
            },
            WearRefusal::NotReported {
                manifest: poison(),
                key: poison(),
            },
        ]
    }

    /// One refusal of every kind, every piece of outside text in it poisoned.
    /// `NotConfirmed`'s reason is a finished message, so it is the one field
    /// that carries the escaped form already.
    ///
    /// Built from the refusals made before anything is written and those made
    /// from `cargo add` on.
    fn samples() -> Vec<WearRefusal> {
        let mut samples = samples_before_cargo_add();
        samples.extend(samples_from_cargo_add());
        samples
    }

    #[test]
    fn every_refusal_prints_outside_text_on_one_line_escaped_once() {
        // Names the wearer typed, what cargo said and what a filesystem
        // reported are all outside text. Each refusal is built with a
        // newline-holding name in every such field, and its message must be
        // one line that shows the name escaped, and not escaped twice.
        let samples = samples();
        assert_every_kind(samples.iter().map(kind), KINDS, "WearRefusal");

        for refusal in &samples {
            assert_escaped_once(&refusal.message(), &format!("{refusal:?}"));
        }
    }

    #[test]
    fn every_refusal_converts_to_a_failure_that_says_the_same_words() {
        for refusal in samples() {
            let message = refusal.message();
            assert_eq!(refusal.into_failure().to_string(), message);
        }
    }

    fn only(refusal: &WearRefusal) -> String {
        let message = refusal.message();
        assert_one_line(&message, &format!("{refusal:?}"));
        message
    }

    #[test]
    fn a_crate_name_that_is_not_one_says_what_a_name_is() {
        assert_eq!(
            only(&WearRefusal::CrateNameInvalid {
                crate_name: "a.b".to_owned()
            }),
            "`a.b` is not a crate name wear can add: a name starts with an ASCII letter or `_`, \
             continues with ASCII letters, digits, `-` and `_`, and is at most 64 bytes; give \
             the `wear` task the crate's name as its manifest spells it"
        );
    }

    #[test]
    fn a_key_that_is_not_one_says_what_a_key_is() {
        assert_eq!(
            only(&WearRefusal::KeyInvalid {
                key: "1x".to_owned()
            }),
            "`1x` cannot be a dependency key: a key starts with an ASCII letter or `_`, \
             continues with ASCII letters, digits, `-` and `_`, and is at most 64 bytes; give \
             the `wear` task another key as its second argument"
        );
    }

    #[test]
    fn a_reserved_key_names_the_keys_a_skeleton_keeps_for_itself() {
        assert_eq!(
            only(&WearRefusal::KeyReserved {
                key: "options".to_owned()
            }),
            "`options` cannot be worn as a dependency key: `options` and `verbatim` under \
             [package.metadata.skeletons] belong to a skeleton's own declaration; give the \
             `wear` task another key as its second argument"
        );
    }

    #[test]
    fn a_key_that_shadows_the_compiler_says_it_names_a_crate_the_compiler_provides() {
        assert_eq!(
            only(&WearRefusal::KeyShadowsCompiler {
                key: "proc-macro".to_owned()
            }),
            "`proc-macro` cannot be worn as a dependency key: it names a crate the compiler \
             provides; give the `wear` task another key as its second argument"
        );
    }

    #[test]
    fn a_skeleton_already_worn_names_where_and_says_to_edit_its_table() {
        assert_eq!(
            only(&WearRefusal::AlreadyWorn {
                package: "tidy".to_owned(),
                key: "tidy".to_owned(),
                manifest: "Cargo.toml".to_owned(),
            }),
            "tidy is already worn, as `tidy` in Cargo.toml; a workspace wears a skeleton once, \
             so to change its options, edit [package.metadata.skeletons.tidy] there"
        );
    }

    #[test]
    fn a_command_line_outside_its_project_names_the_package_and_the_root() {
        assert_eq!(
            only(&WearRefusal::OutsideItsProject {
                package: "tool".to_owned(),
                root: "/work/other".to_owned(),
            }),
            "this command line is built from the package `tool`, which is not a member of the \
             workspace at /work/other, so wear wrote nothing: run the `wear` task from inside \
             the project this command line belongs to"
        );
    }

    #[test]
    fn a_dependency_without_a_wearing_table_says_to_add_the_table() {
        assert_eq!(
            only(&WearRefusal::DependsWithoutWearing {
                manifest: "Cargo.toml".to_owned(),
                crate_name: "tidy".to_owned(),
                key: "neat".to_owned(),
            }),
            "Cargo.toml already depends on tidy under the key `neat`, without wearing it; to \
             wear it, add an empty [package.metadata.skeletons.neat] table to Cargo.toml"
        );
    }

    #[test]
    fn a_dependency_whose_wearing_is_refused_says_to_run_check() {
        assert_eq!(
            only(&WearRefusal::DependsWithRefusedWearing {
                manifest: "Cargo.toml".to_owned(),
                crate_name: "tidy".to_owned(),
                key: "neat".to_owned(),
            }),
            "Cargo.toml already depends on tidy under the key `neat`, and has a \
             [package.metadata.skeletons.neat] table for it, but that wearing is refused; the \
             `check` task says why, so run it and fix what it names in Cargo.toml"
        );
    }

    #[test]
    fn a_taken_key_names_what_holds_it_and_says_to_pick_another() {
        assert_eq!(
            only(&WearRefusal::KeyTaken {
                manifest: "Cargo.toml".to_owned(),
                key: "a-x".to_owned(),
                other: "enum-fill".to_owned(),
                existing: "a_x".to_owned(),
            }),
            "`a-x` is taken in Cargo.toml: its dependency on enum-fill is declared under \
             `a_x`; give the `wear` task another key as its second argument"
        );
    }

    #[test]
    fn cargo_add_failing_to_run_and_refusing_are_worded_apart() {
        assert_eq!(
            only(&WearRefusal::CargoAddUnavailable {
                detail: "failed to run: no such file".to_owned()
            }),
            "running `cargo add` failed: failed to run: no such file"
        );
        assert_eq!(
            only(&WearRefusal::CargoAddFailed {
                stderr: "error: no matching package\nhelp: try again".to_owned()
            }),
            "cargo add failed: error: no matching package\\nhelp: try again"
        );
    }

    #[test]
    fn a_crate_that_is_not_a_skeleton_says_which_table_marks_one() {
        assert_eq!(
            only(&WearRefusal::NotASkeleton {
                package: "plain-crate".to_owned(),
                version: "0.1.0".to_owned(),
            }),
            "plain-crate 0.1.0 is not a skeleton: its manifest has no \
             [package.metadata.skeletons] table, so it cannot be worn; wear a crate that is one"
        );
    }

    #[test]
    fn a_manifest_that_cannot_be_read_names_it_and_why() {
        assert_eq!(
            only(&WearRefusal::ManifestUnreadable {
                manifest: "Cargo.toml".to_owned(),
                detail: "permission denied".to_owned(),
            }),
            "wear could not read Cargo.toml to add the wearing table: permission denied"
        );
    }

    #[test]
    fn a_parent_that_is_not_a_table_names_its_header() {
        for (parent, header) in [
            (Parent::Package, "package"),
            (Parent::Metadata, "package.metadata"),
            (Parent::Skeletons, "package.metadata.skeletons"),
        ] {
            assert_eq!(
                only(&WearRefusal::ParentNotATable {
                    manifest: "Cargo.toml".to_owned(),
                    parent,
                }),
                format!(
                    "[{header}] in Cargo.toml is not a table, so wear cannot add a wearing \
                     table under it; make it a table, then run the `wear` task again"
                )
            );
        }
    }

    #[test]
    fn a_wearing_table_with_no_dependency_says_to_remove_it_or_pick_another_key() {
        assert_eq!(
            only(&WearRefusal::TableWithoutDependency {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            }),
            "Cargo.toml already has a [package.metadata.skeletons.tidy] table, with no \
             dependency declared under `tidy`; remove the table, or give the `wear` task \
             another key as its second argument"
        );
    }

    #[test]
    fn a_wearing_table_at_the_other_spelling_says_to_remove_it() {
        assert_eq!(
            only(&WearRefusal::TableAtOtherSpelling {
                manifest: "Cargo.toml".to_owned(),
                table: "ab_cd".to_owned(),
                key: "ab-cd".to_owned(),
            }),
            "Cargo.toml already has a [package.metadata.skeletons.ab_cd] table, which is \
             `ab-cd` to rustc; remove [package.metadata.skeletons.ab_cd], which names no \
             dependency, then run the `wear` task again"
        );
    }

    #[test]
    fn a_crate_crates_io_spells_differently_says_to_give_it_as_spelled() {
        assert_eq!(
            only(&WearRefusal::CrateSpelledDifferently {
                key: "serde-json".to_owned(),
                spelled: "serde_json".to_owned(),
            }),
            "crates.io spells the crate `serde_json`, so Cargo added it under that key and not \
             as `serde-json`; give the `wear` task `serde_json`, as crates.io spells it"
        );
    }

    #[test]
    fn a_table_that_changed_something_else_is_called_a_defect() {
        assert_eq!(
            only(&WearRefusal::TableDefect {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
                detail: "the edited manifest is not TOML".to_owned(),
            }),
            "wear could not add [package.metadata.skeletons.tidy] to Cargo.toml without \
             changing anything else in it (the edited manifest is not TOML); this is a \
             defect in skeletons"
        );
    }

    #[test]
    fn a_wearing_refused_on_reading_back_carries_the_refusals_own_words() {
        assert_eq!(
            only(&WearRefusal::NotConfirmed {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
                reason: "tidy in Cargo.toml is declared, but cargo resolved no package for it"
                    .to_owned(),
            }),
            "wear added `tidy` to Cargo.toml, and reading it back refused it: tidy in \
             Cargo.toml is declared, but cargo resolved no package for it"
        );
    }

    #[test]
    fn a_wearing_missing_on_reading_back_is_called_a_defect() {
        assert_eq!(
            only(&WearRefusal::NotReported {
                manifest: "Cargo.toml".to_owned(),
                key: "tidy".to_owned(),
            }),
            "wear added `tidy` to Cargo.toml, and cargo metadata does not report it; this is a \
             defect in skeletons"
        );
    }
}
