//! A Cargo home of a test's own, in which Cargo locks workspaces with git and registry sources
//! without reaching the network.

use std::path::{Path, PathBuf};

use super::Fixture;
use crate::workspace::Network;

/// What a vendored crate's `.cargo-checksum.json` holds: no files to check, and no package
/// checksum, so that Cargo takes the directory as it is.
const VENDORED_CHECKSUM: &str = "{\"files\":{},\"package\":null}";

/// A temporary directory used as `CARGO_HOME`, which a test fills with vendored crates and with
/// the git sources Cargo fetches from `file://` repositories.
///
/// A workspace locked in a home that has never fetched a git reference cannot be read back
/// offline: Cargo fails to find the reference, which is not the lockfile abort the tests are
/// about. [`CargoHome::prime`] fetches every reference a test will switch to, so that the
/// workspace read afterwards meets Cargo's own verdict on its lockfile.
pub(in crate::workspace::without_cargo) struct CargoHome {
    directory: tempfile::TempDir,
}

impl CargoHome {
    pub(in crate::workspace::without_cargo) fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    pub(in crate::workspace::without_cargo) fn path(&self) -> &Path {
        self.directory.path()
    }

    /// Adds the crate `name` at `version` to the directory that replaces crates.io in this home.
    /// A lockfile still names crates.io as the crate's source.
    pub(in crate::workspace::without_cargo) fn vendor(&self, name: &str, version: &str) {
        let crate_directory = self.vendored().join(format!("{name}-{version}"));
        std::fs::create_dir_all(crate_directory.join("src")).expect("a crate directory");
        std::fs::write(crate_directory.join("src/lib.rs"), "").expect("a source file");
        std::fs::write(
            crate_directory.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n"),
        )
        .expect("a manifest");
        std::fs::write(
            crate_directory.join(".cargo-checksum.json"),
            VENDORED_CHECKSUM,
        )
        .expect("a checksum file");
        std::fs::write(
            self.path().join("config.toml"),
            format!(
                "[source.crates-io]\nreplace-with = \"vendored\"\n\
                 [source.vendored]\ndirectory = \"{}\"\n",
                self.vendored().display()
            ),
        )
        .expect("a configuration file");
    }

    /// Has Cargo fetch each of `dependencies`, the lines a `[dependencies]` table holds, into
    /// this home, by locking a workspace of its own for each.
    ///
    /// Locking fetches the reference a dependency names, which `cargo fetch` would too but
    /// without the chance of rewriting a lockfile a test is about to read.
    pub(in crate::workspace::without_cargo) fn prime(&self, dependencies: &[String]) {
        for dependency in dependencies {
            let primer = Fixture::new();
            primer.write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
            primer.package("app", "app", &format!("[dependencies]\n{dependency}\n"));
            self.lock(&primer.root, Network::Allowed)
                .unwrap_or_else(|stderr| panic!("{dependency} could not be fetched: {stderr}"));
        }
    }

    /// Runs `cargo generate-lockfile` in `directory`, with this home as Cargo's, and says what
    /// Cargo printed on failure.
    ///
    /// With [`Network::Allowed`] Cargo may fetch, which only a `file://` source in a test's own
    /// repository can use: nothing here names a host. `CARGO_NET_OFFLINE` is set to say so
    /// whatever the environment holds, because an environment that is offline would otherwise
    /// stop a fetch that reaches nothing outside the machine.
    pub(in crate::workspace::without_cargo) fn lock(
        &self,
        directory: &Path,
        network: Network,
    ) -> Result<(), String> {
        let mut generate = crate::cargo::command();
        generate
            .current_dir(directory)
            .env("CARGO_HOME", self.path())
            .arg("generate-lockfile");
        match network {
            Network::Refused => {
                generate.arg("--offline");
            }
            Network::Allowed => {
                generate.env("CARGO_NET_OFFLINE", "false");
            }
        }
        let output = generate
            .output()
            .map_err(|error| format!("cargo could not be run: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn vendored(&self) -> PathBuf {
        self.path().join("vendor")
    }
}
