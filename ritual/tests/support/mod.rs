//! Shared fixture-building and process-running support for the acceptance
//! tests in this directory.
//!
//! The tests in `ritual/tests/` spawn the real `ritual` binary, and those in
//! `wearer/tests/` the real `wearer` binary (this module is compiled into
//! both), against a real, disposable Cargo workspace, wearing real skeletons —
//! either the ones already checked in under
//! `crates/skeletons/test-skeletons/renders/`, taken by a `path =` dependency,
//! or a skeleton crate synthesized fresh into a real git repository for the
//! `tag =` / `branch =` / `rev =` / unqualified `git =` cases. The
//! exceptions are the `fixture_*` files, which check the fixtures themselves
//! and spawn no binary under test. Nothing here invents what `cargo`, `git`, or the
//! command line under test say; it only builds the inputs and reads back the
//! outputs.
//!
//! ## Isolation
//!
//! Every spawned `cargo` and `git` process is given its own `CARGO_HOME` and
//! `HOME`, each a fresh directory under this test's own [`Sandbox`], so a
//! test never reads or writes the real user's Cargo cache and never
//! consults the real user's global git configuration — which may force
//! every commit and tag to be signed, a policy that has nothing to do with
//! disposable fixture history that is never committed anywhere real and is
//! torn down at the end of the test that built it. Every git invocation here
//! overrides that by passing `-c commit.gpgsign=false -c tag.gpgSign=false`
//! explicitly.
//!
//! No test ever passes `--offline` to cargo: every git and registry source
//! these fixtures build is reached only over a `file://` URL naming a
//! directory this same test created, so nothing here ever performs an
//! actual network request, and `--offline` would also refuse the first,
//! necessary resolution against a freshly emptied `CARGO_HOME`.

// Every file under `ritual/tests/` is compiled as its own, separate binary
// crate, and this module is included fresh into each one via `mod
// support;`. Each test file only ever uses the handful of helpers its own
// scenarios need, so whichever helpers a given file does not reach are, from
// that one binary's point of view, genuinely dead code — while being very
// much alive in whichever sibling file does reach them. `allow`, not
// `expect`: `expect` would itself warn in whichever binary happens to use
// everything, and which helpers a given file needs is not something this
// module can predict.
#![allow(
    dead_code,
    reason = "helpers shared across separately compiled test binaries"
)]
// `support` is a private module inside a test binary that has no external
// consumers at all — nothing outside this one compiled test crate ever sees
// anything in it, `pub` or not. `pub(crate)` is the visibility that is true
// for sharing across the files under `tests/`, which is the same tension
// `skeletons`' own crate root documents at its crate-level `#![expect]` in
// `lib.rs`: nursery's `redundant_pub_crate` wants plain `pub` here, and
// rustc's `unreachable_pub` (which would fire on that `pub`) wants exactly
// what this module already does.
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is the true visibility in this private, no-external-consumer module tree"
)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) mod git;
pub(crate) mod json;
pub(crate) mod one_line;
pub(crate) mod passthrough_plain;
pub(crate) mod recorded_processes;
pub(crate) mod shell_quote;
pub(crate) mod slow_filter;
pub(crate) mod sync;
pub(crate) mod wear;

/// Every environment variable that redirects which repository, work tree,
/// index, object store or attribute source git answers about — the same
/// set `skeletons` refuses `sync` on (`REDIRECTING_VARIABLES` in
/// `crates/skeletons/src/git/command.rs`). Every process this suite spawns removes these, plus
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` (with `GIT_CONFIG_NOSYSTEM=1`
/// set instead), so the suite gives the same answer run from an ordinary
/// shell as it does run from inside a git hook — which exports `GIT_DIR`,
/// and would otherwise make every `sync` scenario here fail by the very
/// rule this suite is testing, for a reason that has nothing to do with
/// the scenario itself.
pub(crate) const REDIRECTING_VARIABLES: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_ATTR_SOURCE",
];

/// Removes every entry of [`REDIRECTING_VARIABLES`] plus
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`, and sets `GIT_CONFIG_NOSYSTEM=1`
/// — on `command`, so nothing this suite spawns (the `ritual` binary under
/// test, or a fixture-building `git`/`cargo` invocation) ever answers
/// about, or is redirected by, whatever repository or git configuration
/// happens to enclose the process actually running this suite.
pub(crate) fn isolate_from_the_enclosing_repository(command: &mut Command) {
    for variable in REDIRECTING_VARIABLES {
        command.env_remove(variable);
    }
    command
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

/// What a test in this directory returns.
///
/// The error path carries only a failure to build a fixture or run a process
/// at all, never what the test itself is verifying, which is carried entirely
/// by the assertions in the test body.
pub(crate) type TestOutcome = Result<(), Box<dyn Error>>;

/// A directory this test owns for the length of one test, removed when the
/// test's [`Sandbox`] (or, for a skeleton repository, [`git::SkeletonRepository`])
/// drops it.
///
/// Hand-rolled rather than taken from a crate such as `tempfile` (which
/// `skeletons`' own unit tests use): a temporary directory with cleanup on
/// drop is a handful of lines, so `ritual`'s dev-dependencies stay to what
/// these tests cannot do without: `serde_json`, for parsing `--json` output,
/// and `skeletons` with its `test-util` feature.
pub(crate) struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    /// Creates a fresh, empty directory under the system temporary
    /// directory, named uniquely by process id, wall-clock time, and a
    /// process-local counter so concurrently running tests never collide.
    pub(crate) fn new(label: &str) -> Result<Self, Box<dyn Error>> {
        static UNIQUE: AtomicU64 = AtomicU64::new(0);
        let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let elapsed_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let path = std::env::temp_dir().join(format!(
            "skeletons-test-{label}-{}-{}-{unique}",
            std::process::id(),
            elapsed_since_epoch.as_nanos(),
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// This directory's own path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // Best-effort: a test that already failed and left something
        // read-only or half-written should not also fail at cleanup.
        let _unused = fs::remove_dir_all(&self.path);
    }
}

/// The isolated environment a fixture's `cargo` and `git` processes run in:
/// a `CARGO_HOME` and a `HOME`, both fresh, both torn down together.
///
/// One `Sandbox` is normally shared across an entire test — building the
/// fixture and then running `ritual` against it — so that the same isolated
/// `CARGO_HOME` a setup `cargo generate-lockfile` populated is the one
/// `ritual`'s own `cargo metadata` call reads back.
pub(crate) struct Sandbox {
    cargo_home: TemporaryDirectory,
    home: TemporaryDirectory,
}

impl Sandbox {
    /// Builds a fresh sandbox: an empty `CARGO_HOME` and an empty `HOME`.
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            cargo_home: TemporaryDirectory::new("cargo-home")?,
            home: TemporaryDirectory::new("home")?,
        })
    }

    /// This sandbox's isolated `CARGO_HOME`.
    pub(crate) fn cargo_home(&self) -> &Path {
        self.cargo_home.path()
    }

    /// This sandbox's isolated `HOME`.
    pub(crate) fn home(&self) -> &Path {
        self.home.path()
    }
}

/// What a finished `ritual` process reported back: the whole of what a
/// caller can observe (the exit status, stdout and stderr), plus the process
/// id it ran under — for a scenario that needs to tell this
/// one run's own leftovers (a `skeletons-behind-<pid>-…` temporary directory,
/// most notably) apart from a concurrently running sibling test's.
pub(crate) struct Report {
    /// The status the process exited with.
    pub(crate) exit_code: i32,
    /// Everything the process wrote to stdout.
    pub(crate) stdout: String,
    /// Everything the process wrote to stderr.
    pub(crate) stderr: String,
    /// The process id `ritual` itself ran under.
    pub(crate) pid: u32,
}

/// A disposable Cargo workspace together with the sandbox `ritual` runs
/// against it in.
///
/// Bundling the two keeps both alive for as long as the fixture is needed:
/// a bare [`TemporaryDirectory`] returned as a path and dropped at the end
/// of a builder function would delete the workspace out from under the
/// test that still needs to run `ritual` against it.
pub(crate) struct Fixture {
    workspace: TemporaryDirectory,
    sandbox: Sandbox,
}

impl Fixture {
    /// Builds a fresh, empty workspace root and a fresh sandbox for it.
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            workspace: TemporaryDirectory::new("workspace")?,
            sandbox: Sandbox::new()?,
        })
    }

    /// This fixture's workspace root — the directory `ritual` runs in.
    pub(crate) fn root(&self) -> &Path {
        self.workspace.path()
    }

    /// This fixture's isolated sandbox.
    pub(crate) const fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// Writes `content` at `relative`, under this fixture's own root.
    pub(crate) fn write(&self, relative: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        write(self.root(), relative, content)
    }

    /// Reads the bytes at `relative`, under this fixture's own root.
    pub(crate) fn read(&self, relative: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        read(self.root(), relative)
    }

    /// Runs `ritual` with `arguments`, in this fixture's workspace, using
    /// this fixture's sandbox.
    pub(crate) fn run(&self, arguments: &[&str]) -> Result<Report, Box<dyn Error>> {
        run_ritual(self.root(), &self.sandbox, arguments)
    }

    /// Runs `ritual` with `arguments`, in this fixture's workspace, using
    /// this fixture's sandbox plus `extra_env` — additional environment
    /// variables a scenario needs beyond `CARGO_HOME`/`HOME`: the test-only
    /// captured-crates.io-index path, the test-only remote-query log path,
    /// or a build-script marker path a test-skeleton's own `build.rs` watches
    /// for.
    pub(crate) fn run_with_env(
        &self,
        arguments: &[&str],
        extra_env: &[(&str, &str)],
    ) -> Result<Report, Box<dyn Error>> {
        run_ritual_with_env(self.root(), &self.sandbox, arguments, extra_env)
    }

    /// Runs `cargo generate-lockfile` in this fixture's workspace, using
    /// this fixture's sandbox, so the fixture starts with the same
    /// committed `Cargo.lock` a real wearing repository would.
    ///
    /// Every dependency this suite's fixtures declare resolves over a
    /// `path =` or a local `file://` `git =` URL, so this never reaches the
    /// network.
    pub(crate) fn generate_lockfile(&self) -> Result<(), Box<dyn Error>> {
        let mut command = cargo_command();
        command
            .current_dir(self.root())
            .env("CARGO_HOME", self.sandbox.cargo_home())
            .env("HOME", self.sandbox.home())
            .env_remove("CARGO_TARGET_DIR")
            .arg("generate-lockfile");
        isolate_from_the_enclosing_repository(&mut command);
        require_success(command, "cargo generate-lockfile")?;
        Ok(())
    }

    /// Initializes this fixture's workspace root as its own git repository
    /// and commits every file present so far, giving `sync`'s
    /// dirty-worktree checks a clean baseline.
    pub(crate) fn init_git_repository(&self) -> Result<(), Box<dyn Error>> {
        git::init_wearing_repository(self.root(), self.sandbox.home())
    }
}

/// Escapes `text` for use inside a TOML basic string (`"…"`) by escaping
/// backslashes and quotes — the two characters a real filesystem path in a
/// temporary directory could plausibly contain that TOML's basic-string
/// grammar does not accept literally.
fn escape_toml_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A `path = "…"` dependency line naming `dependency_key`, pointing at the
/// absolute path of the checked-in test skeleton `skeleton_name`.
pub(crate) fn path_dependency_on_test_skeleton(
    dependency_key: &str,
    skeleton_name: &str,
) -> String {
    let skeleton_path = checked_in_test_skeleton(skeleton_name);
    let quoted_path = escape_toml_string(&skeleton_path.to_string_lossy());
    format!("{dependency_key} = {{ path = \"{quoted_path}\" }}\n")
}

/// A `path = "…"` dependency line naming `dependency_key`, pointing at
/// `absolute_path`.
pub(crate) fn path_dependency_on(dependency_key: &str, absolute_path: &Path) -> String {
    let quoted_path = escape_toml_string(&absolute_path.to_string_lossy());
    format!("{dependency_key} = {{ path = \"{quoted_path}\" }}\n")
}

/// The `[package.metadata.skeletons.<dependency_key>]` table that marks
/// `dependency_key` as worn, with `options_toml` written verbatim inside it
/// (empty string for a skeleton with no recorded option values).
pub(crate) fn wearing_table(dependency_key: &str, options_toml: &str) -> String {
    format!("[package.metadata.skeletons.{dependency_key}]\n{options_toml}")
}

/// Writes a package manifest and a placeholder `src/lib.rs` at
/// `relative_dir` (empty string for the workspace root itself) under
/// `root`, naming the crate `name`, with `extra` — dependencies and
/// wearing tables, mostly — appended verbatim after `[package]`.
///
/// A single-crate fixture is Cargo's own implicit workspace of one member:
/// nothing here needs an explicit `[workspace]` table unless a test
/// specifically wants more than one member, which [`write_workspace_root`]
/// below is for.
pub(crate) fn write_package_manifest(
    root: &Path,
    relative_dir: &str,
    name: &str,
    extra: &str,
) -> Result<(), Box<dyn Error>> {
    let manifest = format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         {extra}\n"
    );
    let prefix = if relative_dir.is_empty() {
        String::new()
    } else {
        format!("{relative_dir}/")
    };
    write(root, &format!("{prefix}Cargo.toml"), manifest.as_bytes())?;
    write(
        root,
        &format!("{prefix}src/lib.rs"),
        b"// nothing: this crate exists only to wear a skeleton.\n",
    )?;
    Ok(())
}

/// Writes a workspace root manifest at `root`, naming `members` (each a
/// relative directory holding its own `Cargo.toml`) as its members.
pub(crate) fn write_workspace_root(root: &Path, members: &[&str]) -> Result<(), Box<dyn Error>> {
    use std::fmt::Write as _;

    let mut members_toml = String::new();
    for member in members {
        writeln!(members_toml, "    \"{member}\",")?;
    }
    write(
        root,
        "Cargo.toml",
        format!("[workspace]\nmembers = [\n{members_toml}]\nresolver = \"2\"\n").as_bytes(),
    )
}

/// A command that runs the `cargo` that built this test binary, never
/// whichever `cargo` comes first on `PATH`.
///
/// Cargo sets `CARGO` to its own path for every crate it compiles, so
/// `env!` reads it at compile time and it is present however the test binary
/// is later run. The fixtures generate their lockfiles with it; a different
/// `cargo` on `PATH` (another toolchain, a shim, a stub) would answer for a
/// build that is not the one under test. The command line under test finds
/// cargo the same way, through the `CARGO` it inherits.
pub(crate) fn cargo_command() -> Command {
    Command::new(env!("CARGO"))
}

/// The command line the suite spawns: `skeletons-ritual`'s `ritual`, or, when
/// this module is compiled into `skeletons-wearer`'s tests, that package's
/// `wearer`. Cargo sets the variable for the package's own binaries only, so
/// exactly one of the two is ever present.
const COMMAND_LINE_UNDER_TEST: &str = match (
    option_env!("CARGO_BIN_EXE_ritual"),
    option_env!("CARGO_BIN_EXE_wearer"),
) {
    (Some(path), None) | (None, Some(path)) => path,
    (Some(_), Some(_)) | (None, None) => {
        panic!("a test binary is built for exactly one package's command line")
    }
};

/// Runs the command line under test with `arguments`, with its
/// working directory set to `workspace_root` — the way a project wearing
/// skeletons runs it — and `sandbox`'s isolated `CARGO_HOME`/`HOME`, and
/// collects what it reported.
///
/// [`COMMAND_LINE_UNDER_TEST`] is the path Cargo built for this test run, so
/// this exercises the real binary rather than re-entering the test
/// harness's own process.
pub(crate) fn run_ritual(
    workspace_root: &Path,
    sandbox: &Sandbox,
    arguments: &[&str],
) -> Result<Report, Box<dyn Error>> {
    run_ritual_with_env(workspace_root, sandbox, arguments, &[])
}

/// The same as [`run_ritual`], plus `extra_env` — additional environment
/// variables set on the child process beyond `CARGO_HOME`/`HOME`.
pub(crate) fn run_ritual_with_env(
    workspace_root: &Path,
    sandbox: &Sandbox,
    arguments: &[&str],
    extra_env: &[(&str, &str)],
) -> Result<Report, Box<dyn Error>> {
    let mut command = Command::new(COMMAND_LINE_UNDER_TEST);
    command
        .args(arguments)
        .current_dir(workspace_root)
        .env("CARGO_HOME", sandbox.cargo_home())
        .env("HOME", sandbox.home())
        // Never inherited from the outer `cargo test` invocation: a stray
        // `CARGO_TARGET_DIR` pointed at this repository's own `target/`
        // would let a fixture's own `cargo metadata` call write there
        // instead of under the fixture's disposable workspace.
        .env_remove("CARGO_TARGET_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_from_the_enclosing_repository(&mut command);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let child = command.spawn()?;
    let pid = child.id();
    let output = child.wait_with_output()?;

    let exit_code = output
        .status
        .code()
        .ok_or("the command line under test was killed by a signal instead of exiting")?;

    Ok(Report {
        exit_code,
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
        pid,
    })
}

/// Writes `content` at `relative`, under `root`, creating parent
/// directories as needed.
///
/// Used both to author a fixture workspace's own manifests and files, and
/// to write a claimed file by hand ahead of a `check` run — the test that a
/// hand-written file byte-identical to the render matches needs exactly
/// this: a file the test authors directly, never one `ritual` itself
/// produced first.
pub(crate) fn write(root: &Path, relative: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
    let full_path = root.join(relative);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(full_path, content)?;
    Ok(())
}

/// Reads the bytes at `relative`, under `root`.
///
/// Every "bytes unchanged" assertion in this suite reads through this
/// function on both sides of the operation under test, never trusting an
/// exit code alone to say what a refused `sync` did or did not write.
pub(crate) fn read(root: &Path, relative: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(fs::read(root.join(relative))?)
}

/// The environment variable `build-script-marker`'s build script reads for
/// the path of the marker file it writes when it runs. The tests that assert
/// the file never appears and the one that shows it does appear share this
/// name, so a rename cannot leave the first kind passing with nothing to
/// find.
pub(crate) const BUILD_SCRIPT_MARKER_VARIABLE: &str = "SKELETONS_TEST_ONLY_BUILD_SCRIPT_MARKER";

/// The absolute path of the checked-in test skeleton named `name`, under
/// `crates/skeletons/test-skeletons/renders/`.
///
/// Resolved from this crate's own manifest directory rather than the
/// process's current directory, since every test changes `ritual`'s
/// working directory to a fixture workspace before running it, but never
/// its own.
pub(crate) fn checked_in_test_skeleton(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("crates/skeletons/test-skeletons/renders")
        .join(name)
}

/// The absolute path of the checked-in test skeleton named `name`, under
/// `crates/skeletons/test-skeletons/refused/` — a skeleton deliberately carrying
/// exactly one defect, for exercising a skeleton's own render being refused.
pub(crate) fn checked_in_refused_skeleton(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("crates/skeletons/test-skeletons/refused")
        .join(name)
}

/// The absolute path of the checked-in, captured crates.io sparse-index
/// files under `crates/skeletons/src/behind/captures/` — the same captures
/// `skeletons`'s own `behind/index_entries.rs` unit tests read directly, and the
/// root `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` is pointed at here for the
/// registry-`behind` tests. See the `readme.md` beside those files for how
/// and when they were captured.
pub(crate) fn captured_crates_io_index() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/skeletons/src/behind/captures")
}

/// A `<dependency_key> = "<version>"` dependency line — the shape a
/// dependency sourced from crates.io (real or, here, the vendored directory
/// source standing in for it) takes when its key is the crate's own name.
pub(crate) fn registry_dependency(dependency_key: &str, version: &str) -> String {
    format!("{dependency_key} = \"{version}\"\n")
}

/// A `<dependency_key> = { version = "<version>", registry = "other" }`
/// dependency line — a skeleton taken from a registry other than the default
/// one.
pub(crate) fn other_registry_dependency(dependency_key: &str, version: &str) -> String {
    format!("{dependency_key} = {{ version = \"{version}\", registry = \"other\" }}\n")
}

/// Writes a `[source.crates-io] replace-with = "vendored"` directory-source
/// registry into `cargo_home`'s own `config.toml`, vendoring exactly one
/// skeleton crate at `<cargo_home>/vendor/<name>/`, so a plain
/// `<name> = "<version>"` dependency resolves entirely offline yet reports
/// its source as `registry+https://github.com/rust-lang/crates.io-index` —
/// exactly what `cargo metadata` reports for a real crates.io dependency,
/// and what `skeletons` itself treats as crates.io — load-bearing for every
/// registry-`behind` scenario in this suite.
pub(crate) fn write_vendored_crates_io_skeleton(
    cargo_home: &Path,
    name: &str,
    version: &str,
    file_relative: &str,
    file_contents: &str,
) -> Result<(), Box<dyn Error>> {
    let vendor_directory = cargo_home.join("vendor");
    write_vendored_skeleton_crate(
        &vendor_directory,
        name,
        version,
        file_relative,
        file_contents,
    )?;
    append(
        cargo_home,
        "config.toml",
        &format!(
            "[source.crates-io]\n\
             replace-with = \"vendored\"\n\
             \n\
             [source.vendored]\n\
             directory = \"{}\"\n",
            escape_toml_string(&vendor_directory.to_string_lossy()),
        ),
    )
}

/// The same as [`write_vendored_crates_io_skeleton`], but for a dependency
/// taken from a second registry, named `other`, reached at the sparse URL
/// `https://example.invalid/index/` — a source `skeletons` must recognise as "not
/// crates.io" and never query at all, which is what the test of a skeleton
/// from another registry checks.
pub(crate) fn write_vendored_other_registry_skeleton(
    cargo_home: &Path,
    name: &str,
    version: &str,
    file_relative: &str,
    file_contents: &str,
) -> Result<(), Box<dyn Error>> {
    let vendor_directory = cargo_home.join("vendor-other");
    write_vendored_skeleton_crate(
        &vendor_directory,
        name,
        version,
        file_relative,
        file_contents,
    )?;
    append(
        cargo_home,
        "config.toml",
        &format!(
            "[registries.other]\n\
             index = \"sparse+https://example.invalid/index/\"\n\
             \n\
             [source.other-src]\n\
             registry = \"sparse+https://example.invalid/index/\"\n\
             replace-with = \"vendored-other\"\n\
             \n\
             [source.vendored-other]\n\
             directory = \"{}\"\n",
            escape_toml_string(&vendor_directory.to_string_lossy()),
        ),
    )
}

/// A minimal skeleton crate at `<vendor_directory>/<name>/`, in cargo's
/// directory-source shape: a `Cargo.toml` naming `name` and `version` and
/// carrying `[package.metadata.skeletons]`, a placeholder `src/lib.rs`, one file
/// under `files/`, and the `.cargo-checksum.json` a directory source
/// requires to accept the entry (its `package` checksum is never verified
/// against anything here, since nothing ever downloads this crate — cargo
/// only requires the field to be present and hex).
fn write_vendored_skeleton_crate(
    vendor_directory: &Path,
    name: &str,
    version: &str,
    file_relative: &str,
    file_contents: &str,
) -> Result<(), Box<dyn Error>> {
    let crate_directory = vendor_directory.join(name);
    write(
        &crate_directory,
        "Cargo.toml",
        format!(
            "[package]\n\
             name = \"{name}\"\n\
             version = \"{version}\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [package.metadata.skeletons]\n"
        )
        .as_bytes(),
    )?;
    write(
        &crate_directory,
        "src/lib.rs",
        b"// nothing: a skeleton's own crate is never built by a render.\n",
    )?;
    write(
        &crate_directory,
        &format!("files/{file_relative}"),
        file_contents.as_bytes(),
    )?;
    write(
        &crate_directory,
        ".cargo-checksum.json",
        format!(r#"{{"files":{{}},"package":"{}"}}"#, "0".repeat(64)).as_bytes(),
    )?;
    Ok(())
}

/// Appends `content` to whatever already exists at `relative` under `root`
/// (or creates it, if nothing does yet) — used to build up `config.toml`
/// across more than one call when a fixture vendors skeletons from two
/// registries at once.
fn append(root: &Path, relative: &str, content: &str) -> Result<(), Box<dyn Error>> {
    let existing = fs::read_to_string(root.join(relative)).unwrap_or_default();
    write(root, relative, format!("{existing}{content}").as_bytes())
}

/// Reads the remote-query log `SKELETONS_TEST_ONLY_REMOTE_LOG` was pointed at,
/// or `""` if `skeletons` never created it at all.
///
/// The log is created lazily, only immediately before the first remote query
/// `skeletons` would make, so its total absence — not merely emptiness — is
/// itself the strongest form of "no query was made", and this reads it as
/// `""` either way so a caller can assert on its contents uniformly.
pub(crate) fn read_remote_log(path: &Path) -> Result<String, Box<dyn Error>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

/// Writes a minimal skeleton crate at `root`: a `Cargo.toml` naming
/// `package_name` (which doubles as `[package.metadata.skeletons]`'s marker,
/// carrying no options), a `src/lib.rs` Cargo requires to package a crate,
/// and one file under `files/` at `file_relative` holding `file_contents`.
///
/// Used for skeletons this suite builds itself rather than reading from
/// `crates/skeletons/test-skeletons/`, where a test needs something no
/// checked-in skeleton covers: history to move a git pin along, or a file
/// name of its own choosing.
pub(crate) fn write_minimal_skeleton(
    root: &Path,
    package_name: &str,
    file_relative: &str,
    file_contents: &str,
) -> Result<(), Box<dyn Error>> {
    write(
        root,
        "Cargo.toml",
        format!(
            "[package]\n\
             name = \"{package_name}\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [package.metadata.skeletons]\n"
        )
        .as_bytes(),
    )?;
    write(
        root,
        "src/lib.rs",
        b"// nothing: a skeleton's own crate is never built by a render.\n",
    )?;
    write(
        root,
        &format!("files/{file_relative}"),
        file_contents.as_bytes(),
    )?;
    Ok(())
}

/// The sorted names of every entry directly under `root`, excluding any
/// name in `excluded` — used to prove a command wrote nothing new to a
/// fixture workspace beyond the claimed files it was told about.
pub(crate) fn list_top_level_entries(
    root: &Path,
    excluded: &[&str],
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !excluded.contains(&name.as_str()) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Every regular file under `root`, recursively, as a map from its path
/// relative to `root` (`/`-separated, so the result is stable across
/// platforms) to its bytes — excluding any top-level entry named in
/// `excluded_top_level`, such as `target`, cargo's own incidental build output.
///
/// Used to prove a command changed only what it should: read once before and
/// once after a run, the two maps must differ at exactly the paths the command
/// is meant to write, and nowhere else under `root`. A `BTreeMap` (rather than
/// a `HashMap`) gives a deterministic iteration order, which nothing here
/// relies on directly but which makes a failing assertion's debug output
/// reproducible from one run to the next.
pub(crate) fn snapshot_workspace_files(
    root: &Path,
    excluded_top_level: &[&str],
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut snapshot = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if excluded_top_level.contains(&name.as_str()) {
            continue;
        }
        collect_files_recursively(&entry.path(), root, &mut snapshot)?;
    }
    Ok(snapshot)
}

/// [`snapshot_workspace_files`]'s own walk: starting from `path` (a file or a
/// directory), records every regular file found under it into `snapshot`, keyed
/// by its path relative to `root` — iteratively, with an explicit stack rather
/// than recursion, the same shape `sync/write.rs`'s own
/// `no_skeletons_sync_staging_files_remain` test helper uses.
fn collect_files_recursively(
    path: &Path,
    root: &Path,
    snapshot: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Box<dyn Error>> {
    let mut pending = vec![path.to_path_buf()];
    while let Some(current) = pending.pop() {
        let file_type = fs::symlink_metadata(&current)?.file_type();
        if file_type.is_dir() {
            for entry in fs::read_dir(&current)? {
                pending.push(entry?.path());
            }
        } else if file_type.is_file() {
            let relative = current
                .strip_prefix(root)?
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            snapshot.insert(relative, fs::read(&current)?);
        }
        // A symlink is neither a directory nor a regular file here
        // (`symlink_metadata` never follows it): it is deliberately left out
        // of the snapshot, since a workspace this suite builds never claims
        // a symlink as a skeleton's own file.
    }
    Ok(())
}

/// Creates a symbolic link at `link`, pointing at `original`.
///
/// Used to show that `sync` never writes outside the workspace root: a
/// claimed path is made to pass through a symlinked directory pointing
/// outside the fixture workspace.
pub(crate) fn symlink(original: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    std::os::unix::fs::symlink(original, link)?;
    Ok(())
}

/// Runs `command`, then errors, embedding stdout and stderr, unless it
/// exited successfully.
///
/// Fixture setup (`git init`, `cargo generate-lockfile`, and so on) has no
/// contract of its own to assert on — a failure there is this suite's own
/// fixture being wrong, so it is surfaced as a `TestOutcome` error with the
/// process's own output attached, the same as `run_ritual`'s caller does
/// for the binary under test.
pub(crate) fn require_success(mut command: Command, what: &str) -> Result<Output, Box<dyn Error>> {
    let output = command.output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "{what} failed ({status}); stdout: {stdout}; stderr: {stderr}",
            status = output.status,
            stdout = String::from_utf8_lossy(&output.stdout),
            stderr = String::from_utf8_lossy(&output.stderr),
        )
        .into())
    }
}
