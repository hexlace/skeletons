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

use std::path::PathBuf;

use rituals::Failure;

use super::files::{FilesFlag, Unknowable};
use super::names::{ManifestShown, WearingKey};
use super::non_empty::NonEmpty;
use super::resolve::{Elsewhere, PickRefusal};
use crate::command_line_crate::outside_its_project_message;
use crate::skeleton::Escaped;
use crate::survey::join_and;
use crate::work_tree::writing_command::WritingCommand;

/// What the wearer asked for, as the refusals that depend on the request word
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asked {
    /// The name typed: a key or a crate.
    pub(crate) name: String,
    /// The command line's own manifest, as a message shows it.
    pub(crate) manifest: ManifestShown,
    /// The command a person types to run `remove`, as the command line gives
    /// it, which a message shows with a key after it.
    pub(crate) command: String,
    /// Which flag about the files was typed, if one was.
    pub(crate) files_flag: FilesFlag,
}

/// Why `remove` refused or failed, with what its message names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoveRefusal {
    /// The package the command line was built from is not a member of the
    /// workspace `remove` ran in.
    OutsideItsProject { package: String, root: PathBuf },
    /// The name is neither a key of the manifest's wearings nor the crate of
    /// one. `keys` are the wearings' keys, in key order.
    NotWorn {
        name: String,
        manifest: ManifestShown,
        keys: Vec<WearingKey>,
    },
    /// The name is a key or a crate of wearings in other manifests, and of none
    /// in the command line's own.
    WornElsewhere {
        name: String,
        manifest: ManifestShown,
        elsewhere: NonEmpty<Elsewhere>,
    },
    /// The name is the crate of several wearings in the manifest and the key of
    /// none. `keys` are theirs, in key order, and `asked` carries the command
    /// and the flag the refusal shows the remedy with. `asked` is boxed so that
    /// the largest refusal, which every function returning one pays for on each
    /// move, is not this one.
    SeveralKeys {
        asked: Box<Asked>,
        keys: NonEmpty<WearingKey>,
    },
    /// The files the wearing claims cannot be known, and the flag would delete
    /// them.
    FilesUnknowable {
        key: WearingKey,
        unknowable: Unknowable,
    },
    /// The manifest could not be read to take the wearing out of it.
    ManifestUnreadable {
        manifest: ManifestShown,
        key: WearingKey,
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
        manifest: ManifestShown,
        key: WearingKey,
        detail: String,
    },
    /// `Cargo.lock` could not be read to see whether bringing it current changed
    /// it.
    LockfileUnreadable { lockfile: String, detail: String },
    /// Cargo refused the workspace with the wearing's dependency taken out.
    CargoRefused {
        manifest: ManifestShown,
        key: WearingKey,
        stderr: String,
    },
    /// Reading the workspace back still finds the wearing.
    //
    // A defect in `skeletons` as well, and an `Err` for the reason
    // `ManifestDefect` is one: it is found after the manifest was written, so
    // only a returned failure makes `rollback` undo it.
    StillWorn {
        manifest: ManifestShown,
        key: WearingKey,
    },
    /// Reading the workspace back no longer finds the package the command line
    /// is built from.
    //
    // A defect in `skeletons` for the reason `StillWorn` is one, and an `Err`
    // for the same one: the member was found before the manifest was written,
    // so a read back that cannot find it shows the edit did something it was
    // not meant to, after the project was changed.
    PackageVanished {
        manifest: ManifestShown,
        key: WearingKey,
    },
    /// Cargo still reports a dependency under the key, declared somewhere
    /// `remove` does not edit.
    DependencyRemains {
        manifest: ManifestShown,
        key: WearingKey,
    },
    /// A file could not be deleted.
    DeleteFailed { path: String, detail: String },
}

impl RemoveRefusal {
    /// The refusal for a name that picked no one wearing, with the wearings
    /// that exist in other manifests under that name.
    ///
    /// A pick that found the crate under several keys is refused as that
    /// whatever is elsewhere: its remedy is to name one of them.
    pub(crate) fn from_pick(refusal: PickRefusal, asked: Asked, elsewhere: Vec<Elsewhere>) -> Self {
        match (refusal, NonEmpty::from_vec(elsewhere)) {
            (PickRefusal::SeveralKeys { keys }, _) => Self::SeveralKeys {
                asked: Box::new(asked),
                keys,
            },
            (PickRefusal::Neither { .. }, Some(elsewhere)) => Self::WornElsewhere {
                name: asked.name,
                manifest: asked.manifest,
                elsewhere,
            },
            (PickRefusal::Neither { keys }, None) => Self::NotWorn {
                name: asked.name,
                manifest: asked.manifest,
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
            Self::SeveralKeys { asked, keys } => several_keys_message(asked, keys),
            Self::FilesUnknowable { key, unknowable } => files_unknowable_message(key, unknowable),
            Self::ManifestUnreadable {
                manifest,
                key,
                detail,
            } => manifest_unreadable_message(manifest, key, detail),
            Self::ManifestDefect {
                manifest,
                key,
                detail,
            } => manifest_defect_message(manifest, key, detail),
            Self::LockfileUnreadable { lockfile, detail } => {
                lockfile_unreadable_message(lockfile, detail)
            }
            Self::CargoRefused {
                manifest,
                key,
                stderr,
            } => cargo_refused_message(manifest, key, stderr),
            Self::StillWorn { manifest, key } => still_worn_message(manifest, key),
            Self::PackageVanished { manifest, key } => package_vanished_message(manifest, key),
            Self::DependencyRemains { manifest, key } => dependency_remains_message(manifest, key),
            Self::DeleteFailed { path, detail } => delete_failed_message(path, detail),
        }
    }

    /// This refusal as the [`Failure`] a task returns.
    pub(crate) fn into_failure(self) -> Failure {
        Failure::new(self.message())
    }
}

/// `` `{name}` is neither a key {manifest} wears a skeleton under nor the crate
/// of one, so remove wrote nothing; {what there is to name instead} ``.
fn not_worn_message(name: &str, manifest: &ManifestShown, keys: &[WearingKey]) -> String {
    let (name, manifest) = (Escaped(name), Escaped(manifest.as_str()));
    let instead = match keys {
        [] => format!("{manifest} wears no skeleton"),
        [only] => format!(
            "its key is `{}`, so give the `remove` task that one",
            Escaped(only.as_str())
        ),
        several => format!(
            "its keys are {}, so give the `remove` task one of them",
            backticked(several.iter())
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
fn worn_elsewhere_message(
    name: &str,
    manifest: &ManifestShown,
    elsewhere: &NonEmpty<Elsewhere>,
) -> String {
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
        Escaped(manifest.as_str()),
    )
}

/// `` `{name}` is worn under more than one key in {manifest}, {keys}, so remove
/// wrote nothing; remove one by its key, such as `{command} {first key}{ flag}` ``.
fn several_keys_message(asked: &Asked, keys: &NonEmpty<WearingKey>) -> String {
    let flag = asked
        .files_flag
        .typed()
        .map_or_else(String::new, |flag| format!(" {flag}"));
    format!(
        "`{}` is worn under more than one key in {}, {}, so remove wrote nothing; remove one by \
         its key, such as `{} {}{flag}`",
        Escaped(&asked.name),
        Escaped(asked.manifest.as_str()),
        backticked(keys.iter()),
        Escaped(&asked.command),
        Escaped(keys.first().as_str()),
    )
}

/// `` remove cannot tell which files `{key}` claims, so it wrote nothing:
/// {reasons}; fix that and run the `remove` task again, or give it --keep-files
/// to stop wearing `{key}` and leave every file in place ``.
///
/// Each reason is a refusal's own message, which is already one escaped line,
/// so it is quoted as it stands.
fn files_unknowable_message(key: &WearingKey, unknowable: &Unknowable) -> String {
    let key = Escaped(key.as_str());
    let reasons: Vec<&str> = unknowable.reasons.iter().map(String::as_str).collect();
    format!(
        "remove cannot tell which files `{key}` claims, so it wrote nothing: {}; fix that and \
         run the `remove` task again, or give it --keep-files to stop wearing `{key}` and leave \
         every file in place",
        reasons.join("; ")
    )
}

/// `` remove could not read {manifest} to take `{key}` out of it: {detail} ``.
fn manifest_unreadable_message(manifest: &ManifestShown, key: &WearingKey, detail: &str) -> String {
    format!(
        "remove could not read {} to take `{}` out of it: {}",
        Escaped(manifest.as_str()),
        Escaped(key.as_str()),
        Escaped(detail)
    )
}

/// `` remove could not take `{key}` out of {manifest} without changing anything
/// else in it ({detail}); this is a defect in skeletons ``.
fn manifest_defect_message(manifest: &ManifestShown, key: &WearingKey, detail: &str) -> String {
    format!(
        "remove could not take `{}` out of {} without changing anything else in it ({}); this is \
         a defect in skeletons",
        Escaped(key.as_str()),
        Escaped(manifest.as_str()),
        Escaped(detail)
    )
}

/// `` remove could not read {lockfile} to see whether bringing it current
/// changed it: {detail} ``.
fn lockfile_unreadable_message(lockfile: &str, detail: &str) -> String {
    format!(
        "remove could not read {} to see whether bringing it current changed it: {}",
        Escaped(lockfile),
        Escaped(detail)
    )
}

/// `` remove took `{key}` out of {manifest}, and Cargo refused what was left:
/// {stderr} ``.
fn cargo_refused_message(manifest: &ManifestShown, key: &WearingKey, stderr: &str) -> String {
    format!(
        "remove took `{}` out of {}, and Cargo refused what was left: {}",
        Escaped(key.as_str()),
        Escaped(manifest.as_str()),
        Escaped(stderr)
    )
}

/// `` remove took `{key}` out of {manifest}, and reading it back still finds
/// [package.metadata.skeletons.{key}]; this is a defect in skeletons ``.
fn still_worn_message(manifest: &ManifestShown, key: &WearingKey) -> String {
    let (manifest, key) = (Escaped(manifest.as_str()), Escaped(key.as_str()));
    format!(
        "remove took `{key}` out of {manifest}, and reading it back still finds \
         [package.metadata.skeletons.{key}]; this is a defect in skeletons"
    )
}

/// `` remove took `{key}` out of {manifest}, and reading the workspace back no
/// longer finds the package this command line is built from; this is a defect
/// in skeletons ``.
fn package_vanished_message(manifest: &ManifestShown, key: &WearingKey) -> String {
    format!(
        "remove took `{}` out of {}, and reading the workspace back no longer finds the package \
         this command line is built from; this is a defect in skeletons",
        Escaped(key.as_str()),
        Escaped(manifest.as_str())
    )
}

/// `` remove took `{key}` out of {manifest}, and Cargo still reports a
/// dependency under `{key}`: it is declared somewhere remove does not edit;
/// take it out by hand, then run the `remove` task again ``.
fn dependency_remains_message(manifest: &ManifestShown, key: &WearingKey) -> String {
    let (manifest, key) = (Escaped(manifest.as_str()), Escaped(key.as_str()));
    format!(
        "remove took `{key}` out of {manifest}, and Cargo still reports a dependency under \
         `{key}`: it is declared somewhere remove does not edit; take it out by hand, then {}",
        WritingCommand::Remove.run_again()
    )
}

/// `` remove could not delete {path}: {detail} ``.
fn delete_failed_message(path: &str, detail: &str) -> String {
    format!(
        "remove could not delete {}: {}",
        Escaped(path),
        Escaped(detail)
    )
}

/// `line`, a failure of the deletion as `rollback` has worded it, followed by
/// what became of the files: the clause for those deleted, then the clause for
/// those not.
///
/// `rollback` ends the line with what it did about the manifest and
/// `Cargo.lock`, and says nothing about the files, which it cannot put back.
/// The clauses come after its words, so the whole line is true whichever it
/// said.
///
/// A failed deletion is a file that could not be deleted, and that file is among
/// those not deleted, so `undeleted` is never empty.
pub(crate) fn with_the_files_deleted(
    line: &str,
    deleted: &[String],
    undeleted: &[String],
) -> String {
    assert!(
        !undeleted.is_empty(),
        "the file that failed to delete is among those not deleted"
    );
    format!(
        "{line}{}{}",
        deleted_clause(deleted),
        undeleted_clause(undeleted)
    )
}

/// `` ; the files remove had already deleted are not put back, and git has
/// them: {files} ``, in the singular for one, and `` ; remove had deleted no
/// file `` for none.
fn deleted_clause(deleted: &[String]) -> String {
    let files = escaped_list(deleted);
    match deleted {
        [] => "; remove had deleted no file".to_owned(),
        [_one] => format!(
            "; the file remove had already deleted is not put back, and git has it: {files}"
        ),
        _several => format!(
            "; the files remove had already deleted are not put back, and git has them: {files}"
        ),
    }
}

/// `` ; not deleted: {files} ``.
fn undeleted_clause(undeleted: &[String]) -> String {
    format!("; not deleted: {}", escaped_list(undeleted))
}

/// `paths` escaped, joined as this crate's one list grammar does.
fn escaped_list(paths: &[String]) -> String {
    let escaped: Vec<String> = paths
        .iter()
        .map(|path| Escaped(path.as_str()).to_string())
        .collect();
    join_and(&escaped)
}

/// `items` escaped, each in backticks, joined as this crate's one list grammar
/// does.
fn backticked<'key>(items: impl Iterator<Item = &'key WearingKey>) -> String {
    let quoted: Vec<String> = items
        .map(|item| format!("`{}`", Escaped(item.as_str())))
        .collect();
    join_and(&quoted)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Asked, RemoveRefusal, deleted_clause, undeleted_clause, with_the_files_deleted};
    use crate::remove::files::{FilesFlag, Unknowable};
    use crate::remove::names::{ManifestShown, WearingKey};
    use crate::remove::non_empty::NonEmpty;
    use crate::remove::resolve::{Elsewhere, PickRefusal};
    use crate::survey::poison::{
        POISON_ESCAPED, assert_escaped_once, assert_every_kind, assert_one_line, poison,
    };

    fn key(text: &str) -> WearingKey {
        WearingKey::new(text)
    }

    fn keys_of(texts: &[&str]) -> Vec<WearingKey> {
        texts.iter().map(|text| key(text)).collect()
    }

    fn several_keys(first: &str, rest: &[&str]) -> NonEmpty<WearingKey> {
        NonEmpty::new(key(first), keys_of(rest))
    }

    fn manifest(text: &str) -> ManifestShown {
        ManifestShown::new(text)
    }

    fn reasons(first: &str, rest: &[&str]) -> NonEmpty<String> {
        NonEmpty::new(
            first.to_owned(),
            rest.iter().map(|reason| (*reason).to_owned()).collect(),
        )
    }

    fn elsewhere(manifest: &str, key: &str) -> Elsewhere {
        Elsewhere {
            manifest: manifest.to_owned(),
            key: key.to_owned(),
        }
    }

    fn asked(files_flag: FilesFlag) -> Asked {
        Asked {
            name: "a-skeleton".to_owned(),
            manifest: manifest("ritual/Cargo.toml"),
            command: "cargo ritual skeletons remove".to_owned(),
            files_flag,
        }
    }

    #[test]
    fn a_command_line_outside_its_project_is_worded_for_remove() {
        let refusal = RemoveRefusal::OutsideItsProject {
            package: "skeletons-ritual".to_owned(),
            root: PathBuf::from("/path/to/workspace"),
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
            manifest: manifest("ritual/Cargo.toml"),
            keys: keys_of(&["ci", "dependabot"]),
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
            manifest: manifest("Cargo.toml"),
            keys: keys_of(&["a", "b", "c"]),
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
            manifest: manifest("ritual/Cargo.toml"),
            keys: keys_of(&["ci"]),
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
            manifest: manifest("ritual/Cargo.toml"),
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
            manifest: manifest("ritual/Cargo.toml"),
            elsewhere: NonEmpty::new(elsewhere("tools/Cargo.toml", "ci"), Vec::new()),
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
            manifest: manifest("ritual/Cargo.toml"),
            elsewhere: NonEmpty::new(
                elsewhere("a/Cargo.toml", "x"),
                vec![elsewhere("b/Cargo.toml", "y")],
            ),
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
            manifest: manifest("ritual/Cargo.toml"),
            elsewhere: NonEmpty::new(
                elsewhere("a/Cargo.toml", "x"),
                vec![elsewhere("a/Cargo.toml", "y")],
            ),
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
            asked: Box::new(asked(FilesFlag::Bare)),
            keys: several_keys("ci", &["ci-next"]),
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
        for (files_flag, flag) in [
            (FilesFlag::DeleteFiles, " --delete-files"),
            (FilesFlag::KeepFiles, " --keep-files"),
        ] {
            let refusal = RemoveRefusal::SeveralKeys {
                asked: Box::new(asked(files_flag)),
                keys: several_keys("ci", &["ci-next"]),
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
                keys: several_keys("ci", &["ci-next"]),
            },
            asked(FilesFlag::KeepFiles),
            vec![elsewhere("tools/Cargo.toml", "x")],
        );

        assert_eq!(
            refusal,
            RemoveRefusal::SeveralKeys {
                asked: Box::new(asked(FilesFlag::KeepFiles)),
                keys: several_keys("ci", &["ci-next"]),
            }
        );
    }

    #[test]
    fn a_pick_that_found_nothing_is_worn_elsewhere_when_some_manifest_wears_the_name() {
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::Neither {
                keys: keys_of(&["lint"]),
            },
            asked(FilesFlag::Bare),
            vec![elsewhere("tools/Cargo.toml", "ci")],
        );

        assert_eq!(
            refusal,
            RemoveRefusal::WornElsewhere {
                name: "a-skeleton".to_owned(),
                manifest: manifest("ritual/Cargo.toml"),
                elsewhere: NonEmpty::new(elsewhere("tools/Cargo.toml", "ci"), Vec::new()),
            }
        );
    }

    #[test]
    fn a_pick_that_found_nothing_anywhere_lists_the_manifests_keys() {
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::Neither {
                keys: keys_of(&["lint"]),
            },
            asked(FilesFlag::Bare),
            Vec::new(),
        );

        assert_eq!(
            refusal,
            RemoveRefusal::NotWorn {
                name: "a-skeleton".to_owned(),
                manifest: manifest("ritual/Cargo.toml"),
                keys: keys_of(&["lint"]),
            }
        );
    }

    #[test]
    fn files_that_cannot_be_known_give_each_reason_as_it_stands_and_both_remedies() {
        let refusal = RemoveRefusal::FilesUnknowable {
            key: key("ci"),
            unknowable: Unknowable {
                reasons: reasons("first reason", &["second reason"]),
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
        let refusal = RemoveRefusal::DeleteFailed {
            path: "ci.yml".to_owned(),
            detail: "permission denied".to_owned(),
        };
        let message = refusal.message();

        assert_eq!(refusal.into_failure().to_string(), message);
    }

    /// The failures of editing the manifest and bringing the lockfile current,
    /// each with the fixed text it is held to.
    fn manifest_and_lockfile_failures() -> Vec<(RemoveRefusal, &'static str)> {
        vec![
            (
                RemoveRefusal::ManifestUnreadable {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
                    detail: "permission denied".to_owned(),
                },
                "remove could not read ritual/Cargo.toml to take `ci` out of it: permission denied",
            ),
            (
                RemoveRefusal::ManifestDefect {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
                    detail: "it differs".to_owned(),
                },
                "remove could not take `ci` out of ritual/Cargo.toml without changing anything \
                 else in it (it differs); this is a defect in skeletons",
            ),
            (
                RemoveRefusal::CargoRefused {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
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
        ]
    }

    /// The failures of reading the workspace back, each with the fixed text it
    /// is held to.
    fn read_back_failures() -> Vec<(RemoveRefusal, &'static str)> {
        vec![
            (
                RemoveRefusal::StillWorn {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
                },
                "remove took `ci` out of ritual/Cargo.toml, and reading it back still finds \
                 [package.metadata.skeletons.ci]; this is a defect in skeletons",
            ),
            (
                RemoveRefusal::PackageVanished {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
                },
                "remove took `ci` out of ritual/Cargo.toml, and reading the workspace back no \
                 longer finds the package this command line is built from; this is a defect in \
                 skeletons",
            ),
            (
                RemoveRefusal::DependencyRemains {
                    manifest: manifest("ritual/Cargo.toml"),
                    key: key("ci"),
                },
                "remove took `ci` out of ritual/Cargo.toml, and Cargo still reports a dependency \
                 under `ci`: it is declared somewhere remove does not edit; take it out by hand, \
                 then run the `remove` task again",
            ),
        ]
    }

    /// The failures of deleting the files, each with the fixed text it is held
    /// to.
    fn file_failures() -> Vec<(RemoveRefusal, &'static str)> {
        vec![(
            RemoveRefusal::DeleteFailed {
                path: ".github/workflows/ci.yml".to_owned(),
                detail: "permission denied".to_owned(),
            },
            "remove could not delete .github/workflows/ci.yml: permission denied",
        )]
    }

    /// Every failure that can come after the first write, with the fixed text
    /// it is held to.
    fn failures() -> Vec<(RemoveRefusal, &'static str)> {
        let mut all = manifest_and_lockfile_failures();
        all.extend(read_back_failures());
        all.extend(file_failures());
        all
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
    const KINDS: usize = 13;

    const fn kind_index(refusal: &RemoveRefusal) -> usize {
        match refusal {
            RemoveRefusal::OutsideItsProject { .. } => 0,
            RemoveRefusal::NotWorn { .. } => 1,
            RemoveRefusal::WornElsewhere { .. } => 2,
            RemoveRefusal::SeveralKeys { .. } => 3,
            RemoveRefusal::FilesUnknowable { .. } => 4,
            RemoveRefusal::ManifestUnreadable { .. } => 5,
            RemoveRefusal::ManifestDefect { .. } => 6,
            RemoveRefusal::CargoRefused { .. } => 7,
            RemoveRefusal::StillWorn { .. } => 8,
            RemoveRefusal::DependencyRemains { .. } => 9,
            RemoveRefusal::DeleteFailed { .. } => 10,
            RemoveRefusal::LockfileUnreadable { .. } => 11,
            RemoveRefusal::PackageVanished { .. } => 12,
        }
    }

    /// The poison as a field that holds a [`WearingKey`].
    fn poisoned_key() -> WearingKey {
        key(&poison())
    }

    /// The poison as a field that holds a [`ManifestShown`].
    fn poisoned_manifest() -> ManifestShown {
        manifest(&poison())
    }

    /// One of each refusal about the request that holds text from outside, with
    /// that text in every field that holds any. The reasons of a refusal about
    /// files are other refusals' messages, already escaped, so they are given
    /// escaped and must not be escaped again.
    fn request_samples() -> Vec<RemoveRefusal> {
        vec![
            RemoveRefusal::OutsideItsProject {
                package: poison(),
                root: PathBuf::from(poison()),
            },
            RemoveRefusal::NotWorn {
                name: poison(),
                manifest: poisoned_manifest(),
                keys: vec![poisoned_key(), poisoned_key()],
            },
            RemoveRefusal::WornElsewhere {
                name: poison(),
                manifest: poisoned_manifest(),
                elsewhere: NonEmpty::new(elsewhere(&poison(), &poison()), Vec::new()),
            },
            RemoveRefusal::SeveralKeys {
                asked: Box::new(Asked {
                    name: poison(),
                    manifest: poisoned_manifest(),
                    command: poison(),
                    files_flag: FilesFlag::Bare,
                }),
                keys: NonEmpty::new(poisoned_key(), vec![poisoned_key()]),
            },
            RemoveRefusal::FilesUnknowable {
                key: poisoned_key(),
                unknowable: Unknowable {
                    reasons: NonEmpty::new(POISON_ESCAPED.to_owned(), Vec::new()),
                },
            },
        ]
    }

    /// One of each failure of the run that holds text from outside, with that
    /// text in every field that holds any.
    fn run_samples() -> Vec<RemoveRefusal> {
        vec![
            RemoveRefusal::ManifestUnreadable {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
                detail: poison(),
            },
            RemoveRefusal::ManifestDefect {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
                detail: poison(),
            },
            RemoveRefusal::CargoRefused {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
                stderr: poison(),
            },
            RemoveRefusal::LockfileUnreadable {
                lockfile: poison(),
                detail: poison(),
            },
            RemoveRefusal::StillWorn {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
            },
            RemoveRefusal::PackageVanished {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
            },
            RemoveRefusal::DependencyRemains {
                manifest: poisoned_manifest(),
                key: poisoned_key(),
            },
            RemoveRefusal::DeleteFailed {
                path: poison(),
                detail: poison(),
            },
        ]
    }

    #[test]
    fn every_refusal_about_the_request_prints_text_from_outside_on_one_line_escaped_once() {
        // The poison is a name with a line break in it. It goes into every
        // field of every refusal about the request that holds text from
        // outside, and each message must be one line that shows the name as
        // the two characters `\n`, never raw and never escaped a second time.
        for refusal in &request_samples() {
            assert_escaped_once(&refusal.message(), &format!("{refusal:?}"));
        }
    }

    #[test]
    fn every_failure_of_the_run_prints_text_from_outside_on_one_line_escaped_once() {
        // The same poison, in every field of every failure that can come after
        // the first write.
        for failure in &run_samples() {
            assert_escaped_once(&failure.message(), &format!("{failure:?}"));
        }
    }

    #[test]
    fn the_samples_of_text_from_outside_cover_every_kind_of_refusal() {
        // A kind added without a sample is found here.
        let (request, run) = (request_samples(), run_samples());

        assert_every_kind(
            request.iter().chain(&run).map(kind_index),
            KINDS,
            "RemoveRefusal",
        );
    }

    #[test]
    fn an_empty_key_is_a_key_like_any_other_in_every_message_that_prints_one() {
        // A wearing under the empty key is valid in a manifest, so every
        // message that prints a key prints it as two backticks with nothing
        // between them, and stays one line.
        let by_key: Vec<RemoveRefusal> = vec![
            RemoveRefusal::FilesUnknowable {
                key: key(""),
                unknowable: Unknowable {
                    reasons: reasons("a reason", &[]),
                },
            },
            RemoveRefusal::ManifestUnreadable {
                manifest: manifest("Cargo.toml"),
                key: key(""),
                detail: "denied".to_owned(),
            },
            RemoveRefusal::ManifestDefect {
                manifest: manifest("Cargo.toml"),
                key: key(""),
                detail: "it differs".to_owned(),
            },
            RemoveRefusal::CargoRefused {
                manifest: manifest("Cargo.toml"),
                key: key(""),
                stderr: "no".to_owned(),
            },
            RemoveRefusal::StillWorn {
                manifest: manifest("Cargo.toml"),
                key: key(""),
            },
            RemoveRefusal::PackageVanished {
                manifest: manifest("Cargo.toml"),
                key: key(""),
            },
            RemoveRefusal::DependencyRemains {
                manifest: manifest("Cargo.toml"),
                key: key(""),
            },
        ];

        for refusal in &by_key {
            let message = refusal.message();
            assert_one_line(&message, &format!("{refusal:?}"));
            assert!(message.contains("``"), "{message}");
        }
    }

    #[test]
    fn asking_to_remove_the_empty_name_when_nothing_is_worn_under_it_says_so_plainly() {
        // `remove ""` where no wearing sits under the empty key and no crate
        // has the empty name: the name is shown as two backticks and the
        // message lists the keys there are.
        let refusal = RemoveRefusal::from_pick(
            PickRefusal::Neither {
                keys: keys_of(&["ci"]),
            },
            Asked {
                name: String::new(),
                manifest: manifest("ritual/Cargo.toml"),
                command: "cargo ritual skeletons remove".to_owned(),
                files_flag: FilesFlag::Bare,
            },
            Vec::new(),
        );

        assert_eq!(
            refusal.message(),
            "`` is neither a key ritual/Cargo.toml wears a skeleton under nor the crate of one, \
             so remove wrote nothing; its key is `ci`, so give the `remove` task that one"
        );
    }

    /// `names` as the owned strings a deletion carries.
    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn some_files_deleted_and_some_not_are_each_named_after_what_rollback_said() {
        // The failed file is among those not deleted, and each list is joined
        // as the other messages join theirs.
        assert_eq!(
            with_the_files_deleted(
                "remove could not delete b.yml: permission denied; ritual put the project back \
                 as it found it",
                &paths(&["a.yml", "a2.yml"]),
                &paths(&["b.yml", "c.yml", "d.yml"]),
            ),
            "remove could not delete b.yml: permission denied; ritual put the project back as \
             it found it; the files remove had already deleted are not put back, and git has \
             them: a.yml and a2.yml; not deleted: b.yml, c.yml and d.yml"
        );
    }

    #[test]
    fn one_file_deleted_is_named_in_the_singular() {
        assert_eq!(
            with_the_files_deleted("it failed", &paths(&["a.yml"]), &paths(&["b.yml"])),
            "it failed; the file remove had already deleted is not put back, and git has it: \
             a.yml; not deleted: b.yml"
        );
    }

    #[test]
    fn no_file_deleted_says_so_and_names_every_file_not_deleted() {
        assert_eq!(
            with_the_files_deleted("it failed", &[], &paths(&["a.yml", "b.yml"])),
            "it failed; remove had deleted no file; not deleted: a.yml and b.yml"
        );
    }

    #[test]
    #[should_panic(expected = "the file that failed to delete is among those not deleted")]
    fn a_failed_deletion_with_no_file_not_deleted_is_a_defect() {
        // A failure of the deletion is a file that could not be deleted, so a
        // list of files not deleted that is empty is a defect in the caller.
        let _ = with_the_files_deleted("it failed", &paths(&["a.yml"]), &[]);
    }

    #[test]
    fn each_clause_about_the_files_is_worded_alone_for_every_count() {
        assert_eq!(deleted_clause(&[]), "; remove had deleted no file");
        assert_eq!(
            deleted_clause(&paths(&["a"])),
            "; the file remove had already deleted is not put back, and git has it: a"
        );
        assert_eq!(
            deleted_clause(&paths(&["a", "b", "c"])),
            "; the files remove had already deleted are not put back, and git has them: a, b \
             and c"
        );
        assert_eq!(undeleted_clause(&paths(&["a"])), "; not deleted: a");
    }

    #[test]
    fn the_clauses_about_the_files_print_paths_from_outside_on_one_line_escaped_once() {
        let named = vec![poison()];

        let text = with_the_files_deleted("it failed", &named, &named);

        assert_escaped_once(&text, "the clauses about the files");
    }

    #[test]
    fn no_refusal_names_sync_or_wear() {
        // Every message is worded for `remove` and sends the wearer to
        // nothing else.
        let mut all: Vec<RemoveRefusal> =
            failures().into_iter().map(|(refusal, _)| refusal).collect();
        all.extend([
            RemoveRefusal::OutsideItsProject {
                package: "p".to_owned(),
                root: PathBuf::from("/r"),
            },
            RemoveRefusal::NotWorn {
                name: "n".to_owned(),
                manifest: manifest("Cargo.toml"),
                keys: keys_of(&["a", "b"]),
            },
            RemoveRefusal::WornElsewhere {
                name: "n".to_owned(),
                manifest: manifest("Cargo.toml"),
                elsewhere: NonEmpty::new(elsewhere("t/Cargo.toml", "k"), Vec::new()),
            },
            RemoveRefusal::FilesUnknowable {
                key: key("k"),
                unknowable: Unknowable {
                    reasons: reasons("a reason", &[]),
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
