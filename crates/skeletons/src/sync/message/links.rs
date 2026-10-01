//! The line for a failed link.
//!
//! `sync` creates a missing file by hard-linking it into place, because a
//! link fails where a rename would silently replace a file that appeared in
//! the meantime. The operating system's own words for a refused link
//! ("Operation not permitted") name nothing the user can act on unless they
//! know a link was being made, so the line says that `sync` creates new files
//! with hard links, and then gives the operating system's error, escaped like
//! any text from outside.
//! It says nothing of why the link failed: that is the error's to say.

use crate::claim::ClaimPath;
use crate::skeleton::Escaped;

use super::{leftovers_clause, nothing_written_with_detail};
use crate::sync::write::Leftover;

/// The account of the failure shared by both shapes: what `sync` was doing,
/// and the operating system's error, escaped.
fn account(detail: &str) -> String {
    let detail = Escaped(detail);
    format!("sync creates new files with hard links, and the link failed: {detail}")
}

/// The message when the link for `path` failed before any file was written:
/// `` creating {path} failed before any file was written, so sync removed
/// what it had prepared and wrote nothing: sync creates new files with hard
/// links, and the link failed: {detail} ``.
pub(super) fn link_failed_before_any_write(
    path: &ClaimPath,
    detail: &str,
    leftovers: &[Leftover],
) -> String {
    let path = Escaped(path.as_str());
    format!(
        "creating {path} failed before any file was written, {}",
        nothing_written_with_detail(leftovers, &account(detail))
    )
}

/// The message when the link for `path` failed after some files had landed,
/// which needs a workspace that spans more than one filesystem: `outcome` is
/// the clause naming which files were and were not written.
pub(super) fn link_failed_after_some_writes(
    path: &ClaimPath,
    detail: &str,
    outcome: &str,
    leftovers: &[Leftover],
) -> String {
    let path = Escaped(path.as_str());
    format!(
        "creating {path} failed {outcome}: {}{}",
        account(detail),
        leftovers_clause(leftovers)
    )
}

#[cfg(test)]
mod tests {
    use super::{link_failed_after_some_writes, link_failed_before_any_write};
    use crate::survey::poison::{POISON, assert_escaped_once, claim};

    use super::super::poisoned::poisoned_leftover;

    #[test]
    fn a_failed_link_before_any_write_says_sync_creates_files_with_hard_links() {
        let message = link_failed_before_any_write(
            &claim("d/b.yml"),
            "Operation not permitted (os error 1)",
            &[],
        );

        assert_eq!(
            message,
            "creating d/b.yml failed before any file was written, so sync removed what it had \
             prepared and wrote nothing: sync creates new files with hard links, and the link \
             failed: Operation not permitted (os error 1)"
        );
    }

    #[test]
    fn a_failed_link_after_some_writes_says_what_was_and_was_not_written() {
        let message = link_failed_after_some_writes(
            &claim("m/b.yml"),
            "Operation not supported (os error 45)",
            "after 1 of 2 files was written (a.yml); m/b.yml was not written, and git holds \
             what the written file replaced",
            &[],
        );

        assert_eq!(
            message,
            "creating m/b.yml failed after 1 of 2 files was written (a.yml); m/b.yml was not \
             written, and git holds what the written file replaced: sync creates new files \
             with hard links, and the link failed: Operation not supported (os error 45)"
        );
    }

    #[test]
    fn a_failed_link_names_its_path_and_the_system_error_escaped_once() {
        // Before any write, and after some, each with a leftover that holds
        // the same text.
        for leftovers in [Vec::new(), vec![poisoned_leftover()]] {
            assert_escaped_once(
                &link_failed_before_any_write(&claim(POISON), POISON, &leftovers),
                "the link failed before any write",
            );
            assert_escaped_once(
                &link_failed_after_some_writes(
                    &claim(POISON),
                    POISON,
                    "after 1 of 2 files was written (a.yml); b.yml was not written",
                    &leftovers,
                ),
                "the link failed after some writes",
            );
        }
    }
}
