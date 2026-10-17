//! The one `source` string that names crates.io.

/// The source `cargo metadata` reports for a crates.io dependency, always
/// exactly this string — including when `[source.crates-io] replace-with`
/// serves it from somewhere else, and including in the offline,
/// vendored-directory fixtures `ritual`'s tests use. `Cargo.lock` writes the
/// same string. Exposed for `check/json.rs`, which reports it verbatim as a
/// registry pin's `source`, and for reading a workspace without Cargo, which
/// holds a registry declaration with no `registry =` name to it.
pub(crate) const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
