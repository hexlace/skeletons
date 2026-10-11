//! Every refusal and failure `remove` itself words, and the one place each is
//! worded.
//!
//! The refusals the work tree, the manifest's and lockfile's hand-back and the
//! reading of the workspace share with the other commands are worded where
//! they are shared, for the command that asks. What is here is what only
//! `remove` has to say.
//!
//! Text from outside, a name the wearer typed, a path or what cargo said, goes
//! through [`Escaped`], so a message is one line however its words came. Every
//! message ends by saying what to do, or says that nothing is left to do
//! because the defect is in `skeletons`.

use rituals::Failure;

use super::HOLDING_DIRECTORY;
use super::files::{Mode, Unknowable};
use super::resolve::{Elsewhere, PickRefusal};
use crate::command_line_crate::outside_its_project_message;
use crate::skeleton::Escaped;
use crate::survey::join_and;
use crate::work_tree::writing_command::WritingCommand;

/// What the wearer asked for, as the refusals that depend on the request word
/// it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Asked<'request> {
    /// The name typed: a key or a crate.
    pub(crate) name: &'request str,
    /// The command line's own manifest, as a message shows it.
    pub(crate) manifest: &'request str,
    /// The command a person types to run `remove`, as the command line gives
    /// it, which a message shows with a key after it.
    pub(crate) command: &'request str,
    /// Which flag was typed, if one was.
    pub(crate) mode: Mode,
}

/// Why `remove` refused or failed, with what its message names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoveRefusal {
    /// The package the command line was built from is not a member of the
    /// workspace `remove` ran in.
    OutsideItsProject { package: String, root: String },
    /// The name is neither a key of the manifest's wearings nor the crate of
    /// one. `keys` are the wearings' keys, in key order.
    NotWorn {
        name: String,
        manifest: String,
        keys: Vec<String>,
    },
    /// The name is a key or a crate of wearings in other manifests, and of none
    /// in the command line's own.
    WornElsewhere {
        name: String,
        manifest: String,
        elsewhere: Vec<Elsewhere>,
    },
    /// The name is the crate of several wearings in the manifest and the key of
    /// none. `keys` are theirs, in key order, and `command` and `mode` are how
    /// the refusal shows the remedy.
    SeveralKeys {
        name: String,
        manifest: String,
        keys: Vec<String>,
        command: String,
        mode: Mode,
    },
    /// The files the wearing claims cannot be known, and the mode would delete
    /// them.
    FilesUnknowable { key: String, unknowable: Unknowable },
    /// Something is at the path the files are moved into while they are
    /// deleted.
    HoldingPathInTheWay,
    /// The manifest could not be read to take the wearing out of it.
    ManifestUnreadable {
        manifest: String,
        key: String,
        detail: String,
    },
    /// The manifest could not be edited into the original less the wearing, for
    /// the reason `detail` gives.
    //
    // A defect in `skeletons` would ordinarily panic, and this is an `Err` on
    // purpose. `rollback` puts the manifest and lockfile back when a run
    // returns a failure and does not when it panics, and a run that has begun
    // to write may have changed both, so a panic could leave the project
    // half-written.
    ManifestDefect {
        manifest: String,
        key: String,
        detail: String,
    },
    /// `Cargo.lock` could not be read to see whether bringing it current changed
    /// it.
    LockfileUnreadable { lockfile: String, detail: String },
    /// Cargo refused the workspace with the wearing's dependency taken out.
    CargoRefused {
        manifest: String,
        key: String,
        stderr: String,
    },
    /// Reading the workspace back still finds the wearing.
    //
    // A defect in `skeletons` as well, and an `Err` for the reason
    // `ManifestDefect` is one: it is found after the manifest was written, so
    // only a returned failure makes `rollback` undo it.
    StillWorn { manifest: String, key: String },
    /// Reading the workspace back no longer finds the package the command line
    /// is built from.
    //
    // A defect in `skeletons` for the reason `StillWorn` is one, and an `Err`
    // for the same one: the member was found before the manifest was written,
    // so a read back that cannot find it shows the edit did something it was
    // not meant to, after the project was changed.
    PackageVanished { manifest: String, key: String },
    /// Cargo still reports a dependency under the key, declared somewhere
    /// `remove` does not edit.
    DependencyRemains { manifest: String, key: String },
    /// The directory the files are moved into could not be created.
    HoldingNotCreated { detail: String },
    /// A file could not be moved out of the work tree.
    MoveFailed { path: String, detail: String },
    /// A directory the deletion left empty could not be removed.
    DirectoryNotRemoved { directory: String, detail: String },
    /// The wearing is removed and its files are out of the tree, and the
    /// directory they were moved into could not be deleted.
    HoldingNotDeleted { key: String, detail: String },
}

impl RemoveRefusal {
    /// The refusal for a name that picked no one wearing, with the wearings
    /// that exist in other manifests under that name.
    pub(crate) fn from_pick(
        refusal: PickRefusal,
        asked: &Asked<'_>,
        elsewhere: Vec<Elsewhere>,
    ) -> Self {
        let (name, manifest) = (asked.name.to_owned(), asked.manifest.to_owned());
        match refusal {
            PickRefusal::SeveralKeys { keys } => Self::SeveralKeys {
                name,
                manifest,
                keys,
                command: asked.command.to_owned(),
                mode: asked.mode,
            },
            PickRefusal::Neither { .. } if !elsewhere.is_empty() => Self::WornElsewhere {
                name,
                manifest,
                elsewhere,
            },
            PickRefusal::Neither { keys } => Self::NotWorn {
                name,
                manifest,
                keys,
            },
        }
    }

    /// The refusal's message: one line, ending in what to do.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::OutsideItsProject { package, root } => {
                outside_its_project_message(package, root, WritingCommand::Remove)
            }
            Self::NotWorn {
                name,
                manifest,
                keys,
            } => not_worn_message(name, manifest, keys),
            Self::WornElsewhere {
                name,
                manifest,
                elsewhere,
            } => worn_elsewhere_message(name, manifest, elsewhere),
            Self::SeveralKeys {
                name,
                manifest,
                keys,
                command,
                mode,
            } => several_keys_message(name, manifest, keys, command, *mode),
            Self::FilesUnknowable { key, unknowable } => files_unknowable_message(key, unknowable),
            Self::HoldingPathInTheWay => holding_path_in_the_way_message(),
            Self::ManifestUnreadable {
                manifest,
                key,
                detail,
            } => format!(
                "remove could not read {} to take `{}` out of it: {}",
                Escaped(manifest),
                Escaped(key),
                Escaped(detail)
            ),
            Self::ManifestDefect {
                manifest,
                key,
                detail,
            } => format!(
                "remove could not take `{}` out of {} without changing anything else in it \
                 ({}); this is a defect in skeletons",
                Escaped(key),
                Escaped(manifest),
                Escaped(detail)
            ),
            Self::LockfileUnreadable { lockfile, detail } => format!(
                "remove could not read {} to see whether bringing it current changed it: {}",
                Escaped(lockfile),
                Escaped(detail)
            ),
            Self::CargoRefused {
                manifest,
                key,
                stderr,
            } => format!(
                "remove took `{}` out of {}, and Cargo refused what was left: {}",
                Escaped(key),
                Escaped(manifest),
                Escaped(stderr)
            ),
            Self::StillWorn { manifest, key } => still_worn_message(manifest, key),
            Self::PackageVanished { manifest, key } => package_vanished_message(manifest, key),
            Self::DependencyRemains { manifest, key } => dependency_remains_message(manifest, key),
            Self::HoldingNotCreated { detail } => format!(
                "remove could not create {HOLDING_DIRECTORY} to move the files it deletes into: {}",
                Escaped(detail)
            ),
            Self::MoveFailed { path, detail } => format!(
                "remove could not move {} out of the work tree: {}",
                Escaped(path),
                Escaped(detail)
            ),
            Self::DirectoryNotRemoved { directory, detail } => format!(
                "remove could not remove {}, which deleting its files left empty: {}",
                Escaped(directory),
                Escaped(detail)
            ),
            Self::HoldingNotDeleted { key, detail } => format!(
                "remove stopped wearing `{}` and moved its files into {HOLDING_DIRECTORY}, which \
                 it could not then delete: {}; delete {HOLDING_DIRECTORY} by hand",
                Escaped(key),
                Escaped(detail)
            ),
        }
    }

    /// This refusal as the [`Failure`] a task returns.
    pub(crate) fn into_failure(self) -> Failure {
        Failure::new(self.message())
    }
}

/// `` `{name}` is neither a key {manifest} wears a skeleton under nor the crate
/// of one, so remove wrote nothing; {what there is to name instead} ``.
fn not_worn_message(name: &str, manifest: &str, keys: &[String]) -> String {
    let (name, manifest) = (Escaped(name), Escaped(manifest));
    let instead = match keys {
        [] => format!("{manifest} wears no skeleton"),
        [only] => format!(
            "its key is `{}`, so give the `remove` task that one",
            Escaped(only)
        ),
        several => format!(
            "its keys are {}, so give the `remove` task one of them",
            backticked(several)
        ),
    };
    format!(
        "`{name}` is neither a key {manifest} wears a skeleton under nor the crate of one, so \
         remove wrote nothing; {instead}"
    )
}

/// `` `{name}` is worn in {manifest} as `{key}`, and remove changes only
/// {command line's manifest}, the manifest of the package this command line is
/// built from, so remove wrote nothing; take the dependency and the
/// [package.metadata.skeletons.<key>] table out of {that manifest|those
/// manifests} by hand ``.
fn worn_elsewhere_message(name: &str, manifest: &str, elsewhere: &[Elsewhere]) -> String {
    assert!(
        !elsewhere.is_empty(),
        "a name is worn elsewhere only if some wearing is there"
    );
    let places: Vec<String> = elsewhere
        .iter()
        .map(|place| format!("{} as `{}`", Escaped(&place.manifest), Escaped(&place.key)))
        .collect();
    let mut manifests: Vec<&str> = elsewhere
        .iter()
        .map(|place| place.manifest.as_str())
        .collect();
    manifests.dedup();
    let those = if manifests.len() == 1 {
        "that manifest"
    } else {
        "those manifests"
    };
    format!(
        "`{}` is worn in {}, and remove changes only {}, the manifest of the package this \
         command line is built from, so remove wrote nothing; take the dependency and the \
         [package.metadata.skeletons.<key>] table out of {those} by hand",
        Escaped(name),
        join_and(&places),
        Escaped(manifest),
    )
}

/// `` `{name}` is worn under more than one key in {manifest}, {keys}, so remove
/// wrote nothing; remove one by its key, such as `{command} {first key}{ flag}` ``.
fn several_keys_message(
    name: &str,
    manifest: &str,
    keys: &[String],
    command: &str,
    mode: Mode,
) -> String {
    let [first, _second, ..] = keys else {
        unreachable!("a crate is refused as worn under several keys only when it is");
    };
    let flag = mode
        .flag()
        .map_or_else(String::new, |flag| format!(" {flag}"));
    format!(
        "`{}` is worn under more than one key in {}, {}, so remove wrote nothing; remove one by \
         its key, such as `{} {}{flag}`",
        Escaped(name),
        Escaped(manifest),
        backticked(keys),
        Escaped(command),
        Escaped(first),
    )
}

/// `` remove cannot tell which files `{key}` claims, so it wrote nothing:
/// {reasons}; fix that and run the `remove` task again, or give it --keep-files
/// to stop wearing `{key}` and leave every file in place ``.
///
/// Each reason is a refusal's own message, which is already one escaped line,
/// so it is quoted as it stands.
fn files_unknowable_message(key: &str, unknowable: &Unknowable) -> String {
    assert!(
        !unknowable.reasons.is_empty(),
        "files are unknowable only for a reason"
    );
    let key = Escaped(key);
    format!(
        "remove cannot tell which files `{key}` claims, so it wrote nothing: {}; fix that and \
         run the `remove` task again, or give it --keep-files to stop wearing `{key}` and leave \
         every file in place",
        unknowable.reasons.join("; ")
    )
}

/// `` {holding directory} is in the way at the workspace root, so remove wrote
/// nothing: it moves the files it deletes there first, so that a failure can
/// move them back; delete or move it, then run the `remove` task again ``.
fn holding_path_in_the_way_message() -> String {
    format!(
        "{HOLDING_DIRECTORY} is in the way at the workspace root, so remove wrote nothing: it \
         moves the files it deletes there first, so that a failure can move them back; delete \
         or move it, then {}",
        WritingCommand::Remove.run_again()
    )
}

/// `` remove took `{key}` out of {manifest}, and reading it back still finds
/// [package.metadata.skeletons.{key}]; this is a defect in skeletons ``.
fn still_worn_message(manifest: &str, key: &str) -> String {
    let (manifest, key) = (Escaped(manifest), Escaped(key));
    format!(
        "remove took `{key}` out of {manifest}, and reading it back still finds \
         [package.metadata.skeletons.{key}]; this is a defect in skeletons"
    )
}

/// `` remove took `{key}` out of {manifest}, and reading the workspace back no
/// longer finds the package this command line is built from; this is a defect
/// in skeletons ``.
fn package_vanished_message(manifest: &str, key: &str) -> String {
    format!(
        "remove took `{}` out of {}, and reading the workspace back no longer finds the package \
         this command line is built from; this is a defect in skeletons",
        Escaped(key),
        Escaped(manifest)
    )
}

/// `` remove took `{key}` out of {manifest}, and Cargo still reports a
/// dependency under `{key}`: it is declared somewhere remove does not edit;
/// take it out by hand, then run the `remove` task again ``.
fn dependency_remains_message(manifest: &str, key: &str) -> String {
    let (manifest, key) = (Escaped(manifest), Escaped(key));
    format!(
        "remove took `{key}` out of {manifest}, and Cargo still reports a dependency under \
         `{key}`: it is declared somewhere remove does not edit; take it out by hand, then {}",
        WritingCommand::Remove.run_again()
    )
}

/// `items` escaped, each in backticks, joined as this crate's one list grammar
/// does.
fn backticked(items: &[String]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|item| format!("`{}`", Escaped(item)))
        .collect();
    join_and(&quoted)
}

#[cfg(test)]
mod tests {
    use super::{Asked, RemoveRefusal};
    use crate::remove::files::{Mode, Unknowable};
    use crate::remove::resolve::{Elsewhere, PickRefusal};
    use crate::survey::poison::{
        POISON_ESCAPED, assert_escaped_once, assert_every_kind, assert_one_line, poison,
    };

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn elsewhere(manifest: &str, key: &str) -> Elsewhere {
        Elsewhere {
            manifest: manifest.to_owned(),
            key: key.to_owned(),
        }
    }

    fn asked(mode: Mode) -> Asked<'static> {
        Asked {
            name: "a-skeleton",
            manifest: "ritual/Cargo.toml",
            command: "cargo ritual skeletons remove",
            mode,
        }
    }

    #[test]
    fn a_command_line_outside_its_project_is_worded_for_remove() {
        let refusal = RemoveRefusal::OutsideItsProject {
            package: "skeletons-ritual".to_owned(),
            root: "/path/to/workspace".to_owned(),
        };

        assert_eq!(
            refusal.message(),
            "this command line is built from the package `skeletons-ritual`, which is not a \
             member of the workspace at /path/to/workspace, so remove wrote nothing: run the \
             `remove` task from inside the project this command line belongs to"
        );
    }

    #[test]
    fn a_name_that_is_neither_lists_the_keys_there_are() {
        let refusal = RemoveRefusal::NotWorn {
            name: "nope".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            keys: strings(&["ci", "dependabot"]),
        };

        assert_eq!(
            refusal.message(),
            "`nope` is neither a key ritual/Cargo.toml wears a skeleton under nor the crate of \
             one, so remove wrote nothing; its keys are `ci` and `dependabot`, so give the \
             `remove` task one of them"
        );
    }

    #[test]
    fn a_name_that_is_neither_names_three_keys_with_commas() {
        let refusal = RemoveRefusal::NotWorn {
            name: "nope".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            keys: strings(&["a", "b", "c"]),
        };

        assert!(
            refusal
                .message()
                .ends_with("its keys are `a`, `b` and `c`, so give the `remove` task one of them"),
            "{}",
            refusal.message()
        );
    }

    #[test]
    fn a_name_that_is_neither_names_the_one_key_there_is_in_the_singular() {
        let refusal = RemoveRefusal::NotWorn {
            name: "nope".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            keys: strings(&["ci"]),
        };

        assert_eq!(
            refusal.message(),
            "`nope` is neither a key ritual/Cargo.toml wears a skeleton under nor the crate of \
             one, so remove wrote nothing; its key is `ci`, so give the `remove` task that one"
        );
    }

    #[test]
    fn a_name_that_is_neither_says_so_plainly_when_the_manifest_wears_nothing() {
        let refusal = RemoveRefusal::NotWorn {
            name: "nope".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            keys: Vec::new(),
        };

        assert_eq!(
            refusal.message(),
            "`nope` is neither a key ritual/Cargo.toml wears a skeleton under nor the crate of \
             one, so remove wrote nothing; ritual/Cargo.toml wears no skeleton"
        );
    }

    #[test]
    fn a_wearing_in_another_manifest_is_named_with_the_hand_remedy() {
        let refusal = RemoveRefusal::WornElsewhere {
            name: "ci".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            elsewhere: vec![elsewhere("tools/Cargo.toml", "ci")],
        };

        assert_eq!(
            refusal.message(),
            "`ci` is worn in tools/Cargo.toml as `ci`, and remove changes only \
             ritual/Cargo.toml, the manifest of the package this command line is built from, so \
             remove wrote nothing; take the dependency and the \
             [package.metadata.skeletons.<key>] table out of that manifest by hand"
        );
    }

    #[test]
    fn wearings_in_several_manifests_are_each_named_and_the_remedy_is_plural() {
        let refusal = RemoveRefusal::WornElsewhere {
            name: "a-skeleton".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            elsewhere: vec![
                elsewhere("a/Cargo.toml", "x"),
                elsewhere("b/Cargo.toml", "y"),
            ],
        };

        assert_eq!(
            refusal.message(),
            "`a-skeleton` is worn in a/Cargo.toml as `x` and b/Cargo.toml as `y`, and remove \
             changes only ritual/Cargo.toml, the manifest of the package this command line is \
             built from, so remove wrote nothing; take the dependency and the \
             [package.metadata.skeletons.<key>] table out of those manifests by hand"
        );
    }

    #[test]
    fn two_wearings_in_one_manifest_are_still_one_manifest_to_edit() {
        let refusal = RemoveRefusal::WornElsewhere {
            name: "a-skeleton".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            elsewhere: vec![
                elsewhere("a/Cargo.toml", "x"),
                elsewhere("a/Cargo.toml", "y"),
            ],
        };

        let message = refusal.message();

        assert!(
            message.contains("a/Cargo.toml as `x` and a/Cargo.toml as `y`"),
            "{message}"
        );
        assert!(
            message.ends_with("out of that manifest by hand"),
            "{message}"
        );
    }

    #[test]
    fn a_crate_worn_under_several_keys_shows_the_command_with_the_first_key() {
        let refusal = RemoveRefusal::SeveralKeys {
            name: "a-skeleton".to_owned(),
            manifest: "ritual/Cargo.toml".to_owned(),
            keys: strings(&["ci", "ci-next"]),
            command: "cargo ritual skeletons remove".to_owned(),
            mode: Mode::Bare,
        };

        assert_eq!(
            refusal.message(),
            "`a-skeleton` is worn under more than one key in ritual/Cargo.toml, `ci` and \
             `ci-next`, so remove wrote nothing; remove one by its key, such as `cargo ritual \
             skeletons remove ci`"
        );
    }

    #[test]
    fn the_flag_that_was_typed_is_carried_into_the_command_shown() {
        for (mode, flag) in [
            (Mode::DeleteFiles, " --delete-files"),
            (Mode::KeepFiles, " --keep-files"),
        ] {
            let refusal = RemoveRefusal::SeveralKeys {
                name: "a-skeleton".to_owned(),
                manifest: "ritual/Cargo.toml".to_owned(),
                keys: strings(&["ci", "ci-next"]),
                command: "cargo ritual skeletons remove".to_owned(),
                mode,
            };

            assert!(
                refusal
                    .message()
                    .ends_with(&format!("such as `cargo ritual skeletons remove ci{flag}`")),
                "{}",
                refusal.message()
            );
        }
    }

    #[test]
    fn a_pick_that_found_several_keys_becomes_the_several_keys_refusal_whatever_is_elsewhere() {
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::SeveralKeys {
                keys: strings(&["ci", "ci-next"]),
            },
            &asked(Mode::KeepFiles),
            vec![elsewhere("tools/Cargo.toml", "x")],
        );

        assert_eq!(
            refusal,
            RemoveRefusal::SeveralKeys {
                name: "a-skeleton".to_owned(),
                manifest: "ritual/Cargo.toml".to_owned(),
                keys: strings(&["ci", "ci-next"]),
                command: "cargo ritual skeletons remove".to_owned(),
                mode: Mode::KeepFiles,
            }
        );
    }

    #[test]
    fn a_pick_that_found_nothing_is_worn_elsewhere_when_some_manifest_wears_the_name() {
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::Neither {
                keys: strings(&["lint"]),
            },
            &asked(Mode::Bare),
            vec![elsewhere("tools/Cargo.toml", "ci")],
        );

        assert_eq!(
            refusal,
            RemoveRefusal::WornElsewhere {
                name: "a-skeleton".to_owned(),
                manifest: "ritual/Cargo.toml".to_owned(),
                elsewhere: vec![elsewhere("tools/Cargo.toml", "ci")],
            }
        );
    }

    #[test]
    fn a_pick_that_found_nothing_anywhere_lists_the_manifests_keys() {
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::Neither {
                keys: strings(&["lint"]),
            },
            &asked(Mode::Bare),
            Vec::new(),
        );

        assert_eq!(
            refusal,
            RemoveRefusal::NotWorn {
                name: "a-skeleton".to_owned(),
                manifest: "ritual/Cargo.toml".to_owned(),
                keys: strings(&["lint"]),
            }
        );
    }

    #[test]
    fn files_that_cannot_be_known_give_each_reason_as_it_stands_and_both_remedies() {
        let refusal = RemoveRefusal::FilesUnknowable {
            key: "ci".to_owned(),
            unknowable: Unknowable {
                reasons: strings(&["first reason", "second reason"]),
            },
        };

        assert_eq!(
            refusal.message(),
            "remove cannot tell which files `ci` claims, so it wrote nothing: first reason; \
             second reason; fix that and run the `remove` task again, or give it --keep-files \
             to stop wearing `ci` and leave every file in place"
        );
    }

    #[test]
    fn a_refusal_becomes_the_failure_a_task_returns_with_its_message_as_it_is() {
        let refusal = RemoveRefusal::HoldingPathInTheWay;
        let message = refusal.message();

        assert_eq!(refusal.into_failure().to_string(), message);
    }

    #[test]
    fn something_at_the_holding_path_is_named_with_why_and_what_to_do() {
        assert_eq!(
            RemoveRefusal::HoldingPathInTheWay.message(),
            ".skeletons-remove is in the way at the workspace root, so remove wrote nothing: it \
             moves the files it deletes there first, so that a failure can move them back; \
             delete or move it, then run the `remove` task again"
        );
    }

    /// Every failure that can come after the first write, with the fixed text it
    /// is held to.
    fn failures() -> Vec<(RemoveRefusal, &'static str)> {
        let (manifest, key) = ("ritual/Cargo.toml".to_owned(), "ci".to_owned());
        vec![
            (
                RemoveRefusal::ManifestUnreadable {
                    manifest: manifest.clone(),
                    key: key.clone(),
                    detail: "permission denied".to_owned(),
                },
                "remove could not read ritual/Cargo.toml to take `ci` out of it: permission denied",
            ),
            (
                RemoveRefusal::ManifestDefect {
                    manifest: manifest.clone(),
                    key: key.clone(),
                    detail: "it differs".to_owned(),
                },
                "remove could not take `ci` out of ritual/Cargo.toml without changing anything \
                 else in it (it differs); this is a defect in skeletons",
            ),
            (
                RemoveRefusal::CargoRefused {
                    manifest: manifest.clone(),
                    key: key.clone(),
                    stderr: "error: feature `x` includes `ci`".to_owned(),
                },
                "remove took `ci` out of ritual/Cargo.toml, and Cargo refused what was left: \
                 error: feature `x` includes `ci`",
            ),
            (
                RemoveRefusal::LockfileUnreadable {
                    lockfile: "Cargo.lock".to_owned(),
                    detail: "permission denied".to_owned(),
                },
                "remove could not read Cargo.lock to see whether bringing it current changed it: \
                 permission denied",
            ),
            (
                RemoveRefusal::StillWorn {
                    manifest: manifest.clone(),
                    key: key.clone(),
                },
                "remove took `ci` out of ritual/Cargo.toml, and reading it back still finds \
                 [package.metadata.skeletons.ci]; this is a defect in skeletons",
            ),
            (
                RemoveRefusal::PackageVanished {
                    manifest: manifest.clone(),
                    key: key.clone(),
                },
                "remove took `ci` out of ritual/Cargo.toml, and reading the workspace back no \
                 longer finds the package this command line is built from; this is a defect in \
                 skeletons",
            ),
            (
                RemoveRefusal::DependencyRemains { manifest, key },
                "remove took `ci` out of ritual/Cargo.toml, and Cargo still reports a dependency \
                 under `ci`: it is declared somewhere remove does not edit; take it out by hand, \
                 then run the `remove` task again",
            ),
            (
                RemoveRefusal::HoldingNotCreated {
                    detail: "permission denied".to_owned(),
                },
                "remove could not create .skeletons-remove to move the files it deletes into: \
                 permission denied",
            ),
            (
                RemoveRefusal::MoveFailed {
                    path: ".github/workflows/ci.yml".to_owned(),
                    detail: "permission denied".to_owned(),
                },
                "remove could not move .github/workflows/ci.yml out of the work tree: permission \
                 denied",
            ),
            (
                RemoveRefusal::DirectoryNotRemoved {
                    directory: ".github/workflows".to_owned(),
                    detail: "directory not empty".to_owned(),
                },
                "remove could not remove .github/workflows, which deleting its files left empty: \
                 directory not empty",
            ),
            (
                RemoveRefusal::HoldingNotDeleted {
                    key: "ci".to_owned(),
                    detail: "permission denied".to_owned(),
                },
                "remove stopped wearing `ci` and moved its files into .skeletons-remove, which \
                 it could not then delete: permission denied; delete .skeletons-remove by hand",
            ),
        ]
    }

    #[test]
    fn each_failure_after_the_first_write_says_what_happened_and_what_to_do() {
        for (failure, expected) in failures() {
            assert_eq!(failure.message(), expected, "{failure:?}");
        }
    }

    /// How many kinds of [`RemoveRefusal`] there are. The match in
    /// [`kind_index`] has no wildcard, so a kind added without an arm does not
    /// compile, and the samples must then cover it.
    const KINDS: usize = 17;

    const fn kind_index(refusal: &RemoveRefusal) -> usize {
        match refusal {
            RemoveRefusal::OutsideItsProject { .. } => 0,
            RemoveRefusal::NotWorn { .. } => 1,
            RemoveRefusal::WornElsewhere { .. } => 2,
            RemoveRefusal::SeveralKeys { .. } => 3,
            RemoveRefusal::FilesUnknowable { .. } => 4,
            RemoveRefusal::HoldingPathInTheWay => 5,
            RemoveRefusal::ManifestUnreadable { .. } => 6,
            RemoveRefusal::ManifestDefect { .. } => 7,
            RemoveRefusal::CargoRefused { .. } => 8,
            RemoveRefusal::StillWorn { .. } => 9,
            RemoveRefusal::DependencyRemains { .. } => 10,
            RemoveRefusal::MoveFailed { .. } => 11,
            RemoveRefusal::DirectoryNotRemoved { .. } => 12,
            RemoveRefusal::HoldingNotDeleted { .. } => 13,
            RemoveRefusal::HoldingNotCreated { .. } => 14,
            RemoveRefusal::LockfileUnreadable { .. } => 15,
            RemoveRefusal::PackageVanished { .. } => 16,
        }
    }

    #[test]
    fn every_refusal_prints_text_from_outside_on_one_line_escaped_once() {
        // The poison is a name with a line break in it. It goes into every
        // field of every kind that holds text from outside, and each message
        // must be one line that shows the name as the two characters `\n`,
        // never raw and never escaped a second time. The reasons of a refusal
        // about files are other refusals' messages, already escaped, so they
        // are given escaped and must not be escaped again.
        let text = poison;
        let samples = [
            RemoveRefusal::OutsideItsProject {
                package: text(),
                root: text(),
            },
            RemoveRefusal::NotWorn {
                name: text(),
                manifest: text(),
                keys: vec![text(), text()],
            },
            RemoveRefusal::WornElsewhere {
                name: text(),
                manifest: text(),
                elsewhere: vec![elsewhere(&text(), &text())],
            },
            RemoveRefusal::SeveralKeys {
                name: text(),
                manifest: text(),
                keys: vec![text(), text()],
                command: text(),
                mode: Mode::Bare,
            },
            RemoveRefusal::FilesUnknowable {
                key: text(),
                unknowable: Unknowable {
                    reasons: vec![POISON_ESCAPED.to_owned()],
                },
            },
            RemoveRefusal::ManifestUnreadable {
                manifest: text(),
                key: text(),
                detail: text(),
            },
            RemoveRefusal::ManifestDefect {
                manifest: text(),
                key: text(),
                detail: text(),
            },
            RemoveRefusal::CargoRefused {
                manifest: text(),
                key: text(),
                stderr: text(),
            },
            RemoveRefusal::LockfileUnreadable {
                lockfile: text(),
                detail: text(),
            },
            RemoveRefusal::StillWorn {
                manifest: text(),
                key: text(),
            },
            RemoveRefusal::PackageVanished {
                manifest: text(),
                key: text(),
            },
            RemoveRefusal::DependencyRemains {
                manifest: text(),
                key: text(),
            },
            RemoveRefusal::HoldingNotCreated { detail: text() },
            RemoveRefusal::MoveFailed {
                path: text(),
                detail: text(),
            },
            RemoveRefusal::DirectoryNotRemoved {
                directory: text(),
                detail: text(),
            },
            RemoveRefusal::HoldingNotDeleted {
                key: text(),
                detail: text(),
            },
        ];
        // The one kind with no text from outside is only asked to stay on one
        // line, and is counted with the rest.
        let holding = RemoveRefusal::HoldingPathInTheWay;
        assert_every_kind(
            samples.iter().chain([&holding]).map(kind_index),
            KINDS,
            "RemoveRefusal",
        );

        for refusal in &samples {
            assert_escaped_once(&refusal.message(), &format!("{refusal:?}"));
        }
        assert_one_line(&holding.message(), "the holding path refusal");
    }

    #[test]
    fn no_refusal_names_sync_or_wear() {
        // Every message is worded for `remove` and sends the wearer to
        // nothing else.
        let mut all: Vec<RemoveRefusal> =
            failures().into_iter().map(|(refusal, _)| refusal).collect();
        all.extend([
            RemoveRefusal::HoldingPathInTheWay,
            RemoveRefusal::OutsideItsProject {
                package: "p".to_owned(),
                root: "/r".to_owned(),
            },
            RemoveRefusal::NotWorn {
                name: "n".to_owned(),
                manifest: "Cargo.toml".to_owned(),
                keys: strings(&["a", "b"]),
            },
            RemoveRefusal::WornElsewhere {
                name: "n".to_owned(),
                manifest: "Cargo.toml".to_owned(),
                elsewhere: vec![elsewhere("t/Cargo.toml", "k")],
            },
            RemoveRefusal::FilesUnknowable {
                key: "k".to_owned(),
                unknowable: Unknowable {
                    reasons: strings(&["a reason"]),
                },
            },
        ]);

        for refusal in all {
            let message = refusal.message();
            for other in ["sync", "`wear`", "wear wrote", "wear task"] {
                assert!(!message.contains(other), "{refusal:?} read: {message}");
            }
        }
    }
}
