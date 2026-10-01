//! A worn skeleton whose crate carries a build script is rendered exactly
//! like one that carries none: nothing in `skeletons` ever compiles or runs
//! anything a skeleton ships. `check` and `sync` only ever run `cargo
//! metadata` on the wearer's workspace, never `cargo build`, so a
//! failure inside this script would never surface on its own. It writes a
//! marker file to a path named by an environment variable instead. Tests
//! assert the file's absence after `check` or `sync` runs — proof that reaches
//! the test process itself, rather than trusting that a build never happened
//! because nothing failed loudly — and one test builds the crate on purpose
//! and asserts the file's presence, so that absence means something.
fn main() {
    if let Ok(marker_path) = std::env::var("SKELETONS_TEST_ONLY_BUILD_SCRIPT_MARKER") {
        std::fs::write(marker_path, b"the build script ran\n")
            .expect("write the marker file the tests watch for");
    }
    // A distinctive, otherwise impossible status, in case this script ever
    // runs somewhere the marker variable is not set.
    std::process::exit(91);
}
