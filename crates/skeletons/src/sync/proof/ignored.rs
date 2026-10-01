//! The question the index cannot answer about a file `sync` would create:
//! would git's ignore rules ignore it?
//!
//! A file git ignores is one `git status` never shows and `git add` refuses
//! without `-f`. If `sync` created one, the bone would never be committed,
//! and the next drift would find it untracked and refuse to update it. `skeletons`
//! does not serve one-shot files meant to stay out of git, so `sync` refuses
//! to create one, naming the rule that ignores it.
//!
//! It is asked of git, never read from the rules, since which rule wins
//! depends on every `.gitignore` above the file, `.git/info/exclude`,
//! `core.excludesFile`, and the order and negation of the patterns in them.
//! Two `git check-ignore` runs answer it, both with `--no-index`, so that a
//! tracked file of the same name (which git does not ignore in effect) never
//! answers for the claim, and neither is ever asked for a path that is
//! present: only a write `sync` would create needs the question.
//!
//! - `-q` gives the verdict as its exit status: `0` ignored, `1` not
//!   ignored. Git's own last-match-wins rule decides it, so a `!` pattern
//!   that undoes an earlier one exits `1`, and the claim is created.
//! - `-v` names the rule that ignores it, `<source>:<line>:<pattern>`, for
//!   the message. It is asked only after `-q` said ignored.
//!
//! The command is built by [`WorkTree::git_for_pathspec_magic`], because
//! `check-ignore` exits `128` under the global `--literal-pathspecs` every
//! other builder passes. The path it is given is the claim after `./`
//! ([`git::path_beneath_current_directory`]): `check-ignore` never expands a
//! wildcard in a path, so `*`, `?` and `[` stay ordinary characters of the
//! name, and the `./` is what keeps a claim that opens like pathspec magic,
//! `:(top)a.yml`, the file of that name.

use std::process::Command;

use crate::claim::ClaimPath;
use crate::git::{self, Locale};
use crate::subprocess::Truncated;

use super::super::abort::{GitQuestion, SyncAbort};
use super::super::work_tree::{WorkTree, run_local};
use super::Why;

/// `git check-ignore`'s exit status for "at least one path is ignored".
const EXIT_IGNORED: i32 = 0;

/// `git check-ignore`'s exit status for "no path is ignored". Any other
/// status is a failure: git exits `128` for a fatal error.
const EXIT_NOT_IGNORED: i32 = 1;

/// What one `check-ignore` run is asked to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// `-q`: nothing, only the exit status.
    Verdict,
    /// `-v`: the rule that ignores the path.
    Rule,
}

impl Ask {
    const fn flag(self) -> &'static str {
        match self {
            Self::Verdict => "-q",
            Self::Rule => "-v",
        }
    }
}

/// What `check-ignore -q` said about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Ignored,
    NotIgnored,
}

/// The refusal that follows from git's ignore rules for a claim nothing is
/// at, or `None` when git does not ignore it. A git failure or an answer
/// `skeletons` cannot read is a refusal too, since the claim is then not known to
/// be one git would show.
///
/// A whole-command abort (git could not be run, or ran past its time) is the
/// outer error.
pub(super) fn refusal(work_tree: &WorkTree, path: &ClaimPath) -> Result<Option<Why>, SyncAbort> {
    let question = GitQuestion::Ignored(path.clone());

    let verdict = run_local(
        check_ignore(work_tree, path, Ask::Verdict),
        question.clone(),
    )?;
    match classify_verdict(verdict.code(), verdict.stdout(), verdict.stderr_head()) {
        Err(why) => return Ok(Some(why)),
        Ok(Verdict::NotIgnored) => return Ok(None),
        Ok(Verdict::Ignored) => {}
    }

    let rule = run_local(check_ignore(work_tree, path, Ask::Rule), question)?;
    Ok(Some(
        match classify_rule(rule.code(), rule.stdout(), rule.stderr_head()) {
            Ok(rule) => Why::IgnoredByGit { rule },
            Err(why) => why,
        },
    ))
}

/// `git check-ignore --no-index <ask> -- ./<path>`, from the work tree's own
/// root, from the builder that does not pass `--literal-pathspecs`.
fn check_ignore(work_tree: &WorkTree, path: &ClaimPath, ask: Ask) -> Command {
    let mut command = work_tree.git_for_pathspec_magic(Locale::Fixed);
    command.args(["check-ignore", "--no-index", ask.flag(), "--"]);
    command.arg(git::path_beneath_current_directory(path));
    command
}

/// The pure classifier for `check-ignore -q`, split out so a unit test hands
/// it every exit status and a truncated stream with no process to spawn.
/// Stdout is never expected to hold anything under `-q`, but a stream that
/// ran past its cap is refused whatever the status, like every git answer
/// here.
fn classify_verdict(
    code: Option<i32>,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<Verdict, Why> {
    stdout.map_err(|_truncated| Why::OutputTooLarge {
        command: "check-ignore",
    })?;
    match code {
        Some(EXIT_IGNORED) => Ok(Verdict::Ignored),
        Some(EXIT_NOT_IGNORED) => Ok(Verdict::NotIgnored),
        Some(_) | None => Err(Why::IgnoreCheckFailed {
            detail: git::diagnostic(stderr),
        }),
    }
}

/// The pure classifier for `check-ignore -v`, asked only once `-q` has said
/// the path is ignored. Anything but a successful run that names one rule
/// means git no longer gives the answer it gave a moment ago, and is refused
/// as a failed check rather than a rule.
fn classify_rule(
    code: Option<i32>,
    stdout: Result<&[u8], Truncated>,
    stderr: &[u8],
) -> Result<String, Why> {
    let stdout = stdout.map_err(|_truncated| Why::OutputTooLarge {
        command: "check-ignore",
    })?;
    match code {
        Some(EXIT_IGNORED) => rule_from(stdout).ok_or_else(|| Why::IgnoreCheckFailed {
            detail: format!(
                "git printed a rule `skeletons` cannot read: {}",
                first_line(stdout)
            ),
        }),
        Some(EXIT_NOT_IGNORED) => Err(Why::IgnoreCheckFailed {
            detail: "git named no rule that ignores it the second time it was asked".to_owned(),
        }),
        Some(_) | None => Err(Why::IgnoreCheckFailed {
            detail: git::diagnostic(stderr),
        }),
    }
}

/// The first line of what git printed, without its line ending: all of an
/// unreadable answer that is quoted back to the user. The rest is left out
/// so one detail stays one line of a sentence and never copies whole a
/// stream that ran to the cap.
fn first_line(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The rule out of `check-ignore -v`'s one line for one path, exactly as git
/// prints it: `<source>:<line>:<pattern>`, then a tab and the path, quoted if
/// it holds a character git quotes.
///
/// The line is `<rule> TAB <path> LF`. Git quotes a path holding a tab or a
/// newline, so the path never holds a raw tab and the last tab is what ends
/// the rule; a pattern can hold a tab of its own (captured: git 2.53.0), and
/// splitting at the first tab would cut it short. The rule is kept whole and
/// not split into its three parts, because a source that is a path may hold
/// a `:` of its own and which one ends it cannot be told from the text, so
/// any split could name the wrong pattern.
///
/// `None` for anything else: no line, more than one, no tab, an empty rule,
/// or the `::` git prints for a path no rule matches, which `-v` never does
/// without `--non-matching`.
fn rule_from(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    let line = text.strip_suffix('\n')?;
    if line.contains('\n') {
        return None;
    }
    let (rule, path) = line.rsplit_once('\t')?;
    if rule.is_empty() {
        return None;
    }
    if rule.starts_with("::") {
        return None;
    }
    if path.is_empty() {
        return None;
    }
    if rule.matches(':').count() < 2 {
        return None;
    }
    Some(rule.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::claim::ClaimPath;
    use crate::subprocess::Truncated;
    use crate::sync::test_repository::TestRepository;

    use super::super::Why;
    use super::{Verdict, classify_rule, classify_verdict, refusal, rule_from};

    fn claim(path: &str) -> ClaimPath {
        ClaimPath::from_rendering_path(path).expect("a well-formed test path")
    }

    /// The rule that ignores `path` in `repository`, or `None` when git does
    /// not ignore it, running the real `git check-ignore`.
    fn asked(repository: &TestRepository, path: &str) -> Option<String> {
        match refusal(&repository.work_tree(), &claim(path)).expect("git must answer") {
            None => None,
            Some(Why::IgnoredByGit { rule }) => Some(rule),
            Some(other) => panic!("expected a rule or nothing, got {other:?}"),
        }
    }

    // The classifier for `-q`: exit 0 is ignored, exit 1 is not, anything
    // else is a failure that carries git's own detail, and a truncated
    // stream is refused whatever the exit says.

    #[test]
    fn exit_zero_is_ignored_and_exit_one_is_not_ignored() {
        assert_eq!(
            classify_verdict(Some(0), Ok(b""), b"").expect("exit 0 is a verdict"),
            Verdict::Ignored
        );
        assert_eq!(
            classify_verdict(Some(1), Ok(b""), b"").expect("exit 1 is a verdict"),
            Verdict::NotIgnored
        );
    }

    #[test]
    fn exit_128_is_a_failed_check_carrying_gits_own_detail() {
        let why = classify_verdict(
            Some(128),
            Ok(b""),
            b"fatal: ./x: './x' is outside repository at '/r'\n",
        )
        .expect_err("a fatal exit is refused, not read as a verdict");

        let Why::IgnoreCheckFailed { detail } = why else {
            panic!("expected IgnoreCheckFailed, got {why:?}")
        };
        assert_eq!(detail, "./x: './x' is outside repository at '/r'");
    }

    #[test]
    fn a_process_that_left_no_exit_code_is_a_failed_check_not_a_verdict() {
        // A process ended by a signal has no exit code. Reading it as "not
        // ignored" would create a file git may hide.
        let why = classify_verdict(None, Ok(b""), b"")
            .expect_err("no exit code is not a verdict either way");
        assert!(matches!(why, Why::IgnoreCheckFailed { .. }), "got {why:?}");
    }

    #[test]
    fn truncated_output_is_refused_whatever_the_exit_status_says() {
        for code in [Some(0), Some(1), Some(128), None] {
            let why = classify_verdict(code, Err(Truncated::for_test(16 * 1024 * 1024)), b"")
                .expect_err("a stream cut off at its cap is never read");
            assert!(
                matches!(
                    why,
                    Why::OutputTooLarge {
                        command: "check-ignore"
                    }
                ),
                "exit {code:?}: got {why:?}"
            );
        }
    }

    // The classifier for `-v`, which runs only after `-q` said ignored.

    #[test]
    fn a_rule_line_is_the_rule_git_reported() {
        let rule = classify_rule(
            Some(0),
            Ok(b"cfg/.gitignore:2:*.local.yml\t./cfg/a.local.yml\n"),
            b"",
        )
        .expect("one rule line is a rule");
        assert_eq!(rule, "cfg/.gitignore:2:*.local.yml");
    }

    #[test]
    fn a_second_answer_that_names_no_rule_is_a_failed_check() {
        let why = classify_rule(Some(1), Ok(b""), b"").expect_err("no rule is not a rule");
        assert!(matches!(why, Why::IgnoreCheckFailed { .. }), "got {why:?}");
    }

    #[test]
    fn a_second_answer_that_fails_carries_gits_own_detail() {
        let why = classify_rule(Some(128), Ok(b""), b"fatal: bad object\n")
            .expect_err("a fatal exit is not a rule");
        let Why::IgnoreCheckFailed { detail } = why else {
            panic!("expected IgnoreCheckFailed, got {why:?}")
        };
        assert_eq!(detail, "bad object");
    }

    #[test]
    fn a_second_answer_that_is_not_a_rule_line_is_a_failed_check_quoting_it() {
        let why = classify_rule(Some(0), Ok(b"nothing like a rule\n"), b"")
            .expect_err("an unreadable line is not a rule");
        let Why::IgnoreCheckFailed { detail } = why else {
            panic!("expected IgnoreCheckFailed, got {why:?}")
        };
        assert!(detail.ends_with("nothing like a rule"), "{detail}");
    }

    #[test]
    fn a_second_answer_of_many_lines_is_quoted_by_its_first_line_only() {
        let why = classify_rule(Some(0), Ok(b"nothing like a rule\nsecond\nthird\n"), b"")
            .expect_err("an unreadable answer is not a rule");
        let Why::IgnoreCheckFailed { detail } = why else {
            panic!("expected IgnoreCheckFailed, got {why:?}")
        };
        assert!(detail.ends_with("nothing like a rule"), "{detail}");
        assert!(!detail.contains("second"), "{detail}");
        assert!(!detail.contains('\n'), "{detail}");
    }

    #[test]
    fn a_truncated_rule_is_refused_as_too_large_never_read_in_part() {
        let why = classify_rule(Some(0), Err(Truncated::for_test(16 * 1024 * 1024)), b"")
            .expect_err("a truncated stream is never read");
        assert!(
            matches!(
                why,
                Why::OutputTooLarge {
                    command: "check-ignore"
                }
            ),
            "got {why:?}"
        );
    }

    // The `-v` parse. Each shape is a line git prints (captured: git 2.53.0).

    #[test]
    fn a_plain_rule_line_parses_to_its_source_line_and_pattern_as_printed() {
        assert_eq!(
            rule_from(b".gitignore:1:ig/\t./ig/deep/x.yml\n").as_deref(),
            Some(".gitignore:1:ig/")
        );
    }

    #[test]
    fn a_pattern_holding_a_tab_is_kept_whole() {
        // The path is quoted, so it holds no raw tab; the pattern may, and
        // splitting at the first tab would cut the rule short.
        assert_eq!(
            rule_from(b".gitignore:2:a\tb\t\"./a\\tb\"\n").as_deref(),
            Some(".gitignore:2:a\tb")
        );
    }

    #[test]
    fn a_quoted_source_and_a_quoted_path_parse_as_printed() {
        assert_eq!(
            rule_from(b"\"\\303\\251/.gitignore\":1:*\t\"./\\303\\251/a\"\n").as_deref(),
            Some("\"\\303\\251/.gitignore\":1:*")
        );
    }

    #[test]
    fn a_source_holding_a_colon_is_kept_whole() {
        // Which colon ends the source cannot be told from the text, so the
        // rule is never split into its three parts.
        assert_eq!(
            rule_from(b"a:1:b/.gitignore:2:*.yml\t./a:1:b/x.yml\n").as_deref(),
            Some("a:1:b/.gitignore:2:*.yml")
        );
    }

    #[test]
    fn a_negated_pattern_is_still_the_line_git_printed() {
        // `-q` decides whether the path is ignored. `-v` is asked only when
        // it is, so a negation here means the two disagreed; the rule is
        // shown as printed and not guessed at.
        assert_eq!(
            rule_from(b"cfg/.gitignore:3:!keep.local.yml\t./cfg/keep.local.yml\n").as_deref(),
            Some("cfg/.gitignore:3:!keep.local.yml")
        );
    }

    #[test]
    fn output_that_is_not_one_rule_line_parses_to_nothing() {
        let unreadable: [&[u8]; 8] = [
            b"",
            b"\n",
            b".gitignore:1:x\t./x",                        // no line feed
            b".gitignore:1:x./x\n",                        // no tab
            b"\t./x\n",                                    // no rule
            b"::\t./x\n",                                  // the no-match form
            b".gitignore:1:x\t\n",                         // no path
            b".gitignore:1:x\t./x\n.gitignore:2:y\t./x\n", // two lines
        ];
        for stdout in unreadable {
            assert_eq!(
                rule_from(stdout),
                None,
                "{}",
                String::from_utf8_lossy(stdout)
            );
        }
        assert_eq!(rule_from(b"nocolons\t./x\n"), None);
    }

    // Against real git.

    fn repository_ignoring(rules: &str) -> TestRepository {
        let repository = TestRepository::new();
        repository.write(".gitignore", rules.as_bytes());
        repository.commit_all("initial");
        repository
    }

    #[test]
    fn a_path_a_rule_ignores_is_refused_naming_the_rule() {
        let repository = repository_ignoring("# machine-local\n*.local.yml\n");

        assert_eq!(
            asked(&repository, "cfg/editor.local.yml").as_deref(),
            Some(".gitignore:2:*.local.yml")
        );
    }

    #[test]
    fn a_path_no_rule_ignores_is_not_refused() {
        let repository = repository_ignoring("*.local.yml\n");

        assert_eq!(asked(&repository, "cfg/plain.yml"), None);
    }

    #[test]
    fn a_later_negation_undoes_an_earlier_rule() {
        let repository = repository_ignoring("*.local.yml\n!keep.local.yml\n");

        assert_eq!(asked(&repository, "keep.local.yml"), None);
        assert!(asked(&repository, "other.local.yml").is_some());
    }

    #[test]
    fn a_directory_rule_ignores_a_claim_beneath_it() {
        let repository = repository_ignoring("generated/\n");

        assert_eq!(
            asked(&repository, "generated/deep/x.yml").as_deref(),
            Some(".gitignore:1:generated/")
        );
    }

    #[test]
    fn a_tracked_file_a_claim_reads_like_a_glob_of_does_not_answer_for_the_claim() {
        // `--no-index`: without it `check-ignore` matches its argument
        // against the index as a pathspec, and finds a tracked file for
        // which it answers "not ignored", whatever the rules say. A claim
        // named `*.log` reads as a glob of the tracked `kept.log` there,
        // though it is a different, absent file the rule `*.log` ignores.
        let repository = repository_ignoring("*.log\n");
        repository.write("kept.log", b"tracked\n");
        repository.git(&["add", "-f", "--", "kept.log"]).run_ok();
        repository.commit_all("track a file the rule matches");

        assert_eq!(
            asked(&repository, "*.log").as_deref(),
            Some(".gitignore:1:*.log")
        );
    }

    #[test]
    fn a_glob_character_in_a_claim_is_an_ordinary_character_of_its_name() {
        // Only `x.log` is ignored. A path read as a glob would match it:
        // `*.log`, `?.log` and `[x].log` all do. Read as names they are three
        // files no rule mentions. The control is `x.log` itself, which the
        // same command must still find, so the silence is not a search that
        // cannot reach its target.
        let repository = repository_ignoring("x.log\n");

        for name in ["*.log", "?.log", "[x].log"] {
            assert_eq!(asked(&repository, name), None, "{name} is a name");
        }
        assert_eq!(
            asked(&repository, "x.log").as_deref(),
            Some(".gitignore:1:x.log")
        );
    }

    #[test]
    fn a_claim_that_opens_like_pathspec_magic_is_a_file_of_that_name() {
        // Only `ok.yml` is ignored. `:(top)ok.yml` read as magic is `ok.yml`
        // and would be refused; after `./` it is a file called `:(top)ok.yml`
        // and is not. `x.yml` under a rule of its own is the control.
        let repository = repository_ignoring("/ok.yml\n");

        assert_eq!(asked(&repository, ":(top)ok.yml"), None);
        assert_eq!(asked(&repository, ":(literal)ok.yml"), None);
        assert!(asked(&repository, "ok.yml").is_some());
    }

    #[test]
    fn a_claim_that_begins_with_a_dash_is_a_name_not_an_option() {
        let repository = repository_ignoring("-q.yml\n");

        assert_eq!(
            asked(&repository, "-q.yml").as_deref(),
            Some(".gitignore:1:-q.yml")
        );
        assert_eq!(asked(&repository, "-v.yml"), None);
    }
}
