//! `[package.metadata.skeletons]` on one workspace member: which dependency keys
//! it wears, and the raw option-value table recorded for each.
//!
//! Reading this table never looks at a skeleton's own schema — it only
//! decides the TOML *shape* cargo reported (a table of tables, each entry
//! either a string or an array of strings), which is everything a wearer's
//! own manifest is answerable for. Whether a value is one the skeleton
//! actually declares is the render's question, asked later, over
//! [`WearingTable::choices`]'s own result.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::skeleton::{Choice, Choices};

/// The reserved keys name a skeleton's own declarations, never a dependency
/// this crate wears: `options` its option schema
/// (`[package.metadata.skeletons.options]`) and `verbatim` its list of files
/// shipped as bytes (`[package.metadata.skeletons] verbatim = [...]`).
pub(crate) const RESERVED_WEARING_KEYS: [&str; 2] = ["options", "verbatim"];

/// The dependency key a `[package.metadata.skeletons.<key>]` table names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DependencyKey(String);

impl DependencyKey {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DependencyKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One `[package.metadata.skeletons.<key>]` table's own recorded option values,
/// in the raw shape a wearer wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WearingTable(BTreeMap<String, Value>);

/// Why `[package.metadata.skeletons]` (or one of its keys) could not be read as
/// a wearing table at all — refuses the whole member's wearing, since there
/// is no dependency key to attach a narrower refusal to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WholeTableRefusal {
    /// `[package.metadata.skeletons]` itself is present but not a table.
    NotATable,
}

/// Why one key of an otherwise-well-formed `[package.metadata.skeletons]` table
/// could not become a worn dependency's own table — refuses only that one
/// key; every other key is still read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TableEntryRefusal {
    /// `[package.metadata.skeletons.<key>]` is present but not a table.
    NotATable { key: String },
    /// A real dependency is declared under a reserved key, which can never
    /// be worn. `dependency` is that key, `options` or `verbatim`.
    ReservedKey { dependency: String },
}

/// Reads every `[package.metadata.skeletons.<key>]` table out of `metadata`
/// (a member's own `[package.metadata]`, as `cargo metadata` reports it).
///
/// `declares_dependency` is asked, for each reserved key the table holds,
/// whether this same member declares a dependency under that very key — the
/// only condition under which a reserved key refuses rather than being
/// silently skipped as the member's own skeleton declaration. A dependency
/// under `options` says nothing about `verbatim`, and the other way round.
pub(crate) fn wearing_tables(
    metadata: &Value,
    declares_dependency: impl Fn(&str) -> bool,
) -> Result<
    (
        BTreeMap<DependencyKey, WearingTable>,
        Vec<TableEntryRefusal>,
    ),
    WholeTableRefusal,
> {
    let Some(skeletons) = metadata.get("skeletons") else {
        return Ok((BTreeMap::new(), Vec::new()));
    };
    let Some(skeletons_table) = skeletons.as_object() else {
        return Err(WholeTableRefusal::NotATable);
    };

    let mut tables = BTreeMap::new();
    let mut refusals = Vec::new();
    for (key, value) in skeletons_table {
        if RESERVED_WEARING_KEYS.contains(&key.as_str()) {
            if declares_dependency(key) {
                refusals.push(TableEntryRefusal::ReservedKey {
                    dependency: key.clone(),
                });
            }
            // With no dependency under this key, it is the member's own
            // skeleton declaration (`[package.metadata.skeletons.options.*]`
            // or `verbatim = [...]`), read by the render alone; skipped here
            // either way, whatever shape it has.
            continue;
        }
        match value.as_object() {
            Some(table) => {
                let entries = table
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                tables.insert(DependencyKey(key.clone()), WearingTable(entries));
            }
            None => refusals.push(TableEntryRefusal::NotATable { key: key.clone() }),
        }
    }
    Ok((tables, refusals))
}

/// Which TOML shape a recorded option value turned out to be, for a wearer
/// that named neither a string nor an array of strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeFound {
    Number,
    Boolean,
    Table,
    ArrayHoldingNonString,
}

impl std::fmt::Display for ShapeFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Number => "a number",
            Self::Boolean => "a boolean",
            Self::Table => "a table",
            Self::ArrayHoldingNonString => "an array holding a non-string",
        })
    }
}

/// A recorded option value of the wrong TOML shape: neither a string (an
/// `enum` or `text` choice) nor an array of strings (a `set` choice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionShapeRefusal {
    pub(crate) option: String,
    pub(crate) found: ShapeFound,
}

impl WearingTable {
    /// Turns every recorded option value into a [`Choice`], in option-name
    /// order, refusing at the first value whose TOML shape a `Choice` cannot
    /// represent.
    ///
    /// Whether a `Choice` is one the skeleton accepts — an `enum` value it
    /// lists, a `text` value that is non-empty and free of control
    /// characters, a `set` value it lists — is left to the render
    /// ([`crate::skeleton::render`]), which is handed the result.
    pub(crate) fn choices(&self) -> Result<Choices, OptionShapeRefusal> {
        let mut choices = Choices::new();
        for (option, value) in &self.0 {
            choices.insert(option.clone(), choice_of(option, value)?);
        }
        Ok(choices)
    }
}

/// Turns one recorded value into a [`Choice`], or names the TOML shape a
/// wearer actually wrote when it is neither a string nor an array of
/// strings — a TOML datetime arrives from cargo as a JSON string and is
/// passed through the same as any other string.
fn choice_of(option: &str, value: &Value) -> Result<Choice, OptionShapeRefusal> {
    match value {
        Value::String(text) => Ok(Choice::One(text.clone())),
        Value::Array(items) => {
            let mut texts = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(text) => texts.push(text.clone()),
                    _ => {
                        return Err(OptionShapeRefusal {
                            option: option.to_owned(),
                            found: ShapeFound::ArrayHoldingNonString,
                        });
                    }
                }
            }
            Ok(Choice::Many(texts))
        }
        Value::Number(_) => Err(OptionShapeRefusal {
            option: option.to_owned(),
            found: ShapeFound::Number,
        }),
        Value::Bool(_) => Err(OptionShapeRefusal {
            option: option.to_owned(),
            found: ShapeFound::Boolean,
        }),
        // A TOML table arrives as a JSON object; `Value::Null` never arrives
        // from a real TOML table at all (TOML has no null), so this arm
        // exists only to keep the match exhaustive over `serde_json::Value`.
        Value::Object(_) | Value::Null => Err(OptionShapeRefusal {
            option: option.to_owned(),
            found: ShapeFound::Table,
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DependencyKey, TableEntryRefusal, WholeTableRefusal, wearing_tables};
    use crate::skeleton::{Choice, Choices};

    /// A member that declares no dependency under any key.
    fn no_dependency(_key: &str) -> bool {
        false
    }

    #[test]
    fn no_skeletons_key_at_all_is_no_wearing_tables() {
        let (tables, refusals) = wearing_tables(&json!({}), no_dependency)
            .expect("an absent `skeletons` key is not a refusal");
        assert!(tables.is_empty());
        assert!(refusals.is_empty());
    }

    #[test]
    fn a_present_but_empty_table_wears_at_defaults() {
        let (tables, refusals) =
            wearing_tables(&json!({"skeletons": {"dependabot": {}}}), no_dependency)
                .expect("a well-formed table must not be refused");
        assert!(refusals.is_empty());
        assert_eq!(tables.len(), 1);
        assert!(tables.contains_key(&DependencyKey("dependabot".to_owned())));
    }

    #[test]
    fn a_skeletons_value_that_is_not_a_table_refuses_the_whole_member() {
        let error = wearing_tables(&json!({"skeletons": "nope"}), no_dependency)
            .expect_err("a non-table `skeletons` value must be refused");
        assert_eq!(error, WholeTableRefusal::NotATable);
    }

    #[test]
    fn one_keys_value_that_is_not_a_table_refuses_only_that_key() {
        let (tables, refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": "nope", "lint": {}}}),
            no_dependency,
        )
        .expect("only one key is malformed; the whole table is not refused");
        assert_eq!(
            refusals,
            vec![TableEntryRefusal::NotATable {
                key: "dependabot".to_owned()
            }]
        );
        assert_eq!(tables.len(), 1);
        assert!(tables.contains_key(&DependencyKey("lint".to_owned())));
    }

    /// A member that declares a dependency under exactly `declared`.
    fn dependency_under(declared: &'static str) -> impl Fn(&str) -> bool {
        move |key| key == declared
    }

    #[test]
    fn a_reserved_key_is_skipped_silently_with_no_dependency_under_it() {
        // Each reserved key holds the shape a skeleton's own manifest gives
        // it: `options` a table of option tables, `verbatim` an array of
        // paths. With no dependency keyed the same, both are the member's own
        // declaration, and neither is read as a wearing table.
        for (key, value) in [
            ("options", json!({"cadence": {}})),
            ("verbatim", json!([".github/workflows/ci.yml"])),
        ] {
            let (tables, refusals) =
                wearing_tables(&json!({"skeletons": {key: value}}), no_dependency)
                    .expect("a reserved key with no dependency under it is not a refusal");
            assert!(
                tables.is_empty(),
                "`{key}` must not be read as a wearing table"
            );
            assert!(refusals.is_empty(), "`{key}` must be skipped silently");
        }
    }

    #[test]
    fn a_reserved_key_refuses_when_a_dependency_is_keyed_the_same() {
        for (key, value) in [("options", json!({})), ("verbatim", json!({}))] {
            let (tables, refusals) =
                wearing_tables(&json!({"skeletons": {key: value}}), dependency_under(key))
                    .expect("a reserved-key refusal is per-entry, not whole-table");
            assert!(tables.is_empty(), "`{key}` must not be worn");
            assert_eq!(
                refusals,
                vec![TableEntryRefusal::ReservedKey {
                    dependency: key.to_owned()
                }]
            );
        }
    }

    #[test]
    fn a_dependency_under_a_reserved_key_refuses_whatever_shape_the_key_holds() {
        // With a dependency keyed `verbatim`, the array a skeleton's own
        // manifest would put there is refused as reserved, not skipped and
        // not reported as a table that is not one.
        let (tables, refusals) = wearing_tables(
            &json!({"skeletons": {"verbatim": ["ci.yml"]}}),
            dependency_under("verbatim"),
        )
        .expect("a reserved-key refusal is per-entry, not whole-table");
        assert!(tables.is_empty());
        assert_eq!(
            refusals,
            vec![TableEntryRefusal::ReservedKey {
                dependency: "verbatim".to_owned()
            }]
        );
    }

    #[test]
    fn each_reserved_key_is_checked_against_its_own_dependency() {
        // A dependency keyed `options` refuses `options` and leaves
        // `verbatim` skipped, and the other way round.
        let metadata = json!({"skeletons": {"options": {}, "verbatim": ["ci.yml"]}});
        for (declared, refused) in [("options", "options"), ("verbatim", "verbatim")] {
            let (tables, refusals) = wearing_tables(&metadata, dependency_under(declared))
                .expect("a reserved-key refusal is per-entry, not whole-table");
            assert!(tables.is_empty());
            assert_eq!(
                refusals,
                vec![TableEntryRefusal::ReservedKey {
                    dependency: refused.to_owned()
                }]
            );
        }
    }

    #[test]
    fn a_dependency_keyed_unlike_a_reserved_key_does_not_disturb_it() {
        let (tables, refusals) = wearing_tables(
            &json!({"skeletons": {"options": {}, "verbatim": ["ci.yml"], "lint": {}}}),
            dependency_under("lint"),
        )
        .expect("neither reserved key has a dependency under it");
        assert!(refusals.is_empty());
        assert_eq!(tables.len(), 1);
        assert!(tables.contains_key(&DependencyKey("lint".to_owned())));
    }

    #[test]
    fn a_key_that_only_resembles_a_reserved_one_is_an_ordinary_key() {
        // `Verbatim` and `options ` differ from the reserved spellings, so
        // they are read as wearing tables, or refused as not one, like any
        // other key.
        let (tables, refusals) = wearing_tables(
            &json!({"skeletons": {"Verbatim": {}, "options ": "no"}}),
            no_dependency,
        )
        .expect("only one key is malformed");
        assert_eq!(tables.len(), 1);
        assert!(tables.contains_key(&DependencyKey("Verbatim".to_owned())));
        assert_eq!(
            refusals,
            vec![TableEntryRefusal::NotATable {
                key: "options ".to_owned()
            }]
        );
    }

    #[test]
    fn a_string_value_becomes_a_one_choice() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"cadence": "daily"}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let choices = table.choices().expect("a string choice is valid");
        let mut expected = Choices::new();
        expected.insert("cadence", Choice::One("daily".to_owned()));
        assert_eq!(choices, expected);
    }

    #[test]
    fn an_array_of_strings_becomes_a_many_choice_keeping_order_and_duplicates() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"ecosystems": ["cargo", "cargo", "github-actions"]}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let choices = table
            .choices()
            .expect("an array-of-strings choice is valid");
        let mut expected = Choices::new();
        expected.insert(
            "ecosystems",
            Choice::Many(vec![
                "cargo".to_owned(),
                "cargo".to_owned(),
                "github-actions".to_owned(),
            ]),
        );
        assert_eq!(choices, expected);
    }

    #[test]
    fn a_number_value_is_refused_naming_the_option_and_the_shape() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"cadence": 5}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let error = table
            .choices()
            .expect_err("a number is not a valid choice shape");
        assert_eq!(error.option, "cadence");
        assert_eq!(error.found.to_string(), "a number");
    }

    #[test]
    fn a_boolean_value_is_refused() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"cadence": true}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let error = table
            .choices()
            .expect_err("a boolean is not a valid choice shape");
        assert_eq!(error.found.to_string(), "a boolean");
    }

    #[test]
    fn a_table_value_is_refused() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"cadence": {"nested": true}}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let error = table
            .choices()
            .expect_err("a table is not a valid choice shape");
        assert_eq!(error.found.to_string(), "a table");
    }

    #[test]
    fn an_array_holding_a_non_string_is_refused() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"ecosystems": ["cargo", 5]}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let error = table
            .choices()
            .expect_err("an array holding a non-string is refused");
        assert_eq!(error.found.to_string(), "an array holding a non-string");
    }

    #[test]
    fn the_first_bad_value_in_option_name_order_is_the_one_reported() {
        let (tables, _refusals) = wearing_tables(
            &json!({"skeletons": {"dependabot": {"zzz": 1, "aaa": true}}}),
            no_dependency,
        )
        .expect("must parse");
        let table = &tables[&DependencyKey("dependabot".to_owned())];
        let error = table
            .choices()
            .expect_err("both values are bad; the first in order wins");
        assert_eq!(error.option, "aaa");
    }
}
