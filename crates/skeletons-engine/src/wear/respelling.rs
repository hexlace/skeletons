//! Finding the dependency `cargo add` wrote under another spelling of the
//! key it was given.
//!
//! crates.io holds `serde-json` and `serde_json` as one name, and `cargo add`
//! writes the dependency under the spelling crates.io has, which is not always
//! the one typed. The key `wear` was asked for then names no dependency, and
//! the wearing table written under it would be refused when the workspace is
//! read back, with a remedy that is wrong once `rollback` has removed both.
//! The evidence is in the manifest `cargo add` left, so it is looked for
//! there, before the table is written, and never in what `cargo add` said.
//!
//! Nothing before `cargo add` can have put a dependency under another
//! spelling of the key: the key being taken that way is refused first
//! (`prospect`), so one found here is the one `cargo add` just wrote.

use super::request::Key;
use crate::workspace::underscored;

/// The tables whose entries are dependencies, in either spelling Cargo reads
/// for the dev and build ones, at the top of a manifest and under each
/// `[target.<platform>]`.
const DEPENDENCY_TABLES: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "dev_dependencies",
    "build_dependencies",
];

/// The key a dependency in `manifest` is declared under that `rustc` takes as
/// `key` but that is spelled differently, when `key` itself names no
/// dependency.
///
/// `None` when a dependency is declared under exactly `key`, when none is
/// under any spelling of it, or when `manifest` is not TOML, which the next
/// step reports as it reads the same text.
pub(crate) fn respelt_key(manifest: &str, key: &Key) -> Option<String> {
    let document: toml::Table = manifest.parse().ok()?;
    let keys = dependency_keys(&document);
    if keys.contains(&key.as_str()) {
        return None;
    }
    keys.into_iter()
        .find(|declared| underscored(declared) == key.underscored())
        .map(str::to_owned)
}

/// Every key a dependency is declared under in `document`, in any
/// dependency table.
fn dependency_keys(document: &toml::Table) -> Vec<&str> {
    let target_tables = document
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::values)
        .filter_map(toml::Value::as_table);
    std::iter::once(document)
        .chain(target_tables)
        .flat_map(|scope| {
            DEPENDENCY_TABLES
                .iter()
                .filter_map(|name| scope.get(*name))
                .filter_map(toml::Value::as_table)
                .flat_map(|table| table.keys().map(String::as_str))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::respelt_key;
    use crate::wear::request::Key;

    fn key(text: &str) -> Key {
        Key::new(text).expect("a test passes only keys it knows are valid")
    }

    #[test]
    fn a_dependency_at_the_other_spelling_is_found_as_the_manifest_spells_it() {
        // The shape `cargo add --dev serde-json` leaves: the key typed is
        // absent and the dependency is under the spelling crates.io has.
        let manifest = "[package]\nname = \"cli\"\n\n[dev-dependencies]\nserde_json = \"1\"\n";

        assert_eq!(
            respelt_key(manifest, &key("serde-json")),
            Some("serde_json".to_owned())
        );
    }

    #[test]
    fn the_other_spelling_is_found_in_either_direction_and_in_any_dependency_table() {
        for table in [
            "dependencies",
            "dev-dependencies",
            "build-dependencies",
            "dev_dependencies",
            "build_dependencies",
            "target.'cfg(unix)'.dependencies",
            "target.x86_64-apple-darwin.dev-dependencies",
        ] {
            for (held, asked) in [("a_x", "a-x"), ("a-x", "a_x")] {
                let manifest = format!("[{table}]\n{held} = \"1\"\n");

                assert_eq!(
                    respelt_key(&manifest, &key(asked)),
                    Some(held.to_owned()),
                    "{held} in {table} against {asked}"
                );
            }
        }
    }

    #[test]
    fn a_dependency_at_exactly_the_key_is_not_a_respelling() {
        // The key typed is the key `cargo add` used, so nothing was respelt,
        // even with another spelling of it declared beside it.
        let manifest = "[dev-dependencies]\na-x = \"1\"\na_x = \"1\"\n";

        assert_eq!(respelt_key(manifest, &key("a-x")), None);
    }

    #[test]
    fn no_dependency_at_any_spelling_is_not_a_respelling() {
        let manifest = "[dev-dependencies]\na_xy = \"1\"\nA_x = \"1\"\n\n\
                        [package.metadata.skeletons.a_x]\n";

        assert_eq!(respelt_key(manifest, &key("a-x")), None);
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_left_for_the_next_step_to_report() {
        assert_eq!(respelt_key("[dev-dependencies", &key("a-x")), None);
    }
}
