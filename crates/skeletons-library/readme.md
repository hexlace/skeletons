# skeletons-library

The supported way for a program to ask what [skeletons](https://github.com/hexlace/skeletons#readme)
would say about a workspace. Two functions read a workspace and return, as
data, which files its worn skeletons claim and whether each one matches.
`check` asks cargo. `check_without_cargo` reads the manifests and `Cargo.lock`
as files, for a program that builds without Cargo. Where
`check_without_cargo` gives a report, it is the report `check` gives on the same
files, or, in the one stale shape "Without Cargo" below names, the report
`check` gives once Cargo brings the lock up to date. Neither writes anything in
the workspace or makes a network request, and neither says anything in words: a
caller decides what a drifted file or a refusal means to its own users.

A caller that wants the commands themselves, `check`, `sync`, `wear` and
`remove` with their tables and messages, mounts
[`skeletons`](https://crates.io/crates/skeletons) in a ritual command line
instead. This crate is for a program that wants the
verdict for a file as a value.

## Using it

```sh
cargo add skeletons-library
```

Find a skeleton by its package name, never by the key the wearing project
declares it under. A skeleton's files are read by matching `Files`, and a
package is looked up by matching `SkeletonsNamed`, so each answer that could be
mistaken for "nothing to report" has to be written out:

```rust,no_run
use skeletons_library::{Files, SkeletonsNamed};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = skeletons_library::check(".")?;
    let SkeletonsNamed::Worn(skeletons) = report.skeletons_named("house-rules") else {
        return Err("house-rules is not worn, or could not be identified".into());
    };
    for skeleton in skeletons {
        let Files::Checked(files) = skeleton.files() else {
            return Err(format!("{} was refused", skeleton.dependency_key()).into());
        };
        for file in files.iter().filter(|file| file.state().is_drifted()) {
            println!("{} is {:?}", file.path().display(), file.state());
        }
    }
    Ok(())
}
```

A caller that wants to say why a skeleton was refused, or that the answer for a
package could not be reached, reads those variants as well:

```rust,no_run
use skeletons_library::{Files, SkeletonsNamed};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = skeletons_library::check(".")?;
    match report.skeletons_named("house-rules") {
        SkeletonsNamed::Worn(skeletons) => {
            for skeleton in skeletons {
                match skeleton.files() {
                    Files::Checked(files) => {
                        let drifted = files.iter().filter(|file| file.state().is_drifted());
                        println!("{}: {} drifted", skeleton.dependency_key(), drifted.count());
                    }
                    Files::Refused { refusals, .. } => {
                        for refusal in refusals {
                            println!("{}: {:?}", skeleton.dependency_key(), refusal.kind());
                        }
                    }
                    // A state a later version adds: not checked, never "nothing drifted".
                    _ => println!("{}: could not be checked", skeleton.dependency_key()),
                }
            }
        }
        SkeletonsNamed::NotWorn => println!("house-rules is not worn"),
        SkeletonsNamed::Undetermined(refusals) => {
            for wearing in refusals.iter().flat_map(|refusal| refusal.wearings()) {
                println!("{} may wear it", wearing.manifest().display());
            }
        }
        // An answer a later version adds: not known, never "not worn".
        _ => println!("whether house-rules is worn could not be determined"),
    }
    Ok(())
}
```

The bytes a skeleton renders for a file are in the report whatever the file's
state, a `Missing` one included, so a caller can write them to the file's
`path()` under the report's `root()`, and a second `check` then answers
`Matches`. The library writes nothing, so whether to write, and over a file
that someone changed, is the caller's to decide; this example brings in only
the files that are missing:

```rust,no_run
use skeletons_library::{FileState, Files};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = skeletons_library::check(".")?;
    for skeleton in report.skeletons() {
        // A refused skeleton is left as it is: its files were not all compared.
        let Files::Checked(files) = skeleton.files() else {
            continue;
        };
        for file in files.iter().filter(|file| file.state() == FileState::Missing) {
            let path = report.root().join(file.path());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, file.rendered())?;
        }
    }
    Ok(())
}
```

## What it needs

The two functions need different things. Pick by what the program has.

### With Cargo

`check` needs `cargo`, a `Cargo.lock` that is up to date, and every locked
source already fetched. It runs the program `$CARGO` names whenever that
variable is set, exactly as given and an empty value included, and runs `cargo`
from `PATH` only when it is not set; a value that is not valid Unicode counts
as not set. A source that was never fetched is an error, because `check` never
downloads one.

Building the workspace is not enough to have every locked source. `cargo build`
downloads only what it needs to build on the machine it runs on, so a package
locked for another platform is not there, and neither is a dev-dependency, which
is how `wear` adds a skeleton. A program that calls `check` on a machine that
was only just set up, a CI runner for one, gets an error of which
`is_cargo_metadata_failed` is true, and `detail()` holds cargo's words: the
package it failed to download, and that `--offline` was specified. Run
`cargo fetch --locked` before calling `check`. It downloads every locked source
and leaves the lockfile as it is.

`check` writes nothing in the workspace. Cargo may still write to its own cache
under `CARGO_HOME`, as any cargo command may: it unpacks a source that was
downloaded but is not unpacked, which cargo's automatic cache cleaning can
leave behind, and in an empty `CARGO_HOME` it makes the files it keeps there,
its lock file among them. Where `CARGO_HOME` cannot be written, every locked
source must already be unpacked, as `cargo fetch` leaves it; otherwise cargo
fails, and `check` reports it as `is_cargo_metadata_failed`.

It needs nothing else. The workspace need not have been built, and its
directory need not be a git repository. When every dependency a workspace has is
a `path` dependency, `cargo generate-lockfile --offline` is enough to make the
lockfile. The library depends on no network client and no command-line
framework.

### Without Cargo

`check_without_cargo` runs no cargo, so it needs no cargo, no git and no `PATH`,
and a cargo home need not exist. It reads `Cargo.toml` files, `Cargo.lock` and
cargo's configuration files as files, and
returns the report `check` returns for the same workspace and files, or, in the
one stale shape described at the end of this section, the report `check` returns
once Cargo brings the lock up to date. It needs:

- A `Cargo.lock` that is up to date. The function never writes one, so make it
  where cargo is. A lockfile that is missing, or that the manifests have
  outgrown, gets the error `check` gives, of which `is_lockfile_not_current` is
  true.
- For each locked package that has a `source`, from a registry or a git
  repository, the directory holding that package's own `Cargo.toml`. A package
  like that is not a path on disk, so the function cannot find it, and the
  caller says where it is. Pass it in a `PackageLocations`, keyed by a
  `LockedPackage`: the `name`, `version` and `source` of that package's
  `[[package]]` entry in `Cargo.lock`, exactly as written there. A package with
  no `source` is a path package and needs nothing, since it is found from the
  manifests. A location for a package no wearing resolves to is never read.

A wearing whose package was given no location is not an error. It is a refusal
in the report, of kind `Unlocated`, and `unlocated_package()` gives the identity
to supply. A location whose `Cargo.toml` names another package, or another
version, locates nothing and is refused the same way. Those two are all a
location is checked against: a directory does not say which source it came
from, so one holding the right package at the right version from elsewhere, a
git checkout of another commit with the version unchanged among them, is read
as given. The directory has to hold the locked source itself: for a git
package, the commit after `#` in its `source`. Every other wearing is still
reported. A program that can find a package once asked can insert it and
call again.

Here the program has its own reading of `Cargo.lock`, and keeps every locked
package it fetched in a vendor directory; each entry with a `source` becomes one
location:

```rust,no_run
use std::path::Path;

use skeletons_library::{LockedPackage, PackageLocations};

/// One `[[package]]` entry of `Cargo.lock`, as the program's own reader gives it.
struct LockEntry {
    name: String,
    version: String,
    source: Option<String>,
}

fn locations(entries: Vec<LockEntry>, vendor: &Path) -> PackageLocations {
    entries
        .into_iter()
        .filter_map(|entry| {
            let source = entry.source?;
            let directory = vendor.join(format!("{}-{}", entry.name, entry.version));
            let package = LockedPackage {
                name: entry.name,
                version: entry.version,
                source,
            };
            Some((package, directory))
        })
        .collect()
}

fn check_vendored(entries: Vec<LockEntry>, vendor: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = skeletons_library::check_without_cargo(".", &locations(entries, vendor))?;
    for refusal in report.refusals() {
        if let Some(package) = refusal.unlocated_package() {
            println!("no files for {} {} from {}", package.name, package.version, package.source);
        }
    }
    Ok(())
}
```

A workspace whose skeletons are all path dependencies needs no locations: pass
`&PackageLocations::new()`.

What it does not read, because only cargo does, it refuses by name rather than
guess: `[patch]` and `[replace]` tables, a `package.workspace` key, one package
declared under two dependency keys, two path packages with one name and
version, a wearing whose own dependency is declared with `registry =`, a
lockfile in a format other than 3 or 4, and in a cargo configuration file
`paths`, `[patch]` or `include`. The configuration files are
`.cargo/config` (or `.cargo/config.toml` when there is no `config`) in the
directory given and in each directory above it, then the cargo home's: the
`CARGO_HOME` that is set and not empty, taken from the directory given when
relative, and otherwise `.cargo` in a `HOME` that is set and not empty. The
error has `is_unsupported_without_cargo` true, and `detail()` names the
manifest or configuration file and what it uses. `check` reads such a workspace
where cargo is available. A manifest, lockfile or configuration file that
cannot be read, or that cargo would refuse too, has
`is_manifest_or_lockfile_unreadable` true instead.

A wearing's dependency declared with `registry = "name"` is refused because only
Cargo's configuration says which index that name stands for: the lock entry
could name another registry's package, and the report would read that package's
files and metadata. The remedy is `check`, where Cargo is. A dependency that no
wearing names is not refused, even when it is declared that way. One that moved
to another registry is a named limit: the lock is stale, `check_without_cargo`
does not abort on it, and it reads the dependency as the lock has it, which the
report does not depend on.

It checks `Cargo.lock` against the manifests as far as the files show, and gives
the lockfile error where cargo would. One stale shape it reports through, because
seeing it is cargo's own resolution: a feature change on a package outside the
workspace that enables or disables an optional dependency, with no relock.
`check` aborts there, asking for the relock, and `check_without_cargo` reads the
lock as it stands. The report is still right, because it reads the members,
their wearing tables and the package each wearing's dependency is locked to, and
such a change touches none of them. It is the report `check` gives once the lock
is current. A program that has to be sure the lock is current runs
`cargo metadata --locked` where cargo is, or calls `check` there.

## Support

This crate and `skeletons` are the two a project depends on. `skeletons-engine`
is published only because they are built on it, makes no stability promise, and
is not for a project to depend on.
