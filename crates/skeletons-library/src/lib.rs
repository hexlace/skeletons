#![doc = include_str!("../readme.md")]
//!
//! ## What this crate provides
//!
//! Two functions read a workspace and return a [`Report`]. [`check`] asks
//! cargo. [`check_without_cargo`] reads the manifests and `Cargo.lock` as
//! files, for a caller that has no cargo, and is told where the files of each
//! locked package that is not a path on disk are ([`PackageLocations`],
//! [`LockedPackage`]). The report holds each worn skeleton ([`WornSkeleton`])
//! with the files it claims ([`Files`], [`ClaimedFile`], [`FileState`]), and
//! every refusal ([`Refusal`], [`RefusalKind`], [`Wearing`]) the check could
//! not get past. Nothing here writes, nothing needs the network, and nothing
//! is a sentence about a workspace: the answer is data, and the words are the
//! caller's.
//!
//! Every struct and enum is `#[non_exhaustive]`, so a new state, kind or field
//! is not a breaking change. The one exception is [`LockedPackage`], whose
//! three public fields are a lockfile's whole identity for a package, so a
//! caller builds it with a struct literal that names each one. A match on
//! [`Files`], [`SkeletonsNamed`], [`FileState`] or [`RefusalKind`] therefore
//! needs a wildcard arm, and what the docs of each say that arm must mean is
//! the part to read.

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
mod locked_package;
mod package_locations;
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
pub use locked_package::LockedPackage;
pub use package_locations::PackageLocations;
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
    assert_send_sync::<LockedPackage>();
    assert_send_sync::<PackageLocations>();
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
/// --offline` is enough to make the lockfile. The call writes nothing in the
/// workspace, runs no build script, needs no git, and makes no network
/// request: a source that was never fetched is an error, never a download.
/// Cargo may still write to its
/// own cache under `CARGO_HOME`, as any cargo command may, to unpack a source
/// it downloaded earlier; where that cache cannot be written, every locked
/// source must already be unpacked, as `cargo fetch` leaves it.
///
/// Building the workspace does not fetch every locked source: `cargo build`
/// leaves out packages locked for other platforms, and dev-dependencies, which
/// is how `wear` adds a skeleton. On a machine that has done no more than
/// build, the call fails as a cargo that ran and failed. Run
/// `cargo fetch --locked` first.
///
/// A wearing that cannot be read is not an error. It is a [`Refusal`] in the
/// [`Report`], and every other wearing is still reported.
///
/// Where there is no cargo to run, or no cargo home holding the locked
/// sources, use [`check_without_cargo`], which returns the same [`Report`] for
/// the same workspace and files, bar a lockfile stale in one of the two ways
/// only cargo can see, which its documentation describes.
///
/// # Errors
///
/// - [`is_lockfile_not_current`](ReadWorkspaceError::is_lockfile_not_current)
///   when `Cargo.lock` is missing or out of date.
/// - [`is_cargo_metadata_failed`](ReadWorkspaceError::is_cargo_metadata_failed)
///   when cargo ran and failed, for example on a source that was never fetched:
///   [`detail`](ReadWorkspaceError::detail) then names the package cargo failed
///   to download and says that `--offline` was specified.
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

/// Reads the workspace `directory` belongs to from its manifests and
/// `Cargo.lock` as files, running no cargo, and compares every file its worn
/// skeletons claim with what is on disk.
///
/// The [`Report`] is the one [`check`] returns for the same workspace and
/// files, and a workspace whose lockfile `check` aborts on gets the same abort,
/// except for a lockfile stale in one of the two ways described below, which
/// `check` aborts on and this call reads as it stands. That is what a caller
/// that builds without cargo can use: no cargo, no cargo home, and no `PATH`
/// are needed, and nothing is run.
///
/// A relative `directory` is taken from the process's current directory, and
/// any directory at or below the workspace will do; the manifests above it are
/// found as cargo finds them. A skeleton that is a `path` dependency is found
/// from the manifests. A skeleton from a registry or a git repository is not a
/// path on disk, so `locations` says where its files are: the directory
/// holding that package's own `Cargo.toml`, for each package, keyed by the
/// name, version and source `Cargo.lock` writes for it.
///
/// A wearing whose package is not in `locations`, or is there at a directory
/// whose `Cargo.toml` names another package or another version, is not an
/// error. It is a [`Refusal`] of kind [`RefusalKind::Unlocated`] in the
/// [`Report`], whose [`unlocated_package`](Refusal::unlocated_package) is the
/// identity to supply, and every other wearing is still reported.
///
/// The `Cargo.lock` must be up to date, and the call checks what a reader of
/// the files can see: that every package on disk is locked, that the entries
/// agree with the manifests' declarations, and that the lock holds nothing
/// cargo would drop. Two stale shapes it cannot see, because seeing them is
/// cargo's own resolution. One is a package outside the workspace whose
/// optional dependency is newly enabled through a feature. The other is a
/// dependency moved to a registry with a different index. `check` aborts on
/// both. A lock stale in either way is read as it stands, which is the one
/// place this call's lockfile abort and `check`'s differ.
///
/// A workspace that uses something only cargo reads is refused rather than
/// guessed at: `[patch]`, `[replace]`, `package.workspace`, one package under
/// two dependency keys, two path packages with one name and version, a
/// lockfile in a format other than 3 or 4.
///
/// The call writes nothing, runs no build script, needs no git and makes no
/// network request.
///
/// # Errors
///
/// - [`is_lockfile_not_current`](ReadWorkspaceError::is_lockfile_not_current)
///   when `Cargo.lock` is missing or out of date.
/// - [`is_directory_not_found`](ReadWorkspaceError::is_directory_not_found)
///   when `directory` cannot be read: nothing is there, it is not a directory,
///   or it is not readable.
/// - [`is_manifest_or_lockfile_unreadable`](ReadWorkspaceError::is_manifest_or_lockfile_unreadable)
///   when a manifest or the lockfile cannot be read, or is one cargo would
///   refuse too.
/// - [`is_unsupported_without_cargo`](ReadWorkspaceError::is_unsupported_without_cargo)
///   when the workspace uses something only cargo reads. `check` reads it
///   where cargo is available.
///
/// [`is_cargo_metadata_failed`](ReadWorkspaceError::is_cargo_metadata_failed),
/// [`is_cargo_not_started`](ReadWorkspaceError::is_cargo_not_started) and
/// [`is_metadata_unreadable`](ReadWorkspaceError::is_metadata_unreadable) are
/// never true of these errors, since nothing is run.
///
/// # Examples
///
/// A workspace whose skeletons are all path dependencies needs no locations:
///
/// ```no_run
/// use skeletons_library::PackageLocations;
///
/// let report = skeletons_library::check_without_cargo(".", &PackageLocations::new())?;
/// println!("{} skeletons worn", report.skeletons().len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn check_without_cargo(
    directory: impl AsRef<Path>,
    locations: &PackageLocations,
) -> Result<Report, ReadWorkspaceError> {
    let workspace =
        skeletons_engine::read_without_cargo(directory.as_ref(), &locations.to_engine())
            .map_err(ReadWorkspaceError::from_engine_without_cargo)?;
    Ok(Report::from_survey(&survey(&workspace)))
}
