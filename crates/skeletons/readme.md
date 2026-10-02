# skeletons

**Keep your repos from drifting apart.** `skeletons` reads what a repository
is meant to look like from dependencies the repository declares, reports
where the two have come apart, and puts them back. The thing a repository is
meant to look like is a *skeleton*, an ordinary crate, and a repository wears
one by depending on it.

This crate is `skeletons` as a [ritual](https://github.com/hexlace/ritual)
bundle. It has no binary of its own: a project adds it to its ritual command
line like any other bundle, and its commands answer as
`cargo ritual skeletons …`, where the command follows the key the project
mounted it under: a project that depends on it as
`tools = { package = "skeletons", … }` runs `cargo ritual tools check`
instead. The
[`skeletons` readme](https://github.com/hexlace/skeletons#readme) covers what
it is for, and how a repository wears its first skeleton.

## Mounting the bundle

A project mounts the bundle as a dependency of its CLI crate, and as an
entry in that crate's list of tasks:

```toml
[dependencies]
skeletons = "0.1"

[package.metadata.ritual]
tasks = ["ritual", "skeletons"]
```

then runs `cargo ritual regenerate`. A project with no ritual command line
yet starts with ritual's
[Install](https://github.com/hexlace/ritual#install) and
[Quickstart](https://github.com/hexlace/ritual#quickstart).

## Check, sync and wear

`check` compares a repository's own files against the skeletons it wears. It
reads the wearing tables and the lockfile fresh on every run, and reports, for
every bone (one file a worn skeleton renders), whether the file matches the
skeleton's render, and for every worn skeleton, whether it is current,
behind, pinned, or that could not be determined. It writes nothing to the
repository, and it keeps nothing between runs.

`sync` writes every drifted bone's file back to match. With anything to
write, it refuses outright unless the work tree is clean and git holds
everything it would replace, so that git is the undo. If a rename still
fails partway, the files already written stay and the rest do not, each
named.

`wear` is how a repository starts wearing a skeleton. `wear <crate>[@<version>]
[<key>]` adds the crate as a dev-dependency of the command line's own crate,
through `cargo add`, from crates.io, a git repository or a directory, and adds
an empty `[package.metadata.skeletons.<key>]` table beside it, which is all
`sync` needs to start writing the skeleton's files. It changes a manifest and
`Cargo.lock`, so it refuses unless the work tree is clean, as `sync` does, and
a run that fails after `cargo add` has changed them puts both back exactly as
they were, and says so.

None of the three commands runs a git command that writes the repository's
own index, refs, config or object database. A content filter the repository
configures can, and `sync` runs one while it reads a file to prove git holds
it: git-lfs's clean filter can add an object under `.git/lfs/objects/`, and
never changes one already there. The one repository `check` writes to is a
temporary one of its own, created and removed within the run, when the head
of a `branch` pin has moved, or of a `git` dependency that names none of
`tag`, `branch` or `rev`.

## Going further

The references live beside the code, and every claim above is argued in them:

- [Wearing a skeleton](https://github.com/hexlace/skeletons/blob/main/.docs/wearing.md)
  covers the wearing table, `wear`, where bones land, `check` and `sync`'s
  output, and every way each can refuse.
- [The skeleton format](https://github.com/hexlace/skeletons/blob/main/.docs/skeleton-format.md)
  covers writing a skeleton.
- [The design](https://github.com/hexlace/skeletons/blob/main/.docs/design.md)
  covers why `skeletons` is shaped this way, and its known limits.
