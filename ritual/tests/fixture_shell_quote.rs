//! Acceptance: a fixture path quoted for the shell reads back as that one
//! path.
//!
//! [`support::shell_quote::shell_quoted`] puts a path into a filter command
//! git runs through `sh` and into the scripts a fixture writes, so it has to
//! survive word splitting and quoting whatever the temporary directory is
//! called.

mod support;

use std::path::Path;
use std::process::Command;

use support::shell_quote::shell_quoted;

#[test]
fn a_path_with_spaces_and_quotes_reads_back_as_itself() {
    // Has `sh` echo each quoted path back: a plain one, one holding a
    // space, and one holding a space, a single quote and a dollar sign.
    for path in ["/plain/path", "/a b/c", "/it's a $HOME/dir"] {
        let script = format!("printf %s {}", shell_quoted(Path::new(path)));
        let output = Command::new("sh")
            .args(["-c", &script])
            .output()
            .expect("sh must run");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            path,
            "sh read {script:?} as something other than the path"
        );
    }
}
