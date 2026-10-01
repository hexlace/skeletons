//! Tells the test build whether it is running from a repository checkout or
//! from the published package.
//!
//! Some of this crate's tests read files the package leaves out: the test
//! skeletons, the captured crates.io responses and `cargo` output, and the
//! design documents that sit above the crate directory (see `exclude` in
//! `Cargo.toml`). Those tests are compiled only when `skeletons_checkout` is
//! set, which this script does exactly when `test-skeletons/` exists beside
//! the manifest: a checkout has it, an unpacked `.crate` never does. Without
//! that, a test run from the package would fail to compile, or fail at run
//! time on a path that was never there.
//!
//! The answer is a property of the directory the crate was unpacked or
//! cloned into. The script reruns when it is edited and, in a checkout, when
//! `test-skeletons/` is removed. It watches no directory, so nothing a build
//! writes can make the next one rerun it, and it does not notice the
//! directory coming back: a checkout that lost `test-skeletons/` and has it
//! again needs `cargo clean -p skeletons`.

use std::error::Error;
use std::path::Path;

/// The directory whose presence marks a checkout: the one `exclude` in
/// `Cargo.toml` keeps out of the package entirely.
const CHECKOUT_MARKER_DIRECTORY: &str = "test-skeletons";

/// A file [`CHECKOUT_MARKER_DIRECTORY`] always holds: it keeps git from
/// rewriting the test skeletons' bytes, which their tests compare exactly.
const CHECKOUT_MARKER_FILE: &str = "test-skeletons/.gitattributes";

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory =
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or("cargo did not set CARGO_MANIFEST_DIR")?;

    let manifest_directory = Path::new(&manifest_directory);

    // Cargo reads these directives from this process's stdout.
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(skeletons_checkout)");
    // `try_exists`, not `exists`: a directory that is there but cannot be
    // looked at fails the build rather than reading as a package and
    // compiling the checkout's tests away without a word.
    if manifest_directory
        .join(CHECKOUT_MARKER_DIRECTORY)
        .try_exists()?
    {
        println!("cargo::rustc-cfg=skeletons_checkout");
        // Watched so that removing the directory reruns this script. One file
        // the directory always holds, not the directory itself: Cargo scans a
        // watched directory recursively, so every edit to a test skeleton
        // would rebuild the crate, which otherwise needs no rebuild for one.
        println!("cargo::rerun-if-changed={CHECKOUT_MARKER_FILE}");
    }
    // Without the directory nothing more is watched. Watching the marker file
    // would rerun the script on every build, since Cargo reads a path that
    // does not exist as changed; watching the crate directory would too
    // wherever the target directory sits inside it, as it does by default
    // when a package is built on its own.
    Ok(())
}
