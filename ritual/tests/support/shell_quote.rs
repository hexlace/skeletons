//! Putting a path into text that a shell will read back as that one path.
//!
//! Git runs a configured filter command through `sh`, and a fixture script
//! reads its own paths the same way, so a path written into either has to
//! survive word splitting whatever the system temporary directory is called.

use std::path::Path;

/// `path` in single quotes, for text a POSIX shell reads.
///
/// A single quote cannot appear inside single quotes, so each one in the path
/// closes the quoted run, is written escaped, and opens a new run.
pub(crate) fn shell_quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}
