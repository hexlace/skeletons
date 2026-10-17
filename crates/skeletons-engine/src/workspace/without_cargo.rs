//! Reading a workspace's files as text, with no Cargo to run: `Cargo.lock`
//! and each `Cargo.toml`, each turned into only what a reader of a workspace
//! without Cargo needs of it.
//!
//! Everything here is a pure function over text and paths. Nothing reads a
//! file, and nothing here decides what a manifest or a lockfile means for a
//! workspace as a whole.

pub(super) mod lockfile;
pub(super) mod package_manifest;
pub(super) mod text_error;
