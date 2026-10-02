# Wearing a skeleton

A reference for wearing a skeleton: the wearing table, `wear`, which writes it,
where bones land, what `check` and `sync` say, and every way each can refuse.
The reasons behind each rule are in [design.md](design.md); this document is
what a wearing repository works from directly.

The commands are written for the default dependency key, `skeletons`. A project
that depends on `skeletons` under another key runs them under that key
(`cargo ritual tools check`), and the tool's own messages name only `check`,
`sync` and `wear`, so they read the same either way.

## The wearing table

[`wear`](#wear) writes the table, empty, with the dependency it belongs to; the
rest of this section is what a person can write there by hand or edit
afterwards. A worked example, for a repository that wears the dependabot skeleton
[skeleton-format.md](skeleton-format.md) is written against:

```toml
[dependencies]            # or [dev-dependencies], [build-dependencies], target-specific
dependabot = { package = "a-dependabot-skeleton", version = "0.1" }

# Present = worn. Its keys are the skeleton's options; empty is valid (every option at its default or unset).
[package.metadata.skeletons.dependabot]     # <dependency-key>: the rename, or the package name
cadence = "daily"                        # enum option: a string
ecosystems = ["cargo", "github-actions"] # set option: an array of strings
assignee = "octocat"                     # text option: a string, taken verbatim
```

A skeleton can also claim a file that no option touches and whose `{{` means
something other than a placeholder, such as a GitHub Actions workflow. Its
author declares that file [verbatim](skeleton-format.md#verbatim-files), and a
repository wears the skeleton the same way as any other:

```toml
[dependencies]
ci = { package = "ci-skeleton", path = "../ci-skeleton" }

[package.metadata.skeletons.ci]
```

`ci-skeleton` declares its workflow verbatim, so `.github/workflows/ci.yml` is
claimed byte for byte, `${{ … }}` and all. The wearer writes nothing different:
the table is empty because the skeleton has no option to record, and `check` and
`sync` compare and restore that file exactly as written.

**The rules, in full:**

- **A dependency is worn only when its own manifest also carries a
  `[package.metadata.skeletons.<dependency-key>]` table**, present even if
  it declares no options. A skeleton dependency with no such table produces
  no rows at all — it is exactly as if it were not a dependency for
  `check`'s purposes.
- **The key is the dependency key, not the crate's own name**: for
  `dependabot = { package = "a-dependabot-skeleton", … }`, the table sits
  under `dependabot`, the key the manifest itself declared the dependency
  under — its rename, when it has one.
- **The table sits in the same manifest as the dependency it names**, on
  any workspace member — not only the workspace root.
- **A value is either a string or an array of strings.** A string sets an
  `enum` option (`cadence = "daily"`) or a `text` option
  (`assignee = "octocat"`); an array of strings sets a `set`
  option (`ecosystems = ["cargo", "github-actions"]`), in any order, with
  duplicates kept — the render itself refuses a duplicate. Anything
  else — a number, a boolean, a table, an array holding something other
  than a string — refuses that one worn skeleton, naming the option and the
  shape actually written. A `text` option that declares no default is
  optional: leave it unrecorded and every line holding its placeholder is
  dropped from the render, so a file that still holds those lines reads as
  drifted until the option is recorded or the lines are removed.
- **A key that names no dependency of that manifest is refused**, never
  read as "wears nothing": a wearing table is only ever wrong on purpose,
  and a misspelt dependency key must not go silent.
- **`options` and `verbatim` are the skeleton format's own keys.** A member
  with no dependency keyed `options` has `[package.metadata.skeletons.options]`
  read as its own skeleton schema (see [skeleton-format.md](skeleton-format.md)),
  and one with no dependency keyed `verbatim` has its
  `verbatim = [...]`, the files it ships as bytes, read the same way; both are
  ignored here. A member that *does* declare a dependency under either key can
  never wear it: the key is refused, naming the rename that fixes it. The two
  are judged apart, so a dependency keyed `options` says nothing about
  `verbatim`. This is a known limit, not a bug to be fixed by choosing one
  meaning over the other.

## Where bones land

Every file a worn skeleton renders is one of its bones, claimed at a path
relative to the **workspace root** — whichever member's manifest actually
wears the skeleton. A wearing table on a member two directories deep still
claims `.github/dependabot.yml` at the workspace root, not under that
member's own directory.

Two or more bones that cannot both hold are refused loudly, never merged
and never silently resolved by picking one:

| Paths | Result |
|---|---|
| Identical | refused: claimed by every skeleton or member involved |
| One name to a filesystem that ignores case or Unicode normalization | refused: one file to such a filesystem |
| One an ancestor directory of the other, under that same fold | refused: the two cannot both exist on disk |
| Under one directory spelled two ways (`a/x`, `A/y`) | refused: a directory has one spelling on disk, so the two cannot both be written |

A refusal naming three or more paths says they cannot all be written and
does not call them one file, since they can be joined by any mix of the
relations above.

This is checked whether the two bones come from two different skeletons,
one skeleton worn by two members, or (case, normalisation, nesting, or a shared
directory spelled two ways) the same skeleton against itself, using the same
fold the render itself refuses two of a skeleton's own names by (see
[Determinism](design.md#determinism) in the design).

A claimed path is also read and written only under the exact spelling its
skeleton gives it. A filesystem *lookup* by name can succeed for a name
that is only a case, or a Unicode normalisation, apart from what is really
on disk — the lookup itself folds the difference away — on one filesystem
and not on another, so the directory's own *listing* is compared instead,
by the same fold the render and overlap detection use:

| On disk | Claimed | Case-insensitive (macOS) | Case-sensitive (Linux) |
|---|---|---|---|
| `DEPENDABOT.YML` only | `dependabot.yml` | refused: spelled differently | refused: spelled differently |
| `DEPENDABOT.YML` and `dependabot.yml` | `dependabot.yml` | cannot exist | refused: spelled differently |
| an NFD-normalised name | the same name, NFC | refused: spelled differently | refused: spelled differently |

A claim component is accepted only when the directory it sits in lists an
entry spelled exactly as claimed, byte for byte, and lists no other entry
that spells the same name differently; otherwise the claim is refused as
`unsafe-path`, cause `spelled-differently` (see [Refusals](#refusals)) — in
`check` and `sync` alike, never read, never written. Two spellings of one
name are one file on a case-insensitive filesystem and two files on a
case-sensitive one, so this is checked the same way, and answers the same
way, on both.

A claimed path is also read and written only in the repository the workspace
belongs to:

| Where the claim lies | Result |
|---|---|
| beside a submodule or a nested repository (`sub-notes.yml` next to `sub/`) | written as any claim is |
| inside a checked-out submodule, a nested repository (ignored or not) or a linked worktree: a directory between the workspace root and the claim holds a `.git` of its own | refused: `unsafe-path`, cause `inside-another-repository`, by `check` and `sync` alike |
| inside a submodule that is not checked out | refused by `sync`, which finds the gitlink in the index (see [Sync](#sync)) |

The workspace root's own `.git` is this repository's, and a workspace below
the repository's root is unaffected.

A claimed name is also refused when git refuses to track it. Git will not add
a path whose component it reads as `.git`, and it reads more than the one
spelling that way, so a file at such a name can be created and never
committed. `check` and `sync` refuse the whole set, as git v2.53.0 defines it
(`read-cache.c`, `verify_path_internal`, with the readings in `path.c` and
`utf8.c`):

| A component that is | Example | Result |
|---|---|---|
| `.git` in any ASCII case | `.git`, `.GIT` | refused: `unsafe-path`, cause `inside-git-directory` |
| `.git` or `git~1` in any ASCII case, then any run of spaces and periods | `.git.`, `.git `, `git~1`, `GIT~1 .` | refused: `unsafe-path`, cause `untrackable-name` |
| the same, then a colon and anything after it | `.git::$INDEX_ALLOCATION` | refused: cause `untrackable-name` |
| the same, then a backslash and anything after it; or the same after a backslash that is not the component's first character | `.git\config`, `sub\.git` | refused: cause `untrackable-name` |
| `.git` in any ASCII case with any of 16 invisible code points (U+200C to U+200F, U+202A to U+202E, U+206A to U+206F, U+FEFF) anywhere in it | `.g` U+200C `it` | refused: cause `untrackable-name` |
| anything else, including `.github`, `.gitignore`, `git`, `a.git`, `git~2`, `..git`, `a:b`, `x\y` and `.g it` | | accepted |

A directory is judged as a file is, so `.git./hooks/x.yml` is refused for its
first component. The refusal is decided when the claim is built, from the name
alone, before any name-length check and before anything is created.

A claimed name is also refused when no file could carry it or `sync` could not
stage it. A file name holds at most 255 bytes, and `sync` stages `X` as
`.X.skeletons-sync`, 16 bytes longer:

| Name | Result |
|---|---|
| any component of 255 bytes or fewer, and a last component of 239 or fewer | accepted |
| a last component of 240 to 255 bytes | refused: `unsafe-path`, cause `name-too-long`; the file could exist, but `sync` could not stage it |
| any component of more than 255 bytes | refused: `unsafe-path`, cause `name-too-long`; no file could have the name |

It is counted in bytes (UTF-8), the one bound every supported filesystem
honours, and decided when the claim is read, so `check` and `sync` give the same
answer before anything is created. It is a defect in the skeleton, not in the
repository.

## Check

```text
cargo ritual skeletons check [--drifted] [--behind] [--fail-behind] [--json]
```

| Flag | Meaning |
|---|---|
| `--drifted` | show only bones that have drifted; the exit status still counts every bone |
| `--behind` | show only skeletons that are behind, or where that is undetermined; the exit status still counts every bone |
| `--fail-behind` | also fail when a worn skeleton is behind, or where that is undetermined |
| `--json` | print the answer as one JSON document (format version 1) |

`check` reads the workspace fresh — `cargo metadata --locked
--all-features` — and reports, for every bone, two independent facts,
neither hiding the other: whether its file's bytes match what the locked
skeleton, rendered with the repository's own recorded options, says it
should hold (**matches** or **drifted**, distinguishably **missing** or
**changed**), and whether the skeleton it belongs to is **current**,
**behind**, **pinned**, or **undetermined** (see [Behind](#behind) below).
Comparison is byte-for-byte: a file reformatted with no meaning changed
still reads drifted.

A worked example, for a repository wearing four skeletons — a
registry-pinned one whose bone has drifted and which is behind, a
branch-pinned one whose own render is refused and whose own behind answer
could not be determined, a tag-pinned one whose bones match and which is
current, and a path-pinned one whose bone is missing and which is
permanently pinned — alongside a wearing table naming no real dependency:

<!-- example: check-human -->
```text
dependabot in Cargo.toml: a-dependabot-skeleton 0.1.2 from crates.io, behind (0.2.0 is available)
  drifted (changed)  .github/dependabot.yml

lint in Cargo.toml: lint-skeleton 0.4.0 from branch main of https://github.com/acme/lint-skeleton at 3f2a9c1, undetermined (could not reach https://github.com/acme/lint-skeleton: unable to access 'https://github.com/acme/lint-skeleton/': Could not resolve host: github.com)
  refused: files/clippy.toml:3: placeholder `cadense` names no declared option; this is a defect in lint-skeleton 0.4.0, not in this repository

toolchain in Cargo.toml: toolchain-skeleton 0.3.0 from tag v0.3.0 of https://github.com/acme/toolchain-skeleton, current
  matches            rust-toolchain.toml
  matches            rustfmt.toml

ci in tools/Cargo.toml: ci-skeleton 0.1.0 from path ../ci-skeleton, pinned
  drifted (missing)  .github/workflows/ci.yml

refused: [package.metadata.skeletons.dependabto] in tools/Cargo.toml names no dependency of tools/Cargo.toml; add the dependency, or rename the table to the key the dependency is declared under

4 bones: 2 drifted, 2 match. 4 worn skeletons: 1 behind, 1 current, 1 pinned, 1 undetermined. 2 refusals.
```

One block per worn skeleton, in manifest-then-dependency order: the header
names the wearing (`<dependency> in <manifest>`), the skeleton and its
locked version, how it is pinned, and its own behind state, with the same
detail text `--json`'s `behind.detail` carries in parentheses where there
is one (`behind (0.2.0 is available)`, `undetermined (could not reach
index.crates.io: timed out)`); each line under it is one bone — its drift
state, then the path of the file it claims — in path order. A skeleton with
any refusal about it shows one `refused: …` line per refusal, in path
order, in its own block in place of its bones — only `unsafe-path` can have
more than one. A refusal about no single worn skeleton — a malformed
wearing table, an overlap — sits in a closing `refused:` list instead.
Refusals are always shown, whatever `--drifted`/`--behind` narrow away,
because hiding one would hide why the command failed. The closing count
line counts every bone and every worn skeleton, whatever the filters
narrow away; a refused skeleton's bones are not among them, since its
refusal stands in their place.

A workspace wearing no skeletons at all says so plainly and exits zero:

```text
this workspace wears no skeletons; a manifest wears one with a [package.metadata.skeletons.<dependency>] table beside the dependency
```

`check` exits non-zero when any bone has drifted, or when there is any
refusal, or when the workspace itself could not be read at all — an
unreadable or out-of-date lockfile, in particular (see
[Refusals](#refusals) for every way this can happen), never read as "wears
nothing". A behind, pinned, or undetermined skeleton never affects this on
its own: `--fail-behind` is what makes a behind or undetermined skeleton
fail the command too. `--drifted` and `--behind` only narrow which rows are
printed; neither changes whether the command succeeds, with or without
`--fail-behind`.

When it fails, `check` says why on `stderr`, after the command line's own
`ritual: ` prefix, joining whichever of these clauses apply with `, and `:

| What failed | Clause |
|---|---|
| some bones drifted | `2 of 4 bones have drifted` (`1 of 4 bones has drifted`, `1 of 1 bone has drifted`) |
| any refusal | `there are 2 refusals` (`there is 1 refusal`) |
| with `--fail-behind`, a skeleton is behind or undetermined | `1 worn skeleton is behind (--fail-behind)`, `whether 1 worn skeleton is behind is undetermined (--fail-behind)`, or both at once: `1 worn skeleton is behind, and whether 1 is behind is undetermined (--fail-behind)` (`whether 3 are behind` for three) |

When bones drifted and nothing was refused, the line ends with the remedy:
``2 of 4 bones have drifted; the `sync` task puts
them back`` (`puts it back` for one).

`check` writes nothing in the repository or in Cargo's own caches — not to
the files it reads, and no manifest, cache, or stored hash that could later
go stale. Everything it reports is recomputed fresh on every run, including
behind: nothing about a remote's own answer is ever cached between runs.
The one exception is a branch or default-branch pin whose remote head has
moved: `check` fetches the head alone into a temporary bare repository of
its own, named `skeletons-behind-<process id>-<n>-<n>` under the operating
system's temporary directory (created exclusively, readable only by the
running user), never inside the repository or Cargo's caches, and removes
it as soon as that one answer is read, before the command exits. The
wearing repository and Cargo's own checkout are only ever read. When
`check` cannot create that directory, the skeleton reads `undetermined`
with reason `local-failure` — see [design.md](design.md) for the mechanism.

## Behind

Whether a worn skeleton is behind is read entirely from how it is pinned —
the dependency declaration a wearer already wrote, never a setting of its
own:

| Pinned with | Behind when | Otherwise |
|---|---|---|
| the default registry (crates.io) | a newer non-yanked, non-prerelease version exists | **current** |
| `tag = "…"` | a newer non-prerelease version tag exists on the remote (`<skeleton>-v<version>` tags when the remote has any for this skeleton, else plain `vX.Y.Z`/`X.Y.Z`) | **current** |
| `branch = "…"` | there is newer content in this skeleton's own package directory, past the locked commit | **current** |
| `git = "…"` naming none of `tag`/`branch`/`rev` | there is newer content in this skeleton's own package directory, past the locked commit, on the remote's own default branch | **current** |
| `rev = "…"`, or a local `path = "…"` dependency | — | **pinned**, always |
| a registry other than the default one | — | **undetermined**, always, and never queried |

When the network answer needed to decide behind cannot be reached at all,
the skeleton reads **undetermined** too, never **current** — a network
failure must never read as a silent "not behind". Every undetermined
skeleton names why:

| `reason` | Means |
|---|---|
| `other-registry` | this skeleton comes from a registry other than crates.io, which `check` never asks |
| `unreachable` | the network request itself failed (DNS, connect, TLS, a timeout before the response arrives, or `git` could not reach the remote) |
| `not-in-index` | crates.io does not list this crate at all |
| `unexpected-response` | a response came back that could not be read (crates.io answering with an HTTP status other than success or the not-found statuses 404, 410 and 451 that read as `not-in-index`, or with a body that cannot be read as an index or does not arrive in time; malformed `git ls-remote` output; or an unreadable answer when reading the fetched snapshot) |
| `tag-not-a-version` | the locked tag itself does not parse as a version, so there is nothing to compare a remote tag against |
| `branch-missing` | the locked branch no longer exists on the remote |
| `unrecognised-source` | cargo reported a source this crate does not recognise at all |
| `checkout-unreadable` | Cargo's own checkout of a branch or default-branch pin could not be read, or is not at the commit this pin actually locked |
| `local-failure` | `skeletons` could not create or read the temporary directory it fetches a remote snapshot into — a local failure, not a network one |
| `directory-missing` | this skeleton's own package directory does not exist at all at the remote head |

A `branch =` or default-branch pin's own remote head is read cheaply first
(`git ls-remote`); only when it differs from what is locked does
`skeletons` read this skeleton's own directory from Cargo's checkout and
fetch the head alone into a temporary directory of its own, comparing tree
objects rather than commit history — see [design.md](design.md) for the
mechanism. Behind is never computed for `sync`, which makes no network
request of its own.

## Sync

```text
cargo ritual skeletons sync
```

`sync` takes no arguments. It reads the workspace the same way `check`
does — fresh, from `cargo metadata` and the wearing tables it finds — except
offline: it makes no network request of its own, so `behind` is never
asked, and a clone whose skeleton sources were never fetched aborts naming
`cargo fetch`, exactly as an unreadable lockfile aborts `check`.

For every bone that has drifted, `sync` writes the bytes its skeleton
renders for it, verbatim; a file that already matches is never touched. It
reports what it wrote, in path order, then a closing count:

```text
updated .github/dependabot.yml (a-dependabot-skeleton 0.1.2)
created .github/workflows/ci.yml (ci-skeleton 0.1.0)
every bone now matches (1 updated, 1 created)
```

A missing file is `created`; a changed one is `updated`, keeping its own
existing permissions (a bone claims bytes, so an executable file stays
executable). Nothing drifted at all reports `every bone already matches;
nothing was written`, and a workspace wearing nothing prints the same line
`check` does.

Every message `sync` prints is one line, and it names each path, git path,
mode, rule and word of git's or the operating system's escaped, as
[skeleton-format.md](skeleton-format.md) defines, wherever it names it: in the
line itself and in a remedy that repeats it. A remedy quotes the escaped name,
so for a file named `first`, a newline, then `second.yml` it reads
`git update-index --no-skip-worktree -- first\nsecond.yml`; a name that holds a
control character is typed at the shell as `$'first\nsecond.yml'`, which reads
that escape the same way. A name with nothing to escape reads exactly as it is.

**`sync` is all or nothing until it starts renaming files into place.** It
writes nothing at all — not even a healthy skeleton's own
otherwise-uncomplicated file — when:

- any worn skeleton is refused, or any two bones overlap: each refusal is
  printed as `refused: ` and its full message, the same self-contained text
  `check`'s closing list and `--json`'s `message` carry, sorted by kind and
  then message, followed, on `stderr`, by `sync writes all or nothing, and
  there is 1 refusal, so it wrote nothing` (`there are <n> refusals` for
  more than one);
- the workspace root is not inside a git work tree at all: with no version
  control there is no undo for what `sync` would replace:

  ```text
  /path/to/workspace is not inside a git work tree, so sync wrote nothing: without git there is no undo for what it replaces; commit the workspace to git first
  ```
- `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
  `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_COMMON_DIR` or `GIT_ATTR_SOURCE`
  is set in the environment `sync` runs in, whatever its value — each one
  redirects which repository, work tree, index, object store or attribute
  source git would actually answer about:

  ```text
  GIT_DIR is set, and git would answer from wherever it points rather than from this work tree's own repository, so sync wrote nothing: git sets it for the hooks it runs, so run the `sync` task outside a git hook (the `check` task works inside one), or unset it
  ```

  more than one variable at once reads `GIT_DIR and GIT_WORK_TREE are set
  … from wherever they point … git sets them for the hooks it runs … or
  unset them`. Git exports `GIT_INDEX_FILE` and `GIT_DIR` to a hook, and
  inside `git commit <paths>` or `git commit -a` the index it uses may be
  a temporary one that a file written now is not part of, so a `sync` that
  would write refuses there;
- git itself refuses the repository's ownership as dubious
  (`safe.directory`):

  ```text
  git refuses to read this repository because another user owns it (git's safe.directory check), so sync wrote nothing: <git's own detail>
  ```
- git cannot be asked at all, or answers with something `skeletons` cannot
  read: `git` could not be run, or `rev-parse` or `status` exited
  unsuccessfully for a reason none of the bullets above name (git's own
  first `fatal:` line is quoted, or failing that its first non-blank
  line), or either printed more than 16 MiB, the most `skeletons` reads
  from git. An answer cut short is never read as though it were complete,
  since a `status` cut short may have dropped exactly the record that says
  dirty:

  ```text
  running `git` failed, so sync wrote nothing: <why it could not be run>
  git status failed, so sync wrote nothing: <git's own detail>
  git status printed more than 16 MiB, the most `skeletons` reads from git, so sync wrote nothing
  ```
- a git command ran for more than thirty seconds and was killed. The message
  names the command, what it was answering, what most likely held it up and how
  to find out, per command:

  ```text
  `git cat-file` timed out after 30 s writing what git would check out for plain.yml, and was stopped, so sync wrote nothing: a content filter this repository configures for plain.yml did not finish (`git check-attr filter -- plain.yml` names it); make it finish, then run the `sync` task again
  `git status` timed out after 30 s and was stopped, so sync wrote nothing: a content filter this repository configures, or a slow filesystem, kept it from finishing; run `git status` to see it finish, then run the `sync` task again
  `git ls-files` timed out after 30 s reading git's index entry for plain.yml, and was stopped, so sync wrote nothing: git could not read its own index in time (another git process holding it, or a slow filesystem); run the `sync` task again once `git status` answers promptly
  ```

  `check-ignore` reads `asking whether git ignores <path>, and was stopped, so
  sync wrote nothing: git could not read its ignore rules in time (a slow
  filesystem)`; `ls-files` reads `reading what git's index tracks at the directory <dir>` or
  `reading the paths in git's index` for the questions that are about a
  directory above a claim or about the whole listing, and `rev-parse` reads
  `git could not answer in time (another git process holding the repository, or a
  slow filesystem)`;
- **the whole work tree is not clean**, as git itself defines clean — not
  only the paths `sync` is about to write. A whole-tree `git status`
  reporting anything at all counts: a tracked change anywhere, staged or
  unstaged, a mode-only change included; an untracked file; a conflict; an
  intent-to-add entry; or a change inside a submodule. Ignored files do not
  count. Each dirty path is named with why:

  ```text
  .github/dependabot.yml has uncommitted changes
  .github/workflows/ci.yml is untracked
  build.rs is deleted
  new.rs is marked intent-to-add
  merged.rs is conflicted
  staged.rs has staged changes
  vendor is a submodule with changes of its own
  ```

  A status record `skeletons` cannot classify counts as dirty too,
  never as clean:
  ``git status reported a line `skeletons` cannot read: <the record>``.
  The lines are sorted by path, and followed by, on `stderr`:

  ```text
  the working tree has 2 uncommitted changes, so sync wrote nothing: it writes only into a clean working tree, where git holds everything it could replace; commit, stash or move them, then run the `sync` task again
  ```

  with one change, this instead reads `1 uncommitted change` and `move
  it`, singular throughout. An unrelated uncommitted file anywhere in the
  tree — not only at a path `sync` is about to write — blocks it exactly
  the same way;
- **git cannot be shown to give back the bytes at a path `sync` is about to
  write**, even once the whole tree above reads clean — an
  `--assume-unchanged` or `--skip-worktree` edit is exactly the shape a
  whole-tree `git status` cannot see at all, and so is a file holding bytes
  a content filter would drop before git recorded it. `sync` proves, per
  path, that it is either absent — nothing on disk, no entry in the
  repository's index at or under the path, or at any directory above it, and
  no ignore rule of git's that ignores it — or
  a regular file tracked as one stage-0 entry (mode `100644`/`100755`, never
  a symbolic link or a submodule's own gitlink, never intent-to-add, and
  never flagged skip-worktree or assume-unchanged, since git does not read
  such a file from the work tree) whose on-disk bytes are
  exactly the bytes git would write for that entry on checkout — applying
  whatever content filters the repository configures (`.gitattributes`,
  git-lfs included) as `git checkout` would. Anything else is refused,
  whatever the cause. Each unproven path is named with why, one line per
  path, sorted by path:

  | Line | Means |
  |---|---|
  | ``<path> is not in git's index under exactly this spelling: commit it, or move it away, then run the `sync` task again`` | git has no entry at that exact spelling; the index is asked by exact spelling, so an entry under a case or normalisation variant proves nothing about the claimed one |
  | `<path> is listed in git's index as <git's path>` | git's one entry for it sits at a different path — a differently normalised spelling, or an entry beneath the claimed path |
  | ``<path> is ignored by git (`<source>:<line>:<pattern>`, as `git check-ignore -v` reports it as source:line:pattern), so sync would create a file git status never shows and git add refuses, and a later sync could not update it: remove the ignore rule, or make <path> by hand and run `git add -f -- <path>`, then run the `sync` task again`` | nothing is at the path, the index holds nothing for it, and a `.gitignore`, `.git/info/exclude` or `core.excludesFile` rule ignores it (git's own last match decides, so a later `!` pattern that un-ignores it is not refused). The rule is quoted as `git check-ignore -v` prints it, for example `cfg/.gitignore:2:*.local.yml`. A file that is present and tracked is never refused for a rule, so a claim added with `git add -f` is updated |
  | `<path> could not be checked against git's ignore rules: <why>` | `git check-ignore` exited with a status other than 0 or 1, printed a rule `skeletons` cannot read, or printed more than 16 MiB. The claim is not created, since it is not known to be one git would show |
  | ``<path> is conflicted in git's index: resolve the conflict and commit it, then run the `sync` task again`` | an entry at a stage other than 0 |
  | `<path> is a symbolic link in git's index` | the entry's mode is `120000` |
  | `<path> is a submodule in git's index` | the entry's mode is `160000` |
  | ``<path> is marked skip-worktree in git's index, so git does not read its bytes from the work tree and would ignore what sync wrote there: run `git update-index --no-skip-worktree -- <path>`, then run the `sync` task again`` | the file is present and holds exactly what git holds, but its entry is flagged so git never reads it: a write would be reported `updated` while a commit kept the old bytes. Also `assume-unchanged` (with `--no-assume-unchanged`), and both flags (`skip-worktree and assume-unchanged`, with both options) |
  | ``<path> cannot be written, because git's index tracks <entry> as a file where <path> needs a directory, so git would put it back in place of what sync wrote: if <path> belongs in this repository, run `git rm --cached -- <entry>` and commit, then run the `sync` task again`` | the index tracks a file (or, worded `a symbolic link`, a link) at a directory above the path, hidden from the work tree; creating the directory would leave a tree git reads as that entry deleted and the path untracked. When `<entry>` is another case of the directory, the line adds `(git takes the two for one name where it ignores case, core.ignorecase)` |
  | `<path> is inside <entry>, which git's index tracks as a submodule, another repository, and sync writes only into this one` | the index tracks a gitlink at a directory above the path, whether or not the submodule is checked out; there is no remedy `skeletons` can name |
  | `<path> cannot be written, because git's index tracks <entry> with mode <mode> where <path> needs a directory` | the index tracks some other kind of entry at a directory above the path |
  | `<path> has mode <mode> in git's index, not a regular file's` | any other mode than `100644` or `100755` |
  | `<path>'s entry in git's index could not be read: <why>` | `ls-files` failed, printed something unreadable, or listed more than one entry |
  | ``<path> is not what git would check out for it, so git could not give these bytes back: keep a copy of it outside the repository, remove it, run `git checkout -- <path>`, then run the `sync` task again`` | the file's bytes are not what git would write for its index entry on checkout, whatever the cause: an edit git's own status cannot see, or bytes a content filter would not write back. Git was asked twice and gave the same bytes, so `git checkout` writes them |
  | `<path> is checked out differently each time git is asked (a content filter whose output is not a function of the file alone), so sync can never show that git would give its bytes back, and does not write it` | git was asked twice and gave different bytes, so no checkout exists to bring the file back to, and `git checkout` would only write another variant. No remedy |
  | `<path> could not be compared with what git would check out for it: <git's own detail>` | `cat-file` failed, a missing object, a `required` content filter failing or a filter's download failing among the causes. No remedy: no one command is right for every cause |
  | ``<path>: git <command> printed more than 16 MiB, the most `skeletons` reads, so sync cannot show what it would replace`` (for a file it would create: `…cannot show that git would see the file it would create`) | `ls-files` or `cat-file` answered with more than `skeletons` will read. No remedy |
  | ``<path> is not a regular file: move it away, then run the `sync` task again`` | a symbolic link, directory or other non-file sits there |
  | `<path> could not be read: <why>` | looking at the path failed for a reason other than its being absent |
  | ``<path> changed while sync was checking it: run the `sync` task again`` | the file was missing when surveyed and is there now, or the reverse |

  A path missing from the work tree that git's index still tracks — one
  hidden by `--skip-worktree`, or excluded by a sparse checkout, cone or
  not — is refused too, since git would ignore a file `sync` wrote there
  and the committed bytes would stay in force:

  ```text
  plain.yml is tracked in git's index but absent from the work tree (skip-worktree or a sparse checkout hides it), so git would ignore what sync wrote there: bring plain.yml back into the work tree (`git sparse-checkout add /plain.yml` if a sparse checkout leaves it out, or `git update-index --no-skip-worktree -- plain.yml` and then `git checkout -- plain.yml`), then run the `sync` task again
  ```

  When git's entry is beneath the claimed path rather than at it, the line
  reads `plain.yml is tracked in git's index as <git's path> but absent …`.
  When it is at another case of the claimed path (git ignores case under
  `core.ignorecase`, and would take what `sync` wrote for that entry and
  ignore it), the line reads `plain.yml is tracked in git's index under
  another case, as <git's path>, but absent from the work tree (skip-worktree
  or a sparse checkout hides it), so where git ignores case
  (core.ignorecase) it would take what sync wrote there for that entry and
  ignore it`. Git's own case fold decides that, and it folds ASCII only, so
  a Unicode case variant git keeps visible is not refused.
  Every form ends with the remedy that brings the entry git holds (spelled as
  git spells it) back: `git sparse-checkout add <directory>` (`/<path>` for a
  file at the root, which only a non-cone sparse checkout can leave out) if a
  sparse checkout leaves it out, or `git update-index --no-skip-worktree --
  <path>` and then `git checkout -- <path>`, since clearing the flag alone
  leaves the file deleted; then run the `sync` task again;

  followed by, on `stderr`:

  ```text
  sync cannot show that git holds what 2 files would replace, so it wrote nothing: the lines above say why
  ```

  with one file, this instead reads `1 file` and `the line above says why`,
  singular throughout. The wording follows what the refused paths are, since
  a file `sync` would create replaces nothing: when every refused path is one
  `sync` would create (a claim git ignores, one git tracks but the work tree hides,
  or one under a directory git tracks as a file), the summary reads `sync cannot show that git would
  see the 2 files it would create, so it wrote nothing: the lines above say
  why`, and when both kinds are refused it says both, `sync cannot show that
  git holds what 2 files would replace, or that it would see the 1 file it
  would create, so it wrote nothing: the lines above say why`. A path is one
  `sync` would create when nothing was at it when `sync` looked. The summary
  names no remedy of its own: what fixes one path (committing a file git does
  not hold) breaks another (a file git holds and cannot give back), so each
  line above it carries the one that fits its cause, or none.
- something is already at a path `sync` needs to create while staging a
  write — its own exclusive `.<name>.skeletons-sync` file, or a missing
  parent directory:

  ```text
  .plain.yml.skeletons-sync is already there, and sync stages plain.yml at exactly that path; it never overwrites or removes anything it did not create, so it wrote nothing: move .plain.yml.skeletons-sync away, then run the `sync` task again
  ```

  and, for a parent directory that appeared between `sync` finding it
  missing and creating it:

  ```text
  a appeared while sync was creating it for a/b.yml; it never writes into anything it did not create, so it wrote nothing: run the `sync` task again
  ```

  Never overwritten, never removed — the remedy is always to move it away
  by hand;
- a claimed target, or a directory above it, no longer matches what `sync`
  proved about it, found while `sync` was staging its write — nothing has
  been written yet:

  ```text
  a.yml changed while sync was preparing to write it (a file appeared there), so sync removed what it had prepared and wrote nothing: run the `sync` task again
  ```

  the parenthesis reads:

  | Reads | Means |
  |---|---|
  | `a file appeared there` | a file `sync` was to create is there now |
  | `it is no longer there` | a file `sync` was to replace is gone |
  | `it is no longer a regular file` | a file `sync` was to replace is a symbolic link or a directory now |
  | `its content is no longer what sync proved git can give back` | a file `sync` was to replace holds other bytes now, so replacing it would lose them |
  | `it is now a symbolic link` | the file itself became a link |
  | `x above it is now a symbolic link` | a directory above it became a link (`x above it is now a file` when it became a file) |
  | `it is now inside .git` | the path now has a `.git` component |
  | `x above it is now another git repository` | a directory above it now holds a `.git` of its own |
  | `it can no longer be read: <why>` | looking at the path failed for a reason other than its being gone |
  | `x is now found on disk under another spelling`, or `x is now also present on disk as y` | the path, or a directory above it, is now listed under a differently spelled name; `x` is the path up to and including the differing component |

  A file whose bytes changed after `sync` proved them is never replaced,
  and a directory that became a link is never written through;
- the filesystem takes an index entry git tracks, and hides, for what `sync`
  is writing — git holds it under another Unicode spelling of the path (or of
  a directory above it) that git keeps apart and this filesystem does not, so
  git would take what `sync` wrote for that entry. `sync` asks the filesystem
  itself, after it has created the staging file and before it writes a byte,
  so a filesystem that keeps the two spellings apart never refuses and one
  that folds them does, with nothing written:

  ```text
  é.yml and É.yml, which git's index tracks, are one name on this filesystem, and git would take what sync wrote for É.yml, so sync removed what it had prepared and wrote nothing: bring É.yml back into the work tree (`git sparse-checkout add /É.yml` if a sparse checkout leaves it out, or `git update-index --no-skip-worktree -- É.yml` and then `git checkout -- É.yml`), then run the `check` task to see how it is spelled on disk
  ```

  and, when git's entry is a file at a directory above the path and the two
  spellings of that directory differ only in Unicode normalization, each
  spelling is followed by its code points and a note says why:

  ```text
  café/x.yml (caf\u00E9/x.yml) would be written under café (caf\u00E9), which this filesystem takes for café (cafe\u0301) (the two spellings differ only in Unicode normalization, so each is followed by its characters as \uXXXX code points, as bash 4.3 or later, or zsh, reads them in $'…' under a UTF-8 locale), a file git's index tracks, and git would put that file back in place of what sync wrote, so sync removed what it had prepared and wrote nothing: if café/x.yml belongs in this repository, run `git rm --cached -- café` and commit, then run the `sync` task again
  ```

  The first message's remedy leads with `git sparse-checkout add
  <directory>` for a sparse checkout that leaves it out (`/<path>` for an
  entry at the root, as above);
- the operating system refuses permission to create something `sync` needs
  beside a claim — a missing directory, or the staging file — because this user
  cannot write into the directory it sits in. The message names that directory
  (`the workspace root` for one at the root) and the permission to give:

  ```text
  writing d/x.yml failed, because sync cannot create anything in d (Permission denied (os error 13)), so sync removed what it had prepared and wrote nothing: give yourself write permission there (`chmod u+w d`) and run the `sync` task again
  ```

  at the workspace root it reads `the workspace root` and `chmod u+w .`;
- staging a file fails for any other reason — creating a directory,
  writing or syncing the staging file:

  ```text
  writing .github/dependabot.yml failed, so sync removed what it had prepared and wrote nothing: <the underlying error>
  ```
- a drifted write's own staging path is a path some bone claims, is one name
  with a claimed path to a filesystem that ignores case or Unicode
  normalization, or is a directory above or beneath one — a
  defect in how two of the worn skeletons name their files, not in the
  repository being synced. The comparison is the fold overlap detection uses
  and is made on every platform, before anything is written. The message
  names the other path and how the two relate:

  ```text
  sync stages a/b at a/.b.skeletons-sync, which is itself a claimed path, so it wrote nothing: the two cannot both be written; this is a defect in how the worn skeletons name their files, not in this repository
  sync stages a/b at a/.b.skeletons-sync, which is one name with the claimed path a/.B.skeletons-sync to a filesystem that ignores case or Unicode normalization, so it wrote nothing: the two cannot both be written; this is a defect in how the worn skeletons name their files, not in this repository
  sync stages a/b at a/.b.skeletons-sync, which is a directory above the claimed path a/.b.skeletons-sync/c, so it wrote nothing: the two cannot both be written; this is a defect in how the worn skeletons name their files, not in this repository
  sync stages a/b at a/.b.skeletons-sync, which is beneath the claimed path a, so it wrote nothing: the two cannot both be written; this is a defect in how the worn skeletons name their files, not in this repository
  ```

The refusals above about git — not a work tree, a redirecting variable,
dubious ownership, git that cannot be asked, a dirty tree, an unproven
path — are asked only when there is something to write. With every bone
already matching, `sync` never runs git, and reports `every bone already
matches; nothing was written` inside a git hook, outside a git work tree,
or with a redirecting variable set alike.

When there is something to write, `sync` needs git installed; without it,
`sync` refuses with ``running `git` failed``, listed above.

`sync` shares `check`'s own claim resolution, so the two can never
disagree: every bone `check` reports drifted is one `sync` would write, and
a path either one refuses as unsafe — a symbolic link sitting at or above a
claimed path, in particular — is refused by both. The claim walk refuses a
link at or above a claim before either command touches it, and every
staging file and directory `sync` itself creates is created exclusively
(below), so `sync` never writes outside the workspace root through a
symbolic link — neither the claimed path nor its own staging path.

Every write is staged as a new, exclusively-created file —
`.<name>.skeletons-sync`, beside its own target — before any lands. Staging
creates exclusively: it fails on anything already there, a dangling symbolic
link included, so it never truncates and never writes through one. Something
already at that path is refused by name and left alone, so `sync` wrote
nothing.

Each write is checked again against what `sync` proved, three times: while it
is staged, at the start of the commit for every write at once, and
immediately before it lands. A file `sync` replaces is renamed over its
target. A file it creates is hard-linked to its target and the staging name
removed, so a file that appears there after the last check is refused, never
overwritten. Every file `sync` creates lands before any file it replaces, each
group in path order (the report above stays in path order), so a link that
fails does so before anything is written:

```text
creating d/b.yml failed before any file was written, so sync removed what it had prepared and wrote nothing: sync creates new files with hard links, and the link failed: Operation not permitted (os error 1)
```

On a filesystem that refuses hard links, `sync` can check but not create a
missing file, and says only what the operating system said.
A change found before any file landed writes
nothing:

```text
a.yml changed while sync was writing it (its content is no longer what sync proved git can give back), so sync removed what it had prepared and wrote nothing: run the `sync` task again
```

with the same parentheses as the staging-time message above. If a write is
refused or fails after some files landed — a change, or a rename or link that
fails, such as a full disk or a permission this process does not have — it is
named on `stderr`, along with exactly which files already made it and which
did not:

```text
b.yml changed while sync was writing it (a file appeared there) after 1 of 2 files was written (a.yml); b.yml was not written, and git holds what the written file replaced: run the `sync` task again
```

```text
replacing .github/workflows/ci.yml failed after 1 of 2 files was written (.github/dependabot.yml); .github/workflows/ci.yml was not written, and git holds what the written file replaced: <the underlying error>
```

A failure that comes before any file landed writes nothing, and reads
`replacing <path> failed before any file was written, so sync removed what
it had prepared and wrote nothing: <the underlying error>`. For a file
`sync` was creating, `replacing` reads `creating`. A link that fails
reads as the message above, and only a workspace that spans two filesystems
can meet it after a file has landed: `creating <path> failed <which files were
written>: sync creates new files with hard links, and the link failed: <the
underlying error>`.
Both lists of files, the written and the not written, are in path order,
whatever order they landed in.

The files already written are correct, finished writes that nothing rolls
back; every staging file this run created for the rest — the failed
write's own, and every later write's — is removed, along with any
directory this run created that is now empty because of it, before `sync`
exits, so nothing new is left beside the claimed files themselves. Each
removal first walks the path again from the workspace root and removes only
the very file or directory `sync` created, never through a symbolic link.
Whatever it left in place or could not remove is named too, with the reason,
rather than left silent: `it could not be removed: <the underlying error>`,
`left in place because <why the path is no longer safe to touch>` (for
example `x above it is now a symbolic link`), `left in place because what is
there now is not what sync created`, or `no longer where sync created it`.
Where nothing was written, `so sync removed what it had prepared and wrote
nothing: <what to do>` becomes `so sync wrote nothing, and removed what it had
prepared except <path> (<reason>): find and remove it by hand, then <what to
do>` (`them` for more than one), and where the line ends in the underlying
error, `so sync wrote nothing, and removed what it had prepared except <path>
(<reason>): <the underlying error>; find and remove it by hand`. Where
something was written, or the message is the staging-collision one, the
message ends `; sync did not remove everything it had prepared: <path>
(<reason>); find and remove it by hand`.

When every file was written but the staging name of a file `sync` created
could not be removed, `sync` reports the `created` and `updated` lines, then
fails:

```text
every drifted file was written, but sync did not remove everything it had prepared: .plain.yml.skeletons-sync (it could not be removed: <the underlying error>); find and remove it by hand
```

That name is a second link to the new file, and an untracked file that
would stop the next `sync`, so it has to go.

After the last file lands, `sync` reads each one back. One that no longer
holds what `sync` wrote, because something else changed it in that instant,
fails the run naming it, and nothing is reported as written:

```text
a.yml changed after sync wrote it, so sync cannot confirm what it holds now: run the `check` task to see
```

(`a.yml and b.yml changed after sync wrote them … what each holds now` for
more than one.)

What no check can close is the instant between the last one and the step
itself, across the whole time from the first staging file to the last removal:
an edit that lands there is replaced, and a directory swapped for a link there
is followed, so a staging file can be created, or a file of the same name
removed, where the link points. `sync` looks again before each create, each
rename or link, and each removal, and leaves what it finds changed in place,
named. Every git process it runs has exited before its first look, but a
content filter can start a process of its own that outlives git, and such a
process, like any other on the machine, can act at any moment: each look
catches what it did before that look, not after it. The design's known limits
say more.

`sync` never stages, commits, or stashes
anything in git — the five kinds of git command it runs (`rev-parse`,
`status`, `ls-files`, `cat-file` and `check-ignore`, each run with
`--no-optional-locks` so a read never takes an optional lock) are
all read-only, so the repository's own index, refs, config, object
database and stash are exactly as they were before `sync` ran, whatever it
wrote to the working tree. The one exception is a content filter the
repository itself configures: `cat-file` runs its smudge side while writing
a file's checkout, exactly as `git checkout` would, and `status` may run its
clean side, and git-lfs's own filters can store a new, content-addressed
object under `.git/lfs/objects/` while doing so — never changing an existing
one, and never something `sync` itself asked for.

## Wear

```text
cargo ritual skeletons wear <crate>[@<version>] [<key>] [--git <url> [--branch <name> | --tag <name> | --rev <revision>] | --path <directory>]
```

`wear` starts a repository wearing a skeleton: it adds the skeleton as a
dependency and adds the wearing table for it, which is everything `sync` needs
to begin writing the skeleton's files. It writes the same two pieces a person
could write by hand and nothing else, so a repository that wore a skeleton by
hand and one that used `wear` are the same repository. It does not write the
skeleton's files, which are `sync`'s, and it records no option values: it
writes the table empty, and options are set by editing it.

**The arguments.**

- **`<crate>[@<version>]`** is the skeleton's crate, split at the first `@`. The
  text after it is a version requirement in Cargo's own grammar
  (`a-dependabot-skeleton@0.1`, with no `v`), handed to Cargo as typed, which
  refuses one it cannot read. Cargo also refuses a version beside `--path`.
- **`[<key>]`** is the dependency key, which also names the wearing table (see
  [The wearing table](#the-wearing-table)). Left out, it is the crate's name.
  `wear` passes Cargo `--rename` only when the key differs from the crate's
  name, since Cargo writes a redundant `package = …` for an equal one.
- **No source flag** takes the crate from the default registry. **`--git <url>`**
  takes it from a git repository, at the tip of its default branch unless one
  of **`--branch`**, **`--tag`** or **`--rev`** names the reference; those three
  need `--git` and exclude one another. **`--path <directory>`** takes it from a
  directory, relative to where the command runs, which Cargo writes into the
  manifest relative to the manifest. `--git` and `--path` exclude each other.
  Each value reaches Cargo as `--flag=value`, one argument, so a value that
  begins with `-` is never read as an option of its own.

**The grammar of a key.** A key, and the crate's name, start with an ASCII
letter or `_`, continue with ASCII letters, digits, `-` and `_`, and are at most
64 bytes. That is narrower than Cargo's own rule, which lets a key hold other
Unicode identifier characters and checks a rename only after writing it, so a
key Cargo would then refuse could leave a broken manifest. A key outside this
grammar is refused before anything is written, and can still be written by hand.
`options` and `verbatim` are refused as keys, whether typed or defaulted from a
crate of that name (see [The wearing table](#the-wearing-table)): wear such a
crate under another key. So are `std`, `core`, `alloc`, `proc_macro` and `test`
(with `-` and `_` the same), the crates the compiler provides: a dependency under
one of those keys shadows the compiler's crate in the command line's test build,
which then fails in `rustc`'s words and not ours.

**Where it writes.** `wear` is told which package built the command line it runs
in, and writes into that package's manifest and the workspace's `Cargo.lock`:
there is nothing to point it at, and no case of two candidates to refuse. That
package has to be a member of the workspace the command runs in. For a project
made by `ritual new` it is the crate under `ritual/`, which is also why the
dependency is a dev-dependency: it keeps the skeleton out of the command line's
own build.

**What it writes**, in this order:

1. The dependency, by running `cargo add --dev --package <that package>` with the
   crate, the key and the source, so the version grammar and every source Cargo
   knows are Cargo's. The dependency lands under `[dev-dependencies]`, and
   `Cargo.lock` gains the skeleton's pin. `wear` may reach the network, as
   Cargo does to resolve a registry version or fetch a git source.
2. An empty `[package.metadata.skeletons.<key>]` table in the same manifest.
   The manifest is edited in place, so every comment and blank line the wearer
   wrote stays, and the result is compared against an independent parse of the
   manifest before it is kept: it must hold the one new empty table and nothing
   else different. The table lands after the last `[package…]` table, with one
   blank line around it: right after `[package]`, or after
   `[package.metadata.ritual]` or a sibling wearing table. A table it has to go
   under that is written inline gets the new one inside it, inline.
3. A read of the workspace back through the same reader `check` and `sync` use.
   `wear` keeps what it wrote only if that reader finds a worn skeleton at the
   key.

**What it prints.** On success, two lines on stdout: what was added and where,
with the version Cargo locked and the manifest as a path relative to the
workspace root, then the next step.

```text
added a-dependabot-skeleton 0.1.0 to ritual/Cargo.toml as the dev-dependency `dependabot`, with an empty [package.metadata.skeletons.dependabot] table
commit ritual/Cargo.toml and Cargo.lock, then run the `sync` task to write its files
```

The next step names the `sync` task and nothing before it: a task is never told
the key it is mounted under, so it cannot say how its command line reaches
`sync`. It gives the order itself: `sync` writes only into a clean work tree, so
the line says to commit the manifest it names, and `Cargo.lock` (always at the
workspace root), before running it. Cargo's own output is not repeated. Any
option values edited into the table go in that commit too.

**It refuses before it writes anything** when the request or the project is
wrong, in this order, and each message ends by saying what to do. Every message
below is one line on stderr after the command line's own prefix (`ritual: `),
and, like every message in this document, prints text from outside escaped.

- the crate name is not in the key grammar:

  ```text
  `Bad Name` is not a crate name wear can add: a name starts with an ASCII letter or `_`, continues with ASCII letters, digits, `-` and `_`, and is at most 64 bytes; give the `wear` task the crate's name as its manifest spells it
  ```
- the key is not in the grammar, is `options` or `verbatim`, or names a crate the
  compiler provides:

  ```text
  `x y` cannot be a dependency key: a key starts with an ASCII letter or `_`, continues with ASCII letters, digits, `-` and `_`, and is at most 64 bytes; give the `wear` task another key as its second argument
  `options` cannot be worn as a dependency key: `options` and `verbatim` under [package.metadata.skeletons] belong to a skeleton's own declaration; give the `wear` task another key as its second argument
  `std` cannot be worn as a dependency key: it names a crate the compiler provides; give the `wear` task another key as its second argument
  ```
- the workspace cannot be read, which is `check`'s abort for the same four
  causes (see [Refusals](#refusals)). `cargo metadata` is run `--locked`, so a
  stale or missing lockfile is refused before `cargo add` could rewrite more of
  it than the skeleton, and the lockfile message says why `wear` would otherwise
  write it:

  ```text
  Cargo.lock is missing or out of date, and wear changes it only to add the skeleton; run `cargo update --workspace`, then run the `wear` task again
  ```
- the command line's package is not a member of the workspace the command ran
  in:

  ```text
  this command line is built from the package `skeletons-ritual`, which is not a member of the workspace at /path/to/workspace, so wear wrote nothing: run the `wear` task from inside the project this command line belongs to
  ```
- the skeleton is already worn by any member of the workspace, under any key.
  A workspace wears a skeleton once. A wearing that `check` reports as refused
  wears nothing, and does not count:

  ```text
  a-dependabot-skeleton is already worn, as `dependabot` in ritual/Cargo.toml; a workspace wears a skeleton once, so to change its options, edit [package.metadata.skeletons.dependabot] there
  ```
- `[package.metadata.skeletons]` in the manifest is not a table:

  ```text
  [package.metadata.skeletons] in ritual/Cargo.toml is not a table, so wear cannot add a wearing table under it; make it a table, then run the `wear` task again
  ```
- the manifest already depends on this crate, under any key and in any
  dependency table, target-specific ones included, without wearing it. A second
  dependency on one crate is something Cargo refuses only after `cargo add` has
  written it, so `wear` says it first, naming the key the dependency has, which
  is where the wearing table goes. This is said before a key is found taken, as
  the table is the one remedy that helps. Crate names are compared with `-` and
  `_` the same, as Cargo takes them:

  ```text
  ritual/Cargo.toml already depends on a-dependabot-skeleton under the key `dependabot`, without wearing it; to wear it, add an empty [package.metadata.skeletons.dependabot] table to ritual/Cargo.toml
  ```
- the manifest depends on this crate as above and already has a wearing table at
  that dependency's key, so the skeleton is not worn only because its wearing is
  refused (a worn one is the already-worn refusal above). Adding a table would
  be no remedy, so `wear` sends the wearer to `check`, which says why:

  ```text
  ritual/Cargo.toml already depends on a-dependabot-skeleton under the key `dependabot`, and has a [package.metadata.skeletons.dependabot] table for it, but that wearing is refused; the `check` task says why, so run it and fix what it names in ritual/Cargo.toml
  ```
- the key is already a dependency of the manifest on another crate, in any
  dependency table, target-specific ones included. Keys are compared as `rustc`
  names them, with `-` and `_` the same, so `a-x` cannot be worn beside `a_x`:
  the key is taken, and another is to be given:

  ```text
  `tidy` is taken in ritual/Cargo.toml: its dependency on lint-skeleton is declared under `tidy`; give the `wear` task another key as its second argument
  ```
- the manifest already has a wearing table at the key, with no dependency under
  it. A table is matched exactly, as `sync` reads it:

  ```text
  ritual/Cargo.toml already has a [package.metadata.skeletons.dependabot] table, with no dependency declared under `dependabot`; remove the table, or give the `wear` task another key as its second argument
  ```
- the work tree is not clean, or git cannot be asked. This is the question
  `sync` asks (see [Sync](#sync)), with the whole tree counted, asked last so a
  request that is wrong is refused as wrong whether or not the tree is clean.
  The per-path lines are `sync`'s, on stdout, then the summary on stderr, worded
  for `wear`:

  ```text
  ritual/Cargo.toml has uncommitted changes
  the working tree has 1 uncommitted change, so wear wrote nothing: it writes only into a clean working tree, where git holds the manifest it changes and any Cargo.lock git tracks; commit, stash or move it, then run the `wear` task again
  ```

  Outside git it reads `/path/to/workspace is not inside a git work tree, so
  wear wrote nothing: without git there is no undo for what it changes; commit
  the workspace to git first`. Every other git refusal is `sync`'s with `wear`
  for `sync`, `` run the `wear` task outside a git hook (the `check` task works
  inside one)`` in the lines about a redirecting variable, and `` run the `wear`
  task again`` in the timed-out ones.
- the manifest or `Cargo.lock` is a file git cannot hand back. A clean work tree
  says nothing about a file git is told not to read, so after it `wear` asks
  git's index about the two files, as `sync` does for every path it writes. The
  manifest has to be tracked and read from the work tree, and so does a
  `Cargo.lock` that git tracks; one marked skip-worktree or assume-unchanged,
  or both, is refused with `sync`'s line for it, worded for `wear`:

  ```text
  ritual/Cargo.toml is marked skip-worktree in git's index, so git does not read its bytes from the work tree and would ignore what wear wrote there: run `git update-index --no-skip-worktree -- ritual/Cargo.toml`, then run the `wear` task again
  ```

  A manifest git does not track, which a `.gitignore` rule can leave out of
  `git status`, is refused too, with the command that tracks it:

  ```text
  ritual/Cargo.toml is not tracked by git, so wear wrote nothing: git could not give back what wear changes in it; run `git add -- ritual/Cargo.toml` (`git add -f -- ritual/Cargo.toml` if a .gitignore rule matches it) and commit, then run the `wear` task again
  ```

  An ignored or untracked `Cargo.lock` is allowed: git never held it, Cargo
  regenerates it, and refusing it would shut out projects that ignore their
  lockfile. (One that is untracked and not ignored shows in `git status`, and
  is refused above as an uncommitted change.)
- the manifest, or a `Cargo.lock` that exists, cannot be written in place. Cargo
  writes through a temporary file and a rename, so `cargo add` succeeds on a
  read-only file, but `wear` writes the wearing table in place and its undo
  restores in place, so either would fail with the dependency already added.
  `wear` asks the operating system, before anything is written, whether it can
  open each file for writing (without creating it, truncating it or writing a
  byte), so permissions, an access control list, an immutable flag and a
  read-only mount all answer as they would for the real write. A process that
  can write any file passes, and can then also restore. A missing `Cargo.lock`
  is not refused here; the `--locked` read above refuses it:

  ```text
  ritual/Cargo.toml cannot be written in place, so wear wrote nothing: wear changes it in place, and the operating system refused to open it for writing: Permission denied (os error 13); make it writable, then run the `wear` task again
  ```

**It undoes what it did when it fails after `cargo add`.** Both writes go
through ritual's rollback, which records the manifest and `Cargo.lock` before
the first change and, when the run returns a failure, writes both back exactly
as they were: the bytes, not a `cargo remove`, which restores neither the
lockfile nor a manifest's formatting and comments. The message then ends with
`; ritual put the project back as it found it`:

```text
cargo add failed: error: the version provided, `v1` is not a valid SemVer requirement\n\nhelp: changing the package to `a-dependabot-skeleton@1`\n\nCaused by:\n  unexpected character 'v' while parsing major version number; ritual put the project back as it found it
not-one 0.1.0 is not a skeleton: its manifest has no [package.metadata.skeletons] table, so it cannot be worn; wear a crate that is one; ritual put the project back as it found it
```

These are the failures that come after the first change:

- `cargo add` refuses, with everything Cargo said on one line, escaped as above,
  or could not be run at all (``running `cargo add` failed: …``).
- the crate Cargo added has no `[package.metadata.skeletons]` table, so it is no
  skeleton. Only the read back can tell, for a source Cargo resolves itself.
- the manifest could not be read to add the table to (`wear could not read
  <manifest> to add the wearing table: …`), or the edited manifest is not the
  original plus one empty table (`wear could not add
  [package.metadata.skeletons.<key>] to <manifest> without changing anything
  else in it (…); this is a defect in skeletons`).
- the read back refuses the new wearing (``wear added `<key>` to <manifest>, and
  reading it back refuses it:`` and the refusal's own message), does not report
  it at all (``wear added `<key>` to <manifest>, and cargo metadata does not
  report it; this is a defect in skeletons``), or finds that the skeleton Cargo
  resolved is one another entry already wears under its package name, which no
  check before the write could see (the already-worn message above).

When the undo itself fails, the message names every path that was not put back,
relative to the workspace root as every other path `wear` shows is, and says to
check it before running the `wear` task again:

```text
<the failure>; ritual put the project back except for ritual/Cargo.toml — check it before running the `wear` task again
```

Two things the undo cannot give back. A panic: rollback runs when the run
returns a failure and not when it panics, so a panic leaves whatever `cargo add`
and the table write had already changed, which is why `wear` returns a failure
for every condition it can find after `cargo add`. And a change made around
it: rollback does not check that a file still holds what `wear` last wrote
before putting the original back, so an edit to the manifest or the lockfile
made by something else while `wear` runs is overwritten, and Cargo's own
caches, which are outside the project, are not touched. Git is the undo for
everything else, which is why the work tree has to be clean first.

## `--json`

One pretty-printed JSON document (two-space indentation), carrying
`format_version: 1`, `filters` (which of `--drifted`/`--behind` narrowed
`bones[]`), a `summary` of the counts above (bones by drift state, worn
skeletons by behind state, refusals, and whether the command failed),
`skeletons[]` (one entry per worn skeleton, each carrying its own `behind`
object and `refused`, true when a refusal about it stands — its bones are
then absent from `bones[]`), `bones[]` (one entry per bone not filtered
out, each naming the skeleton it belongs to and carrying its own `drift`
and its skeleton's `behind`), `refusals[]`, and `aborted` (`null`, or an
object naming what stopped the whole command).

**One shape rule governs every object in the document:** an object carries
exactly the fields its own variant lists, and never a field of another
variant. A field its variant lists that has no value on this occasion is
present and `null` — never absent. An object's variant is its `kind`
(`pin.kind`, `refusals[].kind`, `aborted.kind`), its `state` (`drift.state`,
`behind.state`), or, for `behind.newer`, the pin's own kind. The document's
own top level is one variant, so `aborted` is always present, `null` or not.

**Every path is relative to the workspace root** (a bone's `path`,
`overlap`'s `paths`, `manifest`, a path pin's own `path`, `unsafe-path`'s
`at` and `on_disk`), `/`-separated, and the document otherwise carries no
absolute path of its own — with one stated exception. A path pin's own
directory (`pin.path` for `"kind": "path"`) is shown relative to the
workspace root only when the two share a real directory below the
filesystem root, the common case of a sibling checkout
(`../ci-skeleton`). When the only thing they share is the filesystem root
itself, the relative form would carry the same machine-specific directories
the rest of this rule exists to hide — a home directory's own user name
among them — while being harder to read for it, so that one path is shown
absolute instead, exactly as the operating system gives it. There is no
`workspace_root` field: the document is collected from many machines, an
absolute root differs on every clone, and it identifies nothing the
collector did not already know when it ran the command. One path is
relative to something other than the workspace: a `skeleton-invalid`
refusal's `file` is relative to the skeleton's own directory
(`files/clippy.toml`), exactly as the render names it.

**Prose is one line; data is exact.** `message` (a refusal's and an abort's)
and every `detail` (`pin.detail`, `behind.detail`, a refusal's `detail`) are
one line, with any text from outside in them escaped as
[skeleton-format.md](skeleton-format.md) defines, so a newline in a name reads
`\n`. Every other field carries the exact string, however the repository
spelled it, which may hold a newline: `manifest`, `dependency`, `path`,
`paths`, `at`, `on_disk`, a `skeleton-invalid` refusal's `file`, and a pin's
`url`, `tag`, `branch`, `rev` and `source`, as well as `newer`'s `tag` and
`branch`.

`behind` is `{"state": "current"}`, `{"state": "pinned"}`,
`{"state": "behind", "newer": {…}, "detail": "…"}`, or
`{"state": "undetermined", "reason": "…", "detail": "…"}` — the same
`reason` codes as the [Behind](#behind) table above. `newer`'s own key names
what it is: `{"version": "…"}` (registry), `{"tag": "…"}` (a `tag =` pin),
`{"commit": "…"}` (a `branch =` pin — its own branch name is already known
from the pin, so `newer` never repeats it), or, for the one pin whose own
name a wearer never wrote down, `{"commit": "…", "branch": "…"}` (the
default branch, when the remote names it) or `{"commit": "…", "branch":
null}` (when it does not — the shape rule's own `null`, not an absent key).

### Versioning

A reader checks `format_version` is one it knows and ignores any field it
does not — a later release may add a field, or a new value to an *open*
one (`pin.kind`, `drift.reason`, `behind.newer`'s own keys, `behind.reason`,
`refusals[].kind`, `unsafe-path`'s `cause`, `aborted.kind`) — but never
removes or repurposes one within a version. This is deliberate: a fleet
reader collects documents from repositories wearing different versions of
`skeletons`, so every fact a reader decides on (`drift.state`, `behind.state`,
`summary`) stays closed and answerable, while the detail behind it can grow
without forcing every reader to catch up first.

The full document, for the same four-skeleton repository the human example
above describes:

<!-- example: check-json -->
```json
{
  "format_version": 1,
  "filters": [],
  "summary": {
    "bones": 4,
    "matches": 2,
    "drifted": 2,
    "skeletons": 4,
    "current": 1,
    "behind": 1,
    "pinned": 1,
    "undetermined": 1,
    "refusals": 2,
    "failed": true
  },
  "skeletons": [
    {
      "manifest": "Cargo.toml",
      "dependency": "dependabot",
      "skeleton": "a-dependabot-skeleton",
      "version": "0.1.2",
      "pin": {
        "kind": "registry",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "detail": "crates.io"
      },
      "behind": {
        "state": "behind",
        "newer": {
          "version": "0.2.0"
        },
        "detail": "0.2.0 is available"
      },
      "refused": false
    },
    {
      "manifest": "Cargo.toml",
      "dependency": "lint",
      "skeleton": "lint-skeleton",
      "version": "0.4.0",
      "pin": {
        "kind": "branch",
        "url": "https://github.com/acme/lint-skeleton",
        "branch": "main",
        "commit": "3f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39",
        "detail": "branch main of https://github.com/acme/lint-skeleton at 3f2a9c1"
      },
      "behind": {
        "state": "undetermined",
        "reason": "unreachable",
        "detail": "could not reach https://github.com/acme/lint-skeleton: unable to access 'https://github.com/acme/lint-skeleton/': Could not resolve host: github.com"
      },
      "refused": true
    },
    {
      "manifest": "Cargo.toml",
      "dependency": "toolchain",
      "skeleton": "toolchain-skeleton",
      "version": "0.3.0",
      "pin": {
        "kind": "tag",
        "url": "https://github.com/acme/toolchain-skeleton",
        "tag": "v0.3.0",
        "commit": "8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f",
        "detail": "tag v0.3.0 of https://github.com/acme/toolchain-skeleton"
      },
      "behind": {
        "state": "current"
      },
      "refused": false
    },
    {
      "manifest": "tools/Cargo.toml",
      "dependency": "ci",
      "skeleton": "ci-skeleton",
      "version": "0.1.0",
      "pin": {
        "kind": "path",
        "path": "../ci-skeleton",
        "detail": "path ../ci-skeleton"
      },
      "behind": {
        "state": "pinned"
      },
      "refused": false
    }
  ],
  "bones": [
    {
      "path": ".github/dependabot.yml",
      "manifest": "Cargo.toml",
      "dependency": "dependabot",
      "skeleton": "a-dependabot-skeleton",
      "version": "0.1.2",
      "pin": {
        "kind": "registry",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "detail": "crates.io"
      },
      "drift": {
        "state": "drifted",
        "reason": "changed"
      },
      "behind": {
        "state": "behind",
        "newer": {
          "version": "0.2.0"
        },
        "detail": "0.2.0 is available"
      }
    },
    {
      "path": "rust-toolchain.toml",
      "manifest": "Cargo.toml",
      "dependency": "toolchain",
      "skeleton": "toolchain-skeleton",
      "version": "0.3.0",
      "pin": {
        "kind": "tag",
        "url": "https://github.com/acme/toolchain-skeleton",
        "tag": "v0.3.0",
        "commit": "8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f",
        "detail": "tag v0.3.0 of https://github.com/acme/toolchain-skeleton"
      },
      "drift": {
        "state": "matches"
      },
      "behind": {
        "state": "current"
      }
    },
    {
      "path": "rustfmt.toml",
      "manifest": "Cargo.toml",
      "dependency": "toolchain",
      "skeleton": "toolchain-skeleton",
      "version": "0.3.0",
      "pin": {
        "kind": "tag",
        "url": "https://github.com/acme/toolchain-skeleton",
        "tag": "v0.3.0",
        "commit": "8c1e2f4a6b7d9e0f1a2b3c4d5e6f7a8b9c0d1e2f",
        "detail": "tag v0.3.0 of https://github.com/acme/toolchain-skeleton"
      },
      "drift": {
        "state": "matches"
      },
      "behind": {
        "state": "current"
      }
    },
    {
      "path": ".github/workflows/ci.yml",
      "manifest": "tools/Cargo.toml",
      "dependency": "ci",
      "skeleton": "ci-skeleton",
      "version": "0.1.0",
      "pin": {
        "kind": "path",
        "path": "../ci-skeleton",
        "detail": "path ../ci-skeleton"
      },
      "drift": {
        "state": "drifted",
        "reason": "missing"
      },
      "behind": {
        "state": "pinned"
      }
    }
  ],
  "refusals": [
    {
      "kind": "names-no-dependency",
      "manifest": "tools/Cargo.toml",
      "dependency": "dependabto",
      "message": "[package.metadata.skeletons.dependabto] in tools/Cargo.toml names no dependency of tools/Cargo.toml; add the dependency, or rename the table to the key the dependency is declared under"
    },
    {
      "kind": "skeleton-invalid",
      "manifest": "Cargo.toml",
      "dependency": "lint",
      "skeleton": "lint-skeleton",
      "version": "0.4.0",
      "file": "files/clippy.toml",
      "line": 3,
      "detail": "placeholder `cadense` names no declared option",
      "message": "lint in Cargo.toml (lint-skeleton 0.4.0): files/clippy.toml:3: placeholder `cadense` names no declared option; this is a defect in lint-skeleton 0.4.0, not in this repository"
    }
  ],
  "aborted": null
}
```

`skeletons` is in manifest-then-dependency order; `bones` follows the same
skeleton order, and is in path order within each skeleton; `refusals` is by
kind, then message. Within an object, fields are emitted in the order shown
above, and that order is not itself part of the contract.

A lockfile `skeletons` cannot read aborts the whole command, `--json` included:

<!-- example: check-json-aborted -->
```json
{
  "format_version": 1,
  "filters": [],
  "summary": {
    "bones": 0,
    "matches": 0,
    "drifted": 0,
    "skeletons": 0,
    "current": 0,
    "behind": 0,
    "pinned": 0,
    "undetermined": 0,
    "refusals": 0,
    "failed": true
  },
  "skeletons": [],
  "bones": [],
  "refusals": [],
  "aborted": {
    "kind": "lockfile",
    "message": "Cargo.lock is missing or out of date, and `skeletons` reads it without ever writing it; run `cargo update --workspace`, then run the `check` task again"
  }
}
```

A workspace wearing no skeletons at all is the same shape with
`aborted: null` and `failed: false` — `skeletons`, `bones` and `refusals`
all empty, every count zero.

`aborted.kind` (open — see [Versioning](#versioning) above) names why the
workspace itself could not be read at all, before any survey of what it
wears was possible — the same `kind` values [Refusals](#refusals) gives in
full below. `message` carries the same text either command prints to
`stderr` on this same abort, naming the command that failed and, where
there is one, what to run next.

### Every object, field by field

Each object's fields are listed in the order they are emitted. *Open*
fields may gain values within format version 1 (see
[Versioning](#versioning)).

- **`filters`**: `[]`, `["drifted"]`, `["behind"]`, or
  `["drifted", "behind"]` — the flags that narrowed `bones[]`. Nothing else
  is ever filtered.
- **`summary`**: `bones`, `matches`, `drifted` (bones, by drift state);
  `skeletons`, `current`, `behind`, `pinned`, `undetermined` (worn
  skeletons, by behind state); `refusals`; and `failed`, the exit status as
  a boolean. Every count is over everything, whatever the filters:
  `matches + drifted` is `bones`, and the four behind counts sum to
  `skeletons`.
- **`skeletons[]`**: `manifest`, `dependency`, `skeleton` (the skeleton's
  own crate name, the name its refusals are about), `version`, `pin`,
  `behind`, `refused`.
- **`bones[]`**: `path`, `manifest`, `dependency`, `skeleton`, `version`,
  `pin`, `drift`, `behind`. `manifest` and `dependency` together name the
  one `skeletons[]` entry this bone belongs to; `skeleton`, `version`, `pin`
  and `behind` repeat that entry's own, so each bone carries both facts on
  its own.
- **`pin`** (`kind` open; `detail` is always the words the human header
  prints after `from`; every `commit` is a git object id as cargo locked
  it — 40 lowercase hex digits, or 64 in a SHA-256 repository — and a git
  source whose commit is anything else reads as `unrecognised`; the same
  holds for `behind.newer`'s `commit`):
  - `{"kind": "registry", "source": "registry+https://github.com/rust-lang/crates.io-index", "detail": "crates.io"}`
  - `{"kind": "registry", "source": "sparse+https://example.invalid/index/", "detail": "registry sparse+https://example.invalid/index/"}`
  - `{"kind": "tag", "url": "…", "tag": "v0.3.0", "commit": "<40 hex>", "detail": "tag v0.3.0 of …"}`
  - `{"kind": "branch", "url": "…", "branch": "main", "commit": "<40 hex>", "detail": "branch main of … at <7 hex>"}`
  - `{"kind": "default-branch", "url": "…", "commit": "<40 hex>", "detail": "the default branch of … at <7 hex>"}`
  - `{"kind": "rev", "url": "…", "rev": "<as written>", "commit": "<40 hex>", "detail": "rev <as written> of …"}`
  - `{"kind": "path", "path": "../ci-skeleton", "detail": "path ../ci-skeleton"}`
  - `{"kind": "unrecognised", "source": "<cargo's source id>", "detail": "<cargo's source id>"}`
- **`drift`** (`state` closed; `reason` open):
  - `{"state": "matches"}`
  - `{"state": "drifted", "reason": "changed"}`
  - `{"state": "drifted", "reason": "missing"}`
- **`behind`** (`state` closed; `newer`'s keys and `reason` open):
  - `{"state": "current"}`
  - `{"state": "pinned"}`
  - `{"state": "behind", "newer": {"version": "0.2.0"}, "detail": "0.2.0 is available"}`
  - `{"state": "behind", "newer": {"tag": "v0.4.0"}, "detail": "tag v0.4.0 is available"}`
  - `{"state": "behind", "newer": {"commit": "<40 hex>"}, "detail": "main is at <7 hex>"}` (a `branch =` pin)
  - `{"state": "behind", "newer": {"commit": "<40 hex>", "branch": "trunk"}, "detail": "trunk is at <7 hex>"}` (the default branch, named by the remote)
  - `{"state": "behind", "newer": {"commit": "<40 hex>", "branch": null}, "detail": "the default branch is at <7 hex>"}` (the default branch, unnamed)
  - `{"state": "undetermined", "reason": "<reason>", "detail": "<text>"}`
- **`refusals[]`** (`kind` open; `kind` first and `message` last on every
  one):
  - `not-a-table`: `manifest`, `dependency` (`null` for the whole
    `[package.metadata.skeletons]` table), `message`
  - `reserved-key`: `manifest`, `dependency` (the reserved key it was, `"options"` or `"verbatim"`), `message`
  - `names-no-dependency`: `manifest`, `dependency`, `message`
  - `unresolved`: `manifest`, `dependency`, `message`
  - `ambiguous`: `manifest`, `dependency`, `packages` (`[{"name", "version"}, …]`), `message`
  - `not-a-skeleton`: `manifest`, `dependency`, `package` (`{"name", "version"}`), `message`
  - `option-refused`: `manifest`, `dependency`, `skeleton`, `version`, `detail`, `message`
  - `skeleton-invalid`: `manifest`, `dependency`, `skeleton`, `version`,
    `file`, `line` (`null` when the refusal names no line), `detail`,
    `message`
  - `overlap`: `paths`, `claimants` (`[{"manifest", "dependency", "skeleton", "version"}, …]`), `message`
  - `unsafe-path`: `manifest`, `dependency`, `skeleton`, `version`, `path`,
    `cause` (open), `at`, `on_disk`, `message` — see
    [Refusals](#refusals) for when `at` and `on_disk` are `null`
- **`aborted`** (`kind` open): `null`, or `{"kind", "message"}`.

## Refusals

Every message begins with the thing to fix, says what is wrong, and ends
with what to do where there is something to do. Every message prints the text
it echoes from outside (a key, a manifest, a path, a url, a tag, cargo's own
words) escaped, so it is one line whatever that text holds; the escape is the
one [skeleton-format.md](skeleton-format.md)'s Refusals section defines, and
a name with a newline in it reads `first\nsecond`, with a backslash and an
`n`.
[skeleton-format.md](skeleton-format.md)'s own refusals table is not
repeated here: a skeleton's own defect (naming a file) reaches a wearer as
`skeleton-invalid`; an undeclared or malformed choice the render itself
catches (naming no file) reaches a wearer as `option-refused`, including an
empty `text` value or one holding a control character, and choices whose
render exceeds the size limit.

| What is wrong | `kind` | About |
|---|---|---|
| `[package.metadata.skeletons]`, or one of its keys, is not a table | `not-a-table` | no single skeleton |
| a real dependency is declared under a reserved key, `options` or `verbatim` | `reserved-key` | no single skeleton |
| a wearing table's key names no dependency of that manifest | `names-no-dependency` | no single skeleton |
| the declared dependency resolved to no locked package | `unresolved` | no single skeleton |
| the declared dependency resolved to more than one locked package | `ambiguous` | no single skeleton |
| the resolved package is not itself a skeleton | `not-a-skeleton` | no single skeleton |
| a recorded option value's own TOML shape is not a string or an array of strings | `option-refused` | one worn skeleton |
| the skeleton's own render refused, naming a file | `skeleton-invalid` | one worn skeleton |
| the skeleton's own render refused an undeclared or malformed choice, or choices whose render exceeds the size limit | `option-refused` | one worn skeleton |
| two or more bones collide | `overlap` | no single skeleton |
| a claimed path is unsafe to resolve (a symlink at or above it, `.git` or another name git refuses to track, inside another repository, a name too long, not a regular file, spelled differently on disk) | `unsafe-path` | one worn skeleton |

A refusal "about one worn skeleton" sits inside that skeleton's own block,
and its own `refused` flag is set in `--json`'s `skeletons[]`; the rest sit
in the closing `refused:` list, since no single skeleton can be blamed for
them.

`unsafe-path`'s own `cause` values:

| `cause` | Means | `at` | `on_disk` |
|---|---|---|---|
| `spelled-differently` | the claimed path, or a directory above it, is listed on disk under a different spelling | the claimed path up to and including the differing component | the on-disk spellings found, sorted |
| `symbolic-link-above` | a directory above the claimed path is a symbolic link | that directory | — |
| `not-a-directory-above` | a directory above the claimed path exists but is not a directory | that path | — |
| `symbolic-link` | the claimed path itself is a symbolic link | — | — |
| `not-a-file` | the claimed path exists but is not a regular file | — | — |
| `inside-git-directory` | the claimed path has a component that is `.git` in any ASCII case; a defect in the skeleton, not the repository | — | — |
| `untrackable-name` | a component of the claimed path is a name git refuses to track (see above), though not `.git` itself; a defect in the skeleton, not the repository | the claimed path up to and including that component | — |
| `inside-another-repository` | a directory between the workspace root and the claimed path holds a `.git` entry of its own (a directory, or a file as a submodule or a linked worktree has), so the path lies in another repository | that directory | — |
| `name-too-long` | a component of the claimed path is more than 255 bytes, or the last is more than 239, so `sync` could not stage it as `.<name>.skeletons-sync`; a defect in the skeleton, not the repository | the claimed path up to and including that component | — |
| `unreadable` | some other I/O error prevented reading the path | — | — |

`at` is `null` where the table shows —. The text of a `spelled-differently`
refusal and of any message that names two spellings of one name, when the
two differ only in Unicode normalization, follows each spelling, escaped as
any text from outside is, with its non-ASCII characters as `\uXXXX`
(`\UXXXXXXXX` above U+FFFF) code points, and says why:
`café.yml (caf\u00E9.yml) is spelled café.yml (cafe\u0301.yml) on disk (the two
spellings differ only in Unicode normalization, so each is followed by its
characters as \uXXXX code points, as bash 4.3 or later, or zsh, reads them in
$'…' under a UTF-8 locale); … ; rename …`. `--json` carries the exact strings in `at` and `on_disk`; its
`message` is the escaped text above. `on_disk` is `null` for every
cause but `spelled-differently`, where it is an array — empty when only a
filesystem lookup, never the directory listing, found the difference (one
the fold the listing compares by does not reach, such as an ignorable code
point some filesystems skip).

The whole command aborts, rather than reporting anything about individual
bones, when the workspace itself could not be read at all — never read as
"wears nothing":

| `kind` | Means |
|---|---|
| `lockfile` | `Cargo.lock` is missing or out of date, and `--locked` refused to write one; without it, no skeleton's locked version can be identified at all |
| `cargo-metadata-failed` | `cargo metadata` ran and exited unsuccessfully, for any other reason |
| `cargo-unavailable` | `cargo` (or whatever `$CARGO` names) could not be run at all |
| `metadata-unreadable` | the output was too large, not valid JSON in the expected shape, or named a format version this crate does not understand |

`sync` aborts the same way, for the same four causes, before writing
anything — its own work-tree checks (see [Sync](#sync)) only ever run
once the workspace itself has been read successfully. `wear` aborts the same
way too, with `check`'s wording for a `cargo metadata` failure and its own for
the lockfile (see [Wear](#wear), which also holds the refusals only `wear`
makes).
