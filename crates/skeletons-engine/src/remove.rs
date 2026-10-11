//! `remove`: stop wearing a skeleton in one command.
//!
//! It takes the skeleton's dependency and its
//! `[package.metadata.skeletons.<key>]` table out of the running command
//! line's own manifest, brings `Cargo.lock` current, and deletes the files the
//! skeleton wrote only when that is plainly what was asked. Deleting a skeleton's
//! files deletes every one it claims, including one the project needs to build,
//! and git has them. Every decision it
//! makes, and every word it says, is a pure function of values in [`resolve`],
//! [`files`], [`manifest`], [`refusal`], [`message`] and [`confirm`], so each
//! is held by a test that touches no disk, no process and no git. [`give_back`]
//! and [`deletion`] are the two effects, each held by a test against a real
//! directory or repository. The child modules, and what each is for:
//!
//! - [`names`] keeps the two names `remove` passes around as text, the
//!   wearing's key and the manifest as a message shows it, from being swapped;
//! - [`non_empty`] holds a list that cannot be empty, for the refusals whose
//!   words are wrong for an empty one;
//! - [`resolve`] reads the name the wearer typed as a key or a crate, and
//!   finds the one wearing it means;
//! - [`files`] decides from the typed flag and the survey which files git is
//!   asked about, then from git's answers which go, are kept or are left, the
//!   lockfile `remove` brings current among the latter and never deleted, and
//!   which cannot be known at all;
//! - [`manifest`] edits the manifest's text and checks the edit against an
//!   independent parse;
//! - [`refusal`] and [`message`] are the one place each refusal, failure and
//!   success line is worded;
//! - [`confirm`] reads the workspace back and checks that the wearing is gone;
//! - [`give_back`] asks git whether it could give each file back, which
//!   settles the files [`files`] plans to put to git;
//! - [`deletion`] deletes those files, and the directories they leave empty.
//!
//! This module is the command. It settles everything it can refuse before it
//! writes anything, then does all of its writing inside [`rollback::attempt`],
//! so that a failure puts the manifest and `Cargo.lock` back. The files are
//! deleted last, after the manifest and `Cargo.lock` are written and the
//! workspace has been read back, and a deleted file is not put back by a later
//! failure: git has it, and the failure names the files deleted and the files
//! not.

mod confirm;
mod deletion;
mod files;
mod give_back;
mod manifest;
mod message;
mod names;
mod non_empty;
mod refusal;
mod resolve;

use std::path::{Path, PathBuf};

use rituals::{CommandLine, Failure, Outcome, Task, clap, report};
use rituals_compose::rollback;
use rollback::{Changes, Wording};

use crate::check::{AbortingCommand, abort_message};
use crate::command_line_crate::CommandLineCrate;
use crate::command_line_crate::hand_back::{self, Tracking};
use crate::command_line_crate::writable;
use crate::current_directory;
use crate::survey;
use crate::work_tree;
use crate::work_tree::abort::WorkTreeAbort;
use crate::work_tree::writing_command::WritingCommand;
use crate::workspace::{self, Prospect, ReadWorkspaceError};
use deletion::Deletion;
use files::{Claimed, FilesFlag, Settlement};
use message::{Lockfile, Removed};
use names::{ManifestShown, WearingKey};
use refusal::{Asked, RemoveRefusal};
use resolve::Candidate;

/// What `remove --help` says the task does.
const ABOUT: &str = "stop wearing a skeleton: take out its dependency and wearing table, and its \
                     files if none has changed";

/// The command asking git about the work tree, for the words of its refusals.
const WRITING: WritingCommand = WritingCommand::Remove;

/// What a failed run says to do, when it could not put everything back.
const RETRY: &str = "running the `remove` task again";

// `remove`'s own command-line arguments.
//
// No `///` doc comment on this struct, for the reason `wear::WearArguments`
// carries none: `rituals` sets a task's `about` after clap's derived
// `augment_args` and leaves `long_about` alone, so a struct doc comment of
// more than one paragraph would replace `--help`'s text, and `-h` would end in
// "(see more with '--help')" rather than a bare "Print help". Each field keeps
// its own `///` line, which is the argument's help text.
#[derive(Debug, clap::Args)]
struct RemoveArguments {
    /// the wearing's dependency key, or the skeleton's crate
    #[arg(value_name = "KEY|CRATE")]
    name: String,
    /// delete every file the skeleton claims, changed ones included
    #[arg(long, conflicts_with = "keep_files")]
    delete_files: bool,
    /// leave every file the skeleton claims where it is
    #[arg(long)]
    keep_files: bool,
}

impl RemoveArguments {
    /// Which flag the wearer typed about the files.
    fn files_flag(&self) -> FilesFlag {
        match (self.delete_files, self.keep_files) {
            (false, false) => FilesFlag::Bare,
            (true, false) => FilesFlag::DeleteFiles,
            (false, true) => FilesFlag::KeepFiles,
            (true, true) => unreachable!("clap refuses --delete-files together with --keep-files"),
        }
    }
}

/// Builds the `remove` task.
pub(crate) fn task() -> Task {
    Task::receiving_command_line(
        ABOUT,
        |command_line: &CommandLine, arguments: RemoveArguments| run(command_line, &arguments),
    )
}

fn run(command_line: &CommandLine, arguments: &RemoveArguments) -> Outcome {
    let prepared = prepare(command_line, &arguments.name, arguments.files_flag())?;
    let wording = Wording::project(&prepared.workspace_root, RETRY);
    let done = attempt_removal(
        wording,
        &prepared,
        || workspace::relock(&prepared.directory),
        || workspace::read_prospect(&prepared.directory, &prepared.command_line_crate.package),
        deletion::delete_from_disk,
    )?;
    let removed = Removed {
        wearing: &prepared.wearing,
        manifest: &prepared.manifest(),
        files: &prepared.files,
        lockfile: done.lockfile,
    };
    for line in message::removed_lines(&removed) {
        report(line);
    }
    Ok(())
}

/// What a run has settled before it writes anything: the wearing it removes,
/// what becomes of its files, where it runs, and the crate it writes into.
struct Prepared {
    wearing: Candidate,
    files: Settlement,
    directory: PathBuf,
    workspace_root: PathBuf,
    command_line_crate: CommandLineCrate,
    /// Whether git tracks `Cargo.lock`, which the success message needs to
    /// say whether to commit it.
    lockfile: Tracking,
}

impl Prepared {
    /// The command line's own manifest, as a message shows it.
    fn manifest(&self) -> ManifestShown {
        ManifestShown::new(&self.command_line_crate.manifest_shown)
    }
}

/// Everything `remove` can refuse before it writes anything.
///
/// The workspace is read `--locked`, and only `--locked`, so that a lockfile
/// that is stale or missing is refused before bringing it current can rewrite
/// more of it than the skeleton, and so that no read or check made here
/// rewrites the lockfile: a refusal leaves it as it was. What that read holds
/// answers every refusal that is about the name and the manifest as they stand,
/// and finds the crate `remove` writes into. The files are surveyed next, under
/// whatever flag was typed, because whether `check` refuses the wearing decides
/// how the first success line names it, and then planned: `--keep-files` plans
/// to touch none, so a wearing whose files cannot be known stops nothing there
/// and every other flag refuses it. The work tree is asked after those,
/// so a request that is wrong is refused as wrong whether or not the tree is
/// clean, and then the manifest and lockfile are proven ones git can hand back
/// ([`hand_back::check`]) and can be written in place ([`writable::check`]).
/// Every file that is in the work tree, and that is not the lockfile, is then
/// asked of git ([`give_back::fate`]), and only with git's answers is it
/// decided whether a bare run keeps the files that changed or deletes them: a
/// file git could not give back is left whatever the flag and counts for
/// nothing in that decision, and so is the lockfile a skeleton claims, which
/// `remove` brings current and so never deletes, and which was set aside when
/// the files were planned. Before anything is written, the files to delete are
/// checked against the real paths of the manifest and the lockfile
/// ([`assert_nothing_written_is_deleted`]).
fn prepare(
    command_line: &CommandLine,
    name: &str,
    files_flag: FilesFlag,
) -> Result<Prepared, Failure> {
    let directory = current_directory::read()?;
    let package = command_line.identity().package_name();
    let prospect =
        workspace::read_prospect(&directory, package).map_err(|error| aborted(&error))?;
    let Some(member) = &prospect.member else {
        return Err(RemoveRefusal::OutsideItsProject {
            package: package.to_owned(),
            root: prospect.workspace.root,
        }
        .into_failure());
    };
    let manifest = ManifestShown::new(&member.manifest);
    let candidates = resolve::candidates_in(&prospect.workspace, &manifest, &member.declared);
    let wearing = match resolve::pick(name, &candidates) {
        Ok(picked) => picked.clone(),
        Err(refusal) => {
            let elsewhere = resolve::worn_elsewhere(&prospect.workspace, &manifest, name);
            let asked = Asked {
                name: name.to_owned(),
                manifest,
                command: command_line.cargo_command(),
                files_flag,
            };
            return Err(RemoveRefusal::from_pick(refusal, asked, elsewhere).into_failure());
        }
    };
    // The survey is run whatever flag was typed: the files are its to say only
    // when the flag would delete some, but whether `check` refuses the wearing
    // decides how the first line names it.
    let claimed = Claimed::of(
        &survey::survey(&prospect.workspace),
        &manifest,
        wearing.key(),
    );
    let wearing = wearing.according_to(&claimed);
    let command_line_crate = CommandLineCrate::new(package, member, &prospect.workspace.root);
    let plan = files::plan(files_flag, claimed, &command_line_crate.lockfile_shown).map_err(
        |unknowable| {
            RemoveRefusal::FilesUnknowable {
                key: wearing.key().clone(),
                unknowable,
            }
            .into_failure()
        },
    )?;
    // `remove` changes the manifest and `Cargo.lock` in place and deletes
    // files, so git is the only way back for what `rollback` could not take
    // back, and only for what was committed: an edit made by something else
    // after the proof, before the delete, was never committed and goes with the
    // file. Whole-tree dirt counts, as it does for `sync` and `wear`, because
    // the person's own uncommitted edits would otherwise be mixed into what
    // `remove` wrote or deleted. A clean tree says nothing about a file git is
    // told not to read, so the two files are then asked of git's index, and
    // each file in the work tree is put to the proof that git holds exactly
    // what is there, before it is decided which of them are kept.
    let (work_tree, clean) = work_tree::open_clean(&prospect.workspace.root, WRITING)?;
    let lockfile = hand_back::check(&work_tree, &clean, &command_line_crate, WRITING)?;
    writable::check(&command_line_crate, WRITING)?;
    let files = plan
        .settle(|path| give_back::fate(&work_tree, &clean, path))
        .map_err(|abort| git_could_not_be_asked(&abort, &prospect))?;
    assert_nothing_written_is_deleted(&files, &prospect.workspace.root, &command_line_crate);
    Ok(Prepared {
        wearing,
        files,
        directory,
        workspace_root: prospect.workspace.root,
        command_line_crate,
        lockfile,
    })
}

/// Asserts that no file about to be deleted is the manifest or the lockfile the
/// run is about to write.
///
/// This is a claim about the code, not about the project: the lockfile is set
/// aside when the files are planned, and the walk of a skeleton's files refuses
/// any name like a Cargo manifest, so a skeleton cannot claim a manifest. It is
/// checked here, against the real absolute paths and before anything is
/// written, because a panic inside [`rollback::attempt`] would skip the undo
/// and leave the project half written.
fn assert_nothing_written_is_deleted(
    files: &Settlement,
    workspace_root: &Path,
    command_line_crate: &CommandLineCrate,
) {
    for file in files.to_delete() {
        let path = file.to_path(workspace_root);
        assert_ne!(
            path, command_line_crate.manifest_path,
            "the manifest remove writes is never among the files it deletes"
        );
        assert_ne!(
            path, command_line_crate.lockfile_path,
            "the lockfile remove writes is never among the files it deletes"
        );
    }
}

/// The failure for a workspace that could not be read, in the words `check`
/// uses for the same abort.
fn aborted(error: &ReadWorkspaceError) -> Failure {
    let (_kind, message) = abort_message(error, AbortingCommand::Remove);
    Failure::new(message)
}

/// The failure for a question to git that could not be asked, in `remove`'s
/// words.
fn git_could_not_be_asked(abort: &WorkTreeAbort, prospect: &Prospect) -> Failure {
    Failure::new(work_tree::message::abort_message(
        abort,
        &prospect.workspace.root,
        WRITING,
    ))
}

/// What a run that has written everything leaves for the lines it prints.
#[derive(Debug)]
struct Done {
    lockfile: Lockfile,
}

/// The whole of what `remove` changes, inside [`rollback::attempt`], and the
/// words of a failure that left deleted files behind.
///
/// `rollback` words the end of a failure by what it put back, and it cannot put
/// back a file that was deleted, so a failure of the deletion has the clauses
/// about the files appended after what `rollback` said. They name every file
/// deleted and every file not, whichever clause `rollback` ended with.
///
/// This is the one place the attempt is made and its failure completed: the
/// command and the tests of its words both call it, so a test sees the line the
/// command prints.
fn attempt_removal(
    wording: Wording<'_>,
    prepared: &Prepared,
    relock: impl FnOnce() -> Result<(), ReadWorkspaceError>,
    read_back: impl FnOnce() -> Result<Prospect, ReadWorkspaceError>,
    delete_file: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<Done, Failure> {
    // `rollback::attempt` returns only a `Failure`, so what the deletion had
    // done travels beside it.
    let mut failed_deletion: Option<Deletion> = None;
    rollback::attempt(wording, |changes| {
        write_with(
            changes,
            prepared,
            &mut failed_deletion,
            relock,
            read_back,
            delete_file,
        )
    })
    .map_err(|failure| match failed_deletion {
        None => failure,
        Some(deletion) => Failure::new(refusal::with_the_files_deleted(
            &failure.to_string(),
            &deletion.deleted,
            &deletion.undeleted,
        )),
    })
}

/// Takes the wearing out of the manifest, brings `Cargo.lock` current, reads
/// the workspace back to confirm the wearing is gone, and deletes the files that
/// are to be deleted, in that order, with the relock, the read back and the
/// delete taken as arguments so a test can stand in for `cargo` and for a file
/// that cannot be deleted.
///
/// The files go last, so the manifest and the lockfile are already written when
/// a delete fails, and nothing reads the workspace after the first file is
/// gone. None of them is the manifest or the lockfile just written;
/// [`prepare`] has checked that. When a delete fails, what it had done is put
/// in `failed_deletion` for [`attempt_removal`] to word.
///
/// Every failure the wearer can cause or act on is an `Err`, so that
/// `rollback` puts the manifest and `Cargo.lock` back: it does that when the
/// run returns a failure and does not when it panics. A violated invariant is a
/// defect, and a defect panics; the assertions in the process runner that the
/// relock and the read back go through, and the one in [`deletion::delete`]
/// that the workspace root is absolute, can be reached from here, and the
/// project is not put back after one. The proof that git holds each file runs
/// in [`prepare`], before the attempt begins. Nothing here writes a file except
/// through `changes`, or deletes one except through `delete_file`.
fn write_with(
    changes: &mut Changes,
    prepared: &Prepared,
    failed_deletion: &mut Option<Deletion>,
    relock: impl FnOnce() -> Result<(), ReadWorkspaceError>,
    read_back: impl FnOnce() -> Result<Prospect, ReadWorkspaceError>,
    delete_file: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<Done, Failure> {
    let (manifest, key) = (prepared.manifest(), prepared.wearing.key());
    take_out_wearing(changes, &prepared.command_line_crate, &manifest, key)?;
    let lockfile = relock_lockfile(changes, prepared, relock)?;
    let read = read_back().map_err(|error| aborted(&error))?;
    let Some(member) = &read.member else {
        return Err(RemoveRefusal::PackageVanished {
            manifest,
            key: key.clone(),
        }
        .into_failure());
    };
    confirm::confirm(&read.workspace, &member.declared, &manifest, key)
        .map_err(RemoveRefusal::into_failure)?;
    // `read_back` is `FnOnce` and spent above, which is what keeps any read of
    // the workspace from coming after the deletes: the files are not put back,
    // so nothing is checked about what they leave.
    deletion::delete(
        &prepared.workspace_root,
        &prepared.files.to_delete(),
        delete_file,
    )
    .map_err(|failed| {
        *failed_deletion = Some(failed.deletion);
        failed.refusal.into_failure()
    })?;
    Ok(Done { lockfile })
}

/// Writes the manifest with the wearing taken out of it, through `changes`.
fn take_out_wearing(
    changes: &mut Changes,
    command_line_crate: &CommandLineCrate,
    manifest: &ManifestShown,
    key: &WearingKey,
) -> Result<(), Failure> {
    let before = std::fs::read_to_string(&command_line_crate.manifest_path).map_err(|error| {
        RemoveRefusal::ManifestUnreadable {
            manifest: manifest.clone(),
            key: key.clone(),
            detail: error.to_string(),
        }
        .into_failure()
    })?;
    let after = manifest::without_wearing(&before, key).map_err(|refusal| {
        match refusal {
            manifest::ManifestRefusal::Unreadable { detail } => RemoveRefusal::ManifestUnreadable {
                manifest: manifest.clone(),
                key: key.clone(),
                detail,
            },
            manifest::ManifestRefusal::Defect { detail } => RemoveRefusal::ManifestDefect {
                manifest: manifest.clone(),
                key: key.clone(),
                detail,
            },
        }
        .into_failure()
    })?;
    changes.write(&command_line_crate.manifest_path, after)
}

/// Brings `Cargo.lock` current through `relock`, recorded in `changes`, and
/// says whether that changed it.
///
/// Whether it changed is observed, from the file's bytes before and after,
/// because Cargo leaves a lockfile alone when nothing it holds is unneeded and
/// the last line a run prints names `Cargo.lock` only when it has something to
/// commit.
fn relock_lockfile(
    changes: &mut Changes,
    prepared: &Prepared,
    relock: impl FnOnce() -> Result<(), ReadWorkspaceError>,
) -> Result<Lockfile, Failure> {
    let command_line_crate = &prepared.command_line_crate;
    let (manifest, key) = (prepared.manifest(), prepared.wearing.key());
    let lockfile = &command_line_crate.lockfile_path;
    let unreadable = |error: std::io::Error| {
        RemoveRefusal::LockfileUnreadable {
            lockfile: command_line_crate.lockfile_shown.clone(),
            detail: error.to_string(),
        }
        .into_failure()
    };
    let before = std::fs::read(lockfile).map_err(unreadable)?;
    changes.run_changing(&[lockfile.as_path()], || {
        relock().map_err(|error| match error {
            ReadWorkspaceError::CargoMetadataFailed { stderr } => RemoveRefusal::CargoRefused {
                manifest: manifest.clone(),
                key: key.clone(),
                stderr,
            }
            .into_failure(),
            other @ (ReadWorkspaceError::Lockfile
            | ReadWorkspaceError::MetadataUnreadable { .. }
            | ReadWorkspaceError::CargoUnavailable { .. }) => aborted(&other),
        })
    })?;
    let after = std::fs::read(lockfile).map_err(unreadable)?;
    Ok(if before == after {
        Lockfile::Unchanged
    } else {
        Lockfile::Changed(prepared.lockfile)
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rituals::{Failure, clap};
    use rituals_compose::rollback::Wording;

    use super::files::{Disposition, Fate, FilesFlag, Settlement};
    use super::message::Lockfile;
    use super::names::WearingKey;
    use super::resolve::{Candidate, Standing};
    use super::{
        Done, Prepared, RemoveArguments, assert_nothing_written_is_deleted, attempt_removal,
        deletion,
    };
    use crate::claim::ClaimPath;
    use crate::command_line_crate::CommandLineCrate;
    use crate::command_line_crate::hand_back::Tracking;
    use crate::skeleton::Choices;
    use crate::workspace::{
        Declared, Member, Pin, Prospect, ReadWorkspaceError, SkeletonsTable, Wearing, Workspace,
        WornDependency,
    };

    const WORN: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                        [package.metadata.skeletons.tidy]\n\n\
                        [dev-dependencies]\ntidy = \"1\"\n";
    const UNWORN: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n";
    const LOCKFILE: &str = "# the lockfile before\nversion = 4\n";
    const RELOCKED: &str = "# the lockfile after\nversion = 4\n";
    const PUT_BACK: &str = "; ritual put the project back as it found it";

    /// A directory holding a manifest that wears `tidy`, a lockfile, and the
    /// `Prepared` that points at them.
    struct Project {
        directory: tempfile::TempDir,
        prepared: Prepared,
    }

    impl Project {
        fn new(manifest: &str, files: Settlement) -> Self {
            let directory = tempfile::tempdir().expect("a temporary directory");
            let manifest_path = directory.path().join("Cargo.toml");
            let lockfile_path = directory.path().join("Cargo.lock");
            std::fs::write(&manifest_path, manifest).expect("write the manifest");
            std::fs::write(&lockfile_path, LOCKFILE).expect("write the lockfile");
            let prepared = Prepared {
                wearing: Candidate::new(
                    WearingKey::new("tidy"),
                    Standing::Worn {
                        crate_name: "tidy".to_owned(),
                        version: semver::Version::new(0, 1, 0),
                    },
                ),
                files,
                directory: directory.path().to_path_buf(),
                workspace_root: directory.path().to_path_buf(),
                command_line_crate: CommandLineCrate {
                    package: "cli".to_owned(),
                    manifest_path,
                    manifest_shown: "Cargo.toml".to_owned(),
                    lockfile_path,
                    lockfile_shown: "Cargo.lock".to_owned(),
                },
                lockfile: Tracking::Tracked,
            };
            Self {
                directory,
                prepared,
            }
        }

        fn worn() -> Self {
            Self::new(WORN, Settlement::LeftAll)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.path(name)).expect("read the file")
        }

        fn write_file(&self, name: &str, contents: &str) {
            let path = self.path(name);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("create it");
            std::fs::write(path, contents).expect("write the file");
        }

        /// A run whose deletes are the real ones.
        fn run(
            &self,
            relock: impl FnOnce() -> Result<(), ReadWorkspaceError>,
            read_back: impl FnOnce() -> Result<Prospect, ReadWorkspaceError>,
        ) -> Result<Done, Failure> {
            self.run_deleting(relock, read_back, deletion::delete_from_disk)
        }

        /// A run through the function the command runs through, with
        /// `delete_file` as the delete.
        fn run_deleting(
            &self,
            relock: impl FnOnce() -> Result<(), ReadWorkspaceError>,
            read_back: impl FnOnce() -> Result<Prospect, ReadWorkspaceError>,
            delete_file: impl FnMut(&Path) -> std::io::Result<()>,
        ) -> Result<Done, Failure> {
            let wording =
                Wording::project(self.directory.path(), "running the `remove` task again");
            attempt_removal(wording, &self.prepared, relock, read_back, delete_file)
        }
    }

    /// What `cargo metadata` does when it brings the lockfile current.
    fn relocking(directory: &Path) -> impl FnOnce() -> Result<(), ReadWorkspaceError> {
        let lockfile = directory.join("Cargo.lock");
        move || {
            std::fs::write(lockfile, RELOCKED).expect("the stand-in rewrites the lockfile");
            Ok(())
        }
    }

    /// What `cargo metadata` does when the lockfile already needs nothing.
    fn relocking_nothing() -> impl FnOnce() -> Result<(), ReadWorkspaceError> {
        || Ok(())
    }

    fn worn_tidy() -> Wearing {
        Wearing::Worn(WornDependency {
            manifest: "Cargo.toml".to_owned(),
            key: "tidy".to_owned(),
            package: "tidy".to_owned(),
            version: semver::Version::new(0, 1, 0),
            pin: Pin::from_source(None, Path::new("/skeleton")),
            skeleton_directory: PathBuf::from("/skeleton"),
            choices: Ok(Choices::default()),
        })
    }

    fn dependency_on_tidy() -> Declared {
        Declared {
            key: "tidy".to_owned(),
            crate_name: "tidy".to_owned(),
        }
    }

    /// The workspace read back, with the member present.
    fn read_back(wearing: Vec<Wearing>, declared: Vec<Declared>) -> Prospect {
        Prospect {
            workspace: Workspace {
                root: PathBuf::from("/workspace"),
                wearing,
            },
            member: Some(Member {
                manifest_path: PathBuf::from("/workspace/Cargo.toml"),
                manifest: "Cargo.toml".to_owned(),
                declared,
                skeletons: SkeletonsTable::Absent,
            }),
        }
    }

    /// The workspace read back once the wearing and its dependency are gone.
    fn nothing_left() -> impl FnOnce() -> Result<Prospect, ReadWorkspaceError> {
        || Ok(read_back(Vec::new(), Vec::new()))
    }

    fn deleting(paths: &[&str]) -> Settlement {
        Settlement::Deleting(
            paths
                .iter()
                .map(|path| Disposition {
                    path: ClaimPath::from_rendering_path(path).expect("a safe path"),
                    fate: Fate::Deleted,
                })
                .collect(),
        )
    }

    /// Asserts that the manifest and the lockfile are as they were before the
    /// run.
    fn assert_put_back(project: &Project) {
        assert_eq!(project.read("Cargo.toml"), WORN);
        assert_eq!(project.read("Cargo.lock"), LOCKFILE);
    }

    #[test]
    fn a_wearing_taken_out_keeps_the_edited_manifest_and_the_relocked_lockfile() {
        // The whole run succeeds with stand-ins for cargo: the manifest is
        // the original less the wearing and the dependency, the lockfile is
        // what cargo rewrote, and the run says it changed the lockfile, which
        // git tracks.
        let project = Project::worn();

        let done = project
            .run(relocking(project.directory.path()), nothing_left())
            .expect("a wearing that is gone is a success");

        assert_eq!(project.read("Cargo.toml"), UNWORN);
        assert_eq!(project.read("Cargo.lock"), RELOCKED);
        assert_eq!(done.lockfile, Lockfile::Changed(Tracking::Tracked));
    }

    #[test]
    fn a_lockfile_cargo_left_as_it_was_is_reported_unchanged() {
        // Cargo rewrites nothing when the lockfile already satisfies what is
        // left, so the last line has no lockfile to name. Observed from the
        // bytes, not assumed.
        let project = Project::worn();

        let done = project
            .run(relocking_nothing(), nothing_left())
            .expect("a wearing that is gone is a success");

        assert_eq!(project.read("Cargo.lock"), LOCKFILE);
        assert_eq!(done.lockfile, Lockfile::Unchanged);
    }

    #[test]
    fn a_lockfile_git_does_not_track_is_reported_changed_without_being_tracked() {
        // The value carried to the last line is what the preparation found
        // about the lockfile, so an ignored lockfile is changed and not named.
        let mut project = Project::worn();
        project.prepared.lockfile = Tracking::NotTracked;

        let done = project
            .run(relocking(project.directory.path()), nothing_left())
            .expect("a wearing that is gone is a success");

        assert_eq!(done.lockfile, Lockfile::Changed(Tracking::NotTracked));
    }

    #[test]
    fn cargo_refusing_what_is_left_puts_both_files_back_with_cargos_words() {
        // The relock writes the lockfile and then fails, as cargo does when a
        // feature still names the dependency. The failure carries cargo's own
        // words and both files come back byte for byte.
        let project = Project::worn();
        let lockfile = project.path("Cargo.lock");
        let relock = move || -> Result<(), ReadWorkspaceError> {
            std::fs::write(lockfile, RELOCKED).expect("the stand-in rewrites the lockfile");
            Err(ReadWorkspaceError::CargoMetadataFailed {
                stderr: "error: feature `x` includes `tidy`".to_owned(),
            })
        };

        let failure = project
            .run(relock, || {
                panic!("nothing is read back after cargo refuses")
            })
            .expect_err("a refusal by cargo must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "remove took `tidy` out of Cargo.toml, and Cargo refused what was left: error: \
                 feature `x` includes `tidy`{PUT_BACK}"
            )
        );
        assert_put_back(&project);
    }

    #[test]
    fn a_relock_that_cannot_run_cargo_is_worded_as_check_words_it_and_both_files_come_back() {
        let project = Project::worn();

        let failure = project
            .run(
                || {
                    Err(ReadWorkspaceError::CargoUnavailable {
                        detail: "no such file".to_owned(),
                    })
                },
                || panic!("nothing is read back after the relock fails"),
            )
            .expect_err("an unavailable cargo must fail the run");

        let message = failure.to_string();
        assert!(message.contains("no such file"), "{message}");
        assert!(message.ends_with(PUT_BACK), "{message}");
        assert_put_back(&project);
    }

    #[test]
    fn a_workspace_that_cannot_be_read_back_is_worded_as_remove_words_its_lockfile() {
        let project = Project::worn();

        let failure = project
            .run(relocking(project.directory.path()), || {
                Err(ReadWorkspaceError::Lockfile)
            })
            .expect_err("an unreadable workspace must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "Cargo.lock is missing or out of date, and remove changes it only to take the \
                 skeleton out; run `cargo update --workspace`, then run the `remove` task \
                 again{PUT_BACK}"
            )
        );
        assert_put_back(&project);
    }

    #[test]
    fn a_read_back_that_still_finds_the_wearing_is_a_defect_and_both_files_come_back() {
        let project = Project::worn();

        let failure = project
            .run(relocking(project.directory.path()), || {
                Ok(read_back(vec![worn_tidy()], Vec::new()))
            })
            .expect_err("a wearing still there must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "remove took `tidy` out of Cargo.toml, and reading it back still finds \
                 [package.metadata.skeletons.tidy]; this is a defect in skeletons{PUT_BACK}"
            )
        );
        assert_put_back(&project);
    }

    #[test]
    fn a_read_back_that_still_finds_a_dependency_says_to_take_it_out_by_hand() {
        let project = Project::worn();

        let failure = project
            .run(relocking(project.directory.path()), || {
                Ok(read_back(Vec::new(), vec![dependency_on_tidy()]))
            })
            .expect_err("a dependency still declared must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "remove took `tidy` out of Cargo.toml, and Cargo still reports a dependency \
                 under `tidy`: it is declared somewhere remove does not edit; take it out by \
                 hand, then run the `remove` task again{PUT_BACK}"
            )
        );
        assert_put_back(&project);
    }

    #[test]
    fn a_read_back_that_no_longer_finds_the_package_is_a_defect_and_both_files_come_back() {
        // The member was found before the edit, so a read back without it is
        // neither a pass nor a panic: it is a returned failure, so that the
        // edit is undone.
        let project = Project::worn();

        let failure = project
            .run(relocking(project.directory.path()), || {
                Ok(Prospect {
                    workspace: Workspace {
                        root: PathBuf::from("/workspace"),
                        wearing: Vec::new(),
                    },
                    member: None,
                })
            })
            .expect_err("a vanished package must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "remove took `tidy` out of Cargo.toml, and reading the workspace back no longer \
                 finds the package this command line is built from; this is a defect in \
                 skeletons{PUT_BACK}"
            )
        );
        assert_put_back(&project);
    }

    #[test]
    fn a_manifest_with_no_wearing_table_is_refused_before_anything_is_written() {
        // The manifest has no table to take out, which is a defect in
        // `skeletons`. Nothing was written, so there is nothing to put back
        // and the line makes no claim that anything was.
        let project = Project::new(UNWORN, Settlement::LeftAll);

        let failure = project
            .run(
                || panic!("nothing is relocked when the manifest cannot be edited"),
                || panic!("nothing is read back when the manifest cannot be edited"),
            )
            .expect_err("a manifest with no table must fail the run");

        assert_eq!(
            failure.to_string(),
            "remove could not take `tidy` out of Cargo.toml without changing anything else in \
             it (the manifest has no wearing table to take out); this is a defect in skeletons"
        );
        assert_eq!(project.read("Cargo.toml"), UNWORN);
        assert_eq!(project.read("Cargo.lock"), LOCKFILE);
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_refused_before_anything_is_written() {
        let project = Project::new("[package\n", Settlement::LeftAll);

        let failure = project
            .run(
                || panic!("nothing is relocked when the manifest cannot be read"),
                || panic!("nothing is read back when the manifest cannot be read"),
            )
            .expect_err("a manifest that is not TOML must fail the run");

        let message = failure.to_string();
        assert!(
            message.starts_with("remove could not read Cargo.toml to take `tidy` out of it: "),
            "{message}"
        );
        assert!(!message.contains("ritual put"), "{message}");
        assert_eq!(project.read("Cargo.toml"), "[package\n");
    }

    fn names_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("read the directory")
            .map(|entry| {
                entry
                    .expect("read an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_files_to_delete_are_deleted_and_the_directory_they_emptied_goes() {
        // Two files in a directory of their own are deleted, and the directory
        // they left empty is gone. Nothing is left in the project but the
        // manifest and the lockfile.
        let project = Project::new(WORN, deleting(&["ci/one.yml", "ci/two.yml"]));
        project.write_file("ci/one.yml", "one\n");
        project.write_file("ci/two.yml", "two\n");

        project
            .run(relocking(project.directory.path()), nothing_left())
            .expect("a wearing that is gone is a success");

        assert_eq!(
            names_in(project.directory.path()),
            ["Cargo.lock", "Cargo.toml"]
        );
        assert_eq!(project.read("Cargo.toml"), UNWORN);
    }

    #[test]
    #[should_panic(expected = "the lockfile remove writes is never among the files it deletes")]
    fn the_lockfile_among_the_files_to_delete_is_a_defect_in_the_plan() {
        // The decision that sets the lockfile aside is made in `files`, so a
        // settlement that hands it to the deleter is a defect there, found in
        // `prepare` before anything is written.
        let project = Project::new(WORN, deleting(&["Cargo.lock"]));

        assert_nothing_written_is_deleted(
            &project.prepared.files,
            &project.prepared.workspace_root,
            &project.prepared.command_line_crate,
        );
    }

    #[test]
    #[should_panic(expected = "the manifest remove writes is never among the files it deletes")]
    fn the_manifest_among_the_files_to_delete_is_a_defect_in_the_plan() {
        let project = Project::new(WORN, deleting(&["Cargo.toml"]));

        assert_nothing_written_is_deleted(
            &project.prepared.files,
            &project.prepared.workspace_root,
            &project.prepared.command_line_crate,
        );
    }

    #[test]
    fn files_that_are_neither_the_manifest_nor_the_lockfile_may_be_deleted() {
        let project = Project::new(
            WORN,
            deleting(&["ci.yml", "Cargo.lock.bak", "sub/Cargo.toml"]),
        );

        assert_nothing_written_is_deleted(
            &project.prepared.files,
            &project.prepared.workspace_root,
            &project.prepared.command_line_crate,
        );
    }

    #[test]
    fn a_failed_delete_puts_the_manifest_and_lockfile_back_and_names_both_lists() {
        // The first file is really deleted and the second is refused, after
        // the manifest was written, the lockfile relocked and the workspace
        // read back. The manifest and the lockfile come back byte for byte,
        // the first file stays deleted, the second and third are there, and
        // the one line says what rollback did, then which files are gone and
        // which are not.
        let project = Project::new(
            WORN,
            deleting(&["ci/one.yml", "ci/two.yml", "ci/three.yml"]),
        );
        project.write_file("ci/one.yml", "one\n");
        project.write_file("ci/two.yml", "two\n");
        project.write_file("ci/three.yml", "three\n");
        let mut asked = 0_usize;

        let failure = project
            .run_deleting(
                relocking(project.directory.path()),
                nothing_left(),
                |path| {
                    asked += 1;
                    if asked == 2 {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "denied for the test",
                        ))
                    } else {
                        deletion::delete_from_disk(path)
                    }
                },
            )
            .expect_err("a delete that is refused must fail the run");

        assert_eq!(
            failure.to_string(),
            format!(
                "remove could not delete ci/two.yml: denied for the test{PUT_BACK}; the file \
                 remove had already deleted is not put back, and git has it: ci/one.yml; not \
                 deleted: ci/two.yml and ci/three.yml"
            )
        );
        assert_put_back(&project);
        assert!(!project.path("ci/one.yml").exists());
        assert_eq!(project.read("ci/two.yml"), "two\n");
        assert_eq!(project.read("ci/three.yml"), "three\n");
    }

    #[test]
    fn a_directory_that_cannot_be_removed_names_every_file_deleted_and_none_not_deleted() {
        // `link` is a link to a directory, so removing it as a directory is
        // refused for a reason of the operating system's. Every file was
        // deleted by then, so the line says so and has no clause about files
        // not deleted, and the manifest and the lockfile are back.
        let project = Project::new(WORN, deleting(&["link/f.yml"]));
        project.write_file("real/f.yml", "f\n");
        std::os::unix::fs::symlink("real", project.path("link")).expect("create the link");

        let failure = project
            .run(relocking(project.directory.path()), nothing_left())
            .expect_err("a directory that cannot be removed must fail the run");

        let message = failure.to_string();
        assert!(
            message
                .starts_with("remove could not remove link, which deleting its files left empty: "),
            "{message}"
        );
        assert!(
            message.ends_with(&format!(
                "{PUT_BACK}; the file remove had already deleted is not put back, and git has \
                 it: link/f.yml"
            )),
            "{message}"
        );
        assert_put_back(&project);
        assert!(!project.path("real/f.yml").exists());
    }

    /// The command `remove`'s arguments are read through, as the task builds
    /// it.
    fn command() -> clap::Command {
        <RemoveArguments as clap::Args>::augment_args(clap::Command::new("remove"))
    }

    fn files_flag_of(arguments: &[&str]) -> FilesFlag {
        let matches = command()
            .try_get_matches_from(arguments)
            .unwrap_or_else(|error| panic!("{arguments:?} must parse: {error}"));
        <RemoveArguments as clap::FromArgMatches>::from_arg_matches(&matches)
            .expect("matches clap accepted are read back as the arguments")
            .files_flag()
    }

    #[test]
    fn clap_accepts_neither_flag_and_each_alone_and_reads_each_as_itself() {
        assert_eq!(files_flag_of(&["remove", "ci"]), FilesFlag::Bare);
        assert_eq!(
            files_flag_of(&["remove", "ci", "--delete-files"]),
            FilesFlag::DeleteFiles
        );
        assert_eq!(
            files_flag_of(&["remove", "ci", "--keep-files"]),
            FilesFlag::KeepFiles
        );
    }

    #[test]
    fn clap_refuses_both_flags_together_in_either_order() {
        for arguments in [
            ["remove", "ci", "--delete-files", "--keep-files"],
            ["remove", "ci", "--keep-files", "--delete-files"],
        ] {
            let error = command()
                .try_get_matches_from(arguments)
                .expect_err("both flags together must be refused");

            let message = error.to_string();
            assert!(
                message.contains("cannot be used with"),
                "{arguments:?}: {message}"
            );
        }
    }

    #[test]
    fn clap_refuses_a_command_line_with_no_name() {
        assert!(command().try_get_matches_from(["remove"]).is_err());
    }
}
