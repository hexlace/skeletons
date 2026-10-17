//! Decoding the query string of a git source: the part of `git+<url>?<query>#<commit>` that
//! names a branch, a tag or a revision.
//!
//! Cargo writes a git dependency's one reference into its source as a query
//! and percent-encodes the characters a query cannot hold, a `/` in a branch
//! or tag name included, so a reader of a source has to decode it the same
//! way. This is that one decoder.

/// One of the three query shapes a git source's query string can decode to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitQuery {
    Tag(String),
    Branch(String),
    Rev(String),
}

/// Decodes a git source's query string (already percent-decoded key by key,
/// since cargo percent-encodes a `/` inside a branch or tag name) into
/// exactly one of `tag`, `branch` or `rev` — `None` for an unknown key, a
/// duplicated one, or more than one of the three at once.
pub(crate) fn parsed_git_query(query: &str) -> Option<GitQuery> {
    let mut tag = None;
    let mut branch = None;
    let mut rev = None;

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let value = value.into_owned();
        match key.as_ref() {
            "tag" if tag.is_none() => tag = Some(value),
            "branch" if branch.is_none() => branch = Some(value),
            "rev" if rev.is_none() => rev = Some(value),
            _ => return None,
        }
    }

    match (tag, branch, rev) {
        (Some(tag), None, None) => Some(GitQuery::Tag(tag)),
        (None, Some(branch), None) => Some(GitQuery::Branch(branch)),
        (None, None, Some(rev)) => Some(GitQuery::Rev(rev)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{GitQuery, parsed_git_query};

    #[test]
    fn a_tag_decodes_to_a_tag() {
        assert_eq!(
            parsed_git_query("tag=v1.0.0"),
            Some(GitQuery::Tag("v1.0.0".to_owned()))
        );
    }

    #[test]
    fn a_branch_decodes_to_a_branch() {
        assert_eq!(
            parsed_git_query("branch=main"),
            Some(GitQuery::Branch("main".to_owned()))
        );
    }

    #[test]
    fn a_rev_decodes_to_a_rev() {
        assert_eq!(
            parsed_git_query("rev=3f2a9c1d"),
            Some(GitQuery::Rev("3f2a9c1d".to_owned()))
        );
    }

    #[test]
    fn a_percent_encoded_slash_in_a_branch_is_decoded() {
        assert_eq!(
            parsed_git_query("branch=feat%2Fx"),
            Some(GitQuery::Branch("feat/x".to_owned()))
        );
    }

    #[test]
    fn a_percent_encoded_slash_in_a_tag_is_decoded() {
        assert_eq!(
            parsed_git_query("tag=release%2Fv1"),
            Some(GitQuery::Tag("release/v1".to_owned()))
        );
    }

    #[test]
    fn an_unknown_key_decodes_to_nothing() {
        assert_eq!(parsed_git_query("ref=v1"), None);
    }

    #[test]
    fn two_references_at_once_decode_to_nothing() {
        assert_eq!(parsed_git_query("tag=v1&branch=main"), None);
    }

    #[test]
    fn a_duplicated_key_decodes_to_nothing() {
        assert_eq!(parsed_git_query("tag=v1&tag=v2"), None);
    }

    #[test]
    fn an_empty_query_decodes_to_nothing() {
        assert_eq!(parsed_git_query(""), None);
    }
}
