//! Why a lockfile or a manifest could not be read from its text.

use std::fmt;

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
}

impl TextError {
    /// An error for text that Cargo would refuse, or that is not valid TOML.
    pub(crate) fn unreadable(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unreadable,
            detail: detail.into(),
        }
    }

    /// An error for text that Cargo accepts and this reader does not follow.
    pub(crate) fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unsupported,
            detail: detail.into(),
        }
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
    use super::{TextError, TextErrorKind};

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
}
