# skeletons-library

The supported way for a program to ask what [skeletons](https://github.com/hexlace/skeletons#readme)
would say about a workspace. One function, `check`, reads a workspace and
returns, as data, which files its worn skeletons claim and whether each one
matches. It writes nothing, makes no network request, and says nothing in
words: a caller decides what a drifted file or a refusal means to its own
users.

A caller that wants the commands themselves, `check`, `sync` and `wear` with
their tables and messages, mounts [`skeletons`](https://crates.io/crates/skeletons)
in a ritual command line instead. This crate is for a program that wants the
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

## What it needs

`check` needs `cargo` on `PATH` (or named by `$CARGO`), a `Cargo.lock` that is
up to date, and every locked source already fetched. A source that was never
fetched is an error, because `check` never downloads one. The library depends
on no network client and no command-line framework.

## Support

This crate and `skeletons` are the two a project depends on. `skeletons-engine`
is published only because they are built on it, makes no stability promise, and
is not for a project to depend on.
