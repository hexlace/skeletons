//! Keeps every citation `.docs/design.md` makes — `` `file` → `name` ``,
//! naming the file a mechanism or a test lives in and the function itself —
//! pointing at code that still exists.
//!
//! design.md backs every safety sentence with the mechanism that makes it
//! true and, where the mechanism is proved at run time rather than by a
//! type, the test that fails without it. A sentence naming a mechanism
//! that no longer holds, or a test that no longer exists, is a false safety
//! claim that reads as a true one. A citation is only as good as its
//! target existing; this keeps that true after a rename or a deletion that
//! nobody remembered to update the doc for.
//!
//! It also keeps the shape of the fenced blocks in the documents it reads
//! (`.docs/`, the two readmes and the contributing guide): a block opens with
//! its language, or with nothing, and never with a message pulled up onto the
//! opening line.

#[cfg(skeletons_checkout)]
use std::path::{Path, PathBuf};

#[cfg(skeletons_checkout)]
/// `.docs/design.md`, read once at compile time — the one copy this module
/// reads its citations from.
const DESIGN_DOC: &str = include_str!("../../../.docs/design.md");

/// One citation design.md makes: `` `file` → `name` `` inside a
/// parenthesised aside, naming the file a mechanism or a test lives in and
/// the function that is it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Citation<'text> {
    file: &'text str,
    name: &'text str,
}

/// Every citation in `text`, in the order they appear.
///
/// design.md writes a citation as a backtick-quoted file path — a token
/// containing `/` — followed by `` → `name` ``. When two citations share
/// one file, only the first repeats the path
/// (`` (`file.rs` → `a`, and → `b`) ``), so a path token becomes the
/// citation target for every later `` → `name` `` token, until the next
/// path token replaces it. A backtick token with no `/` that is not itself
/// immediately preceded by `→` is ordinary inline code, not a citation, and
/// is skipped — this is what keeps a bare `` `--fail-behind` `` out of the
/// result. A token with a `/`, such as `` `files/` ``, is taken as the
/// current file but is never itself a citation: only a `` → `name` `` after
/// it yields one.
fn citations(text: &str) -> Vec<Citation<'_>> {
    let mut current_file: Option<&str> = None;
    let mut found = Vec::new();
    let mut search_from = 0;
    while let Some(relative_open) = text[search_from..].find('`') {
        let open = search_from + relative_open + 1;
        let Some(relative_close) = text[open..].find('`') else {
            break;
        };
        let close = open + relative_close;
        let token = &text[open..close];
        let before = text[..open - 1].trim_end();
        if token.contains('/') {
            current_file = Some(token);
        } else if before.ends_with('→') {
            if let Some(file) = current_file {
                found.push(Citation { file, name: token });
            }
        }
        search_from = close + 1;
    }
    found
}

/// A fenced block whose opening line holds more than one word after the
/// fence, such as a message pulled up onto the line that opens its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrowdedFence<'text> {
    /// The opening line's number, counting from one.
    line_number: usize,
    /// Everything after the fence on that line, trimmed.
    info_string: &'text str,
}

/// An opening or closing fence: the character it repeats, how many times, and
/// what follows the run, trimmed.
type Fence<'text> = (char, usize, &'text str);

/// The fence `line` is, or `None` when it is not one.
///
/// A fence is three or more backticks or tildes, after at most three spaces
/// of indentation: Markdown reads a line indented four or more spaces as
/// indented code, not a fence. The documents indent a block inside a list
/// item by the width of its marker (two spaces), so the indentation is
/// measured from the line's own start rather than from the item's content.
fn fence_of(line: &str) -> Option<Fence<'_>> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let character = trimmed.chars().next()?;
    match character {
        '`' | '~' => {}
        _ => return None,
    }
    let run = trimmed.chars().take_while(|&c| c == character).count();
    if run < 3 {
        return None;
    }
    // The run is made of one-byte characters, so `run` is also its length in
    // bytes and a valid place to cut.
    Some((character, run, trimmed[run..].trim()))
}

/// Every fenced block in `text` whose opening line holds more than one word
/// after the fence, in the order they appear.
///
/// The blocks' contents are skipped, so a line inside one that looks like a
/// fence is not read as one: a block ends at a fence of the same character,
/// at least as long as the one that opened it, with nothing after it.
fn crowded_fences(text: &str) -> Vec<CrowdedFence<'_>> {
    let mut found = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for (index, line) in text.lines().enumerate() {
        let Some((character, run, rest)) = fence_of(line) else {
            continue;
        };
        match open {
            None => {
                if rest.split_whitespace().count() > 1 {
                    found.push(CrowdedFence {
                        line_number: index + 1,
                        info_string: rest,
                    });
                }
                // A backtick fence cannot hold a backtick in its info string,
                // so a line of that shape opens no block; it is still found
                // above, since the message is what was pulled up onto it.
                if character != '`' || !rest.contains('`') {
                    open = Some((character, run));
                }
            }
            Some((open_character, open_run)) => {
                if character == open_character && run >= open_run && rest.is_empty() {
                    open = None;
                }
            }
        }
    }
    found
}

#[cfg(skeletons_checkout)]
/// The workspace root: two directories above this crate's own manifest
/// (`crates/skeletons/../..`), which is where every citation's `file` is
/// relative to — `crates/skeletons/src/…` and `ritual/tests/…` alike.
fn workspace_root() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", ".."].iter().collect()
}

#[cfg(skeletons_checkout)]
/// Every markdown file whose fenced blocks are kept in shape: those under
/// `.docs/`, the two readmes and the contributing guide, as paths relative to
/// the workspace root, in a fixed order.
fn documented_markdown_files() -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = vec![
        PathBuf::from("readme.md"),
        PathBuf::from("crates/skeletons/readme.md"),
        PathBuf::from("contributing.md"),
    ];
    let docs = Path::new(".docs");
    for entry in std::fs::read_dir(workspace_root().join(docs))? {
        let name = entry?.file_name();
        if Path::new(&name)
            .extension()
            .is_some_and(|extension| extension == "md")
        {
            files.push(docs.join(name));
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(skeletons_checkout)]
/// Whether `file`, read fresh from the workspace root, contains a function
/// named `name` — `fn name(`, so a citation naming a type or a constant
/// rather than a function is caught too, not only a renamed or deleted one.
fn cited_file_declares_function(file: &str, name: &str) -> Result<bool, std::io::Error> {
    let contents = std::fs::read_to_string(workspace_root().join(file))?;
    Ok(contents.contains(&format!("fn {name}(")))
}

#[cfg(test)]
mod tests {
    #[cfg(skeletons_checkout)]
    use super::cited_file_declares_function;
    use super::{Citation, CrowdedFence, citations, crowded_fences};

    #[cfg(skeletons_checkout)]
    #[test]
    fn design_doc_names_at_least_one_citation() {
        // A positive control: without it, a parser that matched nothing at
        // all — a wrong quote character, a loop that never runs — would
        // pass every citation vacuously, having found none to check.
        let found = citations(super::DESIGN_DOC);
        assert!(
            !found.is_empty(),
            "expected design.md to carry at least one `file` → `name` citation"
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn every_citation_in_design_doc_names_a_function_that_still_exists() {
        let found = citations(super::DESIGN_DOC);
        let mut missing = Vec::new();
        for citation in &found {
            match cited_file_declares_function(citation.file, citation.name) {
                Ok(true) => {}
                Ok(false) => missing.push(format!(
                    "{} → {} (file read, but no `fn {}(` in it)",
                    citation.file, citation.name, citation.name
                )),
                Err(error) => {
                    missing.push(format!("{} → {} ({error})", citation.file, citation.name));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "design.md cites a function that no longer exists where named: {}",
            missing.join("; ")
        );
    }

    #[cfg(skeletons_checkout)]
    #[test]
    fn no_fenced_block_in_the_documents_opens_with_more_than_one_word() {
        // A positive control first: the three named documents under `.docs/`
        // must all be among the files read, so a listing that found nothing
        // cannot pass by having nothing to check. Then every file is read
        // fresh from the workspace root and its opening fences are checked.
        let files = super::documented_markdown_files().expect(".docs/ must be readable");
        for expected in [
            ".docs/design.md",
            ".docs/skeleton-format.md",
            ".docs/wearing.md",
        ] {
            assert!(
                files
                    .iter()
                    .any(|file| file.as_path() == std::path::Path::new(expected)),
                "expected {expected} among the documents checked, got {files:?}"
            );
        }
        let mut crowded = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(super::workspace_root().join(file))
                .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.display()));
            for fence in super::crowded_fences(&text) {
                crowded.push(format!(
                    "{}:{} opens with {:?}",
                    file.display(),
                    fence.line_number,
                    fence.info_string
                ));
            }
        }
        assert!(
            crowded.is_empty(),
            "a fenced block opens with more than one word after its fence, which is a \
             message pulled up onto the opening line: {}",
            crowded.join("; ")
        );
    }

    #[test]
    fn a_message_pulled_up_onto_its_opening_fence_is_found_with_its_line() {
        // The shape being kept out: the first line of a block's contents
        // written on the fence line itself. Two well-formed blocks around it
        // are the control, so the check is not just flagging every fence.
        let text = "```text\nfine\n```\n\n\
                    \x20 ```text git failed, so sync wrote nothing\n\
                    \x20 x\n\
                    \x20 ```\n\n\
                    ```\nalso fine\n```\n";
        assert_eq!(
            crowded_fences(text),
            vec![CrowdedFence {
                line_number: 5,
                info_string: "text git failed, so sync wrote nothing",
            }]
        );
    }

    #[test]
    fn a_language_alone_or_no_language_is_not_crowded() {
        let text = "```sh\nx\n```\n~~~toml\nx\n~~~\n```\nx\n```\n";
        assert_eq!(crowded_fences(text), Vec::new());
    }

    #[test]
    fn a_fence_looking_line_inside_a_block_is_contents_not_a_fence() {
        // A longer fence holds a shorter one as contents, and a different
        // character never closes it. The words on the inner line are not an
        // info string, and the block still closes at its own fence.
        let text = "````text\n```text two words\n~~~\n````\n```sh\nx\n```\n";
        assert_eq!(crowded_fences(text), Vec::new());
    }

    #[test]
    fn a_line_indented_four_spaces_is_code_not_a_fence() {
        // Markdown reads four or more spaces of indentation as an indented
        // code line, so the first line opens no block. Were it taken as a
        // fence, it would swallow the crowded fence after it as contents and
        // the message on that line would go unfound.
        let text = "    ```\n```text two words\nx\n```\n";
        assert_eq!(
            crowded_fences(text),
            vec![CrowdedFence {
                line_number: 2,
                info_string: "text two words",
            }]
        );
    }

    #[test]
    fn a_fence_indented_three_spaces_is_still_a_fence() {
        // The edge of the allowed indentation: three spaces opens a block, so
        // the crowded fence after it is contents and is not reported, and the
        // block closes at its own bare fence.
        let text = "   ```\n```text two words\n   ```\n";
        assert_eq!(crowded_fences(text), Vec::new());
    }

    #[test]
    fn a_closing_fence_with_words_after_it_does_not_close_the_block() {
        // "``` not a close" cannot end a block, so the next bare fence does,
        // and the block after it opens crowded.
        let text = "```text\nx\n``` not a close\n```\n```text one two\ny\n```\n";
        assert_eq!(
            crowded_fences(text),
            vec![CrowdedFence {
                line_number: 5,
                info_string: "text one two",
            }]
        );
    }

    #[test]
    fn a_message_with_code_in_it_is_found_and_opens_no_block() {
        // A backtick in the pulled-up message means renderers do not open a
        // block at all, so the crowded fence on the next line is read from
        // its own fence and found too. Were the first line taken as opening
        // a block, the second would be swallowed as its contents.
        let text = "```text run `cargo ritual skeletons sync` again\n\
                    ```text two words\nx\n```\n";
        assert_eq!(
            crowded_fences(text),
            vec![
                CrowdedFence {
                    line_number: 1,
                    info_string: "text run `cargo ritual skeletons sync` again",
                },
                CrowdedFence {
                    line_number: 2,
                    info_string: "text two words",
                },
            ]
        );
    }

    #[test]
    fn two_arrows_after_one_file_path_both_cite_that_file() {
        // design.md's own shape for two tests in one file:
        // `` (`a/b.rs` → `first`, and → `second`) ``. The second arrow
        // repeats no path at all, so the parser must still attribute it to
        // the file named before the first arrow.
        let text = "see (`a/b.rs` → `first`, and → `second`).";
        assert_eq!(
            citations(text),
            vec![
                Citation {
                    file: "a/b.rs",
                    name: "first"
                },
                Citation {
                    file: "a/b.rs",
                    name: "second"
                },
            ]
        );
    }

    #[test]
    fn a_second_file_path_replaces_the_first_as_the_citation_target() {
        let text = "(`a/b.rs` → `first`), then (`c/d.rs` → `second`).";
        assert_eq!(
            citations(text),
            vec![
                Citation {
                    file: "a/b.rs",
                    name: "first"
                },
                Citation {
                    file: "c/d.rs",
                    name: "second"
                },
            ]
        );
    }

    #[test]
    fn inline_code_with_no_preceding_arrow_is_not_a_citation() {
        // `--fail-behind` and `files/` are both ordinary inline code
        // elsewhere in design.md, and no arrow precedes a name here, so
        // nothing is cited. (`files/` is taken as the current file, as any
        // slash-bearing token is; only an arrow after it would turn that
        // into a citation.)
        let text = "run `cargo skeletons sync` and see `--fail-behind` under `files/`.";
        assert_eq!(citations(text), Vec::new());
    }

    #[test]
    fn a_bare_identifier_never_becomes_a_citation_without_a_file() {
        // An arrow with nothing naming a file before it anywhere in the
        // text has no `current_file` to attribute the name to, so it must
        // be dropped rather than cited against an empty or wrong path.
        let text = "→ `orphan`";
        assert_eq!(citations(text), Vec::new());
    }
}
