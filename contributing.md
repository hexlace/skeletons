# Contributing

Contributions are welcome. The process for them is being worked out as of
September 30, 2026, and issues and pull requests are expected to open soon.

This guide covers working on `skeletons` itself: the toolchain, the gate,
releasing, and the command line's generated file.

## Toolchain

The toolchain is pinned in `rust-toolchain.toml`, so plain `rustup` picks it
up. With [mise](https://mise.jdx.dev/), trust the config once per checkout,
then install; `mise.toml` also pins
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny) at the version CI
runs:

```sh
mise trust
mise install
```

The pinned toolchain is what the gate runs on. The minimum supported Rust
version for users is separate: it is the `rust-version` in the workspace
manifest, and [the readme](readme.md#versions) states the policy for it.

Unix is the target: Linux and macOS. Code and tests use `std::os::unix` directly, with no
platform gate.

## The gate

All of these must pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p skeletons --all-targets -- -D warnings
cargo test -p skeletons
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
cargo deny --locked check
```

The two `-p skeletons` commands build the bundle alone. A workspace build
compiles `skeletons` with its `test-util` feature, which `ritual`'s
dev-dependency turns on for every package in that build, so code or a test
seam that depends on the feature being on or off is only seen by the run
that has it the other way.

CI runs the same, with clippy and the tests on both Linux and macOS, and
three more: every crate is packaged and built from its package alone, as
crates.io would receive it; the bundle's tests then run from that unpacked
package (see [Test skeletons](#test-skeletons)); and the test suite runs on
the oldest toolchain its `rust-version` declares.

## Dependencies

`deny.toml` sets what the dependency tree may hold, and CI runs
`cargo deny check` on every pull request, and the advisories check weekly
as well. A new dependency that brings a licence not on the allowlist, or
comes from anywhere but crates.io, fails the check until `deny.toml` lists
it with the crate that needs it. `cargo deny --locked check` in the gate
above runs the same check locally.

## Releasing

`skeletons` follows [Semantic Versioning](https://semver.org/), and it is the
only crate here that is published. Every member inherits
`version.workspace = true`, the `skeletons` requirement in the root
`[workspace.dependencies]` is that same version, and a test in `xtask`
fails if either stops being true. `skeletons-ritual`, `skeletons-wearer` and
`xtask` inherit the version too, and are never published. `rituals` and `rituals-core` come from
crates.io at a version of their own, which a release never touches.

A release takes three steps, and GitHub Actions does the work between them:

1. Run the **Release** workflow from the Actions tab, on `main`, with the tag:
   `v` then `MAJOR.MINOR.PATCH`, such as `v0.1.0`. It refuses a tag that is
   not newer than `main`'s version. It moves every version site and
   `Cargo.lock`, commits that to the branch `release/<tag>` as
   `chore(release): <tag>`, and opens a pull request. A workflow opened the
   pull request, so its CI waits for approval: approve the run, or close and
   reopen the pull request.
2. Merge the pull request. **Release draft** then drafts the GitHub release
   `<tag>` at the merge commit. Its body has a marked place for prose at the
   top, then every pull request merged since the previous release, then
   everyone who authored or co-authored a commit.
3. Write the prose and publish the release. Publishing creates the tag, and
   **Publish** publishes `skeletons` to crates.io through trusted publishing.
   If it fails, run it again: a version crates.io already has is skipped.

Each of these workflows runs as two jobs, split on one rule: **a token that
can write, to this repository or to crates.io, only ever exists in a job that
compiles nothing.** `cargo xtask` is `cargo run`, so it compiles xtask and runs
its dependencies' build scripts, and every step in a job runs as the same user
on the same machine, so code one step runs can read what a later step is
given. Moving a token to a later step does not keep it away from an earlier
one.

- **Release**: `bump` runs `cargo xtask bump` with read access only, and hands
  `Cargo.toml` and `Cargo.lock` on. `open-pull-request` checks the tag and
  that those two files are all that changed, then commits them and opens the
  pull request.
- **Release draft**: `verify` runs `cargo xtask verify-tag` and
  `cargo xtask release-contributors` with read access only, and hands the
  previous release and the Contributors section on. `draft` asks GitHub for
  its generated notes, which needs write access, assembles the body and
  creates the draft.
- **Publish**: `verify` holds no credential: it checks that the tagged commit
  is on `main`, then runs `cargo xtask publish-plan`, which does every build a
  publish verifies. Then `publish` checks out that same commit, gets a token,
  and runs `cargo publish --no-verify`, which packages and uploads without
  compiling anything.

Each writing job checks what it was handed in shell, because the job that
produced it had run third-party code by then. Keep it that way: a step that
compiles, including `cargo xtask`, never goes in a job that can write.

What changed in a release lives in its GitHub release, not in a file here.

The workflows run `cargo xtask`, and so can you:

```sh
cargo xtask bump v0.1.0            # what step 1 does to the tree
cargo xtask verify-tag v0.1.0      # refuses unless the workspace is at v0.1.0
cargo xtask publish-plan v0.1.0    # dry-runs the publish, prints the plan
cargo xtask release-contributors hexlace/skeletons v0.1.0 <commit> <dir>   # needs `gh`
```

`bump` edits `Cargo.toml` and `Cargo.lock` in place; put them back with
`git checkout -- Cargo.toml Cargo.lock` after trying it. `publish-plan` asks
crates.io whether it already has `skeletons` at the tag's version, then
packages and builds everything it would upload without uploading anything,
and it has to run on a committed tree. It prints the plan on stdout: a
`publish=` line and an `exclude=` line, the form the workflow hands from one
job to the next. The dry run checks the package, not the registry: a version
crates.io already has, such as a yanked one, shows up only as a warning,
where a real publish refuses it. None of these commands uploads anything:
only the workflow publishes.

## This repository's command line

Inside this checkout, `cargo ritual <task>` runs this repository's own ritual
command line without installing anything. It is
`cargo run --package skeletons-ritual --`, aliased in `.cargo/config.toml`.
It mounts ritual's management tasks at the top level and the `skeletons`
bundle under `skeletons`, so `cargo ritual skeletons check` runs `skeletons`
the way a project that wears it does. It is `publish = false`: it exists to
work on `skeletons`, and is not part of what `skeletons` ships.

`ritual/src/main.rs` is that command line's generated file. After changing
`[package.metadata.ritual] tasks` in `ritual/Cargo.toml`, run
`cargo ritual regenerate`. Never edit the file by hand.

A second command line, `wearer/` (package `skeletons-wearer`, binary
`wearer`), mounts the bundle under the key `tools`. It exists for the
acceptance tests under `wearer/tests/`, which check that a project using
another dependency key is never told to run a command that does not exist.
Its `src/main.rs` is written by hand rather than generated, and it is a
package of its own because a ritual command line has exactly one binary. It
is `publish = false` too.

## Test skeletons

The render's own tests read from real skeletons under
`crates/skeletons/test-skeletons/` — each one a complete crate, written the way a
skeleton author actually writes one, rather than data built directly in test code
standing in for a skeleton.

- **`test-skeletons/renders/<name>/`** holds a skeleton that renders successfully,
  each exercising one concern (fill, select, indentation, determinism, text
  fidelity, and so on).
- **`test-skeletons/refused/<name>/`** holds a skeleton carrying exactly one defect,
  since a render checks a skeleton whole and reports its first refusal — a skeleton
  fixture with two defects would only ever prove one of them is caught. A
  refusal that does not need a complete skeleton's shape to provoke (an
  unreadable manifest, a missing `package.name`, a bound reached by a
  filesystem entity git cannot store) is exercised by a unit test instead,
  directly against the function that decides it. The order the refusals come
  in is pinned the same way: a test in `crates/skeletons/src/skeleton/`
  builds one skeleton in a scratch directory with a defect at each stage and
  removes them one at a time, so it needs no fixture here and also runs from
  the published package.
- Each test skeleton's directory name is its own package name.
- **`test-skeletons/.gitattributes`** holds `* -text`, so git treats every
  test skeleton file as bytes and no git setting on a contributor's machine
  rewrites one on checkout or commit. Several are fixtures for exact bytes
  (control bytes a render must keep, a missing final newline), and a rewrite
  would silently break what they test.

`crates/skeletons/Cargo.toml` excludes the whole `test-skeletons/` directory
from the published package (the `"/test-skeletons"` entry of its `exclude`
list), along with the captured crates.io responses and `cargo` output that
other tests read. So the tests that read those files, or the documents under
`.docs/`, are compiled only in a checkout: `crates/skeletons/build.rs` sets
the `skeletons_checkout` cfg when `test-skeletons/` sits beside the manifest,
and an unpacked `.crate` never has it. A test that reads a file the package
leaves out goes behind `#[cfg(skeletons_checkout)]`; without that it fails to
compile, or fails at run time on a path that is not there, when the bundle's
tests run from the package. CI does run them there, in its `package` job's
`test the published package` step, and reruns from the package are how a
missing gate is found. `build.rs` notices `test-skeletons/` being removed but
not coming back, so a checkout that lost the directory and has it again needs
`cargo clean -p skeletons` before its tests see the cfg.

The repository's root `.gitignore` still applies inside a test skeleton's own
`files/` or `partials/` — a file created there that matches one of its
patterns (`.DS_Store`, `*~`, and so on) will not be committed, and a local
run that happens to have created one will see it and refuse it as not valid
UTF-8. Do not name a fixture file to match one of those patterns.

## Test-only seams

`behind` never reaches the real network in a test build: crates.io's sparse
index is read through two seams that do not exist in a production build —

- `SKELETONS_TEST_ONLY_CRATES_IO_INDEX`, pointed at a directory of
  captured sparse-index files (see
  `crates/skeletons/src/behind/captures/readme.md`). Unset in a test
  build, every crates.io query answers unreachable rather than falling
  back to a real request.
- `SKELETONS_TEST_ONLY_REMOTE_LOG`, appended one line before each remote
  query `behind` would make (`crates-io <name>`, `git <url> tags`,
  `git <url> branch <name>`, `git <url> default-branch`, or
  `git <url> snapshot <head>` for the fetch a branch pin whose head has
  moved makes), so a test can prove a query was — or, for `sync` and a
  skeleton from another registry, was never — attempted.

A third seam shortens how long a local git question waits:

- `SKELETONS_TEST_ONLY_LOCAL_GIT_TIMEOUT_SECONDS`, a whole number of seconds
  above zero, replaces the 30 seconds every local git command is otherwise
  given (`crates/skeletons/src/git/local_timeout.rs`), so a test of what a
  timeout says need not wait it out. Anything else in the variable, zero
  included, stops the test with a panic naming it, rather than reading as
  unset. A production build does not read the variable at all.
  `ritual/tests/support/slow_filter.rs` builds the repository whose content
  filter stalls, for the tests that use the variable.

`test-util` is `skeletons`' public cargo feature that compiles those seams in,
for tests only: a command line must never enable it outside
`[dev-dependencies]`. The seams are gated on `#[cfg(any(test, feature =
"test-util"))]`, so `skeletons`' own unit-test build has them without the
feature. It is never a default feature and a production command line never
enables it. `ritual/Cargo.toml` and `wearer/Cargo.toml` are what turn it
on for the acceptance suites: each re-declares `skeletons` as a
dev-dependency with `features = ["test-util"]`, and Cargo unifies that into
the `ritual` and `wearer` binaries `cargo test` builds (the ones
`CARGO_BIN_EXE_ritual` and `CARGO_BIN_EXE_wearer` point `tests/` at) — but
never into `cargo build`/`cargo run` of those binaries, since neither
activates a dev-dependency.

A fourth variable, `SKELETONS_TEST_ONLY_BUILD_SCRIPT_MARKER`, is unrelated to
`test-util`: it is read by the `build-script-marker` test skeleton's own
`build.rs` (`crates/skeletons/test-skeletons/renders/build-script-marker/`),
never by `skeletons` itself. Pointed at a path, that build script writes a marker
file there if it ever runs at all — proof, observable from the test process, that
`check`/`sync` never ran a worn skeleton's own build script. A test that
builds the skeleton on purpose
(`the_build_script_marker_skeleton_writes_its_marker_when_it_is_built` in
`ritual/tests/check_drift.rs`) shows the file does appear when the script runs,
so its absence elsewhere means something.

Every fixture that reaches git — a wearing repository's own history, or a
skeleton's upstream repository for a `tag =`/`branch =`/`rev =`/unqualified
`git =` dependency — isolates the machine's own git configuration
explicitly: each spawned `git` gets its own fresh `HOME`, so no ambient
`~/.gitconfig` is ever read, plus
`-c commit.gpgsign=false -c tag.gpgSign=false` and a fixed
`user.name`/`user.email` passed on the command line. A contributor's own
git may sign every commit and tag, prompt for a key, or name its author
differently, and none of that belongs in disposable fixture history built
in a temporary directory and never pushed anywhere; the fixtures build the
same history whatever git configuration the machine running them has. The
suite also removes `GIT_CONFIG_GLOBAL` and
`GIT_CONFIG_SYSTEM` and sets `GIT_CONFIG_NOSYSTEM=1` for every git it
spawns, and removes every one of `skeletons`'s own repository-redirecting
variables (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, and the rest
`REDIRECTING_VARIABLES` in `crates/skeletons/src/git/command.rs` names)
from the processes it runs `skeletons` under — without that, running the
suite from inside a git hook, which exports `GIT_DIR` into every child
process, would trip the very refusal this crate's own `sync` adds against
it. The fixtures also generate their lockfiles with the `cargo` that built the
tests, which Cargo names in `CARGO`, never whichever `cargo` is first on
`PATH`, so a different toolchain or a shim there cannot answer for the build
under test. See `ritual/tests/support/mod.rs` and
`ritual/tests/support/git.rs` for where this isolation is built.

## Tests that run the command line

A test that spawns the command line proves what its name says. It asserts the
specific outcome the name claims (the reason for a refusal, not only that
something was refused) rather than leaning on a unit test elsewhere to pin the
detail.

## Platform-dependent tests

A test whose behaviour depends on the filesystem or on git's own platform
behaviour — whether a filesystem folds case, whether this process can
remove a file from a mode-`0o555` directory, whether git lists an NFD path
under its NFC spelling — checks that premise at run time rather than
assuming it from `target_os`. When the premise does not hold, the test
prints `skipped: <premise>` to `stderr` and returns, rather than either
passing vacuously or being compiled out:

```rust
#[expect(
    clippy::print_stderr,
    reason = "a test that cannot establish its premise says so rather than passing silently"
)]
```

See `crates/skeletons/src/claim/location.rs` →
`a_directory_that_cannot_be_read_is_unreadable` for the pattern this
follows. `#[cfg(target_os = "…")]` is never used to choose between two
*expected outcomes* — that would make macOS and Linux compile and check
different things while claiming to run the same test suite.

CI runs the test suite on both Linux and macOS, and each `test` job lists
the tests that skipped on it, with their premises, in its job summary. To
see the same list locally, run `cargo test --workspace -- --show-output`:
the test harness otherwise discards a passing test's output, `skipped:`
lines included.
