//! The two names `remove` passes around as text, each its own type so that a
//! call that swaps them does not compile.
//!
//! Neither is checked. A key here is whatever the manifest holds, which `wear`
//! would never have written but a person can: the empty key is a key, and
//! `remove` takes it out like any other. The grammar `wear` holds a key to is
//! `wear`'s own.

/// The key a wearing's table sits under in `[package.metadata.skeletons]`, and
/// the key of the dependency beside it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WearingKey(String);

impl WearingKey {
    /// The key as the manifest holds it, whatever that is.
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    /// The key as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A manifest as a message shows it: its path relative to the workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestShown(String);

impl ManifestShown {
    /// The manifest shown as `text`.
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    /// The manifest as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{ManifestShown, WearingKey};

    #[test]
    fn a_key_is_whatever_the_manifest_holds_the_empty_key_included() {
        assert_eq!(WearingKey::new("").as_str(), "");
        assert_eq!(WearingKey::new("a\nb").as_str(), "a\nb");
    }

    #[test]
    fn keys_are_ordered_as_their_text_is() {
        let mut keys = [
            WearingKey::new("ci-next"),
            WearingKey::new(""),
            WearingKey::new("ci"),
        ];

        keys.sort();

        let texts: Vec<&str> = keys.iter().map(WearingKey::as_str).collect();
        assert_eq!(texts, ["", "ci", "ci-next"]);
    }

    #[test]
    fn a_manifest_is_shown_as_it_was_given() {
        assert_eq!(
            ManifestShown::new("ritual/Cargo.toml").as_str(),
            "ritual/Cargo.toml"
        );
    }
}
