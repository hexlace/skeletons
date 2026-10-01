//! Accessors for `skeletons check --json`'s output, against the decided
//! contract (`format_version` 1): a top-level `format_version` number, a
//! `filters` array, a `summary` object, `skeletons[]` and `bones[]` arrays
//! each carrying a `pin`, a `drift` and/or `behind` fact object, a
//! `refusals[]` array, and `aborted` (`null` or an object naming what
//! stopped the whole command).
//!
//! Every accessor fails with a clear message naming the field it expected
//! and the value it actually found, rather than panicking blindly, so a test
//! that reaches an unexpected shape reports why, with the offending value
//! attached, instead of an opaque panic.

use std::error::Error;

use serde_json::Value;

/// Parses `text` as JSON, failing with `text` embedded in the error if it is
/// not valid JSON at all.
pub(crate) fn parse(text: &str) -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(text).map_err(|error| format!("{error}; text was: {text:?}").into())
}

/// The document's own format-version marker, closed at `1` for as long as
/// this suite is written against it.
pub(crate) fn format_version(document: &Value) -> Result<u64, Box<dyn Error>> {
    u64_field(document, "format_version")
}

/// The `--drifted`/`--behind` filters this run was given, as their JSON
/// string values (`"drifted"`, `"behind"`).
pub(crate) fn filters(document: &Value) -> Result<Vec<&str>, Box<dyn Error>> {
    document
        .get("filters")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("expected a `filters` array; document was: {document}").into())
        .and_then(|filters| {
            filters
                .iter()
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        format!("expected a string filter; value was: {value}").into()
                    })
                })
                .collect()
        })
}

/// The document's `summary` object.
pub(crate) fn summary(document: &Value) -> Result<&Value, Box<dyn Error>> {
    document
        .get("summary")
        .ok_or_else(|| format!("expected a `summary` object; document was: {document}").into())
}

/// One of `summary`'s own counts: `bones`, `matches`, `drifted`, `skeletons`,
/// `current`, `behind`, `pinned`, `undetermined`, or `refusals`.
pub(crate) fn summary_count(document: &Value, field: &str) -> Result<u64, Box<dyn Error>> {
    u64_field(summary(document)?, field)
}

/// `summary.failed` — the exit status, as the boolean `--json` carries
/// alongside it.
pub(crate) fn summary_failed(document: &Value) -> Result<bool, Box<dyn Error>> {
    bool_field(summary(document)?, "failed")
}

/// The rows describing every worn skeleton (one per dependency, whether or
/// not it is refused).
pub(crate) fn skeletons(document: &Value) -> Result<&Vec<Value>, Box<dyn Error>> {
    array_field(document, "skeletons")
}

/// The rows describing every bone across every worn skeleton that was not
/// itself refused as a whole.
pub(crate) fn bones(document: &Value) -> Result<&Vec<Value>, Box<dyn Error>> {
    array_field(document, "bones")
}

/// The rows describing every refusal, whether about one worn skeleton or
/// about no single one (a wearing-table defect, an overlap).
pub(crate) fn refusals(document: &Value) -> Result<&Vec<Value>, Box<dyn Error>> {
    array_field(document, "refusals")
}

/// The document's `aborted` field: `None` for `null` (the command ran to
/// completion, however it came out), `Some` for the object naming what
/// stopped it before it could look at any skeleton at all.
pub(crate) fn aborted(document: &Value) -> Result<Option<&Value>, Box<dyn Error>> {
    match document.get("aborted") {
        Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(value)),
        None => Err(format!(
            "expected an `aborted` field (null or an object); document was: {document}"
        )
        .into()),
    }
}

/// A bone's own path, relative to the workspace root.
pub(crate) fn bone_path(row: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(row, "path")
}

/// A bone's own dependency key.
pub(crate) fn bone_dependency(row: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(row, "dependency")
}

/// A bone's own wearing manifest, relative to the workspace root.
pub(crate) fn bone_manifest(row: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(row, "manifest")
}

/// A bone's own drift state: `matches` or `drifted`.
pub(crate) fn bone_drift_state(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["drift", "state"])
}

/// A bone's own drift reason (`changed`/`missing`), present only when
/// `bone_drift_state` is `drifted`.
pub(crate) fn bone_drift_reason(row: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_nested_string(row, &["drift", "reason"])
}

/// A bone's (or a worn-skeleton row's) behind state: `current`, `behind`,
/// `pinned`, or `undetermined`.
pub(crate) fn behind_state(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["behind", "state"])
}

/// A bone's (or a worn-skeleton row's) behind reason — one of
/// `other-registry`, `unreachable`, `not-in-index`, `unexpected-response`,
/// `tag-not-a-version`, `branch-missing`, `unrecognised-source`,
/// `checkout-unreadable`, `local-failure`, `directory-missing` — present
/// only when `state` is `undetermined`.
pub(crate) fn behind_reason(row: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_nested_string(row, &["behind", "reason"])
}

/// A bone's (or a worn-skeleton row's) behind detail — the human text
/// `undetermined` and registry-`behind` carry, present only for those two
/// states.
pub(crate) fn behind_detail(row: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_nested_string(row, &["behind", "detail"])
}

/// A bone's (or a worn-skeleton row's) `behind.newer.version` — the newer
/// registry version making it behind, present only for a registry pin that
/// is behind.
pub(crate) fn behind_newer_version(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["behind", "newer", "version"])
}

/// A bone's (or a worn-skeleton row's) `behind.newer.tag` — the newer tag's
/// own name making it behind, present only for a `tag =` pin that is
/// behind.
pub(crate) fn behind_newer_tag(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["behind", "newer", "tag"])
}

/// A bone's (or a worn-skeleton row's) pin kind: `registry`, `tag`,
/// `branch`, `default-branch`, `rev`, `path`, or `unrecognised`.
pub(crate) fn pin_kind(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["pin", "kind"])
}

/// A bone's (or a worn-skeleton row's) `pin.path` — the path-pin display
/// rule's own text, present only when `pin_kind` is `path`.
pub(crate) fn pin_path(row: &Value) -> Result<&str, Box<dyn Error>> {
    nested_string(row, &["pin", "path"])
}

/// A worn-skeleton row's own dependency key.
pub(crate) fn skeleton_dependency(row: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(row, "dependency")
}

/// A worn-skeleton row's own wearing manifest, relative to the workspace
/// root.
pub(crate) fn skeleton_manifest(row: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(row, "manifest")
}

/// Whether a refusal about this one worn skeleton stands
/// (`skeletons[].refused`); when true, this skeleton's bones are absent
/// from `bones`.
pub(crate) fn skeleton_refused(row: &Value) -> Result<bool, Box<dyn Error>> {
    bool_field(row, "refused")
}

/// A refusal's own `kind`.
pub(crate) fn refusal_kind(refusal: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(refusal, "kind")
}

/// A refusal's own, self-contained `message` — the same text `sync` prints
/// after `refused: ` and the closing `refused:` list in `check`'s human
/// output carries in full.
pub(crate) fn refusal_message(refusal: &Value) -> Result<&str, Box<dyn Error>> {
    string_field(refusal, "message")
}

/// A refusal's own `dependency`, present on every refusal kind except the
/// two about claims that collide (`overlap`) and the whole
/// `[package.metadata.skeletons]` table (`not-a-table` with no single key).
pub(crate) fn refusal_dependency(refusal: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_string_field(refusal, "dependency")
}

/// A refusal's own `skeleton` — the skeleton crate's own name — present
/// only on the three refusal kinds about one worn skeleton
/// (`skeleton-invalid`, `option-refused`, `unsafe-path`).
pub(crate) fn refusal_skeleton(refusal: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_string_field(refusal, "skeleton")
}

/// An `unsafe-path` refusal's own `path` — the claimed path that failed a
/// location check.
pub(crate) fn refusal_path(refusal: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_string_field(refusal, "path")
}

/// An `unsafe-path` refusal's own `cause` — one of `spelled-differently`,
/// `symbolic-link-above`, `symbolic-link`, `not-a-directory-above`,
/// `not-a-file`, `inside-git-directory`, `untrackable-name`,
/// `inside-another-repository`, `name-too-long`, `unreadable`.
pub(crate) fn refusal_cause(refusal: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_string_field(refusal, "cause")
}

/// An `unsafe-path` refusal's own `at`. For `spelled-differently`,
/// `untrackable-name` and `name-too-long` it is the claimed path up to and
/// including the offending component (equal to `path` when that component
/// is the file itself). For `symbolic-link-above`, `not-a-directory-above`
/// and `inside-another-repository` it is the directory above the claimed
/// path where the trouble is. `null` for every other cause.
pub(crate) fn refusal_at(refusal: &Value) -> Result<Option<&str>, Box<dyn Error>> {
    optional_string_field(refusal, "at")
}

/// A `spelled-differently` refusal's own `on_disk` — the on-disk spellings
/// the directory listing found, sorted, each a workspace-relative path.
/// `None` when the field is absent or `null` (every cause other than
/// `spelled-differently`).
pub(crate) fn refusal_on_disk(refusal: &Value) -> Result<Option<Vec<&str>>, Box<dyn Error>> {
    match refusal.get("on_disk") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().ok_or_else(|| {
                    format!("expected `on_disk` to hold only strings; value was: {refusal}").into()
                })
            })
            .collect::<Result<Vec<&str>, Box<dyn Error>>>()
            .map(Some),
        Some(other) => {
            Err(format!("expected `on_disk` to be an array or null; value was: {other}").into())
        }
    }
}

/// A required array field at the top level of `document`.
fn array_field<'value>(
    document: &'value Value,
    field: &str,
) -> Result<&'value Vec<Value>, Box<dyn Error>> {
    document
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("expected a `{field}` array; document was: {document}").into())
}

/// A required string field on any row or refusal object.
fn string_field<'value>(value: &'value Value, field: &str) -> Result<&'value str, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("expected a string `{field}` field; value was: {value}").into())
}

/// A string field that may be absent or `null`.
fn optional_string_field<'value>(
    value: &'value Value,
    field: &str,
) -> Result<Option<&'value str>, Box<dyn Error>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(inner) => inner.as_str().map(Some).ok_or_else(|| {
            format!("expected `{field}` to be a string or null; value was: {value}").into()
        }),
    }
}

/// A required, non-negative integer field.
fn u64_field(value: &Value, field: &str) -> Result<u64, Box<dyn Error>> {
    value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        format!("expected a non-negative integer `{field}` field; value was: {value}").into()
    })
}

/// A required boolean field.
fn bool_field(value: &Value, field: &str) -> Result<bool, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("expected a boolean `{field}` field; value was: {value}").into())
}

/// A required string at a nested path, e.g. `["drift", "state"]`.
fn nested_string<'value>(
    value: &'value Value,
    path: &[&str],
) -> Result<&'value str, Box<dyn Error>> {
    let mut current = value;
    for segment in path {
        current = current.get(segment).ok_or_else(|| {
            format!("expected a `{segment}` field on the way to {path:?}; value was: {value}")
        })?;
    }
    current
        .as_str()
        .ok_or_else(|| format!("expected a string at {path:?}; value was: {value}").into())
}

/// A string at a nested path that may be absent or `null` at its last
/// segment (every parent segment up to it is still required).
fn optional_nested_string<'value>(
    value: &'value Value,
    path: &[&str],
) -> Result<Option<&'value str>, Box<dyn Error>> {
    let (last, parents) = path
        .split_last()
        .ok_or("optional_nested_string needs a non-empty path")?;
    let mut current = value;
    for segment in parents {
        current = current.get(segment).ok_or_else(|| {
            format!("expected a `{segment}` field on the way to {path:?}; value was: {value}")
        })?;
    }
    match current.get(last) {
        None | Some(Value::Null) => Ok(None),
        Some(inner) => inner.as_str().map(Some).ok_or_else(|| {
            format!("expected a string or null at {path:?}; value was: {value}").into()
        }),
    }
}
