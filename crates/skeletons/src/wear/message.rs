//! What `wear` tells the wearer when it has added a skeleton.

use super::confirm::Added;
use crate::skeleton::Escaped;

/// The two lines `wear` prints when it succeeds: what it added and where,
/// then the next step.
///
/// The next step names the `sync` task and nothing before it: a task is
/// never told the key it is mounted under, so it cannot say how its command
/// line reaches `sync`, and the wearer recognises the subcommand on their
/// own.
pub(crate) fn added_lines(added: &Added) -> [String; 2] {
    let (package, manifest, key) = (
        Escaped(&added.package),
        Escaped(&added.manifest),
        Escaped(&added.key),
    );
    [
        format!(
            "added {package} {} to {manifest} as the dev-dependency `{key}`, with an empty \
             [package.metadata.skeletons.{key}] table",
            added.version
        ),
        "now run the `sync` task to write its files".to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::added_lines;
    use crate::survey::poison::{assert_escaped_once, poison};
    use crate::wear::confirm::Added;

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
            added_lines(&added("tidy", "crates/cli/Cargo.toml", "neat")),
            [
                "added tidy 1.2.3 to crates/cli/Cargo.toml as the dev-dependency `neat`, with \
                 an empty [package.metadata.skeletons.neat] table"
                    .to_owned(),
                "now run the `sync` task to write its files".to_owned(),
            ]
        );
    }

    #[test]
    fn the_first_line_prints_outside_text_on_one_line_escaped_once() {
        // The package name, the manifest's path and the key are all text from
        // outside: a path can hold a newline.
        let added = added(&poison(), &poison(), &poison());

        let [first, second] = added_lines(&added);

        assert_escaped_once(&first, "the added line");
        assert!(!second.contains('\n'), "the next-step line is one line");
    }

    #[test]
    fn the_next_step_names_no_command_line_the_task_was_not_told() {
        let [_, second] = added_lines(&added("tidy", "Cargo.toml", "tidy"));

        for unknown in ["cargo ritual", "skeletons sync", "run `sync`"] {
            assert!(!second.contains(unknown), "{second:?} names {unknown:?}");
        }
    }
}
