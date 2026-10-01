//! Every git process this crate spawns, built one way.
//!
//! [`command()`] removes the variables that redirect which repository, work
//! tree, index, object store or attribute source git answers about, and the
//! ones that change what this crate's own pathspec arguments mean, from
//! every invocation it builds — so `check`'s own queries can never
//! accidentally answer about a wearer's repository, and `sync`'s own
//! refusal (`sync::work_tree::open`, which checks
//! [`redirecting_variables_set`] before running git at all) is backed by a
//! removal that holds regardless of whether that refusal ever ran.
//! [`diagnostic()`] reads the one line of git's own stderr every abort message
//! here shows. [`ObjectId`] and [`RepositoryPrefix`] are the two small,
//! validated types `sync`'s own proof (`sync::proof`) and work-tree
//! (`sync::work_tree`) modules, and `behind`'s own directory-scoped `behind`
//! comparison, build git's own answers into. [`command_for_new_repository`]
//! is [`command()`] plus the removal `behind`'s own temporary fetch target
//! needs on top; [`LOCAL_GIT_TIMEOUT`] bounds every local, read-only git
//! question this crate asks, shared rather than each caller picking its own
//! number (a test build can shorten it, see `local_timeout.rs`). [`run_local`]
//! and [`run_bounded`] are the one place a built command is run under those
//! bounds and the shared output caps.

mod command;
mod diagnostic;
mod local_timeout;
mod object_id;
mod prefix;
mod run;

pub(crate) use command::{
    LOCAL_GIT_TIMEOUT, Locale, command, command_for_new_repository, command_for_pathspec_magic,
    path_beneath_current_directory, pathspec_at_depth, pathspec_exactly_ignoring_case,
    pathspec_ignoring_case, redirecting_variables_set,
};
pub(crate) use diagnostic::diagnostic;
pub(crate) use local_timeout::local_timeout;
pub(crate) use object_id::ObjectId;
pub(crate) use prefix::RepositoryPrefix;
pub(crate) use run::{run_bounded, run_local};
