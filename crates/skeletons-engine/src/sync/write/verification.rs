//! Reading every committed write back: the second, independent look at what
//! [`super::Prepared::commit`] left on disk.

use crate::claim::{lists_exact_spelling, read_at_most};

use super::{Committed, PreparedWrite, WriteFailure};

/// Reads every committed write back and confirms it holds exactly its own
/// render, and that it is listed under exactly the spelling its own claim names
/// — a second check of two different properties: [`super::plan`] compared the
/// bytes against disk once already, through the survey it was built from, and
/// [`crate::claim::resolve`] already refused any claim it could not confirm was
/// listed under exactly its own spelling, before this run ever staged a single
/// write. This re-checks both properties after the write meant to establish or
/// fix them, on the target `resolve` itself vetted, not on a fresh walk from
/// the workspace root. For a `Changed` write, it also re-confirms the
/// permission [`super::stage_one`] carried onto the replacement, on the file
/// that actually landed.
///
/// # Errors
///
/// [`WriteFailure::ChangedAfterWrite`], naming every path that no longer reads
/// back as written: it cannot be read, is not a regular file, is a symbolic
/// link, holds different bytes than its own render, its own parent directory no
/// longer lists it under exactly the spelling its claim names, or its
/// permissions no longer match what was carried. Each of these is something the
/// environment did in the instant between the commit and this read, a file
/// another process edited, replaced or removed. It is a condition to report,
/// with the path, and never a bug in this crate (tested by
/// `a_file_changed_after_commit_is_reported_and_not_a_panic` in the parent
/// module, `write.rs`). The write it would need to retry has already taken
/// place, so `sync` reports the path and stops.
///
/// # Panics
///
/// If a target has no parent directory, no file name, or a file name that is
/// not UTF-8. A claim path always has all three, so only a defect in this
/// crate's own claim building could make one false here.
pub(crate) fn verify(committed: &Committed) -> Result<(), WriteFailure> {
    let changed: Vec<_> = committed
        .writes
        .iter()
        .filter(|write| !reads_back_as_written(write))
        .map(|write| write.path.clone())
        .collect();
    if changed.is_empty() {
        Ok(())
    } else {
        Err(WriteFailure::ChangedAfterWrite { paths: changed })
    }
}

/// Whether `write`'s target still reads back exactly as `sync` wrote it.
fn reads_back_as_written(write: &PreparedWrite) -> bool {
    let parent = write
        .target
        .parent()
        .unwrap_or_else(|| unreachable!("a claim path's target always has a parent"));
    let file_name = write
        .target
        .file_name()
        .unwrap_or_else(|| unreachable!("a claim path's target always has a file name"))
        .to_str()
        .unwrap_or_else(|| unreachable!("a claim path component is always valid UTF-8"));

    // `symlink_metadata` never follows a link, so a symbolic link is never
    // `is_file()`: this one test refuses a link, a directory and a missing
    // file alike.
    let Ok(metadata) = std::fs::symlink_metadata(&write.target) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    match &write.carried_permissions {
        Some(carried) if &metadata.permissions() != carried => return false,
        Some(_) | None => {}
    }

    // One byte more than the render holds: enough to see that the file
    // holds more than it should, without reading all of one that grew far
    // past it.
    match read_at_most(&write.target, write.rendered.len().saturating_add(1)) {
        Ok(bytes) if bytes == write.rendered => {}
        Ok(_) | Err(_) => return false,
    }

    matches!(lists_exact_spelling(parent, file_name), Ok(true))
}
