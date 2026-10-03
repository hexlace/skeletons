//! Select grammar: recognising and parsing a `# skeletons:partial <name>`
//! directive line.

use unicode_properties::{GeneralCategory, UnicodeGeneralCategory as _};

use super::name::OptionName;

/// The text that marks a line as directive-shaped, once its leading spaces
/// and tabs are removed.
const MARKER: &str = "# skeletons:";

/// The text a well-formed directive begins with, immediately followed by its
/// option name and nothing else.
const KEYWORD: &str = "# skeletons:partial ";

/// The marker's own literal characters, matched one at a time by
/// [`scan_marker_shape`]: `#`, then `skeletons`, then `:`, with nothing of its
/// own standing between them.
const MARKER_LITERAL: [char; 11] = ['#', 's', 'k', 'e', 'l', 'e', 't', 'o', 'n', 's', ':'];

/// A well-formed select directive: which option it names, and the
/// indentation a selected partial's lines will be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Directive {
    /// The line's leading spaces and tabs, kept verbatim.
    pub(crate) indentation: String,
    /// The `set` option the directive selects from.
    pub(crate) option: OptionName,
}

/// What one line's content was recognised as, once directive scanning has
/// looked at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectiveLine {
    /// The line's text never normalises to the marker at all: setting aside
    /// its leading spaces and tabs, or every invisible character before and
    /// within where the marker would sit, never turns up `#`, then `skeletons`,
    /// then `:` — ordinary text, unrelated to a directive.
    Text,
    /// The line's text normalises to the marker only because an invisible
    /// character other than a space or a tab — a no-break space, a
    /// zero-width space, a stray control byte — takes part before or inside
    /// it: in front of `# skeletons:`, between `#` and `skeletons`, strictly inside
    /// `skeletons` itself, or right before `:`. Neither text, since its author
    /// may have meant a directive, nor a directive, since a directive's own
    /// marker holds nothing but spaces and tabs.
    Hidden,
    /// The line's text, after its leading spaces and tabs, begins `# skeletons:`
    /// but is not exactly `# skeletons:partial <option-name>`; or it normalises
    /// to the marker with nothing invisible in play at all, only ordinary
    /// ASCII spacing gone wrong (`#skeletons:`, `#  skeletons:`, `# skeletons :`).
    Malformed,
    /// A well-formed directive.
    Directive(Directive),
}

/// Recognises `content` (one line, with no terminator) as directive-shaped
/// or not, and parses it when it is.
///
/// A line whose text, after its leading spaces and tabs, begins `# skeletons:`
/// is directive-shaped and must then be exactly `# skeletons:partial ` followed
/// by one well-formed option name and nothing else before its line
/// terminator — a misspelt keyword, a missing or extra argument, or trailing
/// text after the name is [`DirectiveLine::Malformed`], never treated as an
/// ordinary comment.
///
/// Otherwise, `content` is checked against the marker's *normalised* shape
/// (see [`scan_marker_shape`]): with every leading whitespace character
/// dropped, every format (Cf) or non-whitespace control (Cc) character
/// dropped wherever it sits, and any run of whitespace elsewhere read as at
/// most one space, does it begin `#`, then at most one space, then `skeletons`,
/// then at most one space, then `:`? A line that reaches this shape only by
/// looking past an invisible character other than a space or a tab is
/// [`DirectiveLine::Hidden`] — see [`is_invisible`]. One that reaches it
/// with nothing invisible in play is [`DirectiveLine::Malformed`]: only
/// ordinary ASCII spacing is wrong (`#skeletons:partial`, once nothing
/// invisible excuses the missing space), still a directive its author
/// meant, never a comment. A different case (`# Skeletons:partial`) normalises
/// to nothing the marker recognises at all, and is
/// [`DirectiveLine::Text`].
pub(crate) fn scan(content: &str) -> DirectiveLine {
    // Precondition: this scans one line at a time; a directive can never
    // span two lines, so the content handed in never holds a terminator.
    assert!(
        !content.contains('\n'),
        "a line's content never holds a newline"
    );

    if is_directive_shaped(content) {
        return scan_exact_marker(content);
    }

    let outcome = match scan_marker_shape(content) {
        Some(shape) if shape.offending_invisible => DirectiveLine::Hidden,
        Some(_) => DirectiveLine::Malformed,
        None => DirectiveLine::Text,
    };
    // Postcondition: a directive-shaped line already returned above, from
    // `scan_exact_marker`, so `Hidden` -- reserved for a marker recognised
    // only by looking past something other than a plain space or tab -- is
    // never the outcome for a line the exact marker itself already
    // recognises.
    if outcome == DirectiveLine::Hidden {
        assert!(
            !is_directive_shaped(content),
            "a directive-shaped line must never be classified Hidden"
        );
    }
    outcome
}

/// Whether `content`, after its leading spaces and tabs, begins `# skeletons:`.
fn is_directive_shaped(content: &str) -> bool {
    content.trim_start_matches([' ', '\t']).starts_with(MARKER)
}

/// Parses `content`, already known to be directive-shaped by
/// [`is_directive_shaped`], into a well-formed directive or
/// [`DirectiveLine::Malformed`], reading the marker exactly and not through
/// normalisation: a directive's own indentation and marker are its leading
/// spaces and tabs, then `# skeletons:`, nothing more.
fn scan_exact_marker(content: &str) -> DirectiveLine {
    // Precondition, paired with the caller's own check: this never runs on
    // a line that is not already directive-shaped.
    assert!(
        is_directive_shaped(content),
        "scan_exact_marker requires an already directive-shaped line"
    );

    let trimmed = content.trim_start_matches([' ', '\t']);
    let indentation_length = content.len() - trimmed.len();
    let indentation = &content[..indentation_length];

    let Some(candidate) = trimmed.strip_prefix(KEYWORD) else {
        return DirectiveLine::Malformed;
    };
    let Some(option) = OptionName::parse(candidate) else {
        return DirectiveLine::Malformed;
    };

    // Postcondition: the indentation carried forward is exactly the leading
    // run of spaces and tabs `trim_start_matches` above found — never a
    // byte of anything else.
    assert!(
        indentation
            .chars()
            .all(|character| character == ' ' || character == '\t'),
        "a directive's indentation must hold only spaces and tabs"
    );
    DirectiveLine::Directive(Directive {
        indentation: indentation.to_owned(),
        option,
    })
}

/// What scanning `content` for the marker's normalised shape found.
struct MarkerShape {
    /// Whether recognising the marker needed to look past a character of
    /// the invisible set other than a plain space or tab — the line between
    /// [`DirectiveLine::Hidden`] and [`DirectiveLine::Malformed`].
    offending_invisible: bool,
}

/// Whether, with `matched` literal characters of [`MARKER_LITERAL`] already
/// consumed, the marker tolerates one space next: right after `#` (`matched
/// == 1`), and right after `skeletons` (`matched == 10`) — never in the middle of
/// `skeletons` itself, and never anywhere else.
const fn tolerates_a_space_after(matched: usize) -> bool {
    matched == 1 || matched == 10
}

/// Recognises `content` as directive-shaped under normalisation: from its
/// start through the end of its marker, with every leading whitespace
/// character dropped entirely, every format (Cf) or non-whitespace control
/// (Cc) character dropped wherever it sits, and any run of whitespace
/// elsewhere read as at most one space, does it begin `#`, then at most one
/// space, then `skeletons`, then at most one space, then `:`?
///
/// Returns `None` when it does not — including when whitespace sits where
/// the marker allows none, such as inside `skeletons` itself, since collapsing
/// that run would still leave a space the marker has no place for. When it
/// matches, [`MarkerShape::offending_invisible`] says whether anything
/// other than a plain space or a tab took part in reaching that match, up
/// to and including the marker's own closing `:` — nothing past it is ever
/// looked at.
fn scan_marker_shape(content: &str) -> Option<MarkerShape> {
    // Precondition, paired with `scan`'s own: a directive can never span two
    // lines, so this never walks a newline either.
    assert!(
        !content.contains('\n'),
        "a line's content never holds a newline"
    );

    let mut matched = 0;
    let mut in_leading_whitespace = true;
    let mut offending_invisible = false;

    for character in content.chars() {
        if is_offending_invisible(character) {
            offending_invisible = true;
        }

        if is_dropped_for_shape(character) {
            continue;
        }

        if character.is_whitespace() {
            if in_leading_whitespace {
                continue;
            }
            if !tolerates_a_space_after(matched) {
                return None;
            }
            continue;
        }

        in_leading_whitespace = false;
        if character != MARKER_LITERAL[matched] {
            return None;
        }
        matched += 1;
        // Postcondition: indexing `MARKER_LITERAL` above never runs past its
        // end, since reaching its length returns immediately below.
        assert!(
            matched <= MARKER_LITERAL.len(),
            "matched cannot exceed the marker's own length"
        );
        if matched == MARKER_LITERAL.len() {
            return Some(MarkerShape {
                offending_invisible,
            });
        }
    }
    None
}

/// Whether `character` is dropped outright when scanning for the marker's
/// shape: a format character (general category Cf) or a control character
/// (general category Cc) that is not already whitespace. A whitespace Cc
/// character — a tab, a vertical tab, a form feed — is not dropped here; it
/// takes part in the run-collapsing spacing rule in [`scan_marker_shape`]
/// instead, the same as a no-break space does.
fn is_dropped_for_shape(character: char) -> bool {
    !character.is_whitespace() && is_invisible(character)
}

/// Whether `character` is a member of the invisible set other than a plain
/// space or a tab — the sign, wherever it takes part in reaching the
/// marker's shape, that a directive-shaped line is [`DirectiveLine::Hidden`]
/// rather than merely [`DirectiveLine::Malformed`].
fn is_offending_invisible(character: char) -> bool {
    is_invisible(character) && character != ' ' && character != '\t'
}

/// Whether `character` is invisible by this module's rule: whitespace by
/// [`char::is_whitespace`], or Unicode general category Cf (format) — the
/// zero-width space, the word joiner, the byte-order mark — or Cc (control)
/// — a stray control byte, such as `\x01`, that carries no whitespace
/// property of its own. Space and tab are members of this set: they are
/// exactly the two characters a directive's own indentation, and the
/// spacing inside a well-formed marker, are allowed to use.
fn is_invisible(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character.general_category(),
            GeneralCategory::Format | GeneralCategory::Control
        )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory as _};

    use super::{DirectiveLine, OptionName, is_invisible, scan};

    /// The rule as it is worded, written out here apart from
    /// [`is_invisible`] so the tests below can hold the predicate to it:
    /// `char::is_whitespace`, Unicode general category Cf, or general
    /// category Cc.
    fn ruled_invisible(character: char) -> bool {
        character.is_whitespace()
            || matches!(
                character.general_category(),
                GeneralCategory::Format | GeneralCategory::Control
            )
    }

    /// Every character the rule calls invisible that can stand inside one
    /// line's content — every one but `\n`, which ends a line and so never
    /// reaches [`scan`]. The alphabet the hidden-directive properties below
    /// draw their prefixes from: every such character the Unicode data
    /// knows of, not a list written by hand, and not read off the predicate
    /// under test, so a predicate that drops half the rule meets characters
    /// it no longer hides.
    fn invisible_characters_within_a_line() -> Vec<char> {
        (char::MIN..=char::MAX)
            .filter(|&character| ruled_invisible(character))
            .filter(|&character| character != '\n')
            .collect()
    }

    /// A small, fixed alphabet of ordinary ASCII characters, chosen by hand
    /// because none of them is `#`, a letter of `skeletons`, `:`, or anything
    /// whitespace, Cf or Cc — never drawn from [`is_invisible`] or the
    /// marker-recognising code under test, so a property built from it
    /// cannot pass merely because it and the code under test agree on what
    /// counts as invisible.
    fn noise_character() -> impl Strategy<Value = char> {
        proptest::sample::select(vec![
            'q', 'z', 'j', 'k', 'Q', 'Z', '7', '9', '$', '%', '^', '_',
        ])
    }

    /// A fixed spread of invisible characters, drawn from each of
    /// `char::is_whitespace`, format (Cf) and non-whitespace control (Cc):
    /// a no-break space, a zero-width space, a word joiner, an ideographic
    /// space, a byte-order mark, a soft hyphen, and the control bytes
    /// `\x01`, a vertical tab, a form feed, `\x7f` and NEL. Fed to
    /// [`marker_biased_fragment`] below so the differential property that
    /// follows lands on and around the marker's shape far more often than a
    /// fully random string ever would.
    fn invisible_fragment() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(vec![
            "\u{a0}", "\u{200b}", "\u{2060}", "\u{3000}", "\u{feff}", "\u{ad}", "\x01", "\u{b}",
            "\u{c}", "\x7f", "\u{85}",
        ])
    }

    /// One fragment of a generated line: a piece of the marker itself,
    /// ordinary ASCII spacing, plain text that resembles or does not
    /// resemble a directive, or one of [`invisible_fragment`]'s characters.
    /// [`marker_biased_line`] concatenates a short, random sequence of
    /// these -- the generator's bias toward the marker's shape lives
    /// entirely in this fragment pool, not in any weighting of how the
    /// fragments are combined.
    fn marker_biased_fragment() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            proptest::sample::select(vec![
                "#",
                "# ",
                "skeletons",
                "s",
                "keletons",
                ":",
                "partial",
                " s",
                " ",
                "\t",
                "hello",
                "# Skeletons:",
            ]),
            invisible_fragment(),
        ]
    }

    /// A line built by concatenating a short, random sequence of
    /// [`marker_biased_fragment`]'s pieces.
    /// [`differential_line`] below draws its free-form minority of lines from
    /// this generator.
    fn marker_biased_line() -> impl Strategy<Value = String> {
        proptest::collection::vec(marker_biased_fragment(), 0..10)
            .prop_map(|fragments| fragments.concat())
    }

    /// A run of one to three ordinary ASCII spacing characters -- long
    /// enough that a joint filled with it exercises the marker's own
    /// collapsing of a whitespace run into at most one space, not only a
    /// single character standing alone.
    fn spacing_run() -> impl Strategy<Value = String> {
        proptest::collection::vec(proptest::sample::select(vec![' ', '\t']), 1..=3)
            .prop_map(|characters| characters.into_iter().collect())
    }

    /// A filler for one of the marker's own four joints -- before `#`,
    /// right after `#`, inside `skeletons`, and right before `:` -- weighted
    /// toward landing something there: an invisible character from
    /// [`invisible_fragment`] most often, a run of ordinary spacing
    /// sometimes, and nothing at all the rest of the time. This is where
    /// the bias toward the marker's own distinguishing shapes lives;
    /// [`marker_biased_line`] below instead concatenates fragments freely,
    /// with no joints, and is kept as a minority of the generated lines.
    fn marker_joint() -> impl Strategy<Value = String> {
        prop_oneof![
            2 => Just(String::new()),
            2 => spacing_run(),
            5 => invisible_fragment().prop_map(str::to_owned),
        ]
    }

    /// Zero to two joints concatenated, standing in for the marker's own
    /// leading run: dropped entirely by [`scan_marker_shape`] regardless of
    /// how many characters it holds, so its length matters less here than
    /// what is in it.
    fn leading_run() -> impl Strategy<Value = String> {
        proptest::collection::vec(marker_joint(), 0..3).prop_map(|joints| joints.join(""))
    }

    /// `skeletons` itself, whole or split once by a joint -- the marker's third
    /// joint, strictly inside the literal [`scan_marker_shape`] matches
    /// character by character, where only a dropped character, never plain
    /// spacing, can stand there without breaking the match outright.
    fn skeletons_fragment() -> impl Strategy<Value = String> {
        prop_oneof![
            1 => Just("skeletons".to_owned()),
            3 => marker_joint().prop_map(|joint| format!("s{joint}keletons")),
        ]
    }

    /// What follows the marker's closing `:`: a well-formed argument, a
    /// malformed one, or short arbitrary text -- so a generated line lands
    /// on every [`DirectiveLine`] variant, not only [`DirectiveLine::Hidden`]
    /// and [`DirectiveLine::Malformed`].
    fn marker_tail() -> impl Strategy<Value = String> {
        prop_oneof![
            proptest::sample::select(vec![
                "partial s",
                "partial ecosystems",
                "partial",
                "partail s",
                "partial ecosystems docker",
            ])
            .prop_map(str::to_owned),
            "[^\n]{0,10}",
        ]
    }

    /// A line built around the marker's own shape, with an invisible
    /// character from the fixed spread landing at each of its four joints
    /// -- before `#`, right after `#`, inside `skeletons`, and right before `:`
    /// -- with high probability, far more often than a fully random string
    /// would ever place one there. [`differential_line`] below draws most
    /// of its lines from this generator, and the rest from
    /// [`marker_biased_line`]'s free concatenation of fragments.
    fn marker_joint_biased_line() -> impl Strategy<Value = String> {
        (
            leading_run(),
            marker_joint(),
            skeletons_fragment(),
            marker_joint(),
            marker_tail(),
        )
            .prop_map(|(before_hash, after_hash, skeletons, before_colon, tail)| {
                format!("{before_hash}#{after_hash}{skeletons}{before_colon}:{tail}")
            })
    }

    /// The generator [`scan_matches_an_independent_reference_classifier`]
    /// draws from: most lines built around the marker's own shape, by
    /// [`marker_joint_biased_line`], and a minority fully free-form, by
    /// [`marker_biased_line`] -- so the marker's distinguishing shapes are
    /// common inputs, not rare ones, while free-form coverage is kept too.
    fn differential_line() -> impl Strategy<Value = String> {
        prop_oneof![
            4 => marker_joint_biased_line(),
            1 => marker_biased_line(),
        ]
    }

    /// The marker's literal characters, matched one at a time by
    /// [`reference_marker_shape`] -- a copy of the rule's own literal text,
    /// kept apart from [`super::MARKER_LITERAL`] so this classifier shares
    /// no code, only the rule it is built from, with the scanner it checks.
    const REFERENCE_MARKER_LITERAL: [char; 11] =
        ['#', 's', 'k', 'e', 'l', 'e', 't', 'o', 'n', 's', ':'];

    /// Whether the marker tolerates a run of whitespace right after
    /// matching `matched` characters of [`REFERENCE_MARKER_LITERAL`]: only
    /// right after `#` (one matched) and right after `skeletons` (ten matched).
    const fn reference_tolerates_space(matched: usize) -> bool {
        matched == 1 || matched == 10
    }

    /// What [`reference_marker_shape`] found: whether the marker's
    /// normalised shape matched at all, and whether reaching it needed to
    /// look past a character of the invisible set other than a plain space
    /// or a tab.
    struct ReferenceShape {
        offending_invisible: bool,
    }

    /// An independent reimplementation of the marker's normalised shape,
    /// built from the rule's own words rather than from
    /// [`super::scan_marker_shape`]: drop every leading whitespace
    /// character entirely, drop every format (Cf) or non-whitespace
    /// control (Cc) character wherever it sits, and read any other run of
    /// whitespace as at most one space, looking for `#`, then `skeletons`,
    /// then `:`.
    fn reference_marker_shape(content: &str) -> Option<ReferenceShape> {
        let mut matched = 0;
        let mut leading = true;
        let mut offending_invisible = false;

        for character in content.chars() {
            let plain_spacing = character == ' ' || character == '\t';
            if ruled_invisible(character) && !plain_spacing {
                offending_invisible = true;
            }
            if ruled_invisible(character) && !character.is_whitespace() {
                continue; // Cf, or Cc without the whitespace property: dropped outright.
            }
            if character.is_whitespace() {
                if leading {
                    continue; // Leading whitespace is dropped entirely, not collapsed.
                }
                if !reference_tolerates_space(matched) {
                    return None;
                }
                continue;
            }
            leading = false;
            if character != REFERENCE_MARKER_LITERAL[matched] {
                return None;
            }
            matched += 1;
            if matched == REFERENCE_MARKER_LITERAL.len() {
                return Some(ReferenceShape {
                    offending_invisible,
                });
            }
        }
        None
    }

    /// Classifies `content` exactly as [`scan`] is meant to, by an
    /// implementation independent of it: reads the rule's own two cases --
    /// an exact `# skeletons:` prefix, parsed as a well-formed directive is;
    /// otherwise the marker's normalised shape,
    /// via [`reference_marker_shape`] above -- without calling [`scan`] or
    /// any of the production helpers it is built from. Used only by
    /// [`scan_matches_an_independent_reference_classifier`], the one place
    /// this classifier's result and [`scan`]'s are compared.
    fn independent_reference(content: &str) -> DirectiveLine {
        let indentation_length = content.len() - content.trim_start_matches([' ', '\t']).len();
        let raw = &content[indentation_length..];

        if let Some(after_marker) = raw.strip_prefix("# skeletons:") {
            let option = after_marker
                .strip_prefix("partial ")
                .and_then(OptionName::parse);
            return option.map_or(DirectiveLine::Malformed, |option| {
                DirectiveLine::Directive(super::Directive {
                    indentation: content[..indentation_length].to_owned(),
                    option,
                })
            });
        }

        match reference_marker_shape(content) {
            Some(shape) if shape.offending_invisible => DirectiveLine::Hidden,
            Some(_) => DirectiveLine::Malformed,
            None => DirectiveLine::Text,
        }
    }

    #[test]
    fn a_directive_behind_any_invisible_character_is_hidden() {
        // The characters this rule exists for, each alone and each among
        // spaces and tabs on either side: a no-break space (what YAML pasted
        // from a web page arrives with), a zero-width space, a word joiner,
        // an ideographic space, a vertical tab, a form feed and a
        // byte-order mark. A well-formed and a malformed directive alike.
        for invisible in [
            '\u{a0}', '\u{200b}', '\u{2060}', '\u{3000}', '\u{b}', '\u{c}', '\u{feff}',
        ] {
            for prefix in [
                format!("{invisible}"),
                format!("  {invisible}"),
                format!("{invisible}\t "),
                format!("\t{invisible}{invisible} "),
            ] {
                for directive in [
                    "# skeletons:partial workflows",
                    "# skeletons:partail workflows",
                ] {
                    let content = format!("{prefix}{directive}");
                    assert_eq!(
                        scan(&content),
                        DirectiveLine::Hidden,
                        "expected {content:?} to be a hidden directive"
                    );
                }
            }
        }
    }

    #[test]
    fn an_invisible_character_at_each_position_inside_the_marker_is_hidden() {
        // Three positions a character of the invisible set other than a
        // plain space or a tab can hide a directive from, for a no-break
        // space (`char::is_whitespace`), a zero-width space (Cf) and a C0
        // control byte that is not whitespace (Cc, a member of the invisible
        // set alongside whitespace and Cf): right after `#`, right after
        // `# ` (before `skeletons`), and right before `:`. Each is otherwise
        // a clean `# skeletons:partial workflows`, so the invisible
        // character alone is what hides it.
        for invisible in ['\u{a0}', '\u{200b}', '\x01'] {
            for content in [
                format!("#{invisible} skeletons:partial workflows"),
                format!("# {invisible}skeletons:partial workflows"),
                format!("# skeletons{invisible}:partial workflows"),
            ] {
                assert_eq!(
                    scan(&content),
                    DirectiveLine::Hidden,
                    "expected {content:?} to be a hidden directive"
                );
            }
        }

        // Strictly inside `skeletons` the marker tolerates no whitespace at all
        // -- only right after `#` and right after `skeletons` does it -- so a
        // no-break space there breaks the match outright instead of hiding
        // it; only a character dropped outright, never treated as
        // whitespace, can hide a directive from this fourth position.
        for invisible in ['\u{200b}', '\x01'] {
            let content = format!("# s{invisible}keletons:partial workflows");
            assert_eq!(
                scan(&content),
                DirectiveLine::Hidden,
                "expected {content:?} to be a hidden directive"
            );
        }
    }

    #[test]
    fn a_marker_missing_only_its_ascii_spacing_is_malformed_not_text() {
        // `#skeletons:partial ecosystems` normalises to the marker with nothing
        // invisible in play at all -- no space after `#`, or an extra space,
        // or a tab where a space would do, is purely an ASCII spacing
        // defect, so this is refused as malformed, the same as a misspelt
        // keyword, rather than passed through as an ordinary comment. None
        // of these lines carries a leading run of invisible characters at
        // all: what makes each one directive-shaped lives entirely inside
        // the marker itself, which is exactly what normalisation reaches.
        for content in [
            "#skeletons:partial ecosystems",
            "#  skeletons:partial ecosystems",
            "# skeletons :partial ecosystems",
            "#\tskeletons:partial ecosystems",
        ] {
            assert_eq!(
                scan(content),
                DirectiveLine::Malformed,
                "expected {content:?} to be malformed, not hidden or text"
            );
        }
    }

    #[test]
    fn a_line_readable_without_setting_an_invisible_character_aside_is_not_hidden() {
        // Spaces and tabs before `# skeletons:` make a directive, well-formed or
        // not; an invisible character before anything but the marker is
        // ordinary text; and an invisible character after something visible
        // hides nothing, since the line already began with the visible one.
        // A no-break space in front of a marker missing its own space after
        // `#` is still hidden: once the no-break space is set aside, what
        // is left is directive-shaped under normalisation, so the no-break
        // space -- not the missing space, which alone would only be
        // malformed -- is what hides it.
        for (content, expected) in [
            ("  \t# skeletons:partial workflows", "directive"),
            ("\t# skeletons:partail workflows", "malformed"),
            ("\u{feff}hello", "text"),
            ("\u{a0}", "text"),
            ("", "text"),
            ("\u{a0}#skeletons:partial workflows", "hidden"),
            ("\u{a0}# Skeletons:partial workflows", "text"),
            ("x\u{a0}# skeletons:partial workflows", "text"),
            ("x\u{feff}# skeletons:partial workflows", "text"),
        ] {
            let outcome = scan(content);
            let matched = match expected {
                "directive" => matches!(outcome, DirectiveLine::Directive(_)),
                "malformed" => outcome == DirectiveLine::Malformed,
                "hidden" => outcome == DirectiveLine::Hidden,
                "text" => outcome == DirectiveLine::Text,
                other => unreachable!("no expected outcome {other:?}"),
            };
            assert!(
                matched,
                "expected {content:?} to be {expected}, got {outcome:?}"
            );
        }
    }

    #[test]
    fn invisible_characters_away_from_the_marker_and_near_misses_stay_text() {
        // The other direction: a control byte or a no-break space nowhere
        // near a marker, a case mismatch, and a comment that merely
        // mentions `skeletons` -- none of these is directive-shaped by any
        // reading, with or without invisible characters set aside.
        for content in [
            "\x01control-character-before-text",
            "hello \u{a0}world-invisible-inside-text",
            "# Skeletons:partial s",
            "# not skeletons:",
        ] {
            assert_eq!(
                scan(content),
                DirectiveLine::Text,
                "expected {content:?} to be ordinary text"
            );
        }
    }

    #[test]
    fn the_invisible_characters_are_exactly_whitespace_format_and_control_characters() {
        // Every character there is, held to the rule's wording; then the
        // rule's wording itself pinned to named characters from the Unicode
        // data, so neither can drift without a test naming the character.
        // The soft hyphen (Cf) and the combining acute accent (Mn) sit
        // either side of the line the rule draws among characters that
        // render as nothing on their own; `\x01`, `\x1f` and `\u{7f}` are
        // Cc and not whitespace -- members of the invisible set precisely
        // because Cc joins whitespace and Cf in its definition.
        for character in char::MIN..=char::MAX {
            assert_eq!(
                is_invisible(character),
                ruled_invisible(character),
                "{character:?}"
            );
        }
        for character in [
            ' ',
            '\t',
            '\u{b}',
            '\u{c}',
            '\r',
            '\u{85}',
            '\u{a0}',
            '\u{2028}',
            '\u{3000}',
            '\u{ad}',
            '\u{200b}',
            '\u{200d}',
            '\u{2060}',
            '\u{feff}',
            '\u{e0001}',
            '\x01',
            '\x1f',
            '\u{7f}',
            '\u{80}',
        ] {
            assert!(ruled_invisible(character), "{character:?} is invisible");
        }
        for character in ['a', '0', '#', '\u{301}', '\u{34f}', '\u{fe0f}', '\u{3164}'] {
            assert!(
                !ruled_invisible(character),
                "{character:?} is not invisible"
            );
        }
    }

    /// Fails the test, with the actual line and outcome, if `scan` did not
    /// return a well-formed directive.
    fn expect_directive(content: &str) -> super::Directive {
        let outcome = scan(content);
        assert!(
            matches!(outcome, DirectiveLine::Directive(_)),
            "expected a directive for {content:?}, got {outcome:?}"
        );
        let DirectiveLine::Directive(directive) = outcome else {
            unreachable!("checked above")
        };
        directive
    }

    #[test]
    fn a_well_formed_directive_is_recognised() {
        let directive = expect_directive("# skeletons:partial ecosystems");
        assert_eq!(directive.indentation, "");
        assert_eq!(
            directive.option,
            OptionName::parse("ecosystems").expect("valid name")
        );
    }

    #[test]
    fn a_line_that_does_not_begin_the_marker_is_ordinary_text() {
        for content in ["updates:", "# Skeletons:partial ecosystems"] {
            assert_eq!(
                scan(content),
                DirectiveLine::Text,
                "expected {content:?} to be ordinary text"
            );
        }
    }

    #[test]
    fn a_misspelled_keyword_is_malformed() {
        assert_eq!(
            scan("# skeletons:partail ecosystems"),
            DirectiveLine::Malformed
        );
    }

    #[test]
    fn a_missing_argument_is_malformed() {
        assert_eq!(scan("# skeletons:partial"), DirectiveLine::Malformed);
    }

    #[test]
    fn an_extra_argument_is_malformed() {
        assert_eq!(
            scan("# skeletons:partial ecosystems docker"),
            DirectiveLine::Malformed
        );
    }

    #[test]
    fn a_trailing_space_is_malformed() {
        assert_eq!(
            scan("# skeletons:partial ecosystems "),
            DirectiveLine::Malformed
        );
    }

    #[test]
    fn a_control_byte_after_the_option_name_is_malformed() {
        // A byte after the name that no option name can hold is trailing
        // text, however invisible: the line is not a directive.
        assert_eq!(
            scan("# skeletons:partial ecosystems\r"),
            DirectiveLine::Malformed
        );
    }

    #[test]
    fn leading_tabs_and_spaces_are_kept_as_indentation() {
        let directive = expect_directive("  \t# skeletons:partial ecosystems");
        assert_eq!(directive.indentation, "  \t");
    }

    proptest! {
        #[test]
        fn a_marker_behind_invisible_characters_is_hidden_unless_they_are_spaces_and_tabs(
            prefix in proptest::collection::vec(
                proptest::sample::select(invisible_characters_within_a_line()),
                0..6,
            ),
            rest in "[^\n]{0,20}",
        ) {
            // Property: for any run of invisible characters before
            // `# skeletons:` and any rest of the line, the line is a hidden
            // directive exactly when the run holds something other than a
            // space or a tab; when it holds only spaces and tabs, it is a
            // directive or a malformed one, never text and never hidden.
            let prefix: String = prefix.into_iter().collect();
            let content = format!("{prefix}# skeletons:{rest}");
            let only_spaces_and_tabs =
                prefix.chars().all(|character| character == ' ' || character == '\t');
            match (only_spaces_and_tabs, scan(&content)) {
                (false, DirectiveLine::Hidden)
                | (true, DirectiveLine::Directive(_) | DirectiveLine::Malformed) => {}
                (_, outcome) => prop_assert!(
                    false,
                    "{content:?} scanned as {outcome:?}; its prefix is spaces and tabs \
                     only: {only_spaces_and_tabs}"
                ),
            }
        }

        #[test]
        fn a_noise_character_inserted_before_the_markers_own_colon_breaks_it(
            base in proptest::sample::select(vec![
                "#skeletons:", "# skeletons:", "#skeletons :", "# skeletons :",
            ]),
            noise in noise_character(),
            rest in "[^\n]{0,10}",
        ) {
            // Property: splicing one character from `noise_character`
            // anywhere from the very start of an exact, unhidden marker up
            // to (and including) right before its closing `:` breaks the
            // literal match the marker needs -- the inserted character is
            // never the next literal the marker wants, and it is neither
            // whitespace nor dropped either, so recognition fails outright
            // and the line is ordinary text. Even a line that would
            // otherwise scan as a directive or a malformed one becomes text
            // again with one such character in the wrong place.
            let marker_length = base.chars().count();
            for index in 0..marker_length {
                let mut spliced = String::new();
                spliced.extend(base.chars().take(index));
                spliced.push(noise);
                spliced.extend(base.chars().skip(index));
                spliced.push_str(&rest);
                prop_assert_eq!(
                    scan(&spliced),
                    DirectiveLine::Text,
                    "expected {:?} (noise spliced in at {}) to be ordinary text",
                    spliced,
                    index
                );
            }
        }

        #[test]
        fn every_valid_name_round_trips_through_a_directive_line(name in "[a-z][a-z0-9]{0,20}") {
            let line = format!("# skeletons:partial {name}");
            match scan(&line) {
                DirectiveLine::Directive(directive) => {
                    prop_assert_eq!(directive.option.as_str(), name.as_str());
                    prop_assert_eq!(directive.indentation, "");
                }
                other => prop_assert!(false, "expected a directive for {line:?}, got {other:?}"),
            }
        }

        #[test]
        fn leading_spaces_and_tabs_are_kept_as_indentation(
            indentation in "[ \\t]{0,10}",
            name in "[a-z][a-z0-9]{0,10}",
        ) {
            let line = format!("{indentation}# skeletons:partial {name}");
            match scan(&line) {
                DirectiveLine::Directive(directive) => {
                    prop_assert_eq!(directive.indentation, indentation);
                }
                other => prop_assert!(false, "expected a directive for {line:?}, got {other:?}"),
            }
        }
    }

    proptest! {
        // This property's own generator, `differential_line`, lands on the
        // marker's distinguishing shapes far more often than a uniformly
        // random string would, and a dedicated case count larger than
        // proptest's default gives every one of those shapes many chances
        // to appear.
        #![proptest_config(ProptestConfig::with_cases(2048))]

        #[test]
        fn scan_matches_an_independent_reference_classifier(line in differential_line()) {
            // Property: `scan` agrees with an independent reference
            // implementation of the same rule, built fresh from its
            // wording in `independent_reference` above and sharing no
            // marker-recognising helper with `scan` -- so a place the two
            // disagree is a real defect, not a comparison of the code
            // under test against itself. `differential_line` is biased so
            // most generated lines are built around the marker's own four
            // joints -- before `#`, right after `#`, inside `skeletons`, and
            // right before `:` -- with a fixed spread of invisible
            // characters landing at each one with high probability, and a
            // minority of lines are fully free-form instead.
            prop_assert_eq!(
                scan(&line),
                independent_reference(&line),
                "scan and the independent reference disagree on {:?}",
                line
            );
        }
    }
}
