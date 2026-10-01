//! Assembling parsed templates and partials, with a resolved set of option
//! values, into the bytes a wearing repository should hold.

use std::collections::BTreeMap;

use super::choices::Resolved;
use super::declarations::SetOption;
use super::keyed::Key;
use super::measure::{self, DirectiveTotals, Sizing};
use super::shipped_file::ShippedFile;
use super::template::{Fill, Piece, Template, TemplateLine};
use super::validated::ValidatedSkeleton;
use super::walk::TreePath;

/// Every file under a skeleton's `files/`, rendered, keyed by its path relative
/// to `files/` with `/` separators (for example `.github/dependabot.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rendering(BTreeMap<TreePath, Vec<u8>>);

impl Rendering {
    /// The rendered bytes of the file at `path` (relative to `files/`), if
    /// the skeleton ships one there.
    ///
    /// Test-only: `check`/`sync` never look a claimed file up by path — they
    /// walk every rendered file through [`Self::iter`] instead — so this
    /// exists only for tests (this module's, `skeleton`'s and the render's
    /// acceptance suite), which is what `#[cfg(test)]` says outright rather
    /// than hiding behind a module-wide dead-code expectation.
    #[cfg(test)]
    pub(crate) fn get(&self, path: &str) -> Option<&[u8]> {
        self.0.get(path).map(Vec::as_slice)
    }

    /// Every rendered file as `(path, bytes)`, in ascending byte order of
    /// the path string. The order never depends on how the filesystem lists
    /// entries: it is a `BTreeMap`'s own iteration order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.0
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }
}

/// Assembles every parsed file of `validated` into a [`Rendering`], filling
/// placeholders and expanding directives from `resolved`.
pub(crate) fn assemble(validated: &ValidatedSkeleton, resolved: &Resolved) -> Rendering {
    // Precomputed once for the whole render, not once per file: see
    // `measure::directive_totals`. Only ever read to predict each file's
    // length, which `assemble_one` checks its output against.
    let totals = measure::directive_totals(validated, Sizing::Chosen(resolved));

    let mut rendered = BTreeMap::new();
    for (path, file) in validated.files() {
        rendered.insert(
            path.clone(),
            assemble_one(validated, resolved, file, totals.as_ref()),
        );
    }
    Rendering(rendered)
}

/// Assembles one file into its rendered bytes: a template is filled and
/// expanded, a verbatim file is its own bytes.
fn assemble_one(
    validated: &ValidatedSkeleton,
    resolved: &Resolved,
    file: &ShippedFile,
    totals: Option<&DirectiveTotals>,
) -> Vec<u8> {
    let output = match file {
        ShippedFile::Templated(template) => assemble_template(validated, resolved, template),
        ShippedFile::Verbatim(bytes) => bytes.clone(),
    };

    // Postcondition: the same sizing that `check_largest` bounds this
    // skeleton with predicts exactly the bytes assembly produced, for this
    // wearer's choices too — one model of a render's length, checked
    // against the render itself every time. A prediction that overflowed
    // is `None`, which is just another prediction that disagrees.
    let predicted = totals.and_then(|totals| {
        measure::rendered_length(
            file,
            validated.declarations(),
            Sizing::Chosen(resolved),
            totals,
        )
    });
    assert_eq!(
        predicted,
        Some(output.len() as u64),
        "the assembled output must have exactly the length its sizing predicted"
    );
    output
}

/// Assembles one file's [`Template`] into its rendered bytes.
fn assemble_template(
    validated: &ValidatedSkeleton,
    resolved: &Resolved,
    template: &Template,
) -> Vec<u8> {
    let mut output = String::new();
    let mut touched_by_fill_or_select = false;

    for line in &template.lines {
        match line {
            TemplateLine::Text(text_line) => {
                // Counted whether or not the line survives: a template whose
                // only placeholders sit on dropped lines still had fill change
                // its bytes, so it is not held to equal its own source.
                touched_by_fill_or_select |= text_line
                    .pieces()
                    .iter()
                    .any(|piece| matches!(piece, Piece::Placeholder(_)));
                if measure::line_is_present(text_line, Sizing::Chosen(resolved)) {
                    emit_pieces(&mut output, text_line.pieces(), resolved);
                    output.push_str(text_line.terminator());
                }
            }
            TemplateLine::Directive {
                indentation,
                option,
                terminator: _,
            } => {
                touched_by_fill_or_select = true;
                emit_directive(&mut output, validated, resolved, indentation, *option);
            }
        }
    }

    // Postcondition: fill and select are the only two things a render can
    // change about a file's bytes, so a template that used neither must
    // produce exactly the source it was parsed from: a file with no
    // placeholders and no directive renders byte-for-byte identical to its
    // source (tested by `crates/skeletons/src/acceptance/text.rs` →
    // `a_file_with_no_placeholders_or_directives_renders_byte_for_byte_identical`).
    if !touched_by_fill_or_select {
        assert_eq!(
            output, template.source,
            "a template with no placeholder or directive must render equal to its source"
        );
    }
    output.into_bytes()
}

fn emit_pieces(output: &mut String, pieces: &[Piece], resolved: &Resolved) {
    for piece in pieces {
        match piece {
            Piece::Literal(text) => output.push_str(text),
            // One pass only: the value is pushed verbatim, never rescanned
            // for a placeholder or directive of its own, and that holds for
            // a wearer's text as much as for a declared value.
            Piece::Placeholder(Fill::Enum(option)) => {
                output.push_str(resolved.enum_value(*option).as_str());
            }
            Piece::Placeholder(Fill::Text(option)) => {
                let Some(text) = resolved.text_value(*option) else {
                    unreachable!(
                        "an optional text is unset only on a line assembly has already dropped"
                    )
                };
                output.push_str(text.as_str());
            }
        }
    }
}

/// Emits a directive's selected partials, in the option's declared order,
/// each partial's own lines indented to the directive's own leading
/// whitespace. The directive line itself contributes nothing beyond this.
fn emit_directive(
    output: &mut String,
    validated: &ValidatedSkeleton,
    resolved: &Resolved,
    indentation: &str,
    option: Key<SetOption>,
) {
    let declarations = validated.declarations();
    for &value in declarations.set_options()[option].values() {
        if !resolved.is_selected(value) {
            continue;
        }
        let partial = validated.partial(declarations.set_values()[value].partial());
        for partial_line in &partial.lines {
            if !measure::line_is_present(partial_line, Sizing::Chosen(resolved)) {
                continue;
            }
            if partial_line.pieces().is_empty() {
                // A truly blank line stays blank: rendering never adds
                // whitespace to it.
                output.push_str(partial_line.terminator());
                continue;
            }
            output.push_str(indentation);
            emit_pieces(output, partial_line.pieces(), resolved);
            output.push_str(partial_line.terminator());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::choices::{Choice, Choices, resolve};
    use crate::skeleton::declarations::Declarations;
    use crate::skeleton::error::SkeletonIdentity;
    use crate::skeleton::keyed::KeyedBuilder;
    use crate::skeleton::template::{parse_file, parse_partial};

    /// A validated skeleton declaring `toml_text`, shipping `files` and
    /// `partials` as `(path, text)` pairs — built in memory, since what
    /// these tests exercise is assembly, not reading a directory.
    fn validated_skeleton(
        toml_text: &str,
        files: &[(&str, &str)],
        partials: &[(&str, &str)],
    ) -> ValidatedSkeleton {
        let partial_names: Vec<&str> = partials.iter().map(|(path, _text)| *path).collect();
        let (declarations, partial_paths) =
            Declarations::for_test_with_paths(toml_text, &partial_names);
        let parsed_partials = partial_paths.map(|_key, path| {
            let (_path, text) = partials
                .iter()
                .find(|(name, _text)| *name == path.as_str())
                .expect("every partial path comes from `partials`");
            parse_partial(text, &declarations).expect("partial parses")
        });
        let mut parsed_files = BTreeMap::new();
        for (path, text) in files {
            let components: Vec<&str> = path.split('/').collect();
            let template = parse_file(text, &declarations).expect("template parses");
            parsed_files.insert(
                TreePath::for_test(&components),
                ShippedFile::Templated(template),
            );
        }
        ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            parsed_files,
            parsed_partials,
        )
        .expect("test skeleton validates")
    }

    /// As [`validated_skeleton`], also shipping each of `verbatim` as
    /// `(path, bytes)`, declared verbatim, beside the templated `files`.
    fn validated_with_verbatim(
        toml_text: &str,
        files: &[(&str, &str)],
        verbatim: &[(&str, &[u8])],
    ) -> ValidatedSkeleton {
        let verbatim_paths: Vec<&str> = verbatim.iter().map(|(path, _bytes)| *path).collect();
        let declarations = Declarations::for_test_verbatim(toml_text, &[], &verbatim_paths);
        let mut shipped = BTreeMap::new();
        for (path, text) in files {
            let components: Vec<&str> = path.split('/').collect();
            let template = parse_file(text, &declarations).expect("template parses");
            shipped.insert(
                TreePath::for_test(&components),
                ShippedFile::Templated(template),
            );
        }
        for (path, bytes) in verbatim {
            let components: Vec<&str> = path.split('/').collect();
            shipped.insert(
                TreePath::for_test(&components),
                ShippedFile::Verbatim(bytes.to_vec()),
            );
        }
        ValidatedSkeleton::validate(
            &SkeletonIdentity::Named("test".to_owned()),
            declarations,
            shipped,
            KeyedBuilder::new().finish(),
        )
        .expect("test skeleton validates")
    }

    /// `validated` assembled for `choices`.
    fn assemble_for(validated: &ValidatedSkeleton, choices: &Choices) -> Rendering {
        let resolved = resolve(
            &SkeletonIdentity::Named("test".to_owned()),
            validated.declarations(),
            choices,
        )
        .expect("resolves");
        assemble(validated, &resolved)
    }

    const WORKFLOWS: &str = r#"
        [options.workflows]
        type = "set"
        default = []
        [[options.workflows.values]]
        value = "lint"
        partial = "lint.yml"
        "#;

    #[test]
    fn a_directive_with_nothing_selected_vanishes_with_no_blank_line() {
        let validated = validated_skeleton(
            WORKFLOWS,
            &[("file.txt", "before\n# skeletons:partial workflows\nafter\n")],
            &[("lint.yml", "lint-line\n")],
        );
        let rendering = assemble_for(&validated, &Choices::new());
        assert_eq!(
            rendering.get("file.txt"),
            Some(b"before\nafter\n".as_slice()),
            "an empty default selection must remove the directive line and add nothing in its place"
        );
    }

    #[test]
    fn choosing_a_value_selects_its_partial_indented_to_the_directive() {
        let validated = validated_skeleton(
            WORKFLOWS,
            &[(
                "file.txt",
                "before\n  # skeletons:partial workflows\nafter\n",
            )],
            &[("lint.yml", "lint-line\n\nnext-line\n")],
        );
        let mut choices = Choices::new();
        choices.insert("workflows", Choice::Many(vec!["lint".to_owned()]));
        let rendering = assemble_for(&validated, &choices);
        assert_eq!(
            rendering.get("file.txt"),
            Some(b"before\n  lint-line\n\n  next-line\nafter\n".as_slice()),
            "the partial's non-blank lines gain the directive's indentation; the blank line does not"
        );
    }

    /// One optional `text`, `note`, and nothing else.
    const NOTE_ONLY: &str = r#"
        [options.note]
        type = "text"
        "#;

    /// An optional `text`, `note`, and one with a default, `owner`.
    const NOTE_AND_OWNER: &str = r#"
        [options.note]
        type = "text"

        [options.owner]
        type = "text"
        default = "octocat"
        "#;

    /// A `set` selecting one partial by default.
    const WORKFLOWS_ONLY: &str = r#"
        [options.workflows]
        type = "set"
        default = ["lint"]
        [[options.workflows.values]]
        value = "lint"
        partial = "lint.yml"
        "#;

    /// Choices setting the `text` option `note` to `value`.
    fn note(value: &str) -> Choices {
        let mut choices = Choices::new();
        choices.insert("note", Choice::One(value.to_owned()));
        choices
    }

    #[test]
    fn a_file_made_entirely_of_optional_lines_renders_as_empty_when_unset() {
        // Every line is droppable, so nothing survives: the render is empty,
        // and the length its sizing predicted for it, zero, holds too.
        let validated = validated_skeleton(
            NOTE_ONLY,
            &[("file.txt", "{{note}}\n[{{note}}]\n{{note}}")],
            &[],
        );
        assert_eq!(
            assemble_for(&validated, &Choices::new()).get("file.txt"),
            Some(b"".as_slice())
        );
        assert_eq!(
            assemble_for(&validated, &note("hi")).get("file.txt"),
            Some(b"hi\n[hi]\nhi".as_slice())
        );
    }

    #[test]
    fn an_optional_line_between_blank_lines_in_a_partial_leaves_the_blanks() {
        // The dropped line takes its terminator and nothing else with it: the
        // blank lines either side stay blank and unindented, and the lines
        // around them keep the directive's indentation.
        let validated = validated_skeleton(
            &format!("{NOTE_ONLY}{WORKFLOWS_ONLY}"),
            &[("file.txt", "top\n  # skeletons:partial workflows\nbottom\n")],
            &[("lint.yml", "first\n\n{{note}}\n\nlast\n")],
        );
        assert_eq!(
            assemble_for(&validated, &Choices::new()).get("file.txt"),
            Some(b"top\n  first\n\n\n  last\nbottom\n".as_slice())
        );
        assert_eq!(
            assemble_for(&validated, &note("hi")).get("file.txt"),
            Some(b"top\n  first\n\n  hi\n\n  last\nbottom\n".as_slice())
        );
    }

    #[test]
    fn a_defaulted_text_line_is_never_dropped() {
        let validated = validated_skeleton(
            NOTE_AND_OWNER,
            &[
                ("file.txt", "owner: {{owner}}\n"),
                ("other.txt", "{{note}}\n"),
            ],
            &[],
        );
        let rendering = assemble_for(&validated, &Choices::new());
        assert_eq!(
            rendering.get("file.txt"),
            Some(b"owner: octocat\n".as_slice())
        );
        assert_eq!(rendering.get("other.txt"), Some(b"".as_slice()));
    }

    #[test]
    fn a_text_value_is_written_verbatim_even_when_it_looks_like_grammar() {
        let validated = validated_skeleton(
            &format!("{NOTE_AND_OWNER}{WORKFLOWS_ONLY}"),
            &[(
                "file.txt",
                "{{note}}\n{{owner}}\n# skeletons:partial workflows\n",
            )],
            &[("lint.yml", "lint-line\n")],
        );
        // A value that reopened the grammar would show as a second fill or a
        // second partial after it.
        for value in ["{{owner}}", "# skeletons:partial workflows", "}}{{"] {
            assert_eq!(
                assemble_for(&validated, &note(value)).get("file.txt"),
                Some(format!("{value}\noctocat\nlint-line\n").as_bytes()),
                "{value:?}"
            );
        }
    }

    mod property {
        use proptest::prelude::*;

        use super::{Choices, NOTE_ONLY, assemble_for, note, validated_skeleton};

        proptest! {
            // A three-line file whose middle line is optional, rendered with
            // the option set to a random printable string of one to 300
            // characters, or left unset. The expected bytes are rebuilt here
            // from the three lines directly, sharing nothing with assembly,
            // so the property is that assembly drops the middle line exactly
            // when the option is unset and fills it verbatim otherwise. The
            // predicted-length postcondition inside assembly runs on every
            // case too.
            #[test]
            fn an_optional_line_is_dropped_exactly_when_its_option_is_unset(
                value in "[ -~\\u{a1}-\\u{2ff}]{1,300}",
                is_set in any::<bool>(),
            ) {
                let validated = validated_skeleton(
                    NOTE_ONLY,
                    &[("file.txt", "head\nmiddle: {{note}}\ntail\n")],
                    &[],
                );
                let choices = if is_set { note(&value) } else { Choices::new() };
                let rendering = assemble_for(&validated, &choices);

                let expected = if is_set {
                    format!("head\nmiddle: {value}\ntail\n")
                } else {
                    "head\ntail\n".to_owned()
                };
                prop_assert_eq!(
                    rendering.get("file.txt"),
                    Some(expected.as_bytes())
                );
            }
        }
    }

    #[test]
    fn rendering_iterates_in_ascending_path_order() {
        let validated = validated_skeleton(
            "",
            &[("Z.txt", "x\n"), ("a.txt", "x\n"), ("m.txt", "x\n")],
            &[],
        );
        let rendering = assemble_for(&validated, &Choices::new());
        let paths: Vec<&str> = rendering.iter().map(|(path, _bytes)| path).collect();
        assert_eq!(paths, vec!["Z.txt", "a.txt", "m.txt"]);
    }

    #[test]
    fn get_finds_every_rendered_path_and_nothing_else() {
        // Exercises `Rendering::get`'s `BTreeMap::get` lookup by `&str`
        // (via `TreePath`'s `Borrow<str>`) directly against a mix of flat,
        // nested and dotted paths, rather than only through a rendered
        // skeleton. Guards against a key type whose `Ord` disagrees with
        // `str`'s own, which would make some of these paths unreachable by
        // text even though they are present.
        let validated = validated_skeleton(
            "",
            &[
                (".github/dependabot.yml", "dotdir\n"),
                ("Z.txt", "upper\n"),
                ("a.txt", "flat\n"),
                ("a/b/deep.yml", "nested\n"),
                ("ab/c.txt", "sibling-prefix\n"),
                ("root.yml", "root\n"),
            ],
            &[],
        );
        let rendering = assemble_for(&validated, &Choices::new());

        for (path, content) in [
            (".github/dependabot.yml", "dotdir\n"),
            ("Z.txt", "upper\n"),
            ("a.txt", "flat\n"),
            ("a/b/deep.yml", "nested\n"),
            ("ab/c.txt", "sibling-prefix\n"),
            ("root.yml", "root\n"),
        ] {
            assert_eq!(
                rendering.get(path),
                Some(content.as_bytes()),
                "{path} must be found with its own rendered bytes"
            );
        }
        for absent in ["A.txt", "a", "a/b", ".github", "", "root.yml/"] {
            assert_eq!(
                rendering.get(absent),
                None,
                "{absent} must not be found: it is not one of the rendered paths"
            );
        }
    }

    #[test]
    fn a_verbatim_file_renders_its_bytes_though_it_holds_a_brace_pair_and_directives() {
        // Verifies that a verbatim file is emitted exactly as read: it holds
        // a GitHub Actions expression (`${{`), a well-formed directive line
        // and a directive hidden behind a no-break space, and none of them
        // is filled, expanded or refused. Exercised by rendering it beside a
        // templated file that keeps the `text` option used.
        let source = "TOKEN: ${{ secrets.TOKEN }}\n\
                      # skeletons:partial workflows\n\
                      \u{a0}# skeletons:partial workflows\n\
                      no final newline";
        let validated = validated_with_verbatim(
            NOTE_ONLY,
            &[("templated.txt", "{{note}}\n")],
            &[(".github/workflows/ci.yml", source.as_bytes())],
        );

        let rendering = assemble_for(&validated, &Choices::new());

        assert_eq!(
            rendering.get(".github/workflows/ci.yml"),
            Some(source.as_bytes()),
            "a verbatim file must render as exactly its own bytes"
        );
    }

    #[test]
    fn a_verbatim_file_that_is_not_utf8_renders() {
        // Verifies that a verbatim file need not be text: its bytes hold a
        // NUL, a lone continuation byte and an invalid lead byte, and render
        // unchanged. Exercised by rendering it beside a templated file.
        let bytes: &[u8] = &[0x00, b'{', b'{', 0x80, 0xFF, 0xFE, b'\n'];
        let validated = validated_with_verbatim(
            NOTE_ONLY,
            &[("templated.txt", "{{note}}\n")],
            &[("blob.bin", bytes)],
        );

        let rendering = assemble_for(&validated, &Choices::new());

        assert_eq!(rendering.get("blob.bin"), Some(bytes));
    }
}
