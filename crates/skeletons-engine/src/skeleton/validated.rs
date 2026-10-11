//! [`ValidatedSkeleton`]: a skeleton read and checked whole, before any wearer's
//! choices are looked at — its declarations with every `set` value linked to
//! its partial, every file parsed with every placeholder and directive
//! holding the option it names, and every partial parsed. Holding one is the
//! proof that nothing about the skeleton itself is left to refuse.

use std::collections::BTreeMap;

use super::declarations::{Declarations, Declared};
use super::error::{Reason, RenderError, SkeletonIdentity};
use super::keyed::{Key, Keyed};
use super::measure;
use super::shipped_file::ShippedFile;
use super::template::{Fill, Partial};
use super::walk::TreePath;

/// A skeleton that has passed every check a skeleton can fail on its own.
#[derive(Debug)]
pub(crate) struct ValidatedSkeleton {
    declarations: Declarations,
    files: BTreeMap<TreePath, ShippedFile>,
    partials: Keyed<Partial, Partial>,
}

impl ValidatedSkeleton {
    /// Runs the checks left once every file and partial has parsed, in
    /// order — every declared option is used by something, then the largest
    /// render any choice could produce fits the render's size limit — and
    /// returns the skeleton that passed them.
    ///
    /// `partials` are the ones parsed from the same walked `partials/` paths
    /// `declarations` were linked against, key for key.
    ///
    /// # Panics
    ///
    /// If `partials` and the declarations' `set` values do not pair off one
    /// to one, which linking already proved they do: see
    /// [`super::declarations::Schema::link`]. Or if `files` holds a verbatim
    /// file the declarations do not, or the reverse, which reading the files
    /// against those same declarations already made impossible.
    pub(crate) fn validate(
        skeleton: &SkeletonIdentity,
        declarations: Declarations,
        files: BTreeMap<TreePath, ShippedFile>,
        partials: Keyed<Partial, Partial>,
    ) -> Result<Self, RenderError> {
        // Precondition, paired with linking's own postcondition: every
        // `set` value holds the key of exactly one partial, and every
        // partial is held by exactly one value.
        assert_eq!(
            partials.len(),
            declarations.set_values().len(),
            "every partial pairs with exactly one set value"
        );
        // Precondition, paired with the read that built `files` from the
        // declarations: a file is verbatim exactly when the declarations
        // say so.
        for (path, file) in &files {
            let is_verbatim_file = matches!(file, ShippedFile::Verbatim(_));
            assert_eq!(
                is_verbatim_file,
                declarations.is_verbatim(path),
                "`files/{path}` is held verbatim exactly when the declarations say it is"
            );
        }
        let validated = Self {
            declarations,
            files,
            partials,
        };
        check_options_used(skeleton, &validated)?;
        measure::check_largest(skeleton, &validated)?;
        Ok(validated)
    }

    pub(crate) const fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// Every file under `files/`, in path order: parsed, or held as the bytes
    /// it ships.
    pub(crate) const fn files(&self) -> &BTreeMap<TreePath, ShippedFile> {
        &self.files
    }

    /// Every partial under `partials/`, parsed, in path order.
    pub(crate) const fn partials(&self) -> &Keyed<Partial, Partial> {
        &self.partials
    }

    /// The partial a `set` value selects.
    pub(crate) fn partial(&self, key: Key<Partial>) -> &Partial {
        &self.partials[key]
    }
}

/// Every declared option must be used by something in the skeleton: an `enum`
/// or a `text` by at least one placeholder in some partial or in some file
/// not declared verbatim, a `set` by at least one directive in some file not
/// declared verbatim (a partial can never hold a directive, so there is
/// nowhere else for a set option to be used). Nothing in a verbatim file is
/// read, so nothing in one uses an option. An
/// option nothing reaches would let a wearer set a value that changes
/// nothing, which is the same closedness argument as a partial no value
/// selects.
fn check_options_used(
    skeleton: &SkeletonIdentity,
    validated: &ValidatedSkeleton,
) -> Result<(), RenderError> {
    let declarations = validated.declarations();
    let mut enum_used = declarations.enum_options().map(|_key, _option| false);
    let mut text_used = declarations.text_options().map(|_key, _option| false);
    let mut set_used = declarations.set_options().map(|_key, _option| false);

    let mut mark_used = |fill: Fill| match fill {
        Fill::Enum(option) => enum_used[option] = true,
        Fill::Text(option) => text_used[option] = true,
    };
    for file in validated.files().values() {
        match file {
            ShippedFile::Templated(template) => {
                template
                    .used_fill_options()
                    .into_iter()
                    .for_each(&mut mark_used);
                for option in template.used_set_options() {
                    set_used[option] = true;
                }
            }
            // A verbatim file is never scanned, so nothing in it uses an
            // option, whatever text it holds.
            ShippedFile::Verbatim(_bytes) => {}
        }
    }
    for (_key, partial) in validated.partials().iter() {
        partial
            .used_fill_options()
            .into_iter()
            .for_each(&mut mark_used);
    }

    for (name, declared) in declarations.names() {
        let is_used = match declared {
            Declared::Enum(option) => enum_used[option],
            Declared::Text(option) => text_used[option],
            Declared::Set(option) => set_used[option],
        };
        if !is_used {
            return Err(RenderError::about_manifest(
                skeleton.clone(),
                Reason::OptionUnused {
                    option: name.as_str().to_owned(),
                },
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ValidatedSkeleton;
    use crate::skeleton::declarations::Declarations;
    use crate::skeleton::error::{Reason, SkeletonIdentity};
    use crate::skeleton::keyed::KeyedBuilder;
    use crate::skeleton::shipped_file::ShippedFile;
    use crate::skeleton::walk::TreePath;

    #[test]
    #[should_panic(expected = "every partial pairs with exactly one set value")]
    fn validating_partials_that_do_not_pair_with_the_set_values_panics() {
        // Linking proves every `set` value names exactly one partial and
        // every partial is named once; validating re-asserts it against the
        // partials actually handed in. Handing in none, for declarations
        // with one linked value, is a bug in the caller, not a skeleton to
        // refuse.
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
        let _never_built = ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            BTreeMap::new(),
            KeyedBuilder::new().finish(),
        );
    }

    #[test]
    fn a_placeholder_in_a_verbatim_file_leaves_its_option_unused() {
        // Verifies that a verbatim file is never scanned for the placeholder
        // it holds: `note` is named only inside the verbatim file, which
        // therefore does not use it, and validation must refuse it as an
        // option nothing uses. Exercised by validating one verbatim file
        // whose bytes are a well-formed `{{note}}` line.
        let declarations = Declarations::for_test_verbatim(
            r#"
            [options.note]
            type = "text"
            "#,
            &[],
            &["ci.yml"],
        );
        let files = BTreeMap::from([(
            TreePath::for_test(&["ci.yml"]),
            ShippedFile::Verbatim(b"{{note}}\n".to_vec()),
        )]);

        let error = ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            files,
            KeyedBuilder::new().finish(),
        )
        .expect_err("an option named only in a verbatim file is used by nothing");

        assert!(
            matches!(error.reason(), Reason::OptionUnused { option } if option == "note"),
            "expected `note` refused as unused, got {error:?}"
        );
    }

    #[test]
    #[should_panic(expected = "held verbatim exactly when the declarations say it is")]
    fn validating_a_verbatim_file_the_declarations_do_not_name_panics() {
        // Reading builds the files from the declarations, so a verbatim file
        // the declarations do not name is a bug in the caller, not a
        // skeleton to refuse.
        let declarations = Declarations::for_test("", &[]);
        let files = BTreeMap::from([(
            TreePath::for_test(&["ci.yml"]),
            ShippedFile::Verbatim(Vec::new()),
        )]);
        let _never_built = ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            files,
            KeyedBuilder::new().finish(),
        );
    }
}
