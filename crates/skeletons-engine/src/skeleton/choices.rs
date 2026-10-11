//! [`Choices`]: the wearer's input to a render. [`resolve`] validates it
//! against a skeleton's [`Declarations`] and turns it into a [`Resolved`] set of
//! option values a render can act on, without ever carrying the wearer's own
//! order forward: a resolved `set` option is a mark on each of the skeleton's
//! own declared values, which has no order of its own to leak.

use std::collections::BTreeMap;

use super::declarations::{Declarations, Declared, EnumOption, SetOption, SetValue, TextOption};
use super::error::{OptionKind, Reason, RenderError, SkeletonIdentity};
use super::keyed::{Key, Keyed};
use super::value::Value;

/// The option values a wearer chose, by option name. Options not named here
/// take the skeleton's declared default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Choices(BTreeMap<String, Choice>);

impl Choices {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records the choice for `option`.
    ///
    /// # Panics
    ///
    /// If `option` already has a choice: a wearer's manifest cannot name one
    /// key twice, so a second insert is a bug in the caller, not input.
    pub(crate) fn insert(&mut self, option: impl Into<String>, choice: Choice) {
        let option = option.into();
        let replaced = self.0.insert(option.clone(), choice);
        assert!(
            replaced.is_none(),
            "a choice was already recorded for option `{option}`"
        );
    }
}

/// One option's chosen value, in the shape the wearer wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Choice {
    /// A single value: how an `enum` or a `text` option is set.
    One(String),
    /// A list of values, exactly as written, duplicates and order included:
    /// how a `set` option is set. The render never uses this order.
    Many(Vec<String>),
}

/// The wearer's choices, resolved against a skeleton's declarations: one value
/// for every `enum` option, one value or none for every `text` option, and
/// one mark for every `set` value, each the wearer's choice where one was
/// given and the skeleton's default otherwise. A `text` option with no
/// default and no choice resolves to no value, which is what drops its
/// lines. Each `text` option also records whether the wearer stated it, since
/// a value the skeleton's own default supplied is never the wearer's to
/// shorten.
///
/// All four lists are made by mapping the declarations' own, so every key a
/// placeholder or a `set` option holds has an entry here — a resolved value
/// or selection cannot be missing.
#[derive(Debug)]
pub(crate) struct Resolved {
    enum_values: Keyed<EnumOption, Value>,
    text_values: Keyed<TextOption, Option<Value>>,
    text_stated: Keyed<TextOption, bool>,
    selected: Keyed<SetValue, bool>,
}

impl Resolved {
    /// The value a placeholder naming `option` is filled with.
    pub(crate) fn enum_value(&self, option: Key<EnumOption>) -> &Value {
        &self.enum_values[option]
    }

    /// The text a placeholder naming `option` is filled with, or `None` when
    /// the option is optional and the wearer stated nothing.
    pub(crate) fn text_value(&self, option: Key<TextOption>) -> Option<&Value> {
        self.text_values[option].as_ref()
    }

    /// Whether the wearer stated `option`, as against its taking its default
    /// or staying unset.
    pub(crate) fn text_is_stated(&self, option: Key<TextOption>) -> bool {
        self.text_stated[option]
    }

    /// Whether the `set` value `value` is selected.
    pub(crate) fn is_selected(&self, value: Key<SetValue>) -> bool {
        self.selected[value]
    }
}

/// Validates `choices` against `declarations` and resolves every declared
/// option to the value or selection a render should use — the wearer's own
/// choice where one was given, the skeleton's declared default otherwise.
///
/// Validation walks `choices` in option-name order (it is a `BTreeMap`), so
/// which refusal comes first never depends on the order a caller happened to
/// insert them in.
pub(crate) fn resolve(
    skeleton: &SkeletonIdentity,
    declarations: &Declarations,
    choices: &Choices,
) -> Result<Resolved, RenderError> {
    let mut resolved = Resolved {
        enum_values: declarations
            .enum_options()
            .map(|_key, enum_option| enum_option.default().clone()),
        text_values: declarations
            .text_options()
            .map(|_key, text_option| text_option.default().cloned()),
        text_stated: declarations.text_options().map(|_key, _text_option| false),
        selected: declarations
            .set_values()
            .map(|_key, set_value| set_value.is_selected_by_default()),
    };
    for (raw_name, choice) in &choices.0 {
        apply_choice(skeleton, declarations, raw_name, choice, &mut resolved)?;
    }
    Ok(resolved)
}

/// Validates one `(raw_name, choice)` pair from the wearer against
/// `declarations`, replacing that option's default in `resolved` when it is
/// valid.
fn apply_choice(
    skeleton: &SkeletonIdentity,
    declarations: &Declarations,
    raw_name: &str,
    choice: &Choice,
    resolved: &mut Resolved,
) -> Result<(), RenderError> {
    let Some(declared) = declarations.find(raw_name) else {
        return Err(RenderError::about_choice(
            skeleton.clone(),
            Reason::UndeclaredOption {
                option: raw_name.to_owned(),
            },
        ));
    };

    match (declared, choice) {
        (Declared::Enum(option), Choice::One(value_text)) => {
            let enum_option = &declarations.enum_options()[option];
            resolved.enum_values[option] =
                apply_enum_choice(skeleton, raw_name, enum_option, value_text)?;
            Ok(())
        }
        (Declared::Text(option), Choice::One(text)) => {
            resolved.text_values[option] = Some(apply_text_choice(skeleton, raw_name, text)?);
            resolved.text_stated[option] = true;
            Ok(())
        }
        (Declared::Set(option), Choice::Many(values_text)) => {
            let set_option = &declarations.set_options()[option];
            apply_set_choice(
                skeleton,
                raw_name,
                set_option,
                declarations.set_values(),
                values_text,
                &mut resolved.selected,
            )
        }
        (Declared::Enum(_), Choice::Many(_)) => Err(RenderError::about_choice(
            skeleton.clone(),
            Reason::ChoiceShapeMismatch {
                option: raw_name.to_owned(),
                declared: OptionKind::Enum,
            },
        )),
        (Declared::Text(_), Choice::Many(_)) => Err(RenderError::about_choice(
            skeleton.clone(),
            Reason::ChoiceShapeMismatch {
                option: raw_name.to_owned(),
                declared: OptionKind::Text,
            },
        )),
        (Declared::Set(_), Choice::One(_)) => Err(RenderError::about_choice(
            skeleton.clone(),
            Reason::ChoiceShapeMismatch {
                option: raw_name.to_owned(),
                declared: OptionKind::Set,
            },
        )),
    }
}

/// Validates a `One`-shaped choice for a `text` option: the wearer's own
/// words, which need only be a [`Value`] — non-empty, with no control
/// character — since there is no declared set to be one of.
fn apply_text_choice(
    skeleton: &SkeletonIdentity,
    raw_name: &str,
    text: &str,
) -> Result<Value, RenderError> {
    Value::parse(text).ok_or_else(|| {
        RenderError::about_choice(
            skeleton.clone(),
            Reason::TextChoiceInvalid {
                option: raw_name.to_owned(),
                value: text.to_owned(),
            },
        )
    })
}

/// Validates a `One`-shaped choice against an enum option's declared
/// values, returning the value it names.
fn apply_enum_choice(
    skeleton: &SkeletonIdentity,
    raw_name: &str,
    enum_option: &EnumOption,
    value_text: &str,
) -> Result<Value, RenderError> {
    enum_option
        .values()
        .iter()
        .find(|value| value.as_str() == value_text)
        .cloned()
        .ok_or_else(|| {
            RenderError::about_choice(
                skeleton.clone(),
                Reason::ValueNotDeclared {
                    option: raw_name.to_owned(),
                    value: value_text.to_owned(),
                },
            )
        })
}

/// Validates a `Many`-shaped choice against a set option's declared values,
/// refusing a value the option does not declare or one listed twice, and
/// replaces the option's default selection in `selected` with it.
fn apply_set_choice(
    skeleton: &SkeletonIdentity,
    raw_name: &str,
    set_option: &SetOption,
    set_values: &Keyed<SetValue, SetValue>,
    values_text: &[String],
    selected: &mut Keyed<SetValue, bool>,
) -> Result<(), RenderError> {
    // The wearer's choice replaces the default whole: nothing the default
    // selected stays selected unless the wearer names it too.
    for &value in set_option.values() {
        selected[value] = false;
    }
    for value_text in values_text {
        let Some(value) = set_option
            .values()
            .iter()
            .copied()
            .find(|&value| set_values[value].value().as_str() == value_text)
        else {
            return Err(RenderError::about_choice(
                skeleton.clone(),
                Reason::ValueNotDeclared {
                    option: raw_name.to_owned(),
                    value: value_text.clone(),
                },
            ));
        };
        if selected[value] {
            return Err(RenderError::about_choice(
                skeleton.clone(),
                Reason::ValueChosenTwice {
                    option: raw_name.to_owned(),
                    value: value_text.clone(),
                },
            ));
        }
        selected[value] = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Choice, Choices, Reason, resolve};
    use crate::skeleton::declarations::{Declarations, Declared};
    use crate::skeleton::error::OptionKind;
    use crate::skeleton::error::SkeletonIdentity;

    /// The resolved value of the enum option `name`.
    fn enum_value(declarations: &Declarations, resolved: &super::Resolved, name: &str) -> String {
        let Some(Declared::Enum(option)) = declarations.find(name) else {
            panic!("{name} must be a declared enum option")
        };
        resolved.enum_value(option).as_str().to_owned()
    }

    const TEXT_OPTIONS: &str = r#"
        [options.assignee]
        type = "text"

        [options.owner]
        type = "text"
        default = "octocat"
        "#;

    /// The resolved text of the `text` option `name`, or `None` when unset.
    fn text_value(
        declarations: &Declarations,
        resolved: &super::Resolved,
        name: &str,
    ) -> Option<String> {
        let Some(Declared::Text(option)) = declarations.find(name) else {
            panic!("{name} must be a declared text option")
        };
        resolved
            .text_value(option)
            .map(|value| value.as_str().to_owned())
    }

    /// Resolves `choices` against `TEXT_OPTIONS`.
    fn resolve_text(
        choices: &Choices,
    ) -> Result<(Declarations, super::Resolved), crate::skeleton::error::RenderError> {
        let declarations = Declarations::for_test(TEXT_OPTIONS, &[]);
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            choices,
        )?;
        Ok((declarations, resolved))
    }

    #[test]
    fn a_text_option_resolves_to_the_wearers_text_its_default_or_nothing() {
        let (declarations, unset) = resolve_text(&Choices::new()).expect("no choices resolve");
        assert_eq!(text_value(&declarations, &unset, "assignee"), None);
        assert_eq!(
            text_value(&declarations, &unset, "owner").as_deref(),
            Some("octocat")
        );

        let mut choices = Choices::new();
        choices.insert("assignee", Choice::One("hubot".to_owned()));
        choices.insert("owner", Choice::One("monalisa".to_owned()));
        let (declarations, set) = resolve_text(&choices).expect("text choices resolve");
        assert_eq!(
            text_value(&declarations, &set, "assignee").as_deref(),
            Some("hubot")
        );
        assert_eq!(
            text_value(&declarations, &set, "owner").as_deref(),
            Some("monalisa")
        );
    }

    #[test]
    fn a_text_choice_that_is_empty_or_holds_a_control_character_is_refused() {
        for text in [
            "",
            "octo\tcat",
            "octo\ncat",
            "octo\u{1}cat",
            "octo\u{85}cat",
        ] {
            let mut choices = Choices::new();
            choices.insert("assignee", Choice::One(text.to_owned()));
            let error = resolve_text(&choices).expect_err("an invalid text must be refused");
            assert_eq!(
                error.file(),
                None,
                "{text:?}: a wearer's choice names no file"
            );
            assert!(
                matches!(
                    error.reason(),
                    Reason::TextChoiceInvalid { option, value }
                        if option == "assignee" && value == text
                ),
                "{text:?}: {error:?}"
            );
        }
    }

    #[test]
    fn a_text_option_given_a_many_shaped_choice_is_refused() {
        let mut choices = Choices::new();
        choices.insert("assignee", Choice::Many(vec!["octocat".to_owned()]));
        let error = resolve_text(&choices).expect_err("a list for a text option must be refused");
        assert!(matches!(
            error.reason(),
            Reason::ChoiceShapeMismatch { option, declared: OptionKind::Text }
                if option == "assignee"
        ));
    }

    #[test]
    #[should_panic(expected = "a choice was already recorded")]
    fn inserting_a_second_choice_for_one_option_panics() {
        // Pins the caller contract on `Choices::insert`'s own doc comment:
        // a second insert for the same option is a bug in the caller, since
        // a wearer's manifest cannot name one key twice, not input this
        // module refuses.
        let mut choices = Choices::new();
        choices.insert("cadence", Choice::One("daily".to_owned()));
        choices.insert("cadence", Choice::One("weekly".to_owned()));
    }

    #[test]
    fn no_choices_resolves_every_option_to_its_default() {
        let declarations = Declarations::for_test(
            r#"
            [options.cadence]
            type = "enum"
            values = ["daily", "weekly"]
            default = "weekly"
            "#,
            &[],
        );
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            &Choices::new(),
        )
        .expect("no choices must resolve to defaults");
        assert_eq!(enum_value(&declarations, &resolved, "cadence"), "weekly");
    }

    #[test]
    fn a_choice_for_an_enum_option_overrides_its_default() {
        let declarations = Declarations::for_test(
            r#"
            [options.cadence]
            type = "enum"
            values = ["daily", "weekly"]
            default = "weekly"
            "#,
            &[],
        );
        let mut choices = Choices::new();
        choices.insert("cadence", Choice::One("daily".to_owned()));
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            &choices,
        )
        .expect("a valid choice must resolve");
        assert_eq!(enum_value(&declarations, &resolved, "cadence"), "daily");
    }

    #[test]
    fn an_enum_option_given_a_many_shaped_choice_is_refused() {
        let declarations = Declarations::for_test(
            r#"
            [options.cadence]
            type = "enum"
            values = ["daily"]
            default = "daily"
            "#,
            &[],
        );
        let mut choices = Choices::new();
        choices.insert("cadence", Choice::Many(vec!["daily".to_owned()]));
        let error = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            &choices,
        )
        .expect_err("a Many choice for an enum option must be refused");
        assert!(matches!(error.reason(), Reason::ChoiceShapeMismatch { .. }));
    }

    #[test]
    fn a_set_option_given_a_one_shaped_choice_is_refused() {
        let declarations = Declarations::for_test(
            r#"
            [options.workflows]
            type = "set"
            default = []
            [[options.workflows.values]]
            value = "lint"
            partial = "lint.yml"
            "#,
            &["lint.yml"],
        );
        let mut choices = Choices::new();
        choices.insert("workflows", Choice::One("lint".to_owned()));
        let error = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            &declarations,
            &choices,
        )
        .expect_err("a One choice for a set option must be refused");
        assert!(matches!(error.reason(), Reason::ChoiceShapeMismatch { .. }));
    }
}
