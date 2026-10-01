//! [`ShippedFile`]: one file under a skeleton's `files/`, in the one form it
//! is read in.

use super::template::Template;

/// A file under `files/`, read either as a template or as bytes nothing
/// interprets.
///
/// The variant is the decision, made once when the file is read. A verbatim
/// file has no [`Template`] to scan for a placeholder or a directive and no
/// lines to check for a hidden one, so every consumer matches on the variant
/// and none of them can hand a verbatim file to a scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShippedFile {
    /// A file parsed for placeholders and directives, which fill and select
    /// then change.
    Templated(Template),
    /// A file whose render is exactly these bytes. Never required to be
    /// UTF-8, because finding a placeholder or a directive is a text
    /// operation and a verbatim file is never searched for either.
    Verbatim(Vec<u8>),
}
