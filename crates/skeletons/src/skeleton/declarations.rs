//! [`Schema`] and [`Declarations`]: the closed schema of
//! `[package.metadata.skeletons.options]`, walked by hand out of the raw TOML
//! table `manifest` handed it, so every way of getting it wrong carries a
//! precise, owned reason and the dotted key path a skeleton author wrote.
//!
//! A [`Schema`] is what the manifest declares on its own terms; linking it
//! against the files and partials a skeleton actually ships ([`Schema::link`])
//! makes the [`Declarations`] everything after reads, in which each `set` value
//! holds the [`Key`] of its own partial, and the verbatim set only paths that
//! are walked files, rather than paths to look up again.

use std::collections::{BTreeMap, BTreeSet};

use super::error::{OptionKind, Reason, RenderError, SkeletonIdentity, TomlType};
use super::keyed::{Key, Keyed, KeyedBuilder};
use super::name::OptionName;
use super::template::Partial;
use super::value::Value;
use super::verbatim::{self, VerbatimFiles};
use super::walk::TreePath;

/// The root the dotted key paths in a refusal are measured from — the table
/// [`Schema::parse`] is handed already sits at `package.metadata.skeletons`.
pub(super) const ROOT: &str = "package.metadata.skeletons";

/// One `enum` option's declaration: fills a placeholder with one of its
/// closed set of values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnumOption {
    values: Vec<Value>,
    default: Value,
}

impl EnumOption {
    /// Every value the option declares, in declared order.
    pub(crate) fn values(&self) -> &[Value] {
        &self.values
    }

    /// The value a placeholder is filled with when the wearer chooses none.
    pub(crate) const fn default(&self) -> &Value {
        &self.default
    }

    /// The length of the longest value the option declares: what a
    /// placeholder naming it can fill in, at most. Folded from the default,
    /// which is itself one of the values, so there is no empty case to
    /// answer.
    pub(crate) fn longest_value_length(&self) -> usize {
        self.values
            .iter()
            .map(|value| value.as_str().len())
            .fold(self.default.as_str().len(), usize::max)
    }
}

/// One `text` option's declaration: fills a placeholder with the wearer's own
/// text, verbatim.
///
/// With no `default` the option is optional: a wearer who states nothing
/// leaves it unset, and every line holding its placeholder is dropped whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextOption {
    default: Option<Value>,
}

impl TextOption {
    /// The text a placeholder is filled with when the wearer states none, or
    /// `None` for an optional option.
    pub(crate) const fn default(&self) -> Option<&Value> {
        self.default.as_ref()
    }

    /// Whether the option is optional: declares no default, so it can be
    /// left unset.
    pub(crate) const fn is_optional(&self) -> bool {
        self.default.is_none()
    }
}

/// One `set` option's declaration: selects zero or more partials, in
/// declared order, at a directive. Its values live in
/// [`Declarations::set_values`], one list across every `set` option, so a
/// wearer's selection is one mask over that list rather than one per option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SetOption {
    values: Vec<Key<SetValue>>,
}

impl SetOption {
    /// The option's values, in declared order.
    pub(crate) fn values(&self) -> &[Key<SetValue>] {
        &self.values
    }
}

/// One `set` value, linked: the value itself, the partial it selects, and
/// whether the option's `default` selects it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SetValue {
    value: Value,
    partial: Key<Partial>,
    selected_by_default: bool,
}

impl SetValue {
    pub(crate) const fn value(&self) -> &Value {
        &self.value
    }

    /// The partial this value selects, among those `partials/` holds.
    pub(crate) const fn partial(&self) -> Key<Partial> {
        self.partial
    }

    pub(crate) const fn is_selected_by_default(&self) -> bool {
        self.selected_by_default
    }
}

/// One `set` value as the manifest declares it, before its `partial` path
/// has been matched against `partials/`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclaredSetValue {
    /// The option declaring it, for [`Reason::PartialNotFound`].
    option: OptionName,
    value: Value,
    partial: String,
    selected_by_default: bool,
}

/// Which declared option a name refers to, and of which kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Declared {
    Enum(Key<EnumOption>),
    Set(Key<SetOption>),
    Text(Key<TextOption>),
}

impl Declared {
    pub(crate) const fn kind(self) -> OptionKind {
        match self {
            Self::Enum(_) => OptionKind::Enum,
            Self::Set(_) => OptionKind::Set,
            Self::Text(_) => OptionKind::Text,
        }
    }
}

/// Every option `[package.metadata.skeletons.options]` declares, checked on its
/// own terms, before its partials are matched against `partials/`.
#[derive(Debug)]
pub(crate) struct Schema {
    names: BTreeMap<OptionName, Declared>,
    enum_options: Keyed<EnumOption, EnumOption>,
    text_options: Keyed<TextOption, TextOption>,
    set_options: Keyed<SetOption, SetOption>,
    set_values: Keyed<SetValue, DeclaredSetValue>,
    /// The `verbatim` entries as the manifest spells them, in declared order.
    verbatim: Vec<String>,
}

/// Every option a skeleton declares, each `set` value linked to its partial.
///
/// Options are named in a fixed (name) order, so that which refusal comes
/// first never depends on a `HashMap`'s own order; `set` values run in that
/// same option order, then each option's declared order.
#[derive(Debug)]
pub(crate) struct Declarations {
    names: BTreeMap<OptionName, Declared>,
    enum_options: Keyed<EnumOption, EnumOption>,
    text_options: Keyed<TextOption, TextOption>,
    set_options: Keyed<SetOption, SetOption>,
    set_values: Keyed<SetValue, SetValue>,
    /// The files under `files/` shipped byte for byte, never parsed.
    verbatim: VerbatimFiles,
}

impl Declarations {
    /// Whether the file at `path` under `files/` is shipped verbatim.
    pub(crate) fn is_verbatim(&self, path: &TreePath) -> bool {
        self.verbatim.contains(path)
    }

    /// The declared option named `name`, by its raw text, or `None` when
    /// nothing declared is named `name`.
    pub(crate) fn find(&self, name: &str) -> Option<Declared> {
        self.names.get(name).copied()
    }

    /// Every declared option, of any kind, in name order.
    pub(crate) fn names(&self) -> impl Iterator<Item = (&OptionName, Declared)> {
        self.names.iter().map(|(name, declared)| (name, *declared))
    }

    pub(crate) const fn enum_options(&self) -> &Keyed<EnumOption, EnumOption> {
        &self.enum_options
    }

    pub(crate) const fn text_options(&self) -> &Keyed<TextOption, TextOption> {
        &self.text_options
    }

    pub(crate) const fn set_options(&self) -> &Keyed<SetOption, SetOption> {
        &self.set_options
    }

    pub(crate) const fn set_values(&self) -> &Keyed<SetValue, SetValue> {
        &self.set_values
    }
}

impl Schema {
    /// Parses `metadata` — the `[package.metadata.skeletons]` table itself — into
    /// a closed set of option declarations and the list of `verbatim` files.
    ///
    /// The table holds exactly `options` and `verbatim`. Refuses the options
    /// first, then the `verbatim` list.
    pub(crate) fn parse(
        skeleton: &SkeletonIdentity,
        metadata: &toml::Table,
    ) -> Result<Self, RenderError> {
        for key in metadata.keys() {
            if !matches!(key.as_str(), "options" | "verbatim") {
                return Err(schema_error(
                    skeleton,
                    Reason::UnknownKey {
                        key: format!("{ROOT}.{key}"),
                    },
                ));
            }
        }

        let mut schema = Self::parse_options(skeleton, metadata)?;
        schema.verbatim = verbatim::parse(skeleton, metadata)?;
        Ok(schema)
    }

    /// Parses the `options` key of `metadata`, if there is one.
    fn parse_options(
        skeleton: &SkeletonIdentity,
        metadata: &toml::Table,
    ) -> Result<Self, RenderError> {
        let options_table = match metadata.get("options") {
            None => return Ok(Self::from_parsed(BTreeMap::new())),
            Some(toml::Value::Table(table)) => table,
            Some(_not_a_table) => {
                return Err(schema_error(
                    skeleton,
                    Reason::WrongType {
                        key: format!("{ROOT}.options"),
                        expected: TomlType::Table,
                    },
                ));
            }
        };

        let mut parsed = BTreeMap::new();
        for (option_key, option_value) in options_table {
            let option_path = format!("{ROOT}.options.{option_key}");
            let Some(option_name) = OptionName::parse(option_key) else {
                return Err(schema_error(
                    skeleton,
                    Reason::OptionNameInvalid {
                        option: option_key.clone(),
                    },
                ));
            };
            let option = parse_option(skeleton, &option_name, &option_path, option_value)?;
            parsed.insert(option_name, option);
        }

        let schema = Self::from_parsed(parsed);
        check_partials_mapped_once(skeleton, &schema.set_values)?;
        Ok(schema)
    }

    /// Gives every parsed option, and every `set` value, its key — in option
    /// name order, whatever order the TOML table was read in, so every later
    /// "first refusal" over options or `set` values is in name order.
    fn from_parsed(parsed: BTreeMap<OptionName, ParsedOption>) -> Self {
        let mut names = BTreeMap::new();
        let mut enum_options = KeyedBuilder::new();
        let mut text_options = KeyedBuilder::new();
        let mut set_options = KeyedBuilder::new();
        let mut set_values = KeyedBuilder::new();
        for (name, option) in parsed {
            let declared = match option {
                ParsedOption::Enum(enum_option) => Declared::Enum(enum_options.push(enum_option)),
                ParsedOption::Text(text_option) => Declared::Text(text_options.push(text_option)),
                ParsedOption::Set(values) => {
                    let keys = values
                        .into_iter()
                        .map(|declared| set_values.push(declared))
                        .collect();
                    Declared::Set(set_options.push(SetOption { values: keys }))
                }
            };
            names.insert(name, declared);
        }
        Self {
            names,
            enum_options: enum_options.finish(),
            text_options: text_options.finish(),
            set_options: set_options.finish(),
            set_values: set_values.finish(),
            verbatim: Vec::new(),
        }
    }

    /// Links every `set` value to the partial it names among
    /// `partial_paths` — everything `partials/` holds, in path order — and
    /// checks the mapping from the other side: every partial must be named by
    /// some value. ("Named once" was checked by [`Self::parse`], which needs
    /// only the manifest.) Then links every `verbatim` entry to the file it
    /// names among `file_paths` — everything `files/` holds.
    ///
    /// Refuses the first value, in option-name then declared order, whose
    /// partial does not exist, then the first partial, in path order, that
    /// no value names, then the first `verbatim` entry, in declared order,
    /// that is not a file. The partial mapping comes first because it is the
    /// older declaration, and a manifest defective in both reports the one
    /// that was always required.
    pub(crate) fn link(
        self,
        skeleton: &SkeletonIdentity,
        file_paths: &BTreeSet<TreePath>,
        partial_paths: &Keyed<Partial, TreePath>,
    ) -> Result<Declarations, RenderError> {
        let by_path: BTreeMap<&str, Key<Partial>> = partial_paths
            .iter()
            .map(|(key, path)| (path.as_str(), key))
            .collect();

        let set_values = self.set_values.try_map(|_key, declared| {
            let Some(&partial) = by_path.get(declared.partial.as_str()) else {
                return Err(RenderError::about_manifest(
                    skeleton.clone(),
                    Reason::PartialNotFound {
                        option: declared.option.as_str().to_owned(),
                        value: declared.value.as_str().to_owned(),
                        partial: declared.partial.clone(),
                    },
                ));
            };
            Ok(SetValue {
                value: declared.value.clone(),
                partial,
                selected_by_default: declared.selected_by_default,
            })
        })?;

        let mut named = partial_paths.map(|_key, _path| false);
        for (_key, set_value) in set_values.iter() {
            named[set_value.partial] = true;
        }
        for (key, path) in partial_paths.iter() {
            if !named[key] {
                return Err(RenderError::about_manifest(
                    skeleton.clone(),
                    Reason::PartialSelectedByNothing {
                        partial: path.as_str().to_owned(),
                    },
                ));
            }
        }

        // Postcondition: every value names a distinct partial (checked by
        // `parse`), every partial is named (checked just above), so values
        // and partials pair off one to one — which `ValidatedSkeleton` relies on
        // and re-asserts once the partials are parsed.
        assert_eq!(
            set_values.len(),
            partial_paths.len(),
            "set values and partials pair off one to one once linked"
        );

        let verbatim = VerbatimFiles::link(skeleton, &self.verbatim, file_paths, partial_paths)?;
        Ok(Declarations {
            names: self.names,
            enum_options: self.enum_options,
            text_options: self.text_options,
            set_options: self.set_options,
            set_values,
            verbatim,
        })
    }
}

/// One option's table, parsed: an `enum` or `text` option whole, or a `set`
/// option's values, not yet given their keys.
enum ParsedOption {
    Enum(EnumOption),
    Text(TextOption),
    Set(Vec<DeclaredSetValue>),
}

/// Parses one option's table into its declaration, dispatching on its
/// declared `type`.
fn parse_option(
    skeleton: &SkeletonIdentity,
    name: &OptionName,
    option_path: &str,
    option_value: &toml::Value,
) -> Result<ParsedOption, RenderError> {
    let option_key = name.as_str();
    let toml::Value::Table(option_table) = option_value else {
        return Err(schema_error(
            skeleton,
            Reason::WrongType {
                key: option_path.to_owned(),
                expected: TomlType::Table,
            },
        ));
    };

    for key in option_table.keys() {
        if !matches!(key.as_str(), "type" | "values" | "default") {
            return Err(schema_error(
                skeleton,
                Reason::UnknownKey {
                    key: format!("{option_path}.{key}"),
                },
            ));
        }
    }

    let type_key = format!("{option_path}.type");
    let type_string = required_string(skeleton, option_table, "type", &type_key)?;
    match type_string {
        "enum" => parse_enum_option(skeleton, option_key, option_path, option_table)
            .map(ParsedOption::Enum),
        "set" => parse_set_option(skeleton, name, option_path, option_table).map(ParsedOption::Set),
        "text" => parse_text_option(skeleton, option_key, option_path, option_table)
            .map(ParsedOption::Text),
        other => Err(schema_error(
            skeleton,
            Reason::OptionTypeUnknown {
                option: option_key.to_owned(),
                given: other.to_owned(),
            },
        )),
    }
}

fn parse_enum_option(
    skeleton: &SkeletonIdentity,
    option_key: &str,
    option_path: &str,
    option_table: &toml::Table,
) -> Result<EnumOption, RenderError> {
    let values_key = format!("{option_path}.values");
    let values_array = required_array(skeleton, option_table, "values", &values_key)?;

    let mut values = Vec::with_capacity(values_array.len());
    let mut seen = BTreeSet::new();
    for entry in values_array {
        let toml::Value::String(text) = entry else {
            return Err(schema_error(
                skeleton,
                Reason::WrongType {
                    key: values_key.clone(),
                    expected: TomlType::String,
                },
            ));
        };
        let Some(value) = Value::parse(text) else {
            return Err(schema_error(
                skeleton,
                Reason::ValueInvalid {
                    option: option_key.to_owned(),
                    value: text.clone(),
                },
            ));
        };
        if !seen.insert(value.clone()) {
            return Err(schema_error(
                skeleton,
                Reason::ValueDeclaredTwice {
                    option: option_key.to_owned(),
                    value: value.as_str().to_owned(),
                },
            ));
        }
        values.push(value);
    }
    if values.is_empty() {
        return Err(schema_error(
            skeleton,
            Reason::NoValues {
                option: option_key.to_owned(),
            },
        ));
    }

    let default_key = format!("{option_path}.default");
    let default_text = required_string(skeleton, option_table, "default", &default_key)?;
    let Some(default) = values
        .iter()
        .find(|value| value.as_str() == default_text)
        .cloned()
    else {
        return Err(schema_error(
            skeleton,
            Reason::DefaultNotDeclared {
                option: option_key.to_owned(),
                value: default_text.to_owned(),
            },
        ));
    };

    Ok(EnumOption { values, default })
}

/// Parses a `text` option: no `values`, since the wearer supplies the text,
/// and an optional `default`, which is one value like any other declared one.
fn parse_text_option(
    skeleton: &SkeletonIdentity,
    option_key: &str,
    option_path: &str,
    option_table: &toml::Table,
) -> Result<TextOption, RenderError> {
    // `parse_option` let `values` through because `enum` and `set` take it;
    // the manifest is a closed schema, so for `text` it is a key nothing
    // recognises.
    if option_table.contains_key("values") {
        return Err(schema_error(
            skeleton,
            Reason::UnknownKey {
                key: format!("{option_path}.values"),
            },
        ));
    }

    let default_key = format!("{option_path}.default");
    let default = match option_table.get("default") {
        None => None,
        Some(toml::Value::String(text)) => {
            let Some(default) = Value::parse(text) else {
                return Err(schema_error(
                    skeleton,
                    Reason::ValueInvalid {
                        option: option_key.to_owned(),
                        value: text.clone(),
                    },
                ));
            };
            Some(default)
        }
        Some(_not_a_string) => {
            return Err(schema_error(
                skeleton,
                Reason::WrongType {
                    key: default_key,
                    expected: TomlType::String,
                },
            ));
        }
    };
    Ok(TextOption { default })
}

/// Parses a `set` option's `values` and `default`, returning its values with
/// the default's selection recorded on each.
fn parse_set_option(
    skeleton: &SkeletonIdentity,
    name: &OptionName,
    option_path: &str,
    option_table: &toml::Table,
) -> Result<Vec<DeclaredSetValue>, RenderError> {
    let option_key = name.as_str();
    let values_key = format!("{option_path}.values");
    let values_array = required_array(skeleton, option_table, "values", &values_key)?;
    let mut values = parse_set_values(skeleton, name, &values_key, values_array)?;
    if values.is_empty() {
        return Err(schema_error(
            skeleton,
            Reason::NoValues {
                option: option_key.to_owned(),
            },
        ));
    }

    let default_key = format!("{option_path}.default");
    let default_array = required_array(skeleton, option_table, "default", &default_key)?;
    mark_set_default(
        skeleton,
        option_key,
        &default_key,
        default_array,
        &mut values,
    )?;
    Ok(values)
}

/// Parses a `set` option's `values` array — each entry a `{ value, partial }`
/// table — into its declared values, none yet selected by default.
fn parse_set_values(
    skeleton: &SkeletonIdentity,
    name: &OptionName,
    values_key: &str,
    values_array: &[toml::Value],
) -> Result<Vec<DeclaredSetValue>, RenderError> {
    let option_key = name.as_str();
    let mut values = Vec::with_capacity(values_array.len());
    let mut seen = BTreeSet::new();
    for (index, entry) in values_array.iter().enumerate() {
        let entry_path = format!("{values_key}[{index}]");
        let toml::Value::Table(entry_table) = entry else {
            return Err(schema_error(
                skeleton,
                Reason::WrongType {
                    key: entry_path,
                    expected: TomlType::Table,
                },
            ));
        };
        for key in entry_table.keys() {
            if !matches!(key.as_str(), "value" | "partial") {
                return Err(schema_error(
                    skeleton,
                    Reason::UnknownKey {
                        key: format!("{entry_path}.{key}"),
                    },
                ));
            }
        }

        let entry_value_key = format!("{entry_path}.value");
        let value_text = required_string(skeleton, entry_table, "value", &entry_value_key)?;
        let Some(value) = Value::parse(value_text) else {
            return Err(schema_error(
                skeleton,
                Reason::ValueInvalid {
                    option: option_key.to_owned(),
                    value: value_text.to_owned(),
                },
            ));
        };
        if !seen.insert(value.clone()) {
            return Err(schema_error(
                skeleton,
                Reason::ValueDeclaredTwice {
                    option: option_key.to_owned(),
                    value: value.as_str().to_owned(),
                },
            ));
        }

        let partial_key = format!("{entry_path}.partial");
        let partial_text = required_string(skeleton, entry_table, "partial", &partial_key)?;
        values.push(DeclaredSetValue {
            option: name.clone(),
            value,
            partial: partial_text.to_owned(),
            selected_by_default: false,
        });
    }
    Ok(values)
}

/// Parses a `set` option's `default` array against its already-parsed
/// `values`, marking each value it names as selected by default and
/// refusing a value the option does not declare or one listed twice. The
/// mark is kept on the value the default was matched against, so nothing
/// downstream ever looks a default up again.
fn mark_set_default(
    skeleton: &SkeletonIdentity,
    option_key: &str,
    default_key: &str,
    default_array: &[toml::Value],
    values: &mut [DeclaredSetValue],
) -> Result<(), RenderError> {
    for entry in default_array {
        let toml::Value::String(text) = entry else {
            return Err(schema_error(
                skeleton,
                Reason::WrongType {
                    key: default_key.to_owned(),
                    expected: TomlType::String,
                },
            ));
        };
        let Some(matched) = values
            .iter_mut()
            .find(|set_value| set_value.value.as_str() == text)
        else {
            return Err(schema_error(
                skeleton,
                Reason::DefaultNotDeclared {
                    option: option_key.to_owned(),
                    value: text.clone(),
                },
            ));
        };
        if matched.selected_by_default {
            return Err(schema_error(
                skeleton,
                Reason::DefaultListsValueTwice {
                    option: option_key.to_owned(),
                    value: text.clone(),
                },
            ));
        }
        matched.selected_by_default = true;
    }
    Ok(())
}

/// Every partial a `set` option declares is named by exactly one value —
/// the "mapped once" half of the mapping check; "every mapped partial
/// exists" and "every partial is mapped" need the walked `partials/` tree
/// and happen later, in [`Schema::link`].
fn check_partials_mapped_once(
    skeleton: &SkeletonIdentity,
    set_values: &Keyed<SetValue, DeclaredSetValue>,
) -> Result<(), RenderError> {
    let mut named = BTreeSet::new();
    for (_key, set_value) in set_values.iter() {
        if !named.insert(set_value.partial.as_str()) {
            return Err(schema_error(
                skeleton,
                Reason::PartialSelectedTwice {
                    partial: set_value.partial.clone(),
                },
            ));
        }
    }
    Ok(())
}

fn required_string<'table>(
    skeleton: &SkeletonIdentity,
    table: &'table toml::Table,
    key: &str,
    dotted_path: &str,
) -> Result<&'table str, RenderError> {
    match table.get(key) {
        None => Err(schema_error(
            skeleton,
            Reason::MissingKey {
                key: dotted_path.to_owned(),
            },
        )),
        Some(toml::Value::String(text)) => Ok(text),
        Some(_not_a_string) => Err(schema_error(
            skeleton,
            Reason::WrongType {
                key: dotted_path.to_owned(),
                expected: TomlType::String,
            },
        )),
    }
}

fn required_array<'table>(
    skeleton: &SkeletonIdentity,
    table: &'table toml::Table,
    key: &str,
    dotted_path: &str,
) -> Result<&'table Vec<toml::Value>, RenderError> {
    match table.get(key) {
        None => Err(schema_error(
            skeleton,
            Reason::MissingKey {
                key: dotted_path.to_owned(),
            },
        )),
        Some(toml::Value::Array(array)) => Ok(array),
        Some(_not_an_array) => Err(schema_error(
            skeleton,
            Reason::WrongType {
                key: dotted_path.to_owned(),
                expected: TomlType::Array,
            },
        )),
    }
}

fn schema_error(skeleton: &SkeletonIdentity, reason: Reason) -> RenderError {
    RenderError::about_manifest(skeleton.clone(), reason)
}

/// Test-only constructors, for the unit tests of the modules that read
/// [`Declarations`] and need some without a skeleton directory to walk.
#[cfg(test)]
impl Declarations {
    /// Declarations parsed from `toml_text` (the `[package.metadata.skeletons]`
    /// table's own contents) and linked against `partials`, as though
    /// `partials/` held exactly those paths.
    ///
    /// # Panics
    ///
    /// If the fixture does not parse or link: a test's fixture is its own
    /// premise.
    pub(crate) fn for_test(toml_text: &str, partials: &[&str]) -> Self {
        Self::for_test_with_paths(toml_text, partials).0
    }

    /// As [`Self::for_test`], with each of `verbatim` (paths relative to
    /// `files/`, `/`-separated) declared verbatim.
    ///
    /// Sets the linked set directly, so a test of a reader of the
    /// declarations needs neither a manifest declaring it nor a `files/` tree
    /// to walk. Declaring one through a manifest is tested where it is read,
    /// in `verbatim`.
    pub(crate) fn for_test_verbatim(toml_text: &str, partials: &[&str], verbatim: &[&str]) -> Self {
        let mut declarations = Self::for_test(toml_text, partials);
        declarations.verbatim = VerbatimFiles::for_test(verbatim);
        declarations
    }

    /// As [`Self::for_test`], also returning the partial paths linked
    /// against, in the path order a walk would give them.
    pub(crate) fn for_test_with_paths(
        toml_text: &str,
        partials: &[&str],
    ) -> (Self, Keyed<Partial, TreePath>) {
        let skeleton = SkeletonIdentity::Named("test".to_owned());
        let table: toml::Table = toml_text.parse().expect("test fixture must be valid TOML");
        let sorted: BTreeSet<TreePath> = partials
            .iter()
            .map(|path| TreePath::for_test(&[path]))
            .collect();
        let mut builder = KeyedBuilder::new();
        for path in sorted {
            builder.push(path);
        }
        let partial_paths = builder.finish();
        let declarations = Schema::parse(&skeleton, &table)
            .and_then(|schema| schema.link(&skeleton, &BTreeSet::new(), &partial_paths))
            .expect("test fixture declarations must be well-formed");
        (declarations, partial_paths)
    }
}

#[cfg(test)]
mod tests {
    use super::{Declarations, Declared, OptionKind, Schema, SkeletonIdentity, TextOption, Value};
    use crate::skeleton::error::Reason;

    fn parse(toml_text: &str) -> Result<Schema, crate::skeleton::error::RenderError> {
        let table: toml::Table = toml_text.parse().expect("test fixture must be valid TOML");
        Schema::parse(&SkeletonIdentity::Named("test".to_owned()), &table)
    }

    #[test]
    fn a_table_with_no_options_key_declares_nothing() {
        let declarations = Declarations::for_test("", &[]);
        assert_eq!(declarations.names().count(), 0);
    }

    #[test]
    fn a_well_formed_enum_option_is_declared() {
        let declarations = Declarations::for_test(
            r#"
            [options.cadence]
            type = "enum"
            values = ["daily", "weekly"]
            default = "weekly"
            "#,
            &[],
        );
        let declared = declarations.find("cadence").expect("declared");
        assert_eq!(declared.kind(), OptionKind::Enum);
        let Declared::Enum(key) = declared else {
            panic!("expected an enum option, got {declared:?}")
        };
        assert_eq!(
            declarations.enum_options()[key].default().as_str(),
            "weekly"
        );
    }

    /// The `text` option named `name`, which `declarations` must declare.
    fn text_option<'declarations>(
        declarations: &'declarations Declarations,
        name: &str,
    ) -> &'declarations TextOption {
        let Some(Declared::Text(key)) = declarations.find(name) else {
            panic!("{name} must be a declared text option")
        };
        &declarations.text_options()[key]
    }

    #[test]
    fn a_text_option_with_no_default_is_declared_optional() {
        let declarations = Declarations::for_test(
            r#"
            [options.assignee]
            type = "text"
            "#,
            &[],
        );
        assert_eq!(
            declarations.find("assignee").expect("declared").kind(),
            OptionKind::Text
        );
        let option = text_option(&declarations, "assignee");
        assert!(option.is_optional());
        assert!(option.default().is_none());
    }

    #[test]
    fn a_text_option_with_a_default_is_declared_required() {
        let declarations = Declarations::for_test(
            r#"
            [options.assignee]
            type = "text"
            default = "octocat"
            "#,
            &[],
        );
        let option = text_option(&declarations, "assignee");
        assert!(!option.is_optional());
        assert_eq!(option.default().map(Value::as_str), Some("octocat"));
    }

    #[test]
    fn a_text_option_declaring_values_is_refused_at_the_dotted_path_of_values() {
        let error = parse(
            r#"
            [options.assignee]
            type = "text"
            values = ["octocat"]
            "#,
        )
        .expect_err("a text option takes no values");
        let Reason::UnknownKey { key } = error.reason() else {
            panic!("expected an unknown key, got {error:?}")
        };
        assert_eq!(key, "package.metadata.skeletons.options.assignee.values");
    }

    #[test]
    fn a_text_option_declaring_a_key_the_schema_does_not_know_is_refused() {
        let error = parse(
            r#"
            [options.assignee]
            type = "text"
            pattern = "[a-z]+"
            "#,
        )
        .expect_err("a text option takes no pattern");
        assert!(matches!(
            error.reason(),
            Reason::UnknownKey { key } if key.ends_with("assignee.pattern")
        ));
    }

    #[test]
    fn a_text_default_that_is_empty_or_holds_a_control_character_is_refused() {
        for default in ["", "\\u0001", "octo\\ncat", "octo\\u0085cat"] {
            let error = parse(&format!(
                "[options.assignee]\ntype = \"text\"\ndefault = \"{default}\"\n"
            ))
            .expect_err("an invalid text default must be refused");
            let Reason::ValueInvalid { option, .. } = error.reason() else {
                panic!("{default:?}: expected an invalid value, got {error:?}")
            };
            assert_eq!(option, "assignee", "{default:?}: {error:?}");
        }
    }

    #[test]
    fn a_text_default_that_is_not_a_string_is_refused_as_the_wrong_type() {
        let error = parse(
            r#"
            [options.assignee]
            type = "text"
            default = ["x"]
            "#,
        )
        .expect_err("a text default must be a string");
        assert!(matches!(
            error.reason(),
            Reason::WrongType { key, expected: crate::skeleton::error::TomlType::String }
                if key.ends_with("assignee.default")
        ));
    }

    #[test]
    fn an_enum_option_with_no_default_is_still_refused_beside_an_optional_text() {
        let error = parse(
            r#"
            [options.assignee]
            type = "text"

            [options.cadence]
            type = "enum"
            values = ["daily"]
            "#,
        )
        .expect_err("optionality is text-only");
        assert!(matches!(
            error.reason(),
            Reason::MissingKey { key } if key.ends_with("cadence.default")
        ));
    }

    #[test]
    fn a_well_formed_set_option_is_declared_with_its_value_linked_to_its_partial() {
        // Two partials, so linking has to pick the right one: `lint.yml`
        // sorts after `build.yml`, and a value linked by position in the
        // manifest rather than by path would land on the wrong partial.
        let (declarations, partial_paths) = Declarations::for_test_with_paths(
            r#"
            [options.workflows]
            type = "set"
            default = ["lint"]

            [[options.workflows.values]]
            value = "lint"
            partial = "lint.yml"

            [[options.workflows.values]]
            value = "build"
            partial = "build.yml"
            "#,
            &["build.yml", "lint.yml"],
        );
        let declared = declarations.find("workflows").expect("declared");
        assert_eq!(declared.kind(), OptionKind::Set);
        let Declared::Set(key) = declared else {
            panic!("expected a set option, got {declared:?}")
        };
        let values = declarations.set_options()[key].values();
        assert_eq!(values.len(), 2);
        let lint = &declarations.set_values()[values[0]];
        let build = &declarations.set_values()[values[1]];
        assert_eq!(lint.value().as_str(), "lint");
        assert_eq!(partial_paths[lint.partial()].as_str(), "lint.yml");
        assert!(lint.is_selected_by_default());
        assert_eq!(partial_paths[build.partial()].as_str(), "build.yml");
        assert!(!build.is_selected_by_default());
    }

    #[test]
    fn a_set_default_listing_one_value_twice_is_refused() {
        // The default's duplicate check reads the mark a first mention left
        // on the value itself; listing `lint` twice must meet that mark.
        let error = parse(
            r#"
            [options.workflows]
            type = "set"
            default = ["lint", "lint"]

            [[options.workflows.values]]
            value = "lint"
            partial = "lint.yml"
            "#,
        )
        .expect_err("a default listing one value twice must be refused");
        assert!(matches!(
            error.reason(),
            Reason::DefaultListsValueTwice { option, value }
                if option == "workflows" && value == "lint"
        ));
    }

    #[test]
    fn the_longest_value_length_is_the_longest_declared_value_not_the_default() {
        // The longest value is declared neither first nor as the default,
        // so a fold that started from, or stopped at, either would miss it.
        let declarations = Declarations::for_test(
            r#"
            [options.cadence]
            type = "enum"
            values = ["daily", "fortnightly", "weekly"]
            default = "weekly"
            "#,
            &[],
        );
        let Some(Declared::Enum(key)) = declarations.find("cadence") else {
            panic!("`cadence` is declared as an enum option above")
        };
        assert_eq!(
            declarations.enum_options()[key].longest_value_length(),
            "fortnightly".len()
        );
    }

    #[test]
    fn an_unknown_top_level_key_is_refused() {
        let error = parse("surprising = true").expect_err("an unknown key must be refused");
        assert!(matches!(error.reason(), Reason::UnknownKey { key } if key.contains("surprising")));
    }

    #[test]
    fn a_negative_case_a_declared_enum_value_that_is_empty_is_refused() {
        let error = parse(
            r#"
            [options.cadence]
            type = "enum"
            values = [""]
            default = ""
            "#,
        )
        .expect_err("an empty value must be refused");
        assert!(matches!(error.reason(), Reason::ValueInvalid { .. }));
    }

    #[test]
    fn an_enum_option_with_empty_values_is_refused() {
        let error = parse(
            r#"
            [options.cadence]
            type = "enum"
            values = []
            default = "weekly"
            "#,
        )
        .expect_err("empty values must be refused");
        assert!(matches!(error.reason(), Reason::NoValues { option } if option == "cadence"));
    }

    #[test]
    fn an_invalid_option_name_is_refused() {
        let error = parse(
            r#"
            [options.Cadence]
            type = "enum"
            values = ["daily"]
            default = "daily"
            "#,
        )
        .expect_err("an uppercase option name must be refused");
        assert!(
            matches!(error.reason(), Reason::OptionNameInvalid { option } if option == "Cadence")
        );
    }

    #[test]
    fn two_set_options_mapping_the_same_partial_is_refused() {
        let error = parse(
            r#"
            [options.a]
            type = "set"
            default = []
            [[options.a.values]]
            value = "x"
            partial = "shared.yml"

            [options.b]
            type = "set"
            default = []
            [[options.b.values]]
            value = "y"
            partial = "shared.yml"
            "#,
        )
        .expect_err("a partial named by two values must be refused");
        assert!(
            matches!(error.reason(), Reason::PartialSelectedTwice { partial } if partial == "shared.yml")
        );
    }

    #[test]
    fn get_finds_a_declared_option_by_its_text() {
        // Names chosen so that text order and length order actively disagree
        // ("z" is lexicographically last but shortest, so the two orders put it
        // at opposite ends): a single short entry, or names whose length
        // happens to track their text order, cannot exercise `Borrow<str>`
        // soundness, since there is nothing in the tree's actual shape for a
        // mismatched `Ord` to misplace. A key type ordered by length before
        // text would build a tree shaped so differently from `str`'s own order
        // that a `&str` lookup — which navigates using `str`'s `Ord`, not
        // `OptionName`'s — misses at least one of these.
        let declarations = Declarations::for_test(
            r#"
            [options.z]
            type = "enum"
            values = ["x"]
            default = "x"

            [options.ab]
            type = "enum"
            values = ["x"]
            default = "x"

            [options.cde]
            type = "enum"
            values = ["x"]
            default = "x"

            [options.fghi]
            type = "enum"
            values = ["x"]
            default = "x"

            [options.jklmn]
            type = "enum"
            values = ["x"]
            default = "x"
            "#,
            &[],
        );

        for text in ["z", "ab", "cde", "fghi", "jklmn"] {
            let found = declarations
                .find(text)
                .unwrap_or_else(|| panic!("{text} must be found by its own text"));
            let scanned = declarations
                .names()
                .find(|(name, _declared)| name.as_str() == text)
                .map(|(_name, declared)| declared);
            assert_eq!(
                Some(found),
                scanned,
                "{text} must find the same option a scan by name finds"
            );
        }

        assert!(
            declarations.find("nonexistent").is_none(),
            "an undeclared name must not be found"
        );
        assert!(
            declarations.find("Z").is_none(),
            "lookup is exact, not case-insensitive"
        );
    }
}
