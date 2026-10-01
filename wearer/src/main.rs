//! A command line that mounts the `skeletons` bundle under the key `tools`, the
//! way a project that names its dependency `tools` does.
//!
//! It exists for the acceptance tests under `tests/`: everything a wearer sees
//! when the bundle is reached as `wearer tools check` rather than as
//! `ritual skeletons check` is observable only through a command line that
//! really mounts it that way. It is hand-written, not generated, because the
//! generated `main.rs` mounts every task under the key its dependency carries
//! and this repository's own dependency is named `skeletons`.

fn main() -> std::process::ExitCode {
    rituals::run(rituals::identity!(), [("tools", skeletons::task())])
}
