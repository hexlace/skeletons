//! `wear`: start wearing a skeleton in one command.
//!
//! It adds the skeleton as a dev-dependency of the running command line's own
//! crate, through `cargo add`, and adds an empty
//! `[package.metadata.skeletons.<key>]` table for it, which is everything
//! `sync` needs to start writing the skeleton's files. The two pieces are
//! written as one change: a run that fails after `cargo add` has changed the
//! manifest and `Cargo.lock` puts both back exactly as they were, through
//! `rituals_compose::rollback`, and says so.
//!
//! What it writes is the crate the running command line was built from, which
//! is the one place a task is told about itself, so there is no manifest to
//! point it at.

mod cargo_add;
mod confirm;
mod message;
mod prospect;
mod refusal;
mod request;
mod source;
mod table;
#[cfg(test)]
mod test_workspace;

use std::path::{Path, PathBuf};

use rituals::{CommandLine, Failure, Outcome, Task, clap, report};
use rituals_compose::{metadata, rollback};
use rollback::Changes;

use crate::check::{AbortingCommand, abort_message};
use crate::work_tree;
use crate::work_tree::writing_command::WritingCommand;
use crate::workspace::{self, Network, ReadWorkspaceError, Workspace};
use confirm::Added;
use refusal::WearRefusal;
use request::Request;
use source::{Source, SourceFlags};

/// What `wear --help` says the task does.
const ABOUT: &str = "start wearing a skeleton: add it as a dev-dependency, with an empty wearing \
                     table";

/// The command asking git about the work tree, for the words of its refusals.
const WRITING: WritingCommand = WritingCommand::Wear;

/// What a failed run says to do, when it could not put everything back.
const RETRY: &str = "running the `wear` task again";

// `wear`'s own command-line arguments.
//
// No `///` doc comment on this struct, for the reason `check::CheckArguments`
// carries none: `rituals` sets a task's `about` after clap's derived
// `augment_args` and leaves `long_about` alone, so a struct doc comment of
// more than one paragraph would replace `--help`'s text, and `-h` would end in
// "(see more with '--help')" rather than a bare "Print help". Each field keeps
// its own `///` line, which is the argument's help text.
#[derive(Debug, clap::Args)]
struct WearArguments {
    /// the skeleton's crate, and optionally a version requirement
    #[arg(value_name = "CRATE[@VERSION]")]
    skeleton: String,
    /// the dependency key, which also names the wearing table; the crate's name if left out
    #[arg(value_name = "KEY")]
    key: Option<String>,
    #[command(flatten)]
    source: SourceFlags,
}

/// Builds the `wear` task.
pub(crate) fn task() -> Task {
    Task::receiving_command_line(
        ABOUT,
        |command_line: &CommandLine, arguments: WearArguments| run(command_line, arguments),
    )
}

fn run(command_line: &CommandLine, arguments: WearArguments) -> Outcome {
    let WearArguments {
        skeleton,
        key,
        source,
    } = arguments;
    let request = Request::new(&skeleton, key.as_deref(), Source::from_flags(source))
        .map_err(WearRefusal::into_failure)?;
    let prepared = prepare(command_line, request)?;
    let added = rollback::attempt(RETRY, |changes| write(changes, &prepared))?;
    for line in message::added_lines(&added) {
        report(line);
    }
    Ok(())
}

/// What a run has settled before it writes anything: the request, where it
/// runs, and the crate it writes into.
struct Prepared {
    request: Request,
    directory: PathBuf,
    command_line_crate: CommandLineCrate,
}

/// The command line's own crate, which is the only package `wear` writes
/// into, and the two files it changes.
struct CommandLineCrate {
    package: String,
    manifest_path: PathBuf,
    /// `manifest_path` as a message shows it: relative to the workspace root.
    manifest_shown: String,
    lockfile_path: PathBuf,
}

/// Everything `wear` can refuse before it writes anything.
///
/// The workspace is read `--locked` first, so that a lockfile that is stale or
/// missing is refused before `cargo add` can rewrite more of it than the
/// skeleton, and so that ritual's own unlocked `cargo metadata`, which
/// follows, is never the one to rewrite it. What that read holds then answers
/// every refusal that is about the request and the manifest as they stand
/// ([`prospect::check`]), so none of them waits for `cargo add` to have run.
/// The work tree is asked last, so a request that is wrong is refused as wrong
/// whether or not the tree is clean.
fn prepare(command_line: &CommandLine, request: Request) -> Result<Prepared, Failure> {
    let directory = std::env::current_dir().map_err(|error| {
        Failure::new("could not read the current working directory").caused_by(error)
    })?;
    let package = command_line.identity().package_name();
    let prospect =
        workspace::read_prospect(&directory, package).map_err(|error| aborted(&error))?;
    prospect::check(&request, &prospect, package).map_err(WearRefusal::into_failure)?;
    // `wear` changes the manifest and `Cargo.lock` in place, so git is the only
    // undo for a change `rollback` could not take back; whole-tree dirt counts,
    // as it does for `sync`, because the person's own uncommitted edits to those
    // two files would otherwise be mixed into what `wear` wrote. The witness is
    // not needed: `wear` proves nothing per path.
    let (_work_tree, _clean) = work_tree::open_clean(&prospect.workspace.root, WRITING)?;
    let command_line_crate = locate_command_line_crate(command_line, &directory)?;
    Ok(Prepared {
        request,
        directory,
        command_line_crate,
    })
}

/// The failure for a workspace that could not be read, in the words `check`
/// uses for the same abort.
fn aborted(error: &ReadWorkspaceError) -> Failure {
    let (_kind, message) = abort_message(error, AbortingCommand::Wear);
    Failure::new(message)
}

/// Finds the crate the running command line was built from, through
/// `rituals_compose`, as `add` does for the same package.
fn locate_command_line_crate(
    command_line: &CommandLine,
    directory: &Path,
) -> Result<CommandLineCrate, Failure> {
    let package = command_line.identity().package_name();
    let document = metadata::fetch(directory)?;
    let project = document.locate_project(package)?;
    Ok(CommandLineCrate {
        package: package.to_owned(),
        manifest_path: project.manifest_path().to_path_buf(),
        manifest_shown: workspace::relative_to_root(
            project.workspace_root(),
            project.manifest_path(),
        ),
        lockfile_path: project.workspace_root().join("Cargo.lock"),
    })
}

/// The whole of what `wear` changes, inside [`rollback::attempt`].
fn write(changes: &mut Changes, prepared: &Prepared) -> Result<Added, Failure> {
    write_with(
        changes,
        prepared,
        || {
            cargo_add::run(
                &prepared.directory,
                &prepared.command_line_crate.package,
                &prepared.request,
            )
        },
        || workspace::read(&prepared.directory, Network::Allowed),
    )
}

/// Adds the dependency, adds the wearing table, and reads the workspace back
/// to confirm both, in that order, with the first and last step taken as
/// arguments so a test can stand in for `cargo`.
///
/// Every failure the wearer can cause or act on is an `Err`, so that
/// `rollback` puts the project back: it does that when the run returns a
/// failure and does not when it panics. A violated invariant is a defect, and
/// a defect panics; the assertions in [`workspace::read`] and in the process
/// runner can be reached from here, and the project is not put back after one.
/// Nothing here writes a file except through `changes`.
fn write_with(
    changes: &mut Changes,
    prepared: &Prepared,
    add: impl FnOnce() -> Outcome,
    read_back: impl FnOnce() -> Result<Workspace, ReadWorkspaceError>,
) -> Result<Added, Failure> {
    let (manifest, lockfile) = (
        &prepared.command_line_crate.manifest_path,
        &prepared.command_line_crate.lockfile_path,
    );
    changes.run_changing(&[manifest.as_path(), lockfile.as_path()], add)?;
    let key = prepared.request.key();
    table::write(
        changes,
        manifest,
        &prepared.command_line_crate.manifest_shown,
        key,
    )?;
    let workspace = read_back().map_err(|error| aborted(&error))?;
    confirm::confirm(&workspace, &prepared.command_line_crate.manifest_shown, key)
        .map_err(WearRefusal::into_failure)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rituals::{Failure, Outcome, clap};
    use rituals_compose::rollback;

    use super::source::Source;
    use super::test_workspace::{not_a_skeleton, workspace_of, worn};
    use super::{CommandLineCrate, Prepared, Request, WearArguments, write_with};
    use crate::workspace::{ReadWorkspaceError, Workspace};

    const MANIFEST: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n";
    const LOCKFILE: &str = "# the lockfile before\nversion = 4\n";
    const ADDED_MANIFEST: &str = "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
                                  [dev-dependencies]\ntidy = \"1\"\n";
    const ADDED_LOCKFILE: &str = "# the lockfile after\nversion = 4\n";

    /// A directory holding a manifest and a lockfile, and the `Prepared` that
    /// points at them.
    struct Project {
        directory: tempfile::TempDir,
        prepared: Prepared,
    }

    impl Project {
        fn new(key: Option<&str>) -> Self {
            let directory = tempfile::tempdir().expect("a temporary directory");
            let manifest_path = directory.path().join("Cargo.toml");
            let lockfile_path = directory.path().join("Cargo.lock");
            std::fs::write(&manifest_path, MANIFEST).expect("write the manifest");
            std::fs::write(&lockfile_path, LOCKFILE).expect("write the lockfile");
            let request = Request::new("tidy", key, Source::Registry).expect("a valid request");
            let prepared = Prepared {
                request,
                directory: directory.path().to_path_buf(),
                command_line_crate: CommandLineCrate {
                    package: "cli".to_owned(),
                    manifest_path,
                    manifest_shown: "Cargo.toml".to_owned(),
                    lockfile_path,
                },
            };
            Self {
                directory,
                prepared,
            }
        }

        fn manifest(&self) -> String {
            self.read("Cargo.toml")
        }

        fn lockfile(&self) -> String {
            self.read("Cargo.lock")
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.directory.path().join(name)).expect("read the file")
        }
    }

    /// What `cargo add` does when it succeeds: rewrites both files.
    fn adding(directory: &Path) -> impl FnOnce() -> Outcome {
        let directory = directory.to_path_buf();
        move || {
            std::fs::write(directory.join("Cargo.toml"), ADDED_MANIFEST)
                .and_then(|()| std::fs::write(directory.join("Cargo.lock"), ADDED_LOCKFILE))
                .map_err(|error| Failure::new("the stand-in for cargo add failed").caused_by(error))
        }
    }

    /// What `cargo add` does when it fails after changing both files.
    fn adding_then_failing(directory: &Path) -> impl FnOnce() -> Outcome {
        let adding = adding(directory);
        move || {
            adding()?;
            Err(Failure::new("cargo add failed: no such crate"))
        }
    }

    fn reading_back(
        workspace: Workspace,
    ) -> impl FnOnce() -> Result<Workspace, ReadWorkspaceError> {
        move || Ok(workspace)
    }

    fn run(
        project: &Project,
        add: impl FnOnce() -> Outcome,
        read_back: impl FnOnce() -> Result<Workspace, ReadWorkspaceError>,
    ) -> Result<super::Added, Failure> {
        rollback::attempt("running the `wear` task again", |changes| {
            write_with(changes, &project.prepared, add, read_back)
        })
    }

    #[test]
    fn a_crate_that_is_not_a_skeleton_puts_the_manifest_and_lockfile_back_byte_for_byte() {
        // The refusal that has something to undo: `cargo add` has changed both
        // files and the wearing table is written, and only reading the
        // workspace back finds the crate is no skeleton. Both files must come
        // back exactly, and the failure must say so.
        let project = Project::new(None);
        let workspace = workspace_of(vec![not_a_skeleton("Cargo.toml", "tidy", "tidy")]);

        let failure = run(
            &project,
            adding(project.directory.path()),
            reading_back(workspace),
        )
        .expect_err("a crate that is no skeleton must be refused");

        assert_eq!(
            failure.to_string(),
            "tidy 0.1.0 is not a skeleton: its manifest has no [package.metadata.skeletons] \
             table, so it cannot be worn; wear a crate that is one; ritual put the project \
             back as it found it"
        );
        assert_eq!(project.manifest(), MANIFEST);
        assert_eq!(project.lockfile(), LOCKFILE);
    }

    #[test]
    fn a_failing_cargo_add_puts_the_manifest_and_lockfile_back_byte_for_byte() {
        // `cargo add` may fail after writing one of its two files. Whatever it
        // left is undone, and the table is never written.
        let project = Project::new(None);

        let failure = run(
            &project,
            adding_then_failing(project.directory.path()),
            || panic!("nothing is read back after cargo add fails"),
        )
        .expect_err("a failing cargo add must fail the run");

        assert!(
            failure
                .to_string()
                .starts_with("cargo add failed: no such crate"),
            "{failure}"
        );
        assert_eq!(project.manifest(), MANIFEST);
        assert_eq!(project.lockfile(), LOCKFILE);
    }

    #[test]
    fn a_worn_result_keeps_what_cargo_wrote_and_the_wearing_table() {
        let project = Project::new(None);
        let workspace = workspace_of(vec![worn("Cargo.toml", "tidy", "tidy")]);

        let added = run(
            &project,
            adding(project.directory.path()),
            reading_back(workspace),
        )
        .expect("a worn skeleton is a success");

        assert_eq!(added.package, "tidy");
        assert_eq!(added.key, "tidy");
        assert_eq!(added.manifest, "Cargo.toml");
        assert_eq!(project.lockfile(), ADDED_LOCKFILE);
        assert_eq!(
            project.manifest(),
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
             [package.metadata.skeletons.tidy]\n\n\
             [dev-dependencies]\ntidy = \"1\"\n",
            "the dependency cargo added and the empty table must both be there"
        );
    }

    #[test]
    fn a_table_that_is_already_there_is_refused_and_both_files_come_back() {
        // The manifest `cargo add` leaves already has a table for the key. That
        // is refused as the table refusal says, after `cargo add` ran, so the
        // run has to undo what cargo wrote.
        let project = Project::new(None);
        let with_table = format!("{ADDED_MANIFEST}\n[package.metadata.skeletons.tidy]\n");
        let directory = project.directory.path().to_path_buf();
        let adding_a_table_too = move || -> Outcome {
            std::fs::write(directory.join("Cargo.toml"), with_table)
                .and_then(|()| std::fs::write(directory.join("Cargo.lock"), ADDED_LOCKFILE))
                .map_err(|error| Failure::new("the stand-in failed").caused_by(error))
        };

        let failure = run(&project, adding_a_table_too, || {
            panic!("nothing is read back after the table is refused")
        })
        .expect_err("an existing table must be refused");

        assert!(
            failure
                .to_string()
                .starts_with("Cargo.toml already has a [package.metadata.skeletons.tidy] table"),
            "{failure}"
        );
        assert_eq!(project.manifest(), MANIFEST);
        assert_eq!(project.lockfile(), LOCKFILE);
    }

    #[test]
    fn a_workspace_that_cannot_be_read_back_is_worded_as_wear_words_its_lockfile() {
        let project = Project::new(None);

        let failure = run(&project, adding(project.directory.path()), || {
            Err(ReadWorkspaceError::Lockfile)
        })
        .expect_err("an unreadable workspace must fail the run");

        assert!(
            failure.to_string().starts_with(
                "Cargo.lock is missing or out of date, and wear changes it only to add the \
                 skeleton; run `cargo update --workspace`, then run the `wear` task again"
            ),
            "{failure}"
        );
        assert_eq!(project.manifest(), MANIFEST);
        assert_eq!(project.lockfile(), LOCKFILE);
    }

    #[test]
    fn a_skeleton_worn_twice_is_refused_and_both_files_come_back() {
        let project = Project::new(Some("neat"));
        let workspace = workspace_of(vec![
            worn("Cargo.toml", "tidy", "tidy"),
            worn("Cargo.toml", "neat", "tidy"),
        ]);

        let failure = run(
            &project,
            adding(project.directory.path()),
            reading_back(workspace),
        )
        .expect_err("a skeleton already worn must be refused");

        assert!(
            failure
                .to_string()
                .starts_with("tidy is already worn, as `tidy` in Cargo.toml;"),
            "{failure}"
        );
        assert_eq!(project.manifest(), MANIFEST);
        assert_eq!(project.lockfile(), LOCKFILE);
    }

    /// The command `wear`'s arguments are read through, as the task builds it.
    fn command() -> clap::Command {
        <WearArguments as clap::Args>::augment_args(clap::Command::new("wear"))
    }

    #[test]
    fn clap_accepts_every_source_combination_the_flags_allow() {
        // Each argument list is one a wearer can mean: the registry, a path, a
        // git url alone, and a git url with exactly one reference.
        for arguments in [
            vec!["wear", "tidy"],
            vec!["wear", "tidy", "neat"],
            vec!["wear", "tidy@1", "--path", "../tidy"],
            vec!["wear", "tidy", "--git", "u"],
            vec!["wear", "tidy", "--git", "u", "--branch", "b"],
            vec!["wear", "tidy", "--git", "u", "--tag", "t"],
            vec!["wear", "tidy", "--git", "u", "--rev", "r"],
        ] {
            let parsed = command().try_get_matches_from(arguments.clone());
            assert!(parsed.is_ok(), "{arguments:?} must parse: {parsed:?}");
        }
    }

    /// The five flags that name a source, in the order the test combines them.
    const SOURCE_FLAGS: [&str; 5] = ["--git", "--branch", "--tag", "--rev", "--path"];

    /// The only combinations of [`SOURCE_FLAGS`] a wearer can mean: none, a
    /// path, a git url, and a git url with exactly one reference.
    const MEANINGFUL_SOURCES: [&[&str]; 6] = [
        &[],
        &["--path"],
        &["--git"],
        &["--git", "--branch"],
        &["--git", "--tag"],
        &["--git", "--rev"],
    ];

    #[test]
    fn every_subset_of_the_source_flags_is_refused_by_clap_or_read_as_a_source() {
        // `Source::from_flags` panics on every combination it has no reading
        // for, because `clap` is meant to refuse each first, `--path` with
        // `--tag` or `--rev` among them. All 32 subsets of the five flags are
        // given to the command as `wear` builds it. A subset is accepted
        // exactly when it is one a wearer can mean, and an accepted one is
        // read as a source, which would panic on a combination `clap` let
        // through.
        let mut accepted = 0;
        for subset in 0..(1_u32 << SOURCE_FLAGS.len()) {
            let chosen: Vec<&str> = SOURCE_FLAGS
                .iter()
                .enumerate()
                .filter(|(position, _flag)| subset & (1 << position) != 0)
                .map(|(_position, flag)| *flag)
                .collect();
            let mut arguments = vec!["wear", "tidy"];
            for flag in &chosen {
                arguments.extend([*flag, "value"]);
            }

            let parsed = command().try_get_matches_from(arguments.clone());

            if MEANINGFUL_SOURCES.contains(&chosen.as_slice()) {
                let matches = parsed.unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
                let reading = <WearArguments as clap::FromArgMatches>::from_arg_matches(&matches)
                    .expect("matches clap accepted are read back as the arguments");
                let _source = Source::from_flags(reading.source);
                accepted += 1;
            } else {
                assert!(parsed.is_err(), "{arguments:?} must be refused");
            }
        }
        assert_eq!(accepted, MEANINGFUL_SOURCES.len());
    }

    #[test]
    fn clap_refuses_a_command_line_with_no_skeleton() {
        assert!(command().try_get_matches_from(["wear"]).is_err());
    }
}
