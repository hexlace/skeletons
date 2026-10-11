//! A directory's own prefix within its git repository — what
//! `--show-prefix` answers — and turning a path git reported (always
//! relative to the repository's own top level) back into one relative to
//! that directory, as `sync` needs for the workspace root it asked about.

/// A directory's own path relative to its repository's top level, whether
/// that directory is a workspace root or a skeleton's package directory:
/// `""` at the top level itself, otherwise a string ending in `/`, holding
/// no leading `/` and no empty, `.` or `..` component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RepositoryPrefix(String);

impl RepositoryPrefix {
    /// Parses `line` — one `--show-prefix` line, or `""` for "at the top
    /// level" — as a repository prefix.
    ///
    /// # Errors
    ///
    /// Returns `None` when `line` is non-empty and does not end in `/`,
    /// starts with `/`, or holds an empty, `.` or `..` component.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        if line.is_empty() {
            return Some(Self(String::new()));
        }
        if line.starts_with('/') || !line.ends_with('/') {
            return None;
        }
        let without_trailing_slash = &line[..line.len() - 1];
        for component in without_trailing_slash.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return None;
            }
        }
        Some(Self(line.to_owned()))
    }

    /// This prefix's own text, exactly as parsed (`""` at the top level,
    /// otherwise ending in `/`) — for a caller that builds a git object spec
    /// (`<rev>:<prefix>`) out of it, as opposed to [`shown`](Self::shown),
    /// which reads a path git reported back the other way.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// `git_path` (a path git reported, always relative to the repository's
    /// own top level), expressed relative to the directory this prefix
    /// names: the prefix itself stripped, when `git_path` sits under it;
    /// otherwise one `../` per prefix component, prepended — `git_path`
    /// names something elsewhere in the repository, outside this
    /// directory.
    pub(crate) fn shown(&self, git_path: &str) -> String {
        if let Some(under_root) = git_path.strip_prefix(self.0.as_str()) {
            return under_root.to_owned();
        }
        let ascents = self.0.matches('/').count();
        let mut shown = "../".repeat(ascents);
        shown.push_str(git_path);
        shown
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::RepositoryPrefix;

    fn prefix(line: &str) -> RepositoryPrefix {
        RepositoryPrefix::parse(line).expect("a well-formed test prefix")
    }

    #[test]
    fn an_empty_line_parses_to_the_top_level() {
        assert!(RepositoryPrefix::parse("").is_some());
    }

    #[test]
    fn a_single_component_prefix_parses() {
        assert!(RepositoryPrefix::parse("ws/").is_some());
    }

    #[test]
    fn a_multi_component_prefix_parses() {
        assert!(RepositoryPrefix::parse("a/b/").is_some());
    }

    #[test]
    fn a_leading_slash_is_rejected() {
        assert!(RepositoryPrefix::parse("/a/").is_none());
    }

    #[test]
    fn a_line_not_ending_in_a_slash_is_rejected() {
        assert!(RepositoryPrefix::parse("a").is_none());
    }

    #[test]
    fn an_empty_component_is_rejected() {
        assert!(RepositoryPrefix::parse("a//").is_none());
    }

    #[test]
    fn a_dot_dot_component_is_rejected() {
        assert!(RepositoryPrefix::parse("../").is_none());
    }

    #[test]
    fn shown_strips_the_prefix_when_the_path_sits_under_it() {
        assert_eq!(prefix("ws/").shown("ws/x"), "x");
    }

    #[test]
    fn shown_ascends_once_per_prefix_component_when_the_path_sits_outside_it() {
        assert_eq!(prefix("ws/").shown("top"), "../top");
        assert_eq!(prefix("a/b/").shown("c"), "../../c");
    }

    #[test]
    fn shown_at_the_top_level_returns_the_path_unchanged() {
        assert_eq!(prefix("").shown("top-untracked"), "top-untracked");
    }

    proptest! {
        /// However `line` is shaped, `parse` never panics.
        #[test]
        fn parse_never_panics(line in ".*") {
            let _result = RepositoryPrefix::parse(&line);
        }
    }
}
