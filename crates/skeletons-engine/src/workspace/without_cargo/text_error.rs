//! Why a lockfile or a manifest could not be read from its text.

use std::fmt;

/// What was wrong with a lockfile's or a manifest's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextErrorKind {
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

    /// Whether Cargo would refuse the text too.
    #[must_use]
    pub(crate) const fn is_unreadable(&self) -> bool {
        matches!(self.kind, TextErrorKind::Unreadable)
    }

    /// Whether Cargo accepts the text and this reader does not follow it.
    #[must_use]
    pub(crate) const fn is_unsupported(&self) -> bool {
        matches!(self.kind, TextErrorKind::Unsupported)
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
    use super::TextError;

    #[test]
    fn an_unreadable_error_is_only_unreadable() {
        let error = TextError::unreadable("bad key");
        assert!(error.is_unreadable());
        assert!(!error.is_unsupported());
        assert_eq!(error.detail(), "bad key");
        assert_eq!(error.to_string(), "bad key");
    }

    #[test]
    fn an_unsupported_error_is_only_unsupported() {
        let error = TextError::unsupported("format 2");
        assert!(error.is_unsupported());
        assert!(!error.is_unreadable());
        assert_eq!(error.detail(), "format 2");
    }
}
