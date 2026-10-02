//! The lines for the ways git looks away from a path `sync` is about to
//! write, other than a file git is told not to read (which `wear` meets too,
//! so [`crate::work_tree::message::hidden_from_work_tree_line`] is shared): an entry git tracks at a
//! directory above the claim, an entry git hides under another spelling that
//! the filesystem takes for the claim, and a file git ignores. Each names what
//! git holds and where, and a remedy only where one works.

use crate::claim::ClaimPath;
use crate::skeleton::Escaped;
use crate::survey::told_apart;
use crate::sync::fold_variant::{FoldRelation, FoldVariant};
use crate::sync::proof::AboveEntry;

use super::{RUN_SYNC_AGAIN, nothing_written_with_remedy};
use crate::sync::write::Leftover;

/// `` {path} is ignored by git (`{rule}`, as `git check-ignore -v` reports it
/// as source:line:pattern), so sync would create a file git status never
/// shows and git add refuses, and a later sync could not update it: remove
/// the ignore rule, or make {path} by hand and run `git add -f -- {path}`,
/// then run the `sync` task again ``.
///
/// `rule` is `git check-ignore -v`'s own `<source>:<line>:<pattern>`, shown
/// whole because that is the form a user can search for and run themselves.
pub(super) fn ignored_line(path: &ClaimPath, rule: &str) -> String {
    let (path, rule) = (Escaped(path.as_str()), Escaped(rule));
    format!(
        "{path} is ignored by git (`{rule}`, as `git check-ignore -v` reports it as \
         source:line:pattern), so sync would create a file git status never shows and git add \
         refuses, and a later sync could not update it: remove the ignore rule, or make {path} \
         by hand and run `git add -f -- {path}`, then {RUN_SYNC_AGAIN}"
    )
}

/// The line for an index entry at a directory above `path`, worded for what
/// it is. A file or a link gets the remedy that removes it from the index; a
/// submodule is another repository, for which `skeletons` has none to name.
///
/// When git's entry is not spelled byte for byte as the directory it stands
/// for, git listed it because it folds case, and the line says so.
pub(super) fn tracked_above_line(path: &ClaimPath, git_path: &str, entry: &AboveEntry) -> String {
    // Read as they are: whether git spelled the directory as the claim does.
    let respelled = directory_above(path, git_path).as_str() != git_path;
    let (path, git_path) = (Escaped(path.as_str()), Escaped(git_path));
    let kind = match entry {
        AboveEntry::File => "a file",
        AboveEntry::SymbolicLink => "a symbolic link",
        AboveEntry::Submodule => {
            return format!(
                "{path} is inside {git_path}, which git's index tracks as a submodule, another \
                 repository, and sync writes only into this one"
            );
        }
        AboveEntry::Other { mode } => {
            let mode = Escaped(mode);
            return format!(
                "{path} cannot be written, because git's index tracks {git_path} with mode \
                 {mode} where {path} needs a directory"
            );
        }
    };
    let fold_clause = if respelled {
        " (git takes the two for one name where it ignores case, core.ignorecase)"
    } else {
        ""
    };
    format!(
        "{path} cannot be written, because git's index tracks {git_path} as {kind} where {path} \
         needs a directory{fold_clause}, so git would put it back in place of what sync wrote: \
         if {path} belongs in this repository, run `git rm --cached -- {git_path}` and commit, \
         then {RUN_SYNC_AGAIN}"
    )
}

/// The directory above `path` that an index entry `git_path` stands at: the
/// ancestor with as many components as `git_path` has. An entry is only ever
/// asked about at a directory above the claim, so that ancestor exists.
fn directory_above(path: &ClaimPath, git_path: &str) -> ClaimPath {
    path.ancestors()[git_path.split('/').count() - 1].clone()
}

/// The stderr message for a write refused because the filesystem takes an
/// index entry git hides for the staging file just created: nothing was
/// written, and `leftovers` is anything the rollback could not remove.
pub(super) fn folds_onto_tracked_message(
    claim: &ClaimPath,
    variant: &FoldVariant,
    leftovers: &[Leftover],
) -> String {
    match variant.relation() {
        FoldRelation::SamePath => {
            let git_path = variant.git_path();
            // `told_apart` takes the names as they are and escapes them;
            // `bring_back_remedy` does the same for the entry it is given.
            let shown = told_apart(&[claim.as_str(), git_path]);
            let (claim, git_path_shown) = (&shown[0], &shown[1]);
            format!(
                "{claim} and {git_path_shown}, which git's index tracks, are one name on this \
                 filesystem{}, and git would take what sync wrote for {git_path_shown}, {}",
                shown.note_in_parentheses(),
                nothing_written_with_remedy(
                    leftovers,
                    &format!(
                        "bring {git_path_shown} back into the work tree ({}), then run the `check` \
                         task to see how it is spelled on disk",
                        bring_back_remedy(git_path)
                    )
                )
            )
        }
        FoldRelation::DirectoryAbove { claimed } => {
            let git_path = variant.git_path();
            let shown = told_apart(&[claim.as_str(), claimed.as_str(), git_path]);
            let (claim_shown, claimed_shown, git_path_shown) = (&shown[0], &shown[1], &shown[2]);
            // A remedy names the entry itself, never its code points: a
            // command takes the name.
            let (claim_named, git_path_named) = (Escaped(claim.as_str()), Escaped(git_path));
            format!(
                "{claim_shown} would be written under {claimed_shown}, which this filesystem \
                 takes for {git_path_shown}{}, a file git's index tracks, and git would put that \
                 file back in place of what sync wrote, {}",
                shown.note_in_parentheses(),
                nothing_written_with_remedy(
                    leftovers,
                    &format!(
                        "if {claim_named} belongs in this repository, run `git rm --cached -- \
                         {git_path_named}` and commit, then {RUN_SYNC_AGAIN}",
                    )
                )
            )
        }
    }
}

/// How to bring a tracked entry git hides back into the work tree: `git
/// sparse-checkout add` for a sparse checkout that leaves it out, or clearing
/// skip-worktree and then checking the file out. Clearing the flag alone
/// leaves the file deleted, which is why the checkout follows (captured: git
/// 2.53.0).
///
/// The sparse command names the entry's directory, which is what a cone-mode
/// checkout takes. An entry at the root has no directory to name, and in a
/// cone-mode checkout it is never left out; a non-cone checkout can leave it
/// out, and takes the file as a pattern anchored at the root, `/<path>`
/// (captured: `add plain.yml` works too but warns to anchor it).
pub(super) fn bring_back_remedy(git_path: &str) -> String {
    let sparse = match git_path.rsplit_once('/') {
        Some((parent, _name)) => format!("git sparse-checkout add {}", Escaped(parent)),
        None => format!("git sparse-checkout add /{}", Escaped(git_path)),
    };
    let git_path = Escaped(git_path);
    format!(
        "`{sparse}` if a sparse checkout leaves it out, or `git update-index --no-skip-worktree \
         -- {git_path}` and then `git checkout -- {git_path}`"
    )
}

#[cfg(test)]
mod tests {
    use super::{bring_back_remedy, folds_onto_tracked_message, tracked_above_line};
    use crate::survey::poison::{POISON, POISON_FOLDED, assert_escaped_once, claim};
    use crate::sync::fold_variant::fold_variants;
    use crate::sync::proof::AboveEntry;
    use crate::sync::write::{Leftover, LeftoverReason};

    use super::super::poisoned::poisoned_leftover;

    #[test]
    fn a_file_above_names_the_entry_and_offers_the_removal_from_the_index() {
        let line = tracked_above_line(&claim("a/b.yml"), "a", &AboveEntry::File);
        assert_eq!(
            line,
            "a/b.yml cannot be written, because git's index tracks a as a file where a/b.yml \
             needs a directory, so git would put it back in place of what sync wrote: if a/b.yml \
             belongs in this repository, run `git rm --cached -- a` and commit, then run the \
             `sync` task again"
        );
    }

    #[test]
    fn a_symbolic_link_above_says_so_and_the_case_clause_appears_only_for_another_case() {
        let exact = tracked_above_line(&claim("a/b/c.yml"), "a/b", &AboveEntry::SymbolicLink);
        assert!(
            exact.contains("tracks a/b as a symbolic link where"),
            "{exact}"
        );
        assert!(!exact.contains("core.ignorecase"), "{exact}");

        let folded = tracked_above_line(&claim("a/b/c.yml"), "A/b", &AboveEntry::File);
        assert!(
            folded.contains(
                "tracks A/b as a file where a/b/c.yml needs a directory (git takes the two for \
                 one name where it ignores case, core.ignorecase), so git"
            ),
            "{folded}"
        );
    }

    #[test]
    fn a_submodule_above_is_another_repository_with_no_remedy() {
        let line = tracked_above_line(&claim("sub/x.yml"), "sub", &AboveEntry::Submodule);
        assert_eq!(
            line,
            "sub/x.yml is inside sub, which git's index tracks as a submodule, another \
             repository, and sync writes only into this one"
        );
    }

    #[test]
    fn an_entry_of_another_mode_above_names_the_mode_with_no_remedy() {
        let line = tracked_above_line(
            &claim("a/b.yml"),
            "a",
            &AboveEntry::Other {
                mode: "100664".to_owned(),
            },
        );
        assert_eq!(
            line,
            "a/b.yml cannot be written, because git's index tracks a with mode 100664 where \
             a/b.yml needs a directory"
        );
    }

    #[test]
    fn the_bring_back_remedy_names_the_directory_or_anchors_a_root_file() {
        assert_eq!(
            bring_back_remedy("d/f.yml"),
            "`git sparse-checkout add d` if a sparse checkout leaves it out, or `git \
             update-index --no-skip-worktree -- d/f.yml` and then `git checkout -- d/f.yml`"
        );
        assert_eq!(
            bring_back_remedy("f.yml"),
            "`git sparse-checkout add /f.yml` if a sparse checkout leaves it out, or `git \
             update-index --no-skip-worktree -- f.yml` and then `git checkout -- f.yml`"
        );
    }

    #[test]
    fn a_folded_file_names_both_spellings_and_the_way_to_bring_it_back() {
        // The two spellings look alike, so the message shows both as code
        // points and says why; the commands it offers keep the entry's real
        // bytes, since they are to be pasted.
        let claimed = claim("caf\u{e9}.yml");
        let mut found = fold_variants("cafe\u{301}.yml\0".as_bytes(), &[&claimed]);
        let variant = found.remove(0).remove(0);

        let message = folds_onto_tracked_message(&claimed, &variant, &[]);

        assert_eq!(
            message,
            "caf\u{e9}.yml (caf\\u00E9.yml) and cafe\u{301}.yml (cafe\\u0301.yml), which git's \
             index tracks, are one name on this filesystem (the two spellings differ only in \
             Unicode normalization, so each is followed by its characters as \\uXXXX code \
             points, as bash 4.3 or later, or zsh, \
             reads them in $'…' under a UTF-8 locale), and git would take what sync wrote \
             for cafe\u{301}.yml (cafe\\u0301.yml), so sync removed what it had prepared and \
             wrote nothing: bring cafe\u{301}.yml (cafe\\u0301.yml) back into the work tree \
             (`git sparse-checkout add /cafe\u{301}.yml` if a sparse checkout leaves it out, or \
             `git update-index --no-skip-worktree -- cafe\u{301}.yml` and then `git checkout \
             -- cafe\u{301}.yml`), then run the `check` task to see how it is spelled \
             on disk"
        );
    }

    #[test]
    fn a_folded_directory_above_names_the_claimed_directory_and_the_removal() {
        let claimed = claim("caf\u{e9}/x.yml");
        let mut found = fold_variants("cafe\u{301}\0".as_bytes(), &[&claimed]);
        let variant = found.remove(0).remove(0);

        let message = folds_onto_tracked_message(&claimed, &variant, &[]);

        assert_eq!(
            message,
            "caf\u{e9}/x.yml (caf\\u00E9/x.yml) would be written under caf\u{e9} (caf\\u00E9), \
             which this filesystem takes for cafe\u{301} (cafe\\u0301) (the two spellings differ \
             only in Unicode normalization, so each is followed by its characters as \\uXXXX \
             code points, as bash 4.3 or later, or zsh, \
             reads them in $'…' under a UTF-8 locale), a file git's index tracks, and \
             git would put that file back in place of what sync wrote, so sync removed what it \
             had prepared and wrote nothing: if caf\u{e9}/x.yml belongs in this repository, run \
             `git rm --cached -- cafe\u{301}` and commit, then run the `sync` task \
             again"
        );
    }

    #[test]
    fn a_folded_file_that_could_not_be_cleaned_up_names_what_stayed() {
        let claimed = claim("A.yml");
        let mut found = fold_variants(b"a.yml\0", &[&claimed]);
        let variant = found.remove(0).remove(0);
        let leftover = Leftover {
            path: ".A.yml.skeletons-sync".to_owned(),
            reason: LeftoverReason::CouldNotRemove {
                detail: "permission denied".to_owned(),
            },
        };

        let message = folds_onto_tracked_message(&claimed, &variant, &[leftover]);

        assert!(
            message.contains(
                "so sync wrote nothing, and removed what it had prepared except \
                 .A.yml.skeletons-sync (it could not be removed: permission denied): find and \
                 remove it by hand, then bring a.yml back"
            ),
            "{message}"
        );
    }

    #[test]
    fn the_bring_back_remedy_names_the_entry_and_its_directory_escaped_once() {
        // An entry in a directory names the directory for a sparse checkout;
        // an entry at the root names itself. Both shapes repeat the entry in
        // the `git update-index` and `git checkout` commands.
        for git_path in [format!("{POISON}/x.yml"), format!("{POISON}.yml")] {
            assert_escaped_once(&bring_back_remedy(&git_path), &git_path);
        }
    }

    #[test]
    fn a_folded_index_entry_names_the_claim_and_the_entry_escaped_once() {
        // The remedy for a folded directory names the claim and the entry by
        // themselves, not by code points.
        let claimed = claim(&format!("{POISON}/x.yml"));
        let listing = format!("{POISON_FOLDED}\0");
        let variant = fold_variants(listing.as_bytes(), &[&claimed])
            .remove(0)
            .remove(0);
        for leftovers in [Vec::new(), vec![poisoned_leftover()]] {
            assert_escaped_once(
                &folds_onto_tracked_message(&claimed, &variant, &leftovers),
                "a folded directory",
            );
        }
    }
}
