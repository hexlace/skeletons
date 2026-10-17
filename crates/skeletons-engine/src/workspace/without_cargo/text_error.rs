//! Why a lockfile or a manifest could not be read from its text.

use std::fmt;

/// A shape of text that Cargo fails to load, which this reader names when it raises the refusal.
///
/// Only text can be one of these. A manifest that is not there has no text, so the shapes that
/// need no text are not among them ([`CargoCannotLoad`](super::cargo_cannot_load::CargoCannotLoad)
/// holds every shape, and each of these converts into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusedShape {
    /// A dependency of the package writes both `git` and `path`, which Cargo refuses as
    /// ambiguous.
    GitAndPath,
}

/// What was wrong with a lockfile's or a manifest's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextErrorKind {
    /// Cargo itself would refuse the text, or the text is not what it claims to be.
    Unreadable,
    /// Cargo accepts the text, but it says something this reader does not follow.
    Unsupported,
}

/// A lockfile or a manifest that could not be read from its text, with a sentence saying why.
///
/// The sentence names the key or the entry at fault and holds no file path: the caller knows
/// which file it read, and puts the path in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextError {
    kind: TextErrorKind,
    detail: String,
    /// The shape Cargo fails on that this text is, when it is one this reader names.
    refused_shape: Option<RefusedShape>,
}

impl TextError {
    /// An error for text that Cargo would refuse, or that is not valid TOML.
    pub(crate) fn unreadable(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unreadable,
            detail: detail.into(),
            refused_shape: None,
        }
    }

    /// An error for text that is a shape Cargo fails to load, named by `shape`.
    pub(crate) fn unreadable_as(shape: RefusedShape, detail: impl Into<String>) -> Self {
        Self {
            refused_shape: Some(shape),
            ..Self::unreadable(detail)
        }
    }

    /// An error for text that Cargo accepts and this reader does not follow.
    pub(crate) fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unsupported,
            detail: detail.into(),
            refused_shape: None,
        }
    }

    /// The shape Cargo fails to load that this text is, or `None` for any refusal that is not a
    /// shape measured against Cargo.
    #[must_use]
    pub(crate) const fn refused_shape(&self) -> Option<RefusedShape> {
        self.refused_shape
    }

    /// Whether Cargo would refuse the text too, or accepts it and this reader does not follow it.
    #[must_use]
    pub(crate) const fn kind(&self) -> TextErrorKind {
        self.kind
    }

    /// The sentence saying what is wrong.
    #[must_use]
    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for TextError {}

#[cfg(test)]
mod tests {
    use super::{RefusedShape, TextError, TextErrorKind};

    #[test]
    fn an_unreadable_error_has_the_unreadable_kind() {
        let error = TextError::unreadable("bad key");
        assert_eq!(error.kind(), TextErrorKind::Unreadable);
        assert_eq!(error.detail(), "bad key");
        assert_eq!(error.to_string(), "bad key");
    }

    #[test]
    fn an_unsupported_error_has_the_unsupported_kind() {
        let error = TextError::unsupported("format 2");
        assert_eq!(error.kind(), TextErrorKind::Unsupported);
        assert_eq!(error.detail(), "format 2");
    }

    #[test]
    fn only_an_error_made_as_a_shape_cargo_fails_on_carries_that_shape() {
        // The shape is what a read that passes over a manifest matches on, so it must be set
        // where a shape is raised and nowhere else: neither plain kind carries one.
        let shaped = TextError::unreadable_as(RefusedShape::GitAndPath, "ambiguous");

        assert_eq!(shaped.kind(), TextErrorKind::Unreadable);
        assert_eq!(shaped.detail(), "ambiguous");
        assert_eq!(shaped.refused_shape(), Some(RefusedShape::GitAndPath));
        assert_eq!(TextError::unreadable("bad").refused_shape(), None);
        assert_eq!(TextError::unsupported("format 2").refused_shape(), None);
    }
}
