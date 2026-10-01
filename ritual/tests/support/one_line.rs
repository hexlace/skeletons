//! What the one-line acceptance files share: a name holding a real newline,
//! the way it must read once printed, and the two questions every message is
//! asked — does it stay on one line, and does it show the newline as an
//! escape instead of breaking on it.
//!
//! The name is `first` and `second` either side of a newline. Every marker
//! is a distinct word, so a line of output that carries one half of the name
//! and not the other is exactly a message that was split.

use std::error::Error;

use serde_json::Value;

/// A name with a real newline in it, as a file, directory or key would hold it.
pub(crate) const NAME: &str = "first\nsecond";

/// The same name as the two-character escape it must print as.
pub(crate) const ESCAPED: &str = "first\\nsecond";

/// `text` as a TOML basic string, quotes included, with the newline (and the
/// backslash and quote) written as TOML escapes.
pub(crate) fn toml_string(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// Asserts that `output` (a report meant to be read a line at a time) echoes
/// the name, and never splits it.
///
/// Every line that carries either half of the name, in any letter case, must
/// carry the whole of it as the escape. A message that breaks at the newline
/// leaves one line with `first` and another with `second`, and neither holds
/// the escape.
pub(crate) fn assert_name_stays_on_one_line(output: &str, what: &str) {
    let mut mentions = 0;
    for line in output.lines() {
        let lowered = line.to_lowercase();
        if lowered.contains("first") || lowered.contains("second") {
            mentions += 1;
            assert!(
                lowered.contains(ESCAPED),
                "{what}: a line names part of {ESCAPED:?} without the escape, so the message was \
                 split at the newline; the line was {line:?}; the whole output was {output:?}"
            );
        }
    }
    assert!(
        mentions > 0,
        "{what}: the output never names {ESCAPED:?}; the output was {output:?}"
    );
}

/// Asserts that a `--json` human-text field holds no line break and shows the
/// name as the escape, in whatever letter case the message spells it.
pub(crate) fn assert_field_is_one_line(field: &str, text: &str) {
    assert!(
        !text.contains('\n'),
        "`{field}` must be one line, with the newline printed as an escape; it was {text:?}"
    );
    assert!(
        !text.contains('\r'),
        "`{field}` must be one line; it held a carriage return: {text:?}"
    );
    assert!(
        text.to_lowercase().contains(ESCAPED),
        "`{field}` must show the newline as the two characters `\\n`, giving {ESCAPED:?}; it was \
         {text:?}"
    );
}

/// The one refusal in `document`, failing with the document if there is not
/// exactly one.
pub(crate) fn only_refusal(document: &Value) -> Result<&Value, Box<dyn Error>> {
    let refusals = super::json::refusals(document)?;
    match refusals.as_slice() {
        [one] => Ok(one),
        other => Err(format!("expected exactly one refusal; refusals were: {other:?}").into()),
    }
}

/// Says the test cannot establish its premise on this machine, and ends it
/// there instead of passing or failing over something it never measured.
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
pub(crate) fn skip(premise: &str) {
    eprintln!("skipped: {premise}");
}

/// Whether a file whose name is exactly `name` can be created in a fresh
/// directory and comes back from a listing spelled exactly `name`.
///
/// This is the premise every test that writes such a name on disk stands on:
/// a filesystem that refuses the name, or folds it to another spelling,
/// cannot show what `check` does with it.
pub(crate) fn filesystem_keeps_the_name(name: &str) -> bool {
    let Ok(directory) = super::TemporaryDirectory::new("one-line-probe") else {
        return false;
    };
    if std::fs::write(directory.path().join(name), b"probe").is_err() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(directory.path()) else {
        return false;
    };
    let listed: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    listed == [name]
}
