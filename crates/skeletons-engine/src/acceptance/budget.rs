//! The read-budget refusal describes the skeleton's own budget, not the size of
//! whichever file happened to cross it: the budget is shared across the
//! manifest, every file and every partial, so a file well under the limit on
//! its own can still be the one that tips the skeleton over it.

use super::test_skeleton;
use crate::skeleton::{Choices, Reason, render};

/// The render's read-budget constant, restated here because
/// `crate::skeleton::limits` is private to the `skeleton` module tree and
/// unreachable from `acceptance`. The fixture this test reads was built
/// against this exact number.
const BYTES_MAX: u64 = 1024 * 1024;

#[test]
fn a_file_under_the_budget_alone_is_refused_when_the_manifest_pushes_the_skeleton_over_it() {
    // `Cargo.toml` is 147 bytes; `files/big.txt` is 1,048,434 bytes -- under
    // the 1,048,576-byte budget entirely on its own. Read after the
    // manifest, the two together total 1,048,581 bytes: five bytes past the
    // budget. The refusal must describe that -- the skeleton's shared budget was
    // passed -- and must not describe `big.txt` as though it alone held more
    // bytes than the budget allows, which it does not.
    let error = render(
        test_skeleton("refused/manifest-plus-file-together-exceed-byte-budget"),
        &Choices::new(),
    )
    .expect_err("a manifest and a file that together cross the read budget must be refused");

    assert_eq!(error.file(), Some("files/big.txt"));
    assert!(
        matches!(error.reason(), Reason::TooManyBytes { bytes_max } if *bytes_max == BYTES_MAX),
        "expected a byte-budget refusal reporting {BYTES_MAX}, got {error:?}"
    );

    let displayed = error.reason().to_string();
    assert!(
        displayed.contains(&BYTES_MAX.to_string()),
        "the message must name the skeleton's own {BYTES_MAX}-byte budget: got {displayed:?}"
    );
    assert!(
        displayed.contains("budget"),
        "the message must describe the skeleton's read budget being passed, not merely a byte \
         count -- got {displayed:?}"
    );
    assert!(
        !displayed.contains("1048434"),
        "the message must not describe big.txt's own size (1,048,434 bytes, under the budget \
         on its own) as though the file alone held more than the budget: got {displayed:?}"
    );
}
