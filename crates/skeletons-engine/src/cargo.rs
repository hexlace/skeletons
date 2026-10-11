//! The one place that decides which `cargo` this crate runs.
//!
//! `cargo metadata` (`workspace/cargo_metadata.rs`) and `cargo add`
//! (`wear/cargo_add.rs`) are both started through [`command`], so a command
//! line run under `cargo run` or `cargo test` uses the same cargo that
//! launched it in both.

use std::process::Command;

/// The program name used when `$CARGO` is not set.
const CARGO_PROGRAM: &str = "cargo";

/// A [`Command`] for cargo: `$CARGO` when it is set, `cargo` from `$PATH`
/// when it is not.
///
/// `$CARGO` is set under `cargo run` and `cargo test`, so a nested call uses
/// the cargo that launched this process rather than whatever `cargo` resolves
/// to on `$PATH`, as ritual's own `cargo metadata` caller does.
pub(crate) fn command() -> Command {
    Command::new(program(std::env::var("CARGO").ok()))
}

/// The program [`command`] runs, given what `$CARGO` held, so the rule can be
/// tested without touching the process environment.
fn program(configured: Option<String>) -> String {
    configured.unwrap_or_else(|| CARGO_PROGRAM.to_owned())
}

#[cfg(test)]
mod tests {
    use super::program;

    #[test]
    fn cargo_from_the_environment_is_used_as_given() {
        // `$CARGO` names the cargo that launched this process, possibly by
        // absolute path, and is never second-guessed.
        assert_eq!(
            program(Some("/toolchain/bin/cargo".to_owned())),
            "/toolchain/bin/cargo"
        );
    }

    #[test]
    fn cargo_falls_back_to_the_name_on_the_path_when_the_environment_has_none() {
        assert_eq!(program(None), "cargo");
    }
}
