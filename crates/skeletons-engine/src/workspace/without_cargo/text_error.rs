//! Why a lockfile or a manifest could not be read from its text.

use std::fmt;

use super::cargo_cannot_load::CargoCannotLoad;

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
    cargo_cannot_load: Option<CargoCannotLoad>,
}

impl TextError {
    /// An error for text that Cargo would refuse, or that is not valid TOML.
    pub(crate) fn unreadable(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unreadable,
            detail: detail.into(),
            cargo_cannot_load: None,
        }
    }

    /// An error for text that is a shape Cargo fails to load, named by `shape`.
    ///
    /// # Panics
    ///
    /// When `shape` is [`CargoCannotLoad::Missing`]: a manifest that is not there has no text to
    /// be refused.
    pub(crate) fn unreadable_as(shape: CargoCannotLoad, detail: impl Into<String>) -> Self {
        assert_ne!(
            shape,
            CargoCannotLoad::Missing,
            "a manifest that is not there has no text to refuse"
        );
        Self {
            cargo_cannot_load: Some(shape),
            ..Self::unreadable(detail)
        }
    }

    /// An error for text that Cargo accepts and this reader does not follow.
    pub(crate) fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            kind: TextErrorKind::Unsupported,
            detail: detail.into(),
            cargo_cannot_load: None,
        }
    }

    /// The shape Cargo fails to load that this text is, or `None` for any refusal that is not a
    /// shape measured against Cargo.
    #[must_use]
    pub(crate) const fn cargo_cannot_load(&self) -> Option<CargoCannotLoad> {
        self.cargo_cannot_load
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
    use crate::workspace::without_cargo::cargo_cannot_load::CargoCannotLoad;

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
        let shaped = TextError::unreadable_as(CargoCannotLoad::GitAndPath, "ambiguous");

        assert_eq!(shaped.kind(), TextErrorKind::Unreadable);
        assert_eq!(shaped.detail(), "ambiguous");
        assert_eq!(
            shaped.cargo_cannot_load(),
            Some(CargoCannotLoad::GitAndPath)
        );
        assert_eq!(TextError::unreadable("bad").cargo_cannot_load(), None);
        assert_eq!(TextError::unsupported("format 2").cargo_cannot_load(), None);
    }

    #[test]
    #[should_panic(expected = "a manifest that is not there has no text to refuse")]
    fn a_missing_manifest_is_no_text_error() {
        let _ = TextError::unreadable_as(CargoCannotLoad::Missing, "gone");
    }
}
