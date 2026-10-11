//! Where the skeleton comes from: the registry, a git repository or a
//! directory, as the `wear` task's source flags name it.

use std::path::PathBuf;

use rituals::clap;

/// The source flags as `clap` reads them, before they are read as a [`Source`].
//
// No `///` doc comment on this struct, for the reason `check::CheckArguments`
// carries none: `rituals` applies a task's `about` after `augment_args` and
// leaves `long_about` alone, so a struct doc comment of more than one
// paragraph would replace `--help`'s text. Each field keeps its own `///`
// line, which is the flag's help text.
//
// `clap` refuses every combination [`Source::from_flags`] has no reading for:
// `--git` with `--path`, and `--branch`, `--tag` or `--rev` with `--path`,
// without `--git`, or together. Each flag names what it conflicts with
// outright, because a bare `requires = "git"` is not enforced for a command
// line that holds `--path`, which conflicts with `--git`. It cannot be written
// as the enum it becomes: at this `clap` version a flattened `Option` of a
// struct holding a required `--git` still makes `--git` required of every
// command line.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct SourceFlags {
    /// take the skeleton from this git repository
    #[arg(long, value_name = "URL", conflicts_with = "path")]
    pub(crate) git: Option<String>,
    /// with --git, the branch to take it from
    #[arg(long, value_name = "BRANCH", requires = "git", conflicts_with_all = ["tag", "rev"])]
    pub(crate) branch: Option<String>,
    /// with --git, the tag to take it from
    #[arg(long, value_name = "TAG", requires = "git", conflicts_with_all = ["branch", "rev"])]
    pub(crate) tag: Option<String>,
    /// with --git, the commit to take it from
    #[arg(long, value_name = "REV", requires = "git", conflicts_with_all = ["branch", "tag"])]
    pub(crate) rev: Option<String>,
    /// take the skeleton from this directory
    #[arg(long, value_name = "PATH", conflicts_with_all = ["git", "branch", "tag", "rev"])]
    pub(crate) path: Option<PathBuf>,
}

/// Where `cargo add` is told to find the skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// The default registry, with no source flag at all.
    Registry,
    /// A git repository, at the tip of its default branch unless a reference
    /// says otherwise.
    Git {
        url: String,
        reference: GitReference,
    },
    /// A directory, as typed: relative to the directory the command ran in.
    Path(PathBuf),
}

/// Which commit of a git repository the skeleton is taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitReference {
    DefaultBranch,
    Branch(String),
    Tag(String),
    Rev(String),
}

impl Source {
    /// Reads the source flags `clap` accepted.
    ///
    /// # Panics
    ///
    /// Panics on a combination `clap` refuses: `--git` with `--path`, a
    /// `--branch`, `--tag` or `--rev` with `--path` or without `--git`, or more
    /// than one of the three. No command line reaches this far with one, and
    /// every such combination is named here rather than read as the nearest
    /// valid one, so no flag a person typed is ever dropped without a word.
    pub(crate) fn from_flags(flags: SourceFlags) -> Self {
        let SourceFlags {
            git,
            branch,
            tag,
            rev,
            path,
        } = flags;
        match (git, path, GitReference::from_flags(branch, tag, rev)) {
            (None, None, None) => Self::Registry,
            (None, Some(path), None) => Self::Path(path),
            (Some(url), None, reference) => Self::Git {
                url,
                reference: reference.unwrap_or(GitReference::DefaultBranch),
            },
            (Some(_), Some(_), _) | (None, _, Some(_)) => {
                unreachable!("clap refuses these source flags together")
            }
        }
    }
}

impl GitReference {
    /// The reference the flags name, or `None` when none is named.
    fn from_flags(
        branch: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
    ) -> Option<Self> {
        match (branch, tag, rev) {
            (None, None, None) => None,
            (Some(branch), None, None) => Some(Self::Branch(branch)),
            (None, Some(tag), None) => Some(Self::Tag(tag)),
            (None, None, Some(rev)) => Some(Self::Rev(rev)),
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                unreachable!("clap refuses --branch, --tag and --rev together")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{GitReference, Source, SourceFlags};

    const URL: &str = "file:///repository";

    fn git_flags() -> SourceFlags {
        SourceFlags {
            git: Some(URL.to_owned()),
            ..SourceFlags::default()
        }
    }

    fn git_source(reference: GitReference) -> Source {
        Source::Git {
            url: URL.to_owned(),
            reference,
        }
    }

    #[test]
    fn no_flag_at_all_is_the_registry() {
        assert_eq!(Source::from_flags(SourceFlags::default()), Source::Registry);
    }

    #[test]
    fn a_path_is_kept_exactly_as_typed() {
        let flags = SourceFlags {
            path: Some(PathBuf::from("../skeletons/tidy")),
            ..SourceFlags::default()
        };

        assert_eq!(
            Source::from_flags(flags),
            Source::Path(PathBuf::from("../skeletons/tidy"))
        );
    }

    #[test]
    fn a_git_url_with_no_reference_is_its_default_branch() {
        assert_eq!(
            Source::from_flags(git_flags()),
            git_source(GitReference::DefaultBranch)
        );
    }

    #[test]
    fn a_git_branch_is_read_as_a_branch() {
        let flags = SourceFlags {
            branch: Some("feature".to_owned()),
            ..git_flags()
        };

        assert_eq!(
            Source::from_flags(flags),
            git_source(GitReference::Branch("feature".to_owned()))
        );
    }

    #[test]
    fn a_git_tag_is_read_as_a_tag() {
        let flags = SourceFlags {
            tag: Some("1.0.0".to_owned()),
            ..git_flags()
        };

        assert_eq!(
            Source::from_flags(flags),
            git_source(GitReference::Tag("1.0.0".to_owned()))
        );
    }

    #[test]
    fn a_git_rev_is_read_as_a_rev() {
        let flags = SourceFlags {
            rev: Some("abc123".to_owned()),
            ..git_flags()
        };

        assert_eq!(
            Source::from_flags(flags),
            git_source(GitReference::Rev("abc123".to_owned()))
        );
    }

    #[test]
    #[should_panic(expected = "clap refuses these source flags together")]
    fn a_git_url_with_a_path_is_a_combination_clap_never_lets_through() {
        // The panic is the arm that stands for a combination `clap` refuses
        // first; `wear`'s own tests show `clap` refusing it.
        let flags = SourceFlags {
            path: Some(PathBuf::from("p")),
            ..git_flags()
        };

        let _ = Source::from_flags(flags);
    }

    #[test]
    #[should_panic(expected = "clap refuses these source flags together")]
    fn a_git_reference_without_git_is_a_combination_clap_never_lets_through() {
        let flags = SourceFlags {
            rev: Some("r".to_owned()),
            ..SourceFlags::default()
        };

        let _ = Source::from_flags(flags);
    }

    #[test]
    #[should_panic(expected = "clap refuses these source flags together")]
    fn a_git_reference_with_a_path_is_a_combination_clap_never_lets_through() {
        let flags = SourceFlags {
            path: Some(PathBuf::from("p")),
            tag: Some("t".to_owned()),
            ..SourceFlags::default()
        };

        let _ = Source::from_flags(flags);
    }

    #[test]
    #[should_panic(expected = "clap refuses --branch, --tag and --rev together")]
    fn two_git_references_are_a_combination_clap_never_lets_through() {
        let flags = SourceFlags {
            branch: Some("b".to_owned()),
            tag: Some("t".to_owned()),
            ..git_flags()
        };

        let _ = Source::from_flags(flags);
    }
}
