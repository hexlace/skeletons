//! The behind fact: whether a worn skeleton's own remote holds something newer
//! than the version this repository has locked.
//!
//! Every worn dependency gets exactly one answer, decided entirely by how it
//! is pinned (`workspace::Pin`): a path or `rev =` dependency is `Pinned`,
//! permanently; a registry, `tag =`, `branch =`, or unqualified `git =`
//! dependency is asked of its remote; any other registry, or a source this
//! crate does not recognise, is `Undetermined` without ever being asked.
//! Distinct remotes are asked at most once each, concurrently, however many
//! worn dependencies share them.
//!
//! A `tag =` or registry answer needs nothing past its own remote head. A
//! `branch =` or unqualified `git =` answer needs more: a branch-pinned
//! skeleton is behind only when there is newer content in its own package
//! directory, not merely a newer commit anywhere in the repository (a
//! sibling crate's commit is not something `cargo update` would change for
//! it), so a differing head is only the cheap first half of that question
//! (`git_remote::branch_head`/`default_branch_head`, one `ls-remote`). When the head does differ, [`determine`] reads the locked
//! side of the comparison from Cargo's own checkout
//! (`cargo_checkout::locked_directory`, read-only, no network), fetches the
//! remote head alone into a temporary repository this crate owns and removes
//! (`snapshot::directory_trees`), and compares the two directories' own tree
//! objects.

mod branch_directory;
mod cargo_checkout;
mod crates_io;
mod git_remote;
mod index_entries;
mod remote_log;
mod snapshot;
mod temporary_directory;
mod version_tag;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub(crate) use crates_io::CratesIoIndex;

use branch_directory::{
    BranchIntent, DirectoryCheck, resolve_branch, resolve_default_branch, resolve_directory_checks,
};
use crates_io::IndexFetchFailure;
use git_remote::{BranchAnswer, DefaultBranchAnswer, TagsAnswer};

use crate::git::ObjectId;
use crate::workspace::{Pin, WornDependency, WornId};

/// At most this many remote queries run at once — shared by the cheap
/// ref-list phase ([`answer_all`]) and the directory-snapshot phase
/// (`branch_directory`'s own `run_snapshot_queries`). A worn dependency's
/// own answer never depends on how many others share its remote, only on
/// how long they all take together.
const REMOTE_QUERIES_IN_FLIGHT_MAX: usize = 8;

const OTHER_REGISTRY_DETAIL: &str = "`skeletons` asks only crates.io whether a skeleton is behind";
const UNRECOGNISED_SOURCE_DETAIL: &str = "cargo reported a source `skeletons` does not recognise";

/// Whether a worn skeleton's locked version is current, behind, permanently
/// pinned, or impossible to determine.
// `Behind::Behind` is the one variant named exactly like its own enum: it is
// also the one whose own name is the fact's name (`--json`'s `state:
// "behind"`, the human output's word `behind`), so renaming it to satisfy
// the lint would make the type's name and the domain's own word disagree
// everywhere this variant is built or matched.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "Behind::Behind is the domain's own word for this state, not a naming accident"
)]
pub(crate) enum Behind {
    Current,
    Behind(Newer),
    Pinned,
    Undetermined(Undetermined),
}

/// What a remote reports as newer than what is locked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Newer {
    Version(semver::Version),
    Tag(String),
    Commit {
        /// Shown abbreviated in human text ([`ObjectId::abbreviated`]) and
        /// whole in `--json`.
        sha: ObjectId,
        /// The default branch's own name, when the remote named it
        /// (`--symref`'s `ref:` line). Always `None` for an explicit
        /// `branch =` pin — not computed, but hardcoded at the one call
        /// site that builds this field for that pin kind (`resolve_branch`'s
        /// own `newer_branch_field: None`), since that pin's own name is
        /// already known from the pin itself and there is nothing else this
        /// field could hold there (tested by
        /// `resolve_branch_leaves_a_differing_head_pending`, which asserts
        /// `newer_branch_field` is `None`; the contrasting default-branch
        /// case, where the field can be `Some`, is
        /// `resolve_default_branch_carries_the_servers_own_named_branch_into_the_pending_check`).
        branch: Option<String>,
    },
}

/// Why `behind` could not be determined, and the human text explaining it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Undetermined {
    pub(crate) reason: UndeterminedReason,
    pub(crate) detail: String,
}

/// The machine code for why `behind` is undetermined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UndeterminedReason {
    /// The skeleton comes from a registry other than crates.io, which
    /// `behind` never queries.
    OtherRegistry,
    /// The network request itself failed: DNS, connect, TLS, a timeout
    /// before the response arrives, or `git` could not reach the remote.
    Unreachable,
    /// crates.io does not list the crate at all.
    NotInIndex,
    /// A response came back that could not be read: an HTTP status other
    /// than the ones that mean the crate is absent, a crates.io body that
    /// does not read as an index or does not arrive in time, output from
    /// `git` that does not parse, a body too large to read, or an
    /// unreadable answer when reading the fetched snapshot.
    UnexpectedResponse,
    /// The tag the skeleton is locked to does not parse as a version, so
    /// there is nothing to compare the remote's tags against.
    TagNotAVersion,
    /// The branch the skeleton is locked to no longer exists on the remote.
    BranchMissing,
    /// Cargo reported a source for the skeleton that this crate does not
    /// recognise at all.
    UnrecognisedSource,
    /// Cargo's own checkout of a branch or default-branch pin could not be
    /// read, or is not at the commit this pin actually locked.
    CheckoutUnreadable,
    /// A local operation of this crate's own — creating the temporary
    /// directory a remote snapshot is fetched into, or the repository
    /// inside it — failed, before any network request was attempted.
    LocalFailure,
    /// The skeleton's own package directory does not exist at all at a
    /// branch or default-branch pin's own remote head.
    DirectoryMissing,
}

/// Where `behind` reads its remote answers from for the length of one run:
/// the crates.io seam, and, in a test build, the query log.
pub(crate) struct Remotes {
    crates_io: CratesIoIndex,
    #[cfg(any(test, feature = "test-util"))]
    log: Option<std::path::PathBuf>,
}

impl Remotes {
    /// Builds the remotes this process asks for the rest of its run, from
    /// the environment: the real network in a production build, or,
    /// in a test build, whatever `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` and
    /// `SKELETONS_TEST_ONLY_REMOTE_LOG` say.
    pub(crate) fn from_environment() -> Self {
        Self {
            crates_io: CratesIoIndex::from_environment(),
            #[cfg(any(test, feature = "test-util"))]
            log: remote_log::from_environment(),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
fn log_query(remotes: &Remotes, describe: impl FnOnce() -> String) {
    if let Some(path) = &remotes.log {
        remote_log::append(path, &describe());
    }
}

#[cfg(not(any(test, feature = "test-util")))]
fn log_query(_remotes: &Remotes, _describe: impl FnOnce() -> String) {}

/// One distinct remote question — `behind` asks each of these at most once,
/// however many worn dependencies share it. This is only ever the cheap,
/// ref-list half of a branch or default-branch pin's own question; the
/// directory-snapshot half, needed only when the head actually differs, is
/// grouped separately, by `(url, head)` (`branch_directory::resolve_directory_checks`), since
/// which snapshot is needed is not known until this phase's own answer
/// arrives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Query {
    CratesIo { name: String },
    Tags { url: String },
    Branch { url: String, branch: String },
    DefaultBranch { url: String },
}

/// What one worn dependency needs before its own `behind` fact is known:
/// decided already, from its pin alone, or waiting on one remote question.
///
/// [`need`] is the one exhaustive match on [`Pin`] that decides which: a pin
/// can only ever produce one or the other, so no pin is both answered
/// immediately and asked of a remote, and no answer could be read back under
/// the wrong pin's own rule.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Need {
    Answered(Behind),
    Ask(Ask),
}

/// One worn dependency's own remote question: which distinct query it needs
/// answered ([`Ask::query`]), and the locked value that answer is compared
/// against once it arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    CratesIo {
        name: String,
        locked: semver::Version,
    },
    Tag {
        package: String,
        url: String,
        locked_tag: String,
    },
    Branch {
        url: String,
        branch: String,
        locked_commit: ObjectId,
        /// This skeleton's own package directory, as Cargo's checkout holds
        /// it — read only once the branch's own remote head is known to
        /// differ from `locked_commit`.
        skeleton_directory: PathBuf,
    },
    DefaultBranch {
        url: String,
        locked_commit: ObjectId,
        skeleton_directory: PathBuf,
    },
}

impl Ask {
    /// The distinct remote question this ask needs answered — the dedup key
    /// [`answer_all`] runs each of at most once, however many worn
    /// dependencies' own asks need the same one.
    fn query(&self) -> Query {
        match self {
            Self::CratesIo { name, .. } => Query::CratesIo { name: name.clone() },
            Self::Tag { url, .. } => Query::Tags { url: url.clone() },
            Self::Branch { url, branch, .. } => Query::Branch {
                url: url.clone(),
                branch: branch.clone(),
            },
            Self::DefaultBranch { url, .. } => Query::DefaultBranch { url: url.clone() },
        }
    }
}

/// Decides what `dependency` needs, from its pin alone (`workspace::Pin`'s
/// own exhaustive match) — one classification, so a pin can never end up
/// both answered immediately and asked of a remote, and never ends up asked
/// under one pin kind but read back as another's answer.
fn need(dependency: &WornDependency) -> Need {
    match &dependency.pin {
        Pin::Path { .. } | Pin::Rev { .. } => Need::Answered(Behind::Pinned),
        Pin::OtherRegistry { .. } => Need::Answered(Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::OtherRegistry,
            detail: OTHER_REGISTRY_DETAIL.to_owned(),
        })),
        Pin::Unrecognised { .. } => Need::Answered(Behind::Undetermined(Undetermined {
            reason: UndeterminedReason::UnrecognisedSource,
            detail: UNRECOGNISED_SOURCE_DETAIL.to_owned(),
        })),
        Pin::CratesIo => Need::Ask(Ask::CratesIo {
            name: dependency.package.clone(),
            locked: dependency.version.clone(),
        }),
        Pin::Tag { url, tag, .. } => Need::Ask(Ask::Tag {
            package: dependency.package.clone(),
            url: url.clone(),
            locked_tag: tag.clone(),
        }),
        Pin::Branch {
            url,
            branch,
            commit,
        } => Need::Ask(Ask::Branch {
            url: url.clone(),
            branch: branch.clone(),
            locked_commit: commit.clone(),
            skeleton_directory: dependency.skeleton_directory.clone(),
        }),
        Pin::DefaultBranch { url, commit } => Need::Ask(Ask::DefaultBranch {
            url: url.clone(),
            locked_commit: commit.clone(),
            skeleton_directory: dependency.skeleton_directory.clone(),
        }),
    }
}

/// One query's own raw answer, carrying the key it was asked under — what a
/// remote worker hands back, before [`Answers::insert`] files it under its
/// own kind's typed map.
enum Answered {
    CratesIo {
        name: String,
        result: Result<Vec<u8>, IndexFetchFailure>,
    },
    Tags {
        url: String,
        answer: TagsAnswer,
    },
    Branch {
        url: String,
        branch: String,
        answer: BranchAnswer,
    },
    DefaultBranch {
        url: String,
        answer: DefaultBranchAnswer,
    },
}

/// Every query's own answer, once [`answer_all`] has run every one of them:
/// one typed map per query kind, so a lookup can never read a crates.io
/// result back as a git answer or the reverse — the kinds cannot be crossed,
/// because each map only ever holds its own [`Answered`] variant's payload.
#[derive(Default)]
struct Answers {
    crates_io: BTreeMap<String, Result<Vec<u8>, IndexFetchFailure>>,
    tags: BTreeMap<String, TagsAnswer>,
    branches: BTreeMap<(String, String), BranchAnswer>,
    default_branches: BTreeMap<String, DefaultBranchAnswer>,
}

impl Answers {
    /// Files `answered` under its own kind's typed map, keyed the same way
    /// [`Ask::query`] deduplicates that kind's queries.
    fn insert(&mut self, answered: Answered) {
        match answered {
            Answered::CratesIo { name, result } => {
                let previous = self.crates_io.insert(name, result);
                assert!(previous.is_none(), "each distinct query is answered once");
            }
            Answered::Tags { url, answer } => {
                let previous = self.tags.insert(url, answer);
                assert!(previous.is_none(), "each distinct query is answered once");
            }
            Answered::Branch {
                url,
                branch,
                answer,
            } => {
                let previous = self.branches.insert((url, branch), answer);
                assert!(previous.is_none(), "each distinct query is answered once");
            }
            Answered::DefaultBranch { url, answer } => {
                let previous = self.default_branches.insert(url, answer);
                assert!(previous.is_none(), "each distinct query is answered once");
            }
        }
    }

    /// How many distinct queries this holds an answer for, across every
    /// kind's own map — [`answer_all`]'s own postcondition that every query
    /// it was given was answered exactly once.
    fn len(&self) -> usize {
        self.crates_io.len() + self.tags.len() + self.branches.len() + self.default_branches.len()
    }

    /// `behind` for one worn dependency's own registry or tag `ask`, once
    /// [`answer_all`] has answered every query [`need`] built an [`Ask`]
    /// for.
    ///
    /// # Panics
    ///
    /// If `ask`'s own query was never answered — every [`Ask`] this module
    /// builds is turned into a [`Query`] `answer_all` runs before this is
    /// ever called (`determine`'s own order), so this never happens in
    /// practice. If `ask` is a [`Ask::Branch`] or [`Ask::DefaultBranch`] —
    /// those are resolved by [`resolve_branch`]/[`resolve_default_branch`],
    /// never by this method, since they need a possible second,
    /// directory-snapshot phase this method knows nothing about.
    fn behind_for(&self, ask: &Ask) -> Behind {
        match ask {
            Ask::CratesIo { name, locked } => {
                let Some(result) = self.crates_io.get(name) else {
                    unreachable!("every Ask's own query was answered before behind_for runs")
                };
                finalize_crates_io(name, locked, result)
            }
            Ask::Tag {
                package,
                url,
                locked_tag,
            } => {
                let Some(answer) = self.tags.get(url) else {
                    unreachable!("every Ask's own query was answered before behind_for runs")
                };
                finalize_tag(url, package, locked_tag, answer)
            }
            Ask::Branch { .. } | Ask::DefaultBranch { .. } => {
                unreachable!(
                    "a branch or default-branch ask is resolved by resolve_branch/\
                     resolve_default_branch, never behind_for"
                )
            }
        }
    }
}

/// Decides `behind` for every dependency in `worn`.
///
/// Dependencies whose pin needs no remote at all (`Pin::Path`, `Pin::Rev`,
/// `Pin::OtherRegistry`, `Pin::Unrecognised`) are answered immediately.
/// Every other dependency's query is deduplicated by remote target, so two
/// worn dependencies pinned to the same crates.io crate, or the same git
/// url, are asked once between them, and every distinct query runs
/// concurrently, at most [`REMOTE_QUERIES_IN_FLIGHT_MAX`] at a time.
///
/// A branch or default-branch pin runs in up to two phases: first the cheap
/// ref-list question every pin kind shares ([`answer_all`]), which alone
/// settles a pin whose remote head still equals what is locked; then, only
/// for a pin whose head differs, the directory-snapshot question
/// ([`resolve_directory_checks`]) that decides whether the difference is
/// inside this skeleton's own directory at all.
pub(crate) fn determine(worn: &[&WornDependency], remotes: &Remotes) -> BTreeMap<WornId, Behind> {
    let mut needs: Vec<(WornId, Need)> = Vec::with_capacity(worn.len());
    let mut queries: BTreeSet<Query> = BTreeSet::new();
    for dependency in worn {
        let need = need(dependency);
        if let Need::Ask(ask) = &need {
            queries.insert(ask.query());
        }
        needs.push((dependency.id(), need));
    }

    let answers = answer_all(&queries, remotes);

    let mut results = BTreeMap::new();
    let mut pending: Vec<(WornId, DirectoryCheck)> = Vec::new();
    for (id, need) in needs {
        let intent = match need {
            Need::Answered(behind) => BranchIntent::Answered(behind),
            Need::Ask(ask @ (Ask::CratesIo { .. } | Ask::Tag { .. })) => {
                BranchIntent::Answered(answers.behind_for(&ask))
            }
            Need::Ask(Ask::Branch {
                url,
                branch,
                locked_commit,
                skeleton_directory,
            }) => {
                let key = (url.clone(), branch.clone());
                let Some(answer) = answers.branches.get(&key) else {
                    unreachable!("every Ask's own query was answered before this runs")
                };
                resolve_branch(&url, &branch, &locked_commit, skeleton_directory, answer)
            }
            Need::Ask(Ask::DefaultBranch {
                url,
                locked_commit,
                skeleton_directory,
            }) => {
                let Some(answer) = answers.default_branches.get(&url) else {
                    unreachable!("every Ask's own query was answered before this runs")
                };
                resolve_default_branch(&url, &locked_commit, skeleton_directory, answer)
            }
        };
        match intent {
            BranchIntent::Answered(behind) => insert_once(&mut results, id, behind),
            BranchIntent::Pending(check) => pending.push((id, check)),
        }
    }

    resolve_directory_checks(pending, remotes, &mut results);

    assert_eq!(
        results.len(),
        worn.len(),
        "every worn dependency contributes exactly one behind result"
    );
    results
}

fn insert_once(results: &mut BTreeMap<WornId, Behind>, id: WornId, behind: Behind) {
    let previous = results.insert(id, behind);
    assert!(
        previous.is_none(),
        "a WornId is unique per worn dependency, so it contributes exactly one result"
    );
}

/// The human words for what a remote reports as newer than `pin` locks — the
/// text `--json`'s `behind.detail` carries for a behind skeleton and the
/// parenthetical after the state word in `check`'s block header. It names text
/// a remote, a tag or a branch supplied, as it came: the caller escapes it.
pub(crate) fn newer_detail(pin: &Pin, newer: &Newer) -> String {
    match newer {
        Newer::Version(version) => format!("{version} is available"),
        Newer::Tag(tag) => format!("tag {tag} is available"),
        Newer::Commit { sha, branch } => {
            let sha7 = sha.abbreviated();
            branch
                .as_deref()
                .or_else(|| explicit_branch_name(pin))
                .map_or_else(
                    || format!("the default branch is at {sha7}"),
                    |name| format!("{name} is at {sha7}"),
                )
        }
    }
}

fn explicit_branch_name(pin: &Pin) -> Option<&str> {
    match pin {
        Pin::Branch { branch, .. } => Some(branch.as_str()),
        // Every other pin genuinely has no explicit branch name to give —
        // `DefaultBranch` follows the remote's own default, never a name
        // this crate chose, and the rest never involve a branch at all —
        // so `None` here is a real answer for each of them, listed one by
        // one rather than through a wildcard, not a case this function could
        // not reach.
        Pin::CratesIo
        | Pin::OtherRegistry { .. }
        | Pin::Tag { .. }
        | Pin::DefaultBranch { .. }
        | Pin::Rev { .. }
        | Pin::Path { .. }
        | Pin::Unrecognised { .. } => None,
    }
}

/// Runs every distinct query in `queries`, in chunks of at most
/// [`REMOTE_QUERIES_IN_FLIGHT_MAX`], each query on its own thread.
fn answer_all(queries: &BTreeSet<Query>, remotes: &Remotes) -> Answers {
    let queries: Vec<&Query> = queries.iter().collect();
    let mut answers = Answers::default();
    for chunk in queries.chunks(REMOTE_QUERIES_IN_FLIGHT_MAX) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|&query| scope.spawn(|| answer_one(query, remotes)))
                .collect();
            for handle in handles {
                let answered = match handle.join() {
                    Ok(answered) => answered,
                    Err(payload) => std::panic::resume_unwind(payload),
                };
                answers.insert(answered);
            }
        });
    }
    assert_eq!(
        answers.len(),
        queries.len(),
        "every distinct query is answered exactly once"
    );
    answers
}

/// Logs, then runs, one query.
fn answer_one(query: &Query, remotes: &Remotes) -> Answered {
    match query {
        Query::CratesIo { name } => {
            log_query(remotes, || format!("crates-io {name}"));
            Answered::CratesIo {
                name: name.clone(),
                result: remotes.crates_io.fetch(name),
            }
        }
        Query::Tags { url } => {
            log_query(remotes, || format!("git {url} tags"));
            Answered::Tags {
                url: url.clone(),
                answer: git_remote::list_tags(url),
            }
        }
        Query::Branch { url, branch } => {
            log_query(remotes, || format!("git {url} branch {branch}"));
            Answered::Branch {
                url: url.clone(),
                branch: branch.clone(),
                answer: git_remote::branch_head(url, branch),
            }
        }
        Query::DefaultBranch { url } => {
            log_query(remotes, || format!("git {url} default-branch"));
            Answered::DefaultBranch {
                url: url.clone(),
                answer: git_remote::default_branch_head(url),
            }
        }
    }
}

fn finalize_crates_io(
    name: &str,
    locked: &semver::Version,
    result: &Result<Vec<u8>, IndexFetchFailure>,
) -> Behind {
    match result {
        Ok(body) => match index_entries::newest_release_above(locked, body) {
            Ok(Some(newer)) => Behind::Behind(Newer::Version(newer)),
            Ok(None) => Behind::Current,
            Err(failure) => undetermined(UndeterminedReason::UnexpectedResponse, failure.detail),
        },
        Err(IndexFetchFailure::Unreachable { detail }) => undetermined(
            UndeterminedReason::Unreachable,
            format!("could not reach {}: {detail}", crates_io::CRATES_IO_HOST),
        ),
        Err(IndexFetchFailure::NotInIndex) => undetermined(
            UndeterminedReason::NotInIndex,
            format!("crates.io does not list {name}"),
        ),
        Err(IndexFetchFailure::UnexpectedResponse { detail }) => undetermined(
            UndeterminedReason::UnexpectedResponse,
            format!(
                "unexpected answer from {}: {detail}",
                crates_io::CRATES_IO_HOST
            ),
        ),
    }
}

fn finalize_tag(url: &str, package: &str, locked_tag: &str, answer: &TagsAnswer) -> Behind {
    match answer {
        TagsAnswer::Unreachable { detail } => undetermined(
            UndeterminedReason::Unreachable,
            unreachable_detail(url, detail),
        ),
        TagsAnswer::Malformed { detail } => undetermined(
            UndeterminedReason::UnexpectedResponse,
            unexpected_detail(url, detail),
        ),
        TagsAnswer::Tags(names) => {
            let Some(locked_version) = version_tag::locked_tag_version(package, locked_tag) else {
                return undetermined(
                    UndeterminedReason::TagNotAVersion,
                    format!(
                        "tag {locked_tag} is not a version tag `skeletons` recognises \
                         ({package}-vX.Y.Z, vX.Y.Z or X.Y.Z)"
                    ),
                );
            };
            // `release_tags` decides, per skeleton, whether the remote uses
            // the `<package>-v<version>` shape for it at all (preferred
            // whenever any such tag exists, whatever the repository's own
            // crate count), falling back to plain `vX.Y.Z`/`X.Y.Z` tags otherwise.
            let (_scheme, tags) = version_tag::release_tags(package, names);
            let newest = tags
                .into_iter()
                // A prerelease tag never counts as newer, the same reason a
                // prerelease version above the locked one does not count for
                // a registry dependency (`behind/index_entries.rs`): a
                // prerelease is not a release a wearer is behind on.
                .filter(|(_, version)| version.pre.is_empty())
                .filter(|(_, version)| {
                    version.cmp_precedence(&locked_version) == std::cmp::Ordering::Greater
                })
                .max_by(|(_, left), (_, right)| left.cmp_precedence(right));
            match newest {
                Some((name, _)) => Behind::Behind(Newer::Tag(name.to_owned())),
                None => Behind::Current,
            }
        }
    }
}

const fn undetermined(reason: UndeterminedReason, detail: String) -> Behind {
    Behind::Undetermined(Undetermined { reason, detail })
}

fn unreachable_detail(url: &str, detail: &str) -> String {
    format!("could not reach {url}: {detail}")
}

fn unexpected_detail(url: &str, detail: &str) -> String {
    format!("unexpected answer from {url}: {detail}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        Ask, Behind, CratesIoIndex, Need, Newer, OTHER_REGISTRY_DETAIL, Remotes,
        UNRECOGNISED_SOURCE_DETAIL, Undetermined, UndeterminedReason, determine, need,
        newer_detail,
    };
    use crate::git::ObjectId;
    use crate::skeleton::Choices;
    use crate::workspace::{Pin, WornDependency};

    fn commit(number: u8) -> String {
        format!("{number:0>2}c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2")
    }

    fn oid(number: u8) -> ObjectId {
        ObjectId::parse(&commit(number)).expect("a well-formed test object id")
    }

    #[test]
    fn detail_reads_the_registry_wording_for_a_newer_version() {
        let pin = Pin::CratesIo;
        let newer = Newer::Version(semver::Version::new(0, 2, 0));
        assert_eq!(newer_detail(&pin, &newer), "0.2.0 is available");
    }

    #[test]
    fn detail_reads_the_tag_wording_for_a_newer_tag() {
        let pin = Pin::Tag {
            url: "https://example.invalid/skeleton".to_owned(),
            tag: "v0.3.0".to_owned(),
            commit: oid(1),
        };
        let newer = Newer::Tag("v0.4.0".to_owned());
        assert_eq!(newer_detail(&pin, &newer), "tag v0.4.0 is available");
    }

    #[test]
    fn detail_names_an_explicit_branch_by_its_own_pin_name() {
        let pin = Pin::Branch {
            url: "https://example.invalid/skeleton".to_owned(),
            branch: "main".to_owned(),
            commit: oid(1),
        };
        let newer = Newer::Commit {
            sha: oid(2),
            branch: None,
        };
        let sha7 = &commit(2)[..7];
        assert_eq!(newer_detail(&pin, &newer), format!("main is at {sha7}"));
    }

    #[test]
    fn detail_names_a_default_branch_the_server_named() {
        let pin = Pin::DefaultBranch {
            url: "https://example.invalid/skeleton".to_owned(),
            commit: oid(1),
        };
        let newer = Newer::Commit {
            sha: oid(2),
            branch: Some("trunk".to_owned()),
        };
        let sha7 = &commit(2)[..7];
        assert_eq!(newer_detail(&pin, &newer), format!("trunk is at {sha7}"));
    }

    #[test]
    fn detail_falls_back_to_the_default_branch_wording_when_unnamed() {
        let pin = Pin::DefaultBranch {
            url: "https://example.invalid/skeleton".to_owned(),
            commit: oid(1),
        };
        let newer = Newer::Commit {
            sha: oid(2),
            branch: None,
        };
        let sha7 = &commit(2)[..7];
        assert_eq!(
            newer_detail(&pin, &newer),
            format!("the default branch is at {sha7}")
        );
    }

    /// A worn dependency carrying `pin`, with every other field a plausible
    /// placeholder: `need`'s own tests, and `determine`'s, never read
    /// anything but `pin`, `package` and `version`.
    fn worn_dependency(
        key: &str,
        package: &str,
        version: semver::Version,
        pin: Pin,
    ) -> WornDependency {
        WornDependency {
            manifest: "Cargo.toml".to_owned(),
            key: key.to_owned(),
            package: package.to_owned(),
            version,
            skeleton_directory: "/skeleton".into(),
            pin,
            choices: Ok(Choices::new()),
        }
    }

    /// One row per [`Pin`] variant: `need` is the one function that decides
    /// whether a pin is already answered or still needs asking, so this
    /// table is the whole of what either branch can ever produce for a
    /// given pin.
    #[test]
    fn need_classifies_every_pin_variant_exactly_once() {
        let commit = oid(1);
        let cases = answered_pin_variant_cases(&commit)
            .into_iter()
            .chain(ask_pin_variant_cases(&commit));
        for (pin, expected) in cases {
            let dependency =
                worn_dependency("k", "semver", semver::Version::new(1, 0, 0), pin.clone());
            assert_eq!(need(&dependency), expected, "pin was: {pin:?}");
        }
    }

    /// Half of [`need_classifies_every_pin_variant_exactly_once`]'s table:
    /// every [`Pin`] variant [`need`] answers immediately, needing no
    /// remote at all.
    fn answered_pin_variant_cases(commit: &ObjectId) -> Vec<(Pin, Need)> {
        vec![
            (
                Pin::Path {
                    directory: "/anywhere".into(),
                },
                Need::Answered(Behind::Pinned),
            ),
            (
                Pin::Rev {
                    url: "https://example.invalid/skeleton".to_owned(),
                    rev: "deadbee".to_owned(),
                    commit: commit.clone(),
                },
                Need::Answered(Behind::Pinned),
            ),
            (
                Pin::OtherRegistry {
                    source: "sparse+https://example.invalid/index/".to_owned(),
                },
                Need::Answered(Behind::Undetermined(Undetermined {
                    reason: UndeterminedReason::OtherRegistry,
                    detail: OTHER_REGISTRY_DETAIL.to_owned(),
                })),
            ),
            (
                Pin::Unrecognised {
                    source: "vcs+https://example.invalid/skeleton".to_owned(),
                },
                Need::Answered(Behind::Undetermined(Undetermined {
                    reason: UndeterminedReason::UnrecognisedSource,
                    detail: UNRECOGNISED_SOURCE_DETAIL.to_owned(),
                })),
            ),
        ]
    }

    /// The other half of
    /// [`need_classifies_every_pin_variant_exactly_once`]'s table: every
    /// [`Pin`] variant [`need`] turns into a remote [`Ask`].
    fn ask_pin_variant_cases(commit: &ObjectId) -> Vec<(Pin, Need)> {
        vec![
            (
                Pin::CratesIo,
                Need::Ask(Ask::CratesIo {
                    name: "semver".to_owned(),
                    locked: semver::Version::new(1, 0, 0),
                }),
            ),
            (
                Pin::Tag {
                    url: "https://example.invalid/skeleton".to_owned(),
                    tag: "v1.0.0".to_owned(),
                    commit: commit.clone(),
                },
                Need::Ask(Ask::Tag {
                    package: "semver".to_owned(),
                    url: "https://example.invalid/skeleton".to_owned(),
                    locked_tag: "v1.0.0".to_owned(),
                }),
            ),
            (
                Pin::Branch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    branch: "main".to_owned(),
                    commit: commit.clone(),
                },
                Need::Ask(Ask::Branch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    branch: "main".to_owned(),
                    locked_commit: commit.clone(),
                    skeleton_directory: PathBuf::from("/skeleton"),
                }),
            ),
            (
                Pin::DefaultBranch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    commit: commit.clone(),
                },
                Need::Ask(Ask::DefaultBranch {
                    url: "https://example.invalid/skeleton".to_owned(),
                    locked_commit: commit.clone(),
                    skeleton_directory: PathBuf::from("/skeleton"),
                }),
            ),
        ]
    }

    /// The dedup key `Ask::query` builds is the remote target alone, never
    /// the locked value being compared against it: two worn dependencies
    /// pinned to the same crates.io crate at different locked versions share
    /// one query between them, and each still reads its own correct answer
    /// back out of it. The shared remote-query log is the positive control
    /// that only one query for `semver` actually ran, not two.
    #[test]
    fn one_crates_io_name_locked_at_two_versions_reads_each_its_own_answer() {
        let index_directory = tempfile::tempdir().expect("scratch directory");
        std::fs::create_dir_all(index_directory.path().join("se/mv"))
            .expect("create index directory");
        std::fs::write(
            index_directory.path().join("se/mv/semver"),
            b"{\"name\":\"semver\",\"vers\":\"1.1.0\",\"deps\":[],\"cksum\":\"a\",\
              \"features\":{},\"yanked\":false}\n",
        )
        .expect("write capture");

        let log_directory = tempfile::tempdir().expect("scratch directory");
        let log_path = log_directory.path().join("remote.log");

        let remotes = Remotes {
            crates_io: CratesIoIndex::Captured(index_directory.path().to_owned()),
            log: Some(log_path.clone()),
        };

        let behind_dependency = worn_dependency(
            "behind",
            "semver",
            semver::Version::new(1, 0, 0),
            Pin::CratesIo,
        );
        let current_dependency = worn_dependency(
            "current",
            "semver",
            semver::Version::new(1, 1, 0),
            Pin::CratesIo,
        );
        let worn = [&behind_dependency, &current_dependency];

        let results = determine(&worn, &remotes);

        assert_eq!(
            results.get(&behind_dependency.id()),
            Some(&Behind::Behind(Newer::Version(semver::Version::new(
                1, 1, 0
            ))))
        );
        assert_eq!(
            results.get(&current_dependency.id()),
            Some(&Behind::Current)
        );

        let log = std::fs::read_to_string(&log_path).expect("the log must have been written");
        assert_eq!(
            log.matches("crates-io semver").count(),
            1,
            "the two dependencies must share one crates.io query, not two; log was: {log:?}"
        );
    }
}
