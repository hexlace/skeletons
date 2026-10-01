//! Building one git [`Command`]: every one this crate builds comes from one
//! shared body, so none can drift from another on what it removes.

use std::num::NonZeroUsize;
use std::process::Command;
use std::time::Duration;

use crate::claim::ClaimPath;

/// Variables that redirect which repository, work tree, index, object
/// store, or attribute source git answers from (git(1), "The Git
/// Repository"; every variable it lists there was audited against captured
/// behaviour). `sync::work_tree::open` refuses outright, naming the
/// variable, when any of these is set — before running git at all — and
/// [`command`] removes every one of them anyway (the refusal is the first
/// line of defence, this removal the second, tested by
/// `command_removes_every_redirecting_and_pathspec_variable` below), so
/// `check`'s own git, which must never answer about a wearer's repository
/// through a variable set on the outer process, is safe the same way with
/// no refusal needed at all.
pub(crate) const REDIRECTING_VARIABLES: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_ATTR_SOURCE",
];

/// Variables that change what this crate's own pathspec arguments mean.
/// `--literal-pathspecs` — the global option every [`command`] passes —
/// makes git refuse a command that has a pathspec alongside
/// `GIT_GLOB_PATHSPECS` or `GIT_ICASE_PATHSPECS` (git calls the combination
/// "incompatible"; captured: git 2.53.0, `GIT_GLOB_PATHSPECS=1 git
/// --literal-pathspecs status -- x` exits 128 with that message, and
/// `GIT_ICASE_PATHSPECS=1` does the same). Git reads the settings only when
/// the command line has a pathspec, so `status` alone runs. `GIT_LITERAL_PATHSPECS`
/// sets the same reading `--literal-pathspecs` already gives, and
/// `GIT_NOGLOB_PATHSPECS` is not one of the settings git treats as
/// conflicting, so neither of those two ever triggers the refusal —
/// verified the same way, both exit `0`. All four are removed anyway, proven
/// by this module's own `command_removes_every_redirecting_and_pathspec_variable`
/// below: this crate's own flag decides how a pathspec is read, never
/// whichever of git's own compatibility rules a wearer's environment happens
/// to satisfy.
const PATHSPEC_VARIABLES: [&str; 4] = [
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

/// Variables that only shape a repository git itself is about to create —
/// meaningless to every read-only question this crate asks, except the one
/// place it ever creates a repository of its own:
/// [`command_for_new_repository`], `behind`'s own temporary fetch target.
/// Left alone for every other command ([`command`]): a wearer's own
/// `GIT_INDEX_VERSION`, `GIT_DEFAULT_HASH` or `GIT_DEFAULT_REF_FORMAT`
/// changes nothing about how git answers a read.
const NEW_REPOSITORY_VARIABLES: [&str; 2] = ["GIT_DEFAULT_HASH", "GIT_DEFAULT_REF_FORMAT"];

/// How long any of this crate's own local, read-only git questions wait
/// before being treated as failed — a local filesystem operation, never a
/// network call, so this bounds a genuinely stuck git (a hung filesystem, or
/// lock contention `--no-optional-locks` did not avoid) rather than anything
/// expected to take real time. Shared by every module that asks git a purely
/// local question — everything except the git processes that reach a remote
/// (`behind`'s `ls-remote` and its fetch), each of which waits under a bound
/// of its own.
///
/// This is the only value a production build ever uses. A test build can
/// shorten it through `SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS`, read in
/// `local_timeout.rs`, so a test of what a timeout says need not wait it out.
pub(crate) const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether a git process runs under a fixed locale or inherits this
/// process's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locale {
    /// `LC_ALL=C`. For a command whose own diagnostics this crate classifies by
    /// matching English text — `rev-parse`'s dubious-ownership and
    /// not-a-repository diagnostics, in particular — a locale git might
    /// otherwise translate under.
    Fixed,
    /// This process's own locale, unchanged. For exactly the commands that
    /// can run a wearer's own content filter (`status`, whose clean side
    /// may run, and `cat-file --filters`, whose smudge side does): a filter
    /// process inherits git's own environment, and fixing the locale git
    /// runs under could change what the filter itself prints or how it
    /// behaves.
    Inherited,
}

/// Which of [`REDIRECTING_VARIABLES`] are set in the calling process right
/// now, whatever their value — an empty value still counts, since git
/// itself still reads it as set (captured: `GIT_INDEX_FILE=` still changes
/// what `git ls-files` reports). Returned in the constant's own order, so a
/// refusal naming them reads the same on every run.
///
/// Pure over its own lookup, so a test can hand it any lookup it likes;
/// production calls this with `|name| std::env::var_os(name).is_some()`.
pub(crate) fn redirecting_variables_set(is_set: impl Fn(&str) -> bool) -> Vec<&'static str> {
    REDIRECTING_VARIABLES
        .into_iter()
        .filter(|name| is_set(name))
        .collect()
}

/// Builds a `git` [`Command`] with every [`REDIRECTING_VARIABLES`] and
/// [`PATHSPEC_VARIABLES`] entry removed, `GIT_TERMINAL_PROMPT=0` (a remote
/// asking for a password must fail loudly rather than hang this process
/// waiting on a terminal that has none to give), `LC_ALL=C` for
/// [`Locale::Fixed`], and the global options `--no-optional-locks
/// --literal-pathspecs`. This is the builder for a git process that reads
/// its pathspecs literally; a process that needs pathspec magic (the
/// questions [`command_for_pathspec_magic`] lists) is built there instead.
/// Both share that builder's body, so neither can drift from the other on
/// what it removes.
pub(crate) fn command(locale: Locale) -> Command {
    let mut command = command_for_pathspec_magic(locale);
    command.arg("--literal-pathspecs");
    command
}

/// The body [`command`] builds on, without the global `--literal-pathspecs`:
/// for the questions that must ask git to fold case, or to match a glob,
/// itself: is anything tracked under this path, or under any case variant of
/// it that git would take for it; is anything tracked at exactly this
/// directory's path; and which paths does the index list at a given depth?
/// It is also the only builder `git check-ignore` can be run from, which
/// refuses the global flag altogether (captured: git 2.53.0, `git
/// --literal-pathspecs check-ignore -q -- x` exits 128 with `pathspec magic
/// not supported by this command: 'literal'`, and so do
/// `GIT_LITERAL_PATHSPECS` and `GIT_GLOB_PATHSPECS`, both removed here).
///
/// The global flag disables every pathspec magic, `:(icase)` included
/// (captured: git 2.53.0, `--literal-pathspecs ls-files -- ':(literal,icase)X'`
/// prints nothing where the same command without the flag lists `x`), so this
/// builder leaves it off. Everything else holds for both builders, since
/// this is the one body: every redirecting variable and every
/// [`PATHSPEC_VARIABLES`] entry is removed, so `GIT_GLOB_PATHSPECS` and its
/// siblings cannot change what a pathspec means here either (tested by
/// `command_for_pathspec_magic_removes_the_variables_and_leaves_literal_pathspecs_off`
/// below).
///
/// Without the flag a pathspec is read as a glob unless it says otherwise, so
/// every pathspec passed to a command built here carries explicit magic:
/// `literal` for a path this crate names ([`pathspec_ignoring_case`] and
/// [`pathspec_exactly_ignoring_case`]), and `glob` for the depth patterns,
/// which carry no path at all ([`pathspec_at_depth`]). Those three are the
/// only things that build one. `check-ignore` takes pathspecs but never expands
/// a wildcard in one, and still reads a leading `:(` as magic, hence the `./`
/// prefix ([`path_beneath_current_directory`]).
///
/// That pairing is documented here and on those builders, not enforced by a
/// type: a pathspec is one more argument to a [`Command`], which takes any
/// string, so a newtype for it would still be accepted by [`command`], and
/// only wrapping `Command` itself could refuse the mismatch.
pub(crate) fn command_for_pathspec_magic(locale: Locale) -> Command {
    let mut command = Command::new("git");
    for variable in REDIRECTING_VARIABLES {
        command.env_remove(variable);
    }
    for variable in PATHSPEC_VARIABLES {
        command.env_remove(variable);
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    if locale == Locale::Fixed {
        command.env("LC_ALL", "C");
    }
    command.arg("--no-optional-locks");
    command
}

/// The pathspec that matches `path`, and anything beneath it as a directory,
/// under git's own case fold: `:(literal,icase)` followed by the path.
///
/// `literal` keeps `*`, `?`, `[` and `\` ordinary characters, exactly as
/// [`command`]'s global flag does for every other pathspec this crate
/// builds; `icase` asks git to fold case the way it does for
/// `core.ignorecase`. Captured (git 2.53.0, macOS and git 2.47.3, Linux):
/// the fold is ASCII only, so `é` does not match `É`, and a match beneath a
/// directory holds only at a component boundary. That is git's own fold, and
/// deliberately not the crate's `FoldedName`: it is exactly what git
/// ignores, no more and no less.
///
/// Only meaningful to a command built by [`command_for_pathspec_magic`]:
/// under [`command`]'s `--literal-pathspecs` the whole string, magic prefix
/// included, would be read as a file name.
pub(crate) fn pathspec_ignoring_case(path: &ClaimPath) -> String {
    format!(":(literal,icase){path}")
}

/// The pair of pathspecs that match `path` itself, under git's own case
/// fold, and nothing beneath it: the path as a literal, then an exclusion of
/// everything beneath `path`.
///
/// This is what asks whether git tracks an *entry* at a directory above a
/// claim. [`pathspec_ignoring_case`] alone matches the directory's whole
/// subtree, which for an ancestor is every tracked file under it, so the
/// exclusion is what leaves only an entry at the directory's own path: a
/// tracked file, a symbolic link or a gitlink where the claim needs a
/// directory. Captured (git 2.53.0, macOS): the pair finds a file `d` and
/// nothing for a directory `d` that only has files under it; `dx` is not
/// matched by `d`; and `icase` holds on both halves.
///
/// The exclusion is a `glob` of the path, escaped, followed by `/**`, and not
/// `exclude,literal` of `<path>/`. Captured: git reads a pathspec that ends in
/// a `/` as matching a gitlink of that name (a submodule is a directory to
/// it), so `:(exclude,literal,icase)sub/` excludes the very entry, `sub`, the
/// question exists to find. `sub/**` matches what is beneath `sub` and not
/// `sub`. The escaping keeps `*`, `?`, `[` and `\` in the path ordinary
/// characters, as `literal` does for the first half (captured: names holding
/// each of them, and `**`, `!`, `#`, a space, a leading `-` and `:(`).
///
/// An exclusion applies to the whole command, so two ancestors, one beneath
/// the other, cannot share one: excluding beneath `d` would exclude `d/e`.
/// The caller asks once per distinct ancestor.
///
/// Only meaningful to a command built by [`command_for_pathspec_magic`],
/// as for [`pathspec_ignoring_case`].
pub(crate) fn pathspec_exactly_ignoring_case(path: &ClaimPath) -> [String; 2] {
    [
        format!(":(literal,icase){path}"),
        format!(":(exclude,glob,icase){}/**", escape_glob(path.as_str())),
    ]
}

/// `text` with every character a `glob` pathspec reads as a pattern (`*`,
/// `?`, `[`, and the backslash that escapes them) preceded by a backslash, so
/// the pattern matches exactly `text`.
fn escape_glob(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '*' | '?' | '[' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The pathspec that matches every path exactly `depth` components deep:
/// `*` for depth 1, `*/*` for depth 2, and so on, as a `glob` so that `*`
/// does not cross a `/`. It carries no text from a claim, so nothing a
/// claim spells can change what it matches. Captured (git 2.53.0, macOS): a
/// pattern with a wildcard does not recurse, and it matches names that start
/// with a dot and names that hold `*` or `[`.
///
/// Only meaningful to a command built by [`command_for_pathspec_magic`],
/// as for [`pathspec_ignoring_case`].
pub(crate) fn pathspec_at_depth(depth: NonZeroUsize) -> String {
    let mut pathspec = String::from(":(glob)");
    for _level in 1..depth.get() {
        pathspec.push_str("*/");
    }
    pathspec.push('*');
    pathspec
}

/// The path `git check-ignore` is asked about for `path`: `./` and then the
/// path, relative to the directory the command runs in.
///
/// `check-ignore` takes its arguments as pathspecs and, having refused the
/// global `--literal-pathspecs` (see [`command_for_pathspec_magic`]), cannot
/// be told to read them literally. Two things make the answer the same as if
/// it could. It never expands a wildcard in what it is given: `*`, `?` and
/// `[` are ordinary characters of the name, so `*.log` is asked about as a
/// file called `*.log`, and an ignore rule `x.log` does not ignore it
/// (captured: git 2.53.0, macOS, with and without `--no-index`). It does
/// read a leading `:(...)` as pathspec magic, though, and `./` in front is
/// what stops that: `./:(top)a.yml` is the file called `:(top)a.yml`, where
/// `:(top)a.yml` alone is `a.yml`. The prefix also means a name that begins
/// with `-` can never be read as an option, whatever else the caller passes.
pub(crate) fn path_beneath_current_directory(path: &ClaimPath) -> String {
    format!("./{path}")
}

/// [`command`], with every [`NEW_REPOSITORY_VARIABLES`] entry also removed —
/// for the one place this crate ever creates a repository of its own
/// (`behind`'s own temporary fetch target), so a wearer's own
/// `GIT_DEFAULT_HASH`/`GIT_DEFAULT_REF_FORMAT` can never skew the object
/// format or ref layout that throwaway repository is created with (tested
/// by `command_for_new_repository_also_removes_the_new_repository_variables`
/// below).
pub(crate) fn command_for_new_repository(locale: Locale) -> Command {
    let mut command = command(locale);
    for variable in NEW_REPOSITORY_VARIABLES {
        command.env_remove(variable);
    }
    command
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use proptest::prelude::*;

    use super::{
        Locale, NEW_REPOSITORY_VARIABLES, PATHSPEC_VARIABLES, REDIRECTING_VARIABLES, command,
        command_for_new_repository, command_for_pathspec_magic, path_beneath_current_directory,
        pathspec_at_depth, pathspec_exactly_ignoring_case, pathspec_ignoring_case,
        redirecting_variables_set,
    };
    use crate::claim::ClaimPath;

    #[test]
    fn command_for_pathspec_magic_removes_the_variables_and_leaves_literal_pathspecs_off() {
        // The global `--literal-pathspecs` disables every pathspec magic, so
        // the builder must not pass it; `command` is the control, which
        // must. Both must still remove every variable that redirects git or
        // changes what a pathspec means, so a wearer's environment can
        // never change how the one magic pathspec is read.
        let arguments_of = |built: &std::process::Command| -> Vec<String> {
            built
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect()
        };
        let control = command(Locale::Fixed);
        assert!(
            arguments_of(&control).contains(&"--literal-pathspecs".to_owned()),
            "the control must carry the flag, or this test proves nothing"
        );

        let built = command_for_pathspec_magic(Locale::Fixed);
        let arguments = arguments_of(&built);
        assert!(
            !arguments.contains(&"--literal-pathspecs".to_owned()),
            "the flag would disable :(icase), got {arguments:?}"
        );
        assert!(
            arguments.contains(&"--no-optional-locks".to_owned()),
            "the builder must still take no optional lock, got {arguments:?}"
        );

        let envs: std::collections::BTreeMap<_, _> = built.get_envs().collect();
        for variable in REDIRECTING_VARIABLES.into_iter().chain(PATHSPEC_VARIABLES) {
            assert_eq!(
                envs.get(std::ffi::OsStr::new(variable)),
                Some(&None),
                "{variable} must be removed from a command built for pathspec magic"
            );
        }
        assert_eq!(
            envs.get(std::ffi::OsStr::new("GIT_TERMINAL_PROMPT")),
            Some(&Some(std::ffi::OsStr::new("0"))),
            "a prompt must fail loudly here as everywhere"
        );
        assert_eq!(
            envs.get(std::ffi::OsStr::new("LC_ALL")),
            Some(&Some(std::ffi::OsStr::new("C"))),
            "the fixed locale must apply"
        );
    }

    #[test]
    fn command_for_pathspec_magic_inherits_the_locale_when_asked() {
        let built = command_for_pathspec_magic(Locale::Inherited);
        let envs: std::collections::BTreeMap<_, _> = built.get_envs().collect();
        assert!(
            !envs.contains_key(std::ffi::OsStr::new("LC_ALL")),
            "an inherited locale must leave LC_ALL alone"
        );
    }

    #[test]
    fn the_case_folding_pathspec_is_literal_and_case_folding_and_nothing_else() {
        // Whatever the claim holds, the pathspec is the magic prefix and the
        // claim byte for byte, so a glob character stays the ordinary
        // character `literal` makes it.
        for path in ["a.yml", "d/f.yml", "[ab].yml", "*", "what?/x\\y"] {
            let claim = ClaimPath::from_rendering_path(path).expect("a well-formed test path");
            assert_eq!(
                pathspec_ignoring_case(&claim),
                format!(":(literal,icase){path}")
            );
        }
    }

    #[test]
    fn the_exact_ancestor_pathspecs_are_a_literal_match_and_an_exclusion_of_its_subtree() {
        // Two strings and no more: a third would be one more match or
        // exclusion nobody reasoned about. A glob character in the path stays
        // a literal character in the match (`literal`) and in the exclusion
        // (escaped); `**` is the only wildcard the exclusion holds.
        for (path, escaped) in [
            ("a", "a"),
            (".github/workflows", ".github/workflows"),
            ("[ab]", "\\[ab]"),
            ("*", "\\*"),
            ("what?/x\\y", "what\\?/x\\\\y"),
            ("a**b", "a\\*\\*b"),
        ] {
            let claim = ClaimPath::from_rendering_path(path).expect("a well-formed test path");
            assert_eq!(
                pathspec_exactly_ignoring_case(&claim),
                [
                    format!(":(literal,icase){path}"),
                    format!(":(exclude,glob,icase){escaped}/**"),
                ]
            );
        }
    }

    #[test]
    fn a_depth_pathspec_is_a_glob_of_that_many_components() {
        let depth = |depth: usize| pathspec_at_depth(NonZeroUsize::new(depth).expect("non-zero"));
        assert_eq!(depth(1), ":(glob)*");
        assert_eq!(depth(2), ":(glob)*/*");
        assert_eq!(depth(4), ":(glob)*/*/*/*");
    }

    #[test]
    fn a_check_ignore_path_is_the_claim_after_a_dot_slash() {
        // The prefix is what keeps a claim that opens like pathspec magic a
        // plain name to `check-ignore`, and a leading `-` a name, not an
        // option; each shape is kept whole after it.
        let path = |text: &str| {
            path_beneath_current_directory(
                &ClaimPath::from_rendering_path(text).expect("a well-formed test path"),
            )
        };
        assert_eq!(path("a/b.yml"), "./a/b.yml");
        assert_eq!(path(":(top)b.yml"), "./:(top)b.yml");
        assert_eq!(path("-b.yml"), "./-b.yml");
        assert_eq!(path("[ab]*?.yml"), "./[ab]*?.yml");
    }

    #[test]
    fn command_for_new_repository_also_removes_the_new_repository_variables() {
        // `command_for_new_repository` starts from `command` and removes
        // more on top; this proves the extra removal actually reaches the
        // built `Command`, the same way
        // `command_removes_every_redirecting_and_pathspec_variable` (below)
        // proves it for `command` itself.
        let built = command_for_new_repository(Locale::Inherited);
        let envs: std::collections::BTreeMap<_, _> = built.get_envs().collect();
        for variable in NEW_REPOSITORY_VARIABLES {
            assert_eq!(
                envs.get(std::ffi::OsStr::new(variable)),
                Some(&None),
                "{variable} must be removed for a repository this crate creates itself"
            );
        }
    }

    #[test]
    fn command_removes_every_redirecting_and_pathspec_variable() {
        // `Command::env_remove` records an explicit removal as `(name,
        // None)` in `get_envs`, distinct from a name `command` never
        // mentions at all — this is what lets a test tell "removed" apart
        // from "left alone" without spawning git at all.
        let built = command(Locale::Inherited);
        let envs: std::collections::BTreeMap<_, _> = built.get_envs().collect();
        for variable in REDIRECTING_VARIABLES {
            assert_eq!(
                envs.get(std::ffi::OsStr::new(variable)),
                Some(&None),
                "{variable} must be removed from every command this crate builds"
            );
        }
        for variable in PATHSPEC_VARIABLES {
            assert_eq!(
                envs.get(std::ffi::OsStr::new(variable)),
                Some(&None),
                "{variable} must be removed from every command this crate builds"
            );
        }
    }

    #[test]
    fn each_redirecting_variable_alone_is_reported() {
        for name in REDIRECTING_VARIABLES {
            let found = redirecting_variables_set(|candidate| candidate == name);
            assert_eq!(found, vec![name]);
        }
    }

    #[test]
    fn every_redirecting_variable_together_is_reported_in_constant_order() {
        let found = redirecting_variables_set(|_name| true);
        assert_eq!(found, REDIRECTING_VARIABLES.to_vec());
    }

    #[test]
    fn none_set_is_an_empty_list() {
        assert!(redirecting_variables_set(|_name| false).is_empty());
    }

    #[test]
    fn present_with_an_empty_value_still_counts() {
        // The lookup this function is given is presence, not "non-empty" —
        // production calls it with `var_os(name).is_some()`, which is true
        // for a variable set to the empty string too.
        let found = redirecting_variables_set(|name| name == "GIT_INDEX_FILE");
        assert_eq!(found, vec!["GIT_INDEX_FILE"]);
    }

    proptest! {
        /// However many, and whichever, variables a lookup reports set,
        /// `redirecting_variables_set` never panics and never reports a
        /// name outside its own constant.
        #[test]
        fn never_panics_and_never_invents_a_name(
            flags in proptest::collection::vec(any::<bool>(), 7)
        ) {
            let found = redirecting_variables_set(|name| {
                REDIRECTING_VARIABLES
                    .iter()
                    .position(|candidate| *candidate == name)
                    .is_some_and(|index| flags[index])
            });
            for name in &found {
                assert!(REDIRECTING_VARIABLES.contains(name));
            }
        }
    }
}
