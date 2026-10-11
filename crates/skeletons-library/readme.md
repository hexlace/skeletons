# skeletons-library

The supported way for a program to ask what [skeletons](https://github.com/hexlace/skeletons#readme)
would say about a workspace. One function, `check`, reads a workspace and
returns, as data, which files its worn skeletons claim and whether each one
matches. It writes nothing in the workspace, makes no network request, and
says nothing in words: a caller decides what a drifted file or a refusal means
to its own users.

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

`check` needs `cargo`, a `Cargo.lock` that is up to date, and every locked
source already fetched. It runs the program `$CARGO` names whenever that
variable is set, exactly as given and an empty value included, and runs `cargo`
from `PATH` only when it is not set; a value that is not valid Unicode counts
as not set. A source that was never fetched is an error, because `check` never
downloads one.

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

## Support

This crate and `skeletons` are the two a project depends on. `skeletons-engine`
is published only because they are built on it, makes no stability promise, and
is not for a project to depend on.
