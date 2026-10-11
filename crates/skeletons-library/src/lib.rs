#![doc = include_str!("../readme.md")]
//!
//! ## What this crate provides
//!
//! One function, [`check`], reads a workspace and returns a [`Report`]. The
//! report holds each worn skeleton ([`WornSkeleton`]) with the files it claims
//! ([`Files`], [`ClaimedFile`], [`FileState`]), and every refusal
//! ([`Refusal`], [`RefusalKind`], [`Wearing`]) the check could not get past.
//! Nothing here writes, nothing needs the network, and nothing is a sentence
//! about a workspace: the answer is data, and the words are the caller's.
//!
//! Every struct and enum is `#[non_exhaustive]`, so a new state, kind or field
//! is not a breaking change. A match on [`Files`], [`SkeletonsNamed`],
//! [`FileState`] or [`RefusalKind`] therefore needs a wildcard arm, and what
//! the docs of each say that arm must mean is the part to read.

// Every value of these types comes from `check` at run time, so no constant
// context can hold one. `const` on an accessor is a promise about its
// representation that buys a caller nothing and is a breaking change to take
// back, which is why none is written, and the lint that asks for it is
// expected here once instead of at each accessor.
#![expect(
    clippy::missing_const_for_fn,
    reason = "no value of these types exists before run time, and `const` on an accessor \
              would promise a representation that cannot be taken back"
)]

mod claimed_file;
mod files;
mod read_workspace_error;
mod refusal;
mod report;
mod skeletons_named;
mod version;
mod worn_skeleton;

use std::path::Path;

use skeletons_engine::{Network, survey};

pub use claimed_file::{ClaimedFile, FileState};
pub use files::Files;
pub use read_workspace_error::ReadWorkspaceError;
pub use refusal::{Refusal, RefusalKind, Wearing};
pub use report::Report;
pub use skeletons_named::SkeletonsNamed;
pub use version::Version;
pub use worn_skeleton::WornSkeleton;

// Every public type crosses threads, which a caller's async code relies on.
// One field that was not `Send` or `Sync` would break that for every caller
// without a word, so the build says so instead.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Report>();
    assert_send_sync::<SkeletonsNamed<'static>>();
    assert_send_sync::<WornSkeleton>();
    assert_send_sync::<Files<'static>>();
    assert_send_sync::<ClaimedFile>();
    assert_send_sync::<FileState>();
    assert_send_sync::<Refusal>();
    assert_send_sync::<RefusalKind>();
    assert_send_sync::<Wearing>();
    assert_send_sync::<Version>();
    assert_send_sync::<ReadWorkspaceError>();
};

/// Reads the workspace `directory` belongs to and compares every file its worn
/// skeletons claim with what is on disk.
///
/// Cargo walks up from `directory` to the workspace root, and a relative
/// `directory` is taken from the process's current directory. The call needs
/// `cargo`, a `Cargo.lock` that is up to date, and every locked source already
/// fetched. The program `$CARGO` names is run whenever that variable is set,
/// exactly as given and an empty value included; `cargo` from `PATH` is run only
/// when it is not set, and a value that is not valid Unicode counts as not set.
///
/// Nothing else is needed. The workspace need not have been built, and
/// `directory` need not be a git repository. For a workspace whose
/// dependencies are all `path` dependencies, `cargo generate-lockfile
/// --offline` is enough to make the lockfile. The call writes nothing in the workspace, runs no
/// build script, needs no git, and makes no network request: a source that was
/// never fetched is an error, never a download. Cargo may still write to its
/// own cache under `CARGO_HOME`, as any cargo command may, to unpack a source
/// it downloaded earlier; where that cache cannot be written, every locked
/// source must already be unpacked, as `cargo fetch` leaves it.
///
/// A wearing that cannot be read is not an error. It is a [`Refusal`] in the
/// [`Report`], and every other wearing is still reported.
///
/// # Errors
///
/// - [`is_lockfile_not_current`](ReadWorkspaceError::is_lockfile_not_current)
///   when `Cargo.lock` is missing or out of date.
/// - [`is_cargo_metadata_failed`](ReadWorkspaceError::is_cargo_metadata_failed)
///   when cargo ran and failed, for example on a source that was never fetched.
/// - [`is_cargo_not_started`](ReadWorkspaceError::is_cargo_not_started) when
///   cargo could not be started although `directory` is a directory. A
///   `directory` that cannot be entered is reported this way too, and
///   [`detail`](ReadWorkspaceError::detail) holds `failed to run: ` followed by
///   the operating system's words, which do not say which.
/// - [`is_directory_not_found`](ReadWorkspaceError::is_directory_not_found)
///   when cargo could not be started and no directory was found at
///   `directory`.
/// - [`is_metadata_unreadable`](ReadWorkspaceError::is_metadata_unreadable)
///   when cargo's output could not be used.
///
/// # Examples
///
/// ```no_run
/// let report = skeletons_library::check(".")?;
/// println!("{} skeletons worn", report.skeletons().len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn check(directory: impl AsRef<Path>) -> Result<Report, ReadWorkspaceError> {
    let directory = directory.as_ref();
    let workspace = skeletons_engine::read(directory, Network::Refused)
        .map_err(|error| ReadWorkspaceError::from_engine(error, directory))?;
    Ok(Report::from_survey(&survey(&workspace)))
}
