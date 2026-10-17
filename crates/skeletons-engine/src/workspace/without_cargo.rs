//! Reading a workspace from its files with no Cargo to run: `Cargo.lock` and each `Cargo.toml`,
//! each turned into only what a reader of a workspace without Cargo needs of it, and then the
//! workspace itself, built from them.
//!
//! The lockfile and manifest readers are pure functions over text. Reading the workspace is the
//! only part that touches the file system, and it ends in the same document the readers of
//! `cargo metadata`'s output share.

mod currency;
mod document;
mod error;
mod file_system;
mod layout;
mod located;
mod lock_entries;
pub(super) mod lockfile;
mod member_patterns;
pub(super) mod package_manifest;
mod path_package;
mod read;
mod targets;
pub(super) mod text_error;
mod unsupported;
mod workspace_search;

#[cfg(test)]
mod currency_differential;
#[cfg(test)]
mod differential;
#[cfg(test)]
mod fixture;

pub use error::ReadWithoutCargoError;
pub use read::read_without_cargo;
