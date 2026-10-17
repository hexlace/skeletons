//! The cases where a package comes from a git repository, which the test builds as a `file://`
//! source and fetches into a Cargo home before the workspace is changed.

use super::super::cargo_configuration::HomeVariables;
use super::super::fixture::{CargoHome, Fixture, GitRepository, cargo_version};
use super::{
    Verdict, assert_both_say_in_home, assert_reader_refuses, cargo_verdict_in_home, locked_members,
    write_app,
};
use crate::workspace::Network;

/// The line that depends on `g` by the default branch, at `url` as the manifest writes it.
fn dependency_at(url: &str) -> String {
    format!("g = {{ git = \"{url}\" }}\n")
}

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
        Self::with_repository(GitRepository::new("g"))
    }

    /// The cases over `g_repository`, which holds the package `g`.
    fn with_repository(g_repository: GitRepository) -> Self {
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
            // The spelling that `a_git_url_with_an_escaped_character_is_not_the_url_it_decodes_to`
            // sets against the plain one: a different database, so it has to be fetched too.
            dependency_at(&cases.escaped_url()),
        ]);
        cases
    }

    /// `g`'s URL with its last character written as a percent escape, which names the same
    /// directory and is a different URL: `…/repo` and `…/re%70o`.
    fn escaped_url(&self) -> String {
        let url = self.g_repository.url();
        let last = url.chars().last().expect("a URL has a last character");
        assert!(
            last.is_ascii_alphanumeric(),
            "a temporary directory's name ends in a letter or a digit, not {last:?}"
        );
        format!("{}%{:02x}", &url[..url.len() - 1], u32::from(last))
    }

    /// Locks two members, `app` writing `first` and `other` writing `second` as the line that
    /// depends on `g`, and requires cargo and the reader to say `expected` of the result.
    fn assert_two_members(&self, first: &str, second: &str, expected: Verdict) {
        let fixture = locked_members(&["app", "other"]);
        write_app(&fixture, &format!("[dependencies]\n{first}"));
        fixture.package("other", "other", &format!("[dependencies]\n{second}"));
        let toolchain = cargo_version();
        self.home
            .lock(&fixture.root, Network::Allowed)
            .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));

        assert_both_say_in_home(&fixture, &self.home, expected);
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

    /// Locks `app` with `dependency`, a line that depends on `g`, and requires that Cargo calls
    /// the lockfile current, as it is just written, and that the reader refuses it with the
    /// detail `detail` gives for the fixture.
    fn assert_current_to_cargo_and_refused_by_the_reader(
        &self,
        dependency: &str,
        detail: impl FnOnce(&Fixture) -> String,
    ) {
        let fixture = locked_members(&["app"]);
        write_app(&fixture, &format!("[dependencies]\n{dependency}"));
        let toolchain = cargo_version();
        self.home
            .lock(&fixture.root, Network::Allowed)
            .unwrap_or_else(|stderr| panic!("{toolchain} could not lock the workspace: {stderr}"));

        assert_eq!(
            cargo_verdict_in_home(&fixture, &self.home),
            Verdict::Current,
            "cargo, on {toolchain}"
        );
        assert_reader_refuses(
            &fixture,
            &HomeVariables::cargo_home(self.home.path()),
            &detail(&fixture),
        );
    }
}

/// The words the reader refuses the dependency `g` of the member `app` with when `url`, as the
/// manifest writes it, carries a query of its own. The manifest is read before the lockfile, so
/// it is the manifest that is refused, whatever Cargo then wrote in the lock.
fn query_refusal(fixture: &Fixture, url: &str) -> String {
    format!(
        "{}: [dependencies] `g`: `git = \"{url}\"` has a query of its own, which Cargo keeps in \
         the lockfile's source and this reader does not follow",
        fixture.root.join("app/Cargo.toml").display()
    )
}

/// The cases over a repository whose directory is named `repo?x=1`: the query reaches git as
/// part of a `file://` path, so a URL with that query names it.
fn cases_with_a_query_in_the_url() -> GitCases {
    GitCases::with_repository(GitRepository::in_directory_named("g", "repo?x=1"))
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

#[test]
fn a_git_url_respelled_with_a_trailing_slash_or_dot_git_is_current() {
    // Cargo compares git sources by their canonical URL, which drops a trailing slash and a
    // `.git`, so a lockfile written for `…/g` is current for `…/g/` and `…/g.git`, and the
    // other way round for the slash. Only the spelling that was locked is fetched: the
    // respellings name the same database.
    let cases = GitCases::new();
    let url = cases.g_repository.url();
    let plain = dependency_at(&url);

    cases.assert_change(&plain, &dependency_at(&format!("{url}/")), Verdict::Current);
    cases.assert_change(
        &plain,
        &dependency_at(&format!("{url}.git")),
        Verdict::Current,
    );
    cases.assert_change(&dependency_at(&format!("{url}/")), &plain, Verdict::Current);
}

// There is no `github.com` case beside these. Cargo reads a GitHub URL by other rules, and a
// differential for them would need GitHub's git database built offline, which means recreating
// Cargo's cache layout under a directory named by a hash that differs between 1.85 and 1.95.
// The GitHub rules are held instead by the fixed-value tests in `canonical_git_url.rs`, which
// carry Cargo's own test pairs.

#[test]
fn a_git_url_respelled_from_dot_git_to_plain_is_current() {
    // The respelling the case above cannot reach with a directory named `g`: a `.git` spelling
    // of it names no repository to fetch. A directory named `g.git` gives one, so the lockfile
    // is written for `…/g.git` and the dependency then declares `…/g` and `…/g/`, which Cargo
    // reads as the same source once the `.git` is dropped. Only the locked spelling is fetched:
    // the plain ones name no directory, so they cannot be, and a lockfile that is current needs
    // no fetch.
    let repository = GitRepository::in_directory_ending("g", ".git");
    let dot_git = repository.url();
    let plain = dot_git
        .strip_suffix(".git")
        .expect("the directory's name ends in .git")
        .to_owned();
    let cases = GitCases::with_repository(repository);

    cases.assert_change(
        &dependency_at(&dot_git),
        &dependency_at(&plain),
        Verdict::Current,
    );
    cases.assert_change(
        &dependency_at(&dot_git),
        &dependency_at(&format!("{plain}/")),
        Verdict::Current,
    );
}

#[test]
fn two_members_spelling_one_git_url_differently_are_current_once_locked() {
    // The lockfile holds the first member's spelling, and Cargo reads the second as the same
    // source, so neither order is out of date.
    let cases = GitCases::new();
    let url = cases.g_repository.url();
    let plain = dependency_at(&url);
    let slashed = dependency_at(&format!("{url}/"));

    cases.assert_two_members(&plain, &slashed, Verdict::Current);
    cases.assert_two_members(&slashed, &plain, Verdict::Current);
}

#[test]
fn a_file_url_naming_localhost_is_current_once_locked() {
    // `file://localhost/x` is `file:///x` to the URL parser, and the lockfile holds the parsed
    // form, which only a parser reads back as the same URL.
    let cases = GitCases::new();
    let url = cases.g_repository.url();
    let path = url
        .strip_prefix("file://")
        .expect("the fixture's URL is a file URL");

    cases.assert_current(&dependency_at(&format!("file://localhost{path}")));
}

#[test]
fn a_git_url_with_a_query_of_its_own_is_current_to_cargo_and_refused_by_the_reader() {
    // Cargo keeps the URL's `?x=1` in the lock (`git+…/repo?x=1#<commit>`) and reads it back as
    // the same source. The reader does not model that, and refuses the dependency by name
    // rather than call the lock stale.
    let cases = cases_with_a_query_in_the_url();
    let url = cases.g_repository.url();

    cases.assert_current_to_cargo_and_refused_by_the_reader(&cases.g_dependency(""), |fixture| {
        query_refusal(fixture, &url)
    });
}

#[test]
fn a_git_url_with_a_query_of_its_own_and_a_branch_is_current_to_cargo_and_refused_by_the_reader() {
    // The reference is written after the URL's own query (`…/repo?x=1?branch=dev`), which is
    // not one `branch` either.
    let cases = cases_with_a_query_in_the_url();
    let url = cases.g_repository.url();

    cases.assert_current_to_cargo_and_refused_by_the_reader(
        &cases.g_dependency("branch = \"dev\""),
        |fixture| query_refusal(fixture, &url),
    );
}

#[test]
fn a_git_url_with_an_escaped_character_is_not_the_url_it_decodes_to() {
    // The over-merging guard: `…/re%70o` and `…/repo` name the same directory, and Cargo keeps
    // them apart because it does not decode an escape in the path. A comparison that decoded
    // would call this current. Both are fetched first, so the abort is the lockfile's.
    let cases = GitCases::new();

    cases.assert_change(
        &dependency_at(&cases.g_repository.url()),
        &dependency_at(&cases.escaped_url()),
        Verdict::Aborted,
    );
    cases.assert_change(
        &dependency_at(&cases.escaped_url()),
        &dependency_at(&cases.g_repository.url()),
        Verdict::Aborted,
    );
}
