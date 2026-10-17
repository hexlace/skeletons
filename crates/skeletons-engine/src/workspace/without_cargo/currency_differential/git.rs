//! The cases where a package comes from a git repository, which the test builds as a `file://`
//! source and fetches into a Cargo home before the workspace is changed.

use super::super::fixture::{CargoHome, GitRepository, cargo_version};
use super::{Verdict, assert_both_say_in_home, locked_members, write_app};
use crate::workspace::Network;

/// Two git repositories, one for the package `g` and one for the package `h` (the names the
/// manifests write), in a Cargo home that has already fetched every reference the cases below
/// write. Fetching first matters: a reference the home has never seen fails offline as a missing
/// reference, where a home that has it gives the lockfile abort the cases are about, which is
/// what an online run sees after Cargo fetches the new reference.
struct GitCases {
    home: CargoHome,
    g_repository: GitRepository,
    h_repository: GitRepository,
    /// The commit that `g_repository`'s `main` and `v1` name.
    first: String,
    /// The commit that `g_repository`'s `dev` names.
    second: String,
}

impl GitCases {
    fn new() -> Self {
        let g_repository = GitRepository::new("g");
        let first = g_repository.revision("main");
        let second = g_repository.revision("dev");
        assert_ne!(first, second, "dev is a commit ahead of main");
        let cases = Self {
            home: CargoHome::new(),
            g_repository,
            h_repository: GitRepository::new("h"),
            first,
            second,
        };
        cases.home.prime(&[
            cases.g_dependency(""),
            cases.g_dependency("branch = \"main\""),
            cases.g_dependency("branch = \"dev\""),
            cases.g_dependency("tag = \"v1\""),
            cases.g_dependency(&format!("rev = \"{}\"", cases.first)),
            cases.g_dependency(&format!("rev = \"{}\"", cases.second)),
            cases.h_dependency(),
        ]);
        cases
    }

    /// The line that depends on `g` by `reference`, as written after the URL: nothing for the
    /// default branch, otherwise `branch = "…"`, `tag = "…"` or `rev = "…"`.
    fn g_dependency(&self, reference: &str) -> String {
        let separator = if reference.is_empty() { "" } else { ", " };
        format!(
            "g = {{ git = \"{}\"{separator}{reference} }}\n",
            self.g_repository.url()
        )
    }

    /// The line that depends on `h` by its default branch.
    fn h_dependency(&self) -> String {
        format!("h = {{ git = \"{}\" }}\n", self.h_repository.url())
    }

    /// Locks `app` with the dependencies `before`, changes them to `after`, and requires cargo
    /// and the reader to say `expected` of the result.
    fn assert_change(&self, before: &str, after: &str, expected: Verdict) {
        let fixture = locked_members(&["app"]);
        write_app(&fixture, &format!("[dependencies]\n{before}"));
        let toolchain = cargo_version();
        self.home
            .lock(&fixture.root, Network::Allowed)
            .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));
        write_app(&fixture, &format!("[dependencies]\n{after}"));

        assert_both_say_in_home(&fixture, &self.home, expected);
    }

    /// Requires cargo and the reader to say the lockfile is current for `dependencies`, just
    /// locked.
    fn assert_current(&self, dependencies: &str) {
        self.assert_change(dependencies, dependencies, Verdict::Current);
    }
}

#[test]
fn a_git_dependency_just_locked_is_current_however_its_reference_is_spelled() {
    // Every spelling of a git reference, locked and left alone: the default branch, a branch, a
    // tag, a full revision and a short one.
    let cases = GitCases::new();
    let short = &cases.first[..7];

    cases.assert_current(&cases.g_dependency(""));
    cases.assert_current(&cases.g_dependency("branch = \"main\""));
    cases.assert_current(&cases.g_dependency("tag = \"v1\""));
    cases.assert_current(&cases.g_dependency(&format!("rev = \"{}\"", cases.first)));
    cases.assert_current(&cases.g_dependency(&format!("rev = \"{short}\"")));
}

#[test]
fn a_git_dependency_moved_to_another_branch_is_aborted() {
    let cases = GitCases::new();

    cases.assert_change(
        &cases.g_dependency("branch = \"main\""),
        &cases.g_dependency("branch = \"dev\""),
        Verdict::Aborted,
    );
}

#[test]
fn a_git_dependency_moved_to_another_revision_is_aborted() {
    let cases = GitCases::new();

    cases.assert_change(
        &cases.g_dependency(&format!("rev = \"{}\"", cases.first)),
        &cases.g_dependency(&format!("rev = \"{}\"", cases.second)),
        Verdict::Aborted,
    );
}

#[test]
fn a_git_dependency_moved_from_a_tag_to_a_branch_is_aborted() {
    let cases = GitCases::new();

    cases.assert_change(
        &cases.g_dependency("tag = \"v1\""),
        &cases.g_dependency("branch = \"dev\""),
        Verdict::Aborted,
    );
}

#[test]
fn a_git_dependency_given_the_branch_it_already_followed_is_aborted() {
    // The default branch and `branch = "main"` are the same commit, and the lockfile still
    // records which was written, so Cargo wants it rewritten.
    let cases = GitCases::new();

    cases.assert_change(
        &cases.g_dependency(""),
        &cases.g_dependency("branch = \"main\""),
        Verdict::Aborted,
    );
}

#[test]
fn an_added_git_dependency_is_aborted() {
    let cases = GitCases::new();

    cases.assert_change(
        &cases.g_dependency(""),
        &format!("{}{}", cases.g_dependency(""), cases.h_dependency()),
        Verdict::Aborted,
    );
}

#[test]
fn a_removed_git_dependency_is_aborted() {
    let cases = GitCases::new();

    cases.assert_change(&cases.g_dependency(""), "", Verdict::Aborted);
}
