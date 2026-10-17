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
    /// A `HOME` that holds no configuration, for the Cargo this home runs: Cargo's git reads
    /// `~/.gitconfig` through it.
    home: tempfile::TempDir,
}

impl CargoHome {
    pub(in crate::workspace::without_cargo) fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("a temporary directory"),
            home: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    pub(in crate::workspace::without_cargo) fn path(&self) -> &Path {
        self.directory.path()
    }

    /// The `HOME` of the Cargo this home runs: a directory that holds no configuration.
    pub(in crate::workspace::without_cargo) fn home(&self) -> &Path {
        self.home.path()
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

    /// Moves this home's `config.toml`, which is the replacement of crates.io that
    /// [`vendor`](Self::vendor) writes, into `cargo_directory`, a workspace's own `.cargo`, so
    /// that the replacement is the workspace's and the home holds none.
    pub(in crate::workspace::without_cargo) fn move_configuration_into(
        &self,
        cargo_directory: &Path,
    ) {
        std::fs::create_dir_all(cargo_directory).expect("a configuration directory");
        std::fs::rename(
            self.path().join("config.toml"),
            cargo_directory.join("config.toml"),
        )
        .expect("the home's configuration moved");
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
        let mut generate = self.cargo_command();
        generate.current_dir(directory).arg("generate-lockfile");
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

    /// A cargo command with this home as `CARGO_HOME` and a `HOME` of its own, so that the user's
    /// `~/.gitconfig` is never read when Cargo fetches a git source.
    fn cargo_command(&self) -> std::process::Command {
        let mut command = crate::cargo::command();
        command
            .env("CARGO_HOME", self.path())
            .env("HOME", self.home.path());
        command
    }

    fn vendored(&self) -> PathBuf {
        self.path().join("vendor")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;

    use super::CargoHome;

    #[test]
    fn the_cargo_command_has_a_home_of_its_own_and_this_cargo_home() {
        // Cargo reads git's configuration through `HOME` when it fetches a git source, so the
        // command must not inherit the running user's. The command is built and inspected
        // without being run: its `HOME` must be the empty directory this home owns, which is not
        // its `CARGO_HOME`, and its `CARGO_HOME` must be this home.
        let cargo_home = CargoHome::new();

        let command = cargo_home.cargo_command();
        let environment: BTreeMap<&OsStr, Option<&OsStr>> = command.get_envs().collect();

        assert_eq!(
            environment.get(OsStr::new("HOME")),
            Some(&Some(cargo_home.home.path().as_os_str())),
            "HOME is the directory of its own"
        );
        assert_eq!(
            environment.get(OsStr::new("CARGO_HOME")),
            Some(&Some(cargo_home.path().as_os_str())),
            "CARGO_HOME is this home"
        );
        assert_ne!(
            cargo_home.home.path(),
            cargo_home.path(),
            "the two directories are separate"
        );
        assert_eq!(
            std::fs::read_dir(cargo_home.home.path())
                .expect("the home directory exists")
                .count(),
            0,
            "the home holds no configuration"
        );
    }
}
