//! The word each refusal kind reports, which is the `kind` the command line's
//! JSON documents for it. A caller prints or keys on these, so a change to one
//! is a change to the crate's interface.

use skeletons_library::RefusalKind;

#[test]
fn each_kind_reports_the_word_the_command_line_documents() {
    // Given each of the ten kinds, `as_str` is the lowercase, hyphenated word
    // that `check --json` reports as the refusal's `kind`. The pairs are
    // written out here, so a word changed in the library alone fails.
    let documented = [
        (RefusalKind::NotATable, "not-a-table"),
        (RefusalKind::ReservedKey, "reserved-key"),
        (RefusalKind::NamesNoDependency, "names-no-dependency"),
        (RefusalKind::Unresolved, "unresolved"),
        (RefusalKind::Ambiguous, "ambiguous"),
        (RefusalKind::NotASkeleton, "not-a-skeleton"),
        (RefusalKind::OptionRefused, "option-refused"),
        (RefusalKind::SkeletonInvalid, "skeleton-invalid"),
        (RefusalKind::Overlap, "overlap"),
        (RefusalKind::UnsafePath, "unsafe-path"),
    ];

    for (kind, word) in documented {
        assert_eq!(kind.as_str(), word, "{kind:?}");
    }
}
