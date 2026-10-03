//! What `wear` tells the wearer when it has added a skeleton.

use super::confirm::Added;
use super::hand_back::Tracking;
use crate::skeleton::Escaped;

/// The two lines `wear` prints when it succeeds: what it added and where,
/// then the next step.
///
/// The next step names the `sync` task and nothing before it: a task is
/// never told the key it is mounted under, so it cannot say how its command
/// line reaches `sync`, and the wearer recognises the subcommand on their
/// own. It says to commit first because `sync` writes only into a clean work
/// tree, and `wear` has just changed the manifest and `Cargo.lock`; the
/// lockfile is always at the workspace root, which is where the wearer sees
/// it named. It is named only when git tracks it: `git add` refuses a
/// lockfile git ignores, so telling the wearer to commit it would send them
/// at a command that fails.
pub(crate) fn added_lines(added: &Added, lockfile: Tracking) -> [String; 2] {
    let (package, manifest, key) = (
        Escaped(&added.package),
        Escaped(&added.manifest),
        Escaped(&added.key),
    );
    let committed = match lockfile {
        Tracking::Tracked => format!("{manifest} and Cargo.lock"),
        Tracking::NotTracked => manifest.to_string(),
    };
    [
        format!(
            "added {package} {} to {manifest} as the dev-dependency `{key}`, with an empty \
             [package.metadata.skeletons.{key}] table",
            added.version
        ),
        format!("commit {committed}, then run the `sync` task to write its files"),
    ]
}

#[cfg(test)]
mod tests {
    use super::added_lines;
    use crate::survey::poison::{assert_escaped_once, poison};
    use crate::wear::confirm::Added;
    use crate::wear::hand_back::Tracking;

    fn added(package: &str, manifest: &str, key: &str) -> Added {
        Added {
            package: package.to_owned(),
            version: semver::Version::new(1, 2, 3),
            manifest: manifest.to_owned(),
            key: key.to_owned(),
        }
    }

    #[test]
    fn the_two_lines_say_what_was_added_and_what_to_run_next() {
        assert_eq!(
            added_lines(
                &added("tidy", "crates/cli/Cargo.toml", "neat"),
                Tracking::Tracked
            ),
            [
                "added tidy 1.2.3 to crates/cli/Cargo.toml as the dev-dependency `neat`, with \
                 an empty [package.metadata.skeletons.neat] table"
                    .to_owned(),
                "commit crates/cli/Cargo.toml and Cargo.lock, then run the `sync` task to write \
                 its files"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn the_next_step_leaves_the_lockfile_out_when_git_does_not_track_it() {
        // `git add` refuses a lockfile git ignores, so naming it would send
        // the wearer at a command that fails.
        let [_, second] = added_lines(
            &added("tidy", "crates/cli/Cargo.toml", "neat"),
            Tracking::NotTracked,
        );

        assert_eq!(
            second,
            "commit crates/cli/Cargo.toml, then run the `sync` task to write its files"
        );
    }

    #[test]
    fn both_lines_print_outside_text_on_one_line_escaped_once_tracked_lockfile_or_not() {
        // The package name, the manifest's path and the key are all text from
        // outside: a path can hold a newline, and the manifest is named on
        // both lines.
        let added = added(&poison(), &poison(), &poison());

        for lockfile in [Tracking::Tracked, Tracking::NotTracked] {
            let [first, second] = added_lines(&added, lockfile);

            assert_escaped_once(&first, "the added line");
            assert_escaped_once(&second, "the next-step line");
        }
    }

    #[test]
    fn the_next_step_names_no_command_line_the_task_was_not_told() {
        for lockfile in [Tracking::Tracked, Tracking::NotTracked] {
            let [_, second] = added_lines(&added("tidy", "Cargo.toml", "tidy"), lockfile);

            for unknown in ["cargo ritual", "skeletons sync", "run `sync`"] {
                assert!(!second.contains(unknown), "{second:?} names {unknown:?}");
            }
        }
    }
}
