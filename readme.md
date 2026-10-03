# Skeletons

[![CI](https://github.com/hexlace/skeletons/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/hexlace/skeletons/actions/workflows/ci.yml)

**Keep your repos from drifting apart.**

A template is copied once. A skeleton is a dependency.

Every repository starts as a copy of something: a toolchain pin, a lint
table, a CI workflow, a licence. From the day it is copied, each copy
changes on its own schedule, and nothing notices until two repositories that
were meant to agree plainly do not. Setting a project up happens once.
Drift happens across every project, forever, and it compounds.

`skeletons` is for the forever part. It reads what a repository is meant
to look like from dependencies the repository declares, reports where the
two have come apart, and puts them back.

`skeletons` is a bundle of tasks for
[ritual](https://github.com/hexlace/ritual). It has no binary of its own: a
project adds it to its ritual command line like any other bundle, and its
commands answer as `cargo ritual skeletons …`.

The thing a repository is meant to look like is a **skeleton**, and a
skeleton is just a crate. A repository wears one by depending on it, so the
lockfile records exactly which version it took, and taking a newer one is a
dependency update like any other.

## What it does

Given a skeleton and the option values a repository chose, `skeletons`
renders the exact bytes each file the skeleton ships should hold, or refuses
and says what is wrong.

`check` compares every file a repository's skeletons claim against that
render, and reports whether each skeleton is behind a newer version. `sync`
writes a drifted file back to match, and only when git can hand back
whatever it replaces. Both work from the manifests and the lockfile as they
are on every run. Nothing is stored between runs, so there is nothing to go
stale. `wear` is how a repository starts: it adds a skeleton as a dependency
and the wearing table that says the repository means to wear it, which
leaves the files to `sync`.

## Mounting the bundle

`skeletons` runs inside a ritual command line. No ritual yet? Ritual's
[Install](https://github.com/hexlace/ritual#install) and
[Quickstart](https://github.com/hexlace/ritual#quickstart) give a project
one, and its section on
[importing a task from somewhere else](https://github.com/hexlace/ritual#import-a-task-from-somewhere-else)
covers mounting a bundle from a git repository or another registry.

With one in hand there is nothing else to install. A project mounts
`skeletons` the way it would mount any bundle: as a dependency of the CLI
crate, and as an entry in that crate's list of tasks. In `ritual/Cargo.toml`:

```toml
[dependencies]
skeletons = "0.1"

[package.metadata.ritual]
tasks = ["ritual", "skeletons"]
```

`tasks` already holds `ritual` in a project that `ritual new` made, so
`skeletons` goes beside it. The dependency key is the command name, which is
why everything below runs as `cargo ritual skeletons …`. A project that
depends on it as `tools = { package = "skeletons", … }` runs
`cargo ritual tools check` instead, and the tool's own messages name only
the `check`, `sync` and `wear` tasks, so they read the same either way. Then
regenerate the command line's `src/main.rs`, which is written from that list:

```sh
cargo ritual regenerate
```

Now ask it something:

```sh
cargo ritual skeletons check
```

```text
this workspace wears no skeletons; a manifest wears one with a [package.metadata.skeletons.<dependency>] table beside the dependency
```

The first `cargo ritual` after a change builds the command line, so Cargo
prints its progress before the output. Later runs reuse the build.

## Wearing a first skeleton

A repository wears a skeleton by depending on it and saying, beside the
dependency, that it means to wear it. `wear` writes both. The examples from
here on wear `a-dependabot-skeleton`, which is invented for this readme: a
dependabot configuration with a cadence, a list of ecosystems and an optional
assignee. Substitute the skeleton your team wears, or
[write one](#writing-a-skeleton).

`wear` rewrites a manifest and `Cargo.lock`, so it refuses to run in a git
work tree with uncommitted changes, and outside a git repository altogether.
Commit first, then run it from the project's root:

```sh
cargo ritual skeletons wear a-dependabot-skeleton dependabot --path ../skeletons/a-dependabot-skeleton
```

```text
added a-dependabot-skeleton 0.1.0 to ritual/Cargo.toml as the dev-dependency `dependabot`, with an empty [package.metadata.skeletons.dependabot] table
commit ritual/Cargo.toml and Cargo.lock, then run the `sync` task to write its files
```

The first argument is the skeleton's crate, with a version requirement after
an `@` when it should have one (`a-dependabot-skeleton@0.1`, in Cargo's own
grammar). The second is the key to wear it under, which is the crate's name
when left out. With no source flag the crate comes from crates.io. `--git`,
with at most one of `--branch`, `--tag` and `--rev`, takes it from a git
repository, and `--path` from a directory, relative to where the command
runs.

That is the whole of wearing, and the two pieces `wear` wrote are what a
person could have written by hand. In `ritual/Cargo.toml`:

```toml
[package.metadata.skeletons.dependabot]

[dev-dependencies]
dependabot = { path = "../../skeletons/a-dependabot-skeleton", package = "a-dependabot-skeleton" }
```

The dependency went into `ritual/Cargo.toml` because `wear` writes into the
crate the running command line is built from, and the root manifest of a
project made by `ritual new` is virtual, a `[workspace]` with no `[package]`,
and cannot hold dependencies. It is a dev-dependency so the skeleton stays out
of the command line's own build; by hand, any of a manifest's dependency
tables works. The wearing table is named for the dependency key, `dependabot`
and not `a-dependabot-skeleton`, which is easy to get wrong when typing both.

The path is relative to `ritual/`, which is why it is not the one typed above,
and leads to a `skeletons/` directory beside the project: outside the
workspace, so Cargo does not read the skeleton as one of its members. The
dependency's source is Cargo's business: a path, a git repository or a
registry all work, and moving a skeleton from one to another changes that one
line. It does decide what `check` can say about newer versions (see
[Behind](#behind)).

The table is written empty, which wears the skeleton with every option at its
default. Options are set by editing it:

```toml
[package.metadata.skeletons.dependabot]
cadence = "daily"
ecosystems = ["cargo", "github-actions"]
assignee = "octocat"
```

A few rules cover a first wearing:

- A dependency is worn only when its manifest also carries a
  `[package.metadata.skeletons.<dependency-key>]` table for it. The table
  can be empty. A dependency with no table is not worn.
- The table is named for the dependency key, meaning the rename when there is
  one: `dependabot` above, not `a-dependabot-skeleton`. It sits in the same
  manifest as the dependency, on any workspace member.
- A string sets an `enum` or a `text` option, and an array of strings sets a
  `set` option. Anything else is refused, naming the option.
- An option left out takes the skeleton's default. A `text` option with no
  default is optional, and while it is unset every line holding its
  placeholder is left out of the file.
- A table whose key names no dependency of its manifest is refused rather
  than read as wearing nothing, so a misspelt key cannot go quiet.

Every file a worn skeleton renders lands at a path relative to the workspace
root, whichever member wears it. [`.docs/wearing.md`](.docs/wearing.md) has
the rest: what `wear` refuses and how it puts the project back, what happens
when two skeletons claim one path, and every way a wearing can be refused.

## Check

```sh
cargo ritual skeletons check
```

```text
dependabot in ritual/Cargo.toml: a-dependabot-skeleton 0.1.0 from path ../skeletons/a-dependabot-skeleton, pinned
  drifted (missing)  .github/dependabot.yml

1 bone: 1 drifted. 1 worn skeleton: 1 pinned.
ritual: 1 of 1 bone has drifted; the `sync` task puts it back
```

There is one block per worn skeleton. Its header names the wearing (the
dependency and the manifest), the skeleton and the version the lockfile
holds, where it came from, and whether it is behind. Each line under the
header is one bone: whether its file matches, and its path. This one is
`drifted (missing)` because nothing has written `.github/dependabot.yml`
yet; a file that exists but differs reads `drifted (changed)`. Paths in the
header are relative to the workspace root, so the `../../skeletons/…` written
in `ritual/Cargo.toml` reads `../skeletons/…` here.

Two facts are reported for every bone, and neither hides the other: whether
its file matches the render, and where its skeleton stands: current, behind,
pinned or undetermined. A file can have drifted under a skeleton that is up
to date, and can match one that is behind. The comparison is byte for byte,
so a file that was only reformatted still reads as drifted.

`check` fails on any drift, any refusal, and a workspace it cannot read at
all, such as one whose `Cargo.lock` is missing or out of date, since
`check` reads the lockfile and never writes it. When it fails it exits
non-zero and says why in one line on stderr, the last line above. Being behind
does not fail it on its own:

| Flag | Meaning |
|---|---|
| `--drifted` | show only bones that have drifted |
| `--behind` | show only skeletons that are behind, or where that is undetermined |
| `--fail-behind` | also fail when a worn skeleton is behind, or where that is undetermined |
| `--json` | print the answer as one JSON document, for collecting across many repositories |

The first two only narrow what is printed. The exit status still counts
everything, so a repository cannot stop noticing its own drift by narrowing
its output. `check` writes nothing to the repository. The full reading of
its output, and the `--json` document, are in
[`.docs/wearing.md`](.docs/wearing.md#check).

## Sync

`sync` writes what the skeleton renders over every file that has drifted. It
refuses to write into a work tree with uncommitted changes, and outside a git
repository altogether. If the project is not a repository yet, `git init` makes
it one. Either way, commit first:

```sh
git add -A && git commit -m "wear a-dependabot-skeleton"
cargo ritual skeletons sync
```

```text
created .github/dependabot.yml (a-dependabot-skeleton 0.1.0)
every bone now matches (0 updated, 1 created)
```

The file it wrote is the skeleton's `files/.github/dependabot.yml` with the
wearing table's values filled in and one fragment inserted for each
ecosystem chosen:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "daily"
    assignees: ["octocat"]
  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "daily"
    assignees: ["octocat"]
```

Commit what `sync` wrote. Suppose someone then changes the cadence in that
file by hand: `check` reads `drifted (changed)`, and `sync` says why it will
not touch it yet:

```text
.github/dependabot.yml has uncommitted changes
ritual: the working tree has 1 uncommitted change, so sync wrote nothing: it writes only into a clean working tree, where git holds everything it could replace; commit, stash or move it, then run the `sync` task again
```

That refusal is the point. Git is the undo: `sync` writes only when the whole
work tree is clean and every file it would replace holds exactly what git
would check out for it, so nothing it overwrites is the only copy. Once the
hand edit is committed, `sync` replaces it:

```text
updated .github/dependabot.yml (a-dependabot-skeleton 0.1.0)
every bone now matches (1 updated, 0 created)
```

`sync` writes every drifted file or none of them, and if writing fails
part-way it names which files it wrote. It never stages, commits or stashes
anything, and it makes no network request of its own, so it needs the
skeletons' sources already fetched, which `cargo fetch` does. The full
reading, including how a failure part-way through is reported, is in
[`.docs/wearing.md`](.docs/wearing.md#sync).

## Concepts

### Terms

- **skeleton**: the crate a repository wears. An ordinary Cargo crate with a
  `[package.metadata.skeletons]` table, a `files/` directory of whole files,
  and, when it has `set` options, a `partials/` directory of fragments.
- **bone**: one claim a worn skeleton makes about a repository. A bone is
  one file, and the claim is that the file holds exactly the bytes the
  skeleton renders for it.
- **wearing table**: the `[package.metadata.skeletons.<dependency-key>]`
  table in a repository's manifest. Its presence means the dependency is
  worn, and its keys are the option values the repository chose.
- **option**: a variation a skeleton's author allowed for, declared in the
  skeleton's manifest with a type and, apart from an optional `text`, a
  default.
- **drift**: a bone whose file is not what the render says it should be,
  either `missing` or `changed`.
- **behind**: a worn skeleton for which a newer version exists than the one
  the lockfile holds. A separate question from drift.

### Options

There are three types, and nothing else. An **`enum`** is a closed list with
one value chosen, which fills a `{{placeholder}}`. A **`set`** is a closed
list with any number chosen, each selecting a fragment to insert, in the
skeleton's own order rather than the order the repository listed them. A
**`text`** is the repository's own words, which fill a placeholder verbatim
and take any non-empty text with no control character. With a default, a
`text` is required like an `enum`. Without one it is optional: unset, every
line holding its placeholder is dropped whole, terminator included.

Every value is stated and none is inferred. One that a skeleton did not
declare, or that falls outside a closed list, is refused by name.

`skeletons` guarantees that a file matches the render. It does not
guarantee that the render is valid YAML, TOML or anything else. A `text`
value goes in verbatim, so the quotes around a placeholder are the
skeleton's, and a value with a quote in it breaks the file the way any
hand-edit would.

### Behind

Whether a skeleton is behind is read from how its dependency is declared,
never from a setting of its own:

| Declared with | Behind when |
|---|---|
| a version from crates.io | a newer non-yanked, non-prerelease version is published |
| a git `tag` | a newer version tag exists on the remote |
| a git `branch`, or `git` alone | the skeleton's own directory holds newer content past the locked commit |
| a git `rev`, or a `path` | never: it reads **pinned** |
| another registry | never asked: it reads **undetermined** |

A skeleton is **current** when none of that holds. When the answer needs a
network request that cannot be made, it reads **undetermined**, never
current, because a failure to ask must not read as "not behind". A behind
skeleton taken from crates.io looks like this in `check`'s header:

```text
dependabot in Cargo.toml: a-dependabot-skeleton 0.1.2 from crates.io, behind (0.2.0 is available)
```

### Writing a skeleton

A skeleton is data: bytes in, bytes out. A render reads only the skeleton's
manifest, its `files/` and its `partials/`, and runs nothing the skeleton
ships. [`.docs/skeleton-format.md`](.docs/skeleton-format.md) is
the reference for writing one: the layout of the crate, its manifest, the
placeholder and directive grammars, and every way a render can refuse.

A skeleton can be very small. This one has one option and one file; it is
not the `a-dependabot-skeleton` worn above, which also has `ecosystems` and
`assignee`. Its `Cargo.toml` declares the option:

```toml
[package]
name = "a-small-skeleton"
version = "0.1.0"
edition = "2024"

[package.metadata.skeletons.options.cadence]
type = "enum"
values = ["daily", "weekly", "monthly"]
default = "weekly"
```

and `files/.github/dependabot.yml` holds the placeholder:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "{{cadence}}"
```

Beside those sits an empty `src/lib.rs`, because Cargo will not package a
crate with no target.

Every `{{` in a file that fills opens a placeholder, and there is no escape, so
a file that holds a `{{` that is not one, a GitHub Actions workflow with
`${{ secrets.TOKEN }}` in it, is listed in a `verbatim` array in the manifest.
A verbatim file gets no fill and no select: a render is its bytes, exactly as
written, and it need not be UTF-8. See
[Verbatim files](.docs/skeleton-format.md#verbatim-files).

## Where next

- [`skeletons` on docs.rs](https://docs.rs/skeletons) is the crate's own
  page: mounting the bundle, and what `check`, `sync` and `wear` guarantee.
- [`.docs/wearing.md`](.docs/wearing.md) is the reference for a repository
  that wears skeletons: the wearing table, `wear`, where bones land, `check`
  and `sync`'s output, and every refusal.
- [`.docs/skeleton-format.md`](.docs/skeleton-format.md) is the reference for
  writing a skeleton.
- [`.docs/design.md`](.docs/design.md) explains why `skeletons` is shaped the
  way it is, and lists its known limits.
- [Releases](https://github.com/hexlace/skeletons/releases) lists what changed
  in each release.
- [`contributing.md`](contributing.md) covers working on `skeletons` itself.

## Contributing

Contributions are welcome. The process for them is being worked out as of
September 30, 2026, and issues and pull requests are expected to open soon.

The bundle is the crate in `crates/skeletons`, and it is the only crate this
repository publishes. `ritual/` is this repository's own ritual command line,
which mounts the bundle the way any project's does. It is how `skeletons` is
worked on and tested, and it is never published. `wearer/` is a second
command line that mounts the bundle under another key, for the acceptance
tests that check its messages hold under any key; it is never published
either.

## Versions

`skeletons` follows [Semantic Versioning](https://semver.org/).

The minimum supported Rust version is the `rust-version` the crate declares.
Raising it is not a breaking change, and it happens in a minor release.

## License

MIT. See [license.md](license.md).
