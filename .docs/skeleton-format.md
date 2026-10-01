# The skeleton format

A reference for writing a skeleton: the layout of its crate, its manifest
schema, the fill and select grammars, and every way a render can refuse. The
reasons behind each rule are in [design.md](design.md); this document is
what a skeleton author works from directly.

## Layout

A skeleton is an ordinary Cargo crate with two extra directories:

```text
a-skeleton/
  Cargo.toml          # [package], and [package.metadata.skeletons]
  src/lib.rs          # required by Cargo to package a crate; holds no logic
  files/              # whole files a wearing repository should hold
  partials/           # fragments a `set` option's directives insert
```

`src/lib.rs` exists only because Cargo will not package a crate with no
target — its content is never read by a render. `Cargo.toml` must be a
regular file, not a symbolic link, and its `package.name` must be
non-empty. `files/` must exist and hold at least one file; `partials/`
may be absent entirely if the skeleton declares no `set` options.

No two entries in one directory under `files/` or `partials/` may have
names that are one name to a filesystem ignoring case and Unicode
normalization — equal once decomposed (NFD), case-folded, and decomposed
again: `Dependabot.yml` beside `dependabot.yml`, or `é.txt` written
precomposed beside `é.txt` written as `e` and a combining accent. A default
macOS volume holds only one of each pair, so the skeleton would render
differently there than on Linux. Each directory is checked on its own, so
`files/A/x` and `files/a/y` collide at `A` and `a`; the two trees are
checked separately, so `files/x` and `partials/x` never collide.

Nothing under `files/` or `partials/`, at any depth, may be named
`Cargo.toml` — file or directory — nor any name that is one name with it by
the rule above, such as `cargo.toml`. Cargo takes a directory holding one
for a package of its own and leaves the whole directory out of the skeleton's
package, so the skeleton would render one way from its author's checkout and
another from a registry.

**Cargo can leave a skeleton's files out of its package without a word.** A
wearer of a registry dependency renders the package, not the author's
checkout, so a file the package lacks is missing for every wearer while the
author's own render succeeds. What Cargo leaves out depends on where the crate
is packaged:

- **Outside a git repository, or inside one that does not yet track the
  skeleton's `Cargo.toml`**, such as straight after `git init` or when a new
  skeleton is added to a repository that holds others, Cargo walks the
  directory itself and skips every name that begins with `.`, at any depth. A
  skeleton's `files/.github/dependabot.yml`, `files/.env.example` and
  `files/dir/.dot.yml` are all left out, and so is everything under a
  directory such as `files/.hidden-dir/`. Cargo prints no warning. Inside a
  repository, where `cargo package` first refuses the uncommitted files, it
  names only the ones not beginning with `.`, and it drops the dotfiles
  silently once `--allow-dirty` is passed.
- **Inside a git repository that tracks the skeleton's `Cargo.toml`**, Cargo
  lists what git tracks, dotfiles included, and leaves out whatever git
  ignores. With
  `.env.*` in a `.gitignore` in the skeleton's directory or any directory above
  it, `files/.env.example` is left out, again with no warning. An untracked file
  that is not ignored does not slip through this way: `cargo package` stops with
  an error naming it, until it is committed or `--allow-dirty` is passed.
- **`package.include`** changes both cases. With
  `include = ["src/**", "files/**", "partials/**"]`, Cargo packages dotfiles
  outside a git repository too. Inside one, an ignored file that `include`
  names is no longer dropped silently: `cargo package` stops, naming it as not
  committed, until it is added with `git add -f` and committed. `exclude` only
  removes files. It brings none back.

Before every publish, compare what `files/` and `partials/` hold with what the
package would contain. From the skeleton's directory, on a committed tree if it
is in a git repository:

```sh
package=$(mktemp) && on_disk=$(mktemp) &&
  cargo package --list > "$package" &&
  find files partials -type f > "$on_disk" &&
  { grep -vxFf "$package" "$on_disk"; [ "$?" -eq 1 ]; }
```

It exits 0 when the package holds every file, and prints each path that is on
disk and missing from the package, exiting 1, when it does not. `grep` exits 1
when it finds nothing to print, which is the one answer that means every file
is packaged; the last line turns that into success and everything else,
`grep`'s own errors included, into failure. Any other failure stops it with a
non-zero status before anything is compared: `cargo package --list` refuses
an uncommitted file, for one, and its error is then the answer. A skeleton
with no `partials/` leaves that word out of the `find`, which otherwise fails
on the missing directory. The listings are written outside the skeleton's
directory because, in a git repository, a file written inside it would itself
be uncommitted.

## The manifest

A worked example, for a skeleton that manages a dependabot configuration:

```toml
[package]
name = "a-dependabot-skeleton"
version = "0.1.0"
edition = "2024"
description = "A dependabot configuration, with a cadence, an assignee and the ecosystems to update"
license = "MIT"

# Present even when a skeleton declares no options: the
# `[package.metadata.skeletons]` table is what marks a crate as a skeleton at
# all, and this sub-table creates it implicitly.
[package.metadata.skeletons.options.cadence]
type = "enum"                            # fills: {{cadence}} becomes the chosen value
values = ["daily", "weekly", "monthly"]  # the closed set a wearer chooses from
default = "weekly"                       # required, and one of `values`

[package.metadata.skeletons.options.ecosystems]
type = "set"                             # selects: `# skeletons:partial ecosystems` lines
values = [                               # the closed set, in the order partials render
    { value = "cargo", partial = "cargo.yml" },
    { value = "github-actions", partial = "github-actions.yml" },
]
default = ["cargo"]                      # required (may be []), each one of `values`

[package.metadata.skeletons.options.assignee]
type = "text"                            # fills: {{assignee}} becomes the wearer's own text
                                         # no `default`, so it is optional: unset drops its lines
```

`files/.github/dependabot.yml`:

```yaml
version: 2
updates:
  # skeletons:partial ecosystems
```

`partials/cargo.yml` (written flush-left):

```yaml
- package-ecosystem: "cargo"
  directory: "/"
  schedule:
    interval: "{{cadence}}"
  assignees: ["{{assignee}}"]
```

`partials/github-actions.yml` (written flush-left):

```yaml
- package-ecosystem: "github-actions"
  directory: "/"
  schedule:
    interval: "{{cadence}}"
  assignees: ["{{assignee}}"]
```

Rendered with the defaults, `.github/dependabot.yml`'s `updates:` line is
followed by `cargo.yml`'s own content, indented to match the directive
(here, two spaces), with `{{cadence}}` replaced by `weekly`. The wearer has
stated nothing for `assignee`, so the `assignees:` line is dropped, terminator
and all:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
```

Rendered with `assignee = "octocat"`, the same line is kept and filled:

```yaml
    assignees: ["octocat"]
```

The flow form, `assignees: ["{{assignee}}"]`, is what makes the line droppable.
The block form, an `assignees:` line above a `- "{{assignee}}"` line, would
leave a bare `assignees:` behind when the option is unset, because only the
line holding the placeholder is dropped; and a line that also held a required
value would drop that value without saying so, which is why an optional
placeholder shares its line with no other option's (see
[Fill grammar](#fill-grammar)).

**Skeletons guarantees the file matches the render. It does not guarantee the
render is valid YAML, TOML, or anything else.** A `text` value goes in
verbatim, so the quotes around `{{assignee}}` above are the skeleton's, and a
wearer whose value holds a quote breaks the file the way any hand-edit would.

### The rules, in full

- **`[package.metadata.skeletons]` must exist.** A crate with no such table is
  not a skeleton at all. It may hold only two keys, `options` and `verbatim`,
  each of which may be absent or empty.
- **An option** (a key under `options`) of type `enum` or `set` holds exactly
  `type`, `values` and `default` — all three required. One of type `text`
  holds `type` and an optional `default`, and nothing else: `values` is a key
  it does not take. Any other key at any level is refused, naming its dotted
  path from the manifest's own root (for example,
  `package.metadata.skeletons.options.cadence.defualt` for a misspelt
  key).
- **`type`** is `"enum"`, `"set"` or `"text"`. Nothing else.
- **An option's own name** (the key under `options`) follows the [name
  grammar](#name-grammar): `cadence`, `github-actions`, `v2` are names;
  `Cadence`, `2fa`, `x-`, `snake_case` are not.
- **A value** is a non-empty string holding no control character. `values`
  must hold at least one, and no value may be declared twice within one
  option. The same rule governs a `text` option's `default` and the text a
  wearer supplies for one.
- **An `enum` option's** `values` is an array of strings; `default` is one
  of them.
- **A `text` option** fills a placeholder with the wearer's own words, taken
  verbatim, with no pattern and no length bound beyond the value rule above.
  Its `default`, when it has one, is a string; the option is then required like
  an `enum`. With no `default` it is optional: unset, every line holding its
  placeholder is dropped whole, terminator included.
- **A `set` option's** `values` is an array of `{ value, partial }` tables;
  `partial` is compared, exactly and as written, against the path (relative
  to `partials/`, `/`-separated) of a file the skeleton actually ships there.
  `default` is an array of strings, each one of `values`, none repeated;
  `[]` selects nothing. The order `values` is written in is the order
  selected partials render in — never the order a wearer lists them.
- **Partials and values are one-to-one.** Every file under `partials/`, at
  any depth, must be named by exactly one `set` value across every option;
  a partial two values both name, or a partial no value names, is refused.
- **Every declared option must be used by something in the skeleton.** An
  `enum` or a `text` needs at least one placeholder naming it, anywhere in
  `partials/` or in a file under `files/` not declared verbatim; a `set` needs
  at least one directive naming it, in a file under `files/` not declared
  verbatim (a partial can never hold a directive, so there is nowhere else for
  a set option to be reached from). Nothing in a verbatim file is read, so
  nothing in one uses an option.
- **`verbatim`** is an array of strings, each the path of one file under
  `files/`, relative to `files/` and `/`-separated
  (`".github/workflows/ci.yml"`): the spelling that file lands under in a
  wearing repository and is keyed by in a render. `[]` is the same as no
  `verbatim` at all, and the order of the entries does not matter. Each
  entry is compared, exactly as written, with the paths `files/` holds: no
  `./`, no trailing `/`, no case folding, no Unicode folding. An entry must
  name a file, and none may be listed twice. A directory holding files is
  refused, with a note to list each file under it instead, because a
  directory would quietly make a file added there later verbatim. A partial
  is refused, spelled relative to `partials/` (`cargo.yml`) or as it sits in
  the tree (`partials/cargo.yml`), because only a file under `files/` can be
  verbatim. Any other path is refused as naming no file. A declaration
  nothing needed is not refused: the file is claimed as written all the same.
  See [Verbatim files](#verbatim-files).

A repository that *wears* a skeleton uses this same
`[package.metadata.skeletons]` table namespace, keyed by dependency
instead of by `options` — see [wearing.md](wearing.md). One consequence: a
skeleton can never also wear a skeleton, since its
`[package.metadata.skeletons]` may hold only `options` and `verbatim`.

### Verbatim files

Every `{{` in a file opens a placeholder, so a file holding a `{{` that is not
one cannot be written as a template. A GitHub Actions workflow is the common
case: its expressions, `${{ secrets.TOKEN }}` and `${{ github.ref }}`, have
exactly the shape a placeholder has. A skeleton claims such a file by
declaring it verbatim, and a verbatim file gets no fill and no select: a
render yields its bytes, exactly as written.

A skeleton that ships one workflow, and nothing else:

```toml
[package]
name = "a-ci-skeleton"
version = "0.1.0"
edition = "2024"
description = "A CI workflow"
license = "MIT"

# No options: this skeleton has nothing to choose, only a file to claim.
[package.metadata.skeletons]
verbatim = [".github/workflows/ci.yml"]
```

`files/.github/workflows/ci.yml`:

```yaml
name: ci
on: [push]
jobs:
  test:
    runs-on: ubuntu-latest
    env:
      TOKEN: ${{ secrets.TOKEN }}
    steps:
      - env:
          REF: ${{ github.ref }}
        run: echo "$REF"
```

The render holds `.github/workflows/ci.yml` as those bytes and nothing else, and
a repository wearing the skeleton claims the file at that path
(see [wearing.md](wearing.md)).

Nothing in a verbatim file is read as text. A placeholder in it fills nothing,
a directive-shaped line in it selects nothing, and no line of it is refused as
a hidden directive. So an option whose only placeholder or directive sits in a
verbatim file is used by nothing, and is refused as unused. A declaration is
honoured whether or not the file needed it. A verbatim file is read against the
same read budget as any other, and its length counts toward both render size
limits, the same under every choice.

## Name grammar

The same grammar names an option everywhere one is written: the manifest's
own option keys, a `{{name}}` placeholder, and a `# skeletons:partial name`
directive. A name is a lowercase ASCII letter, then any number of lowercase
ASCII letters, digits and hyphens, never ending in a hyphen.

| Accepted | Refused | Why refused |
|---|---|---|
| `cadence` | `Cadence` | uppercase |
| `github-actions` | `2fa` | starts with a digit |
| `v2` | `x-` | ends in a hyphen |
| `a-b-c` | `snake_case` | underscore is not in the grammar |

## Fill grammar

A placeholder is `{{`, immediately followed by a well-formed option name,
immediately followed by `}}`, all on one line, naming a declared `enum` or
`text` option. Every `{{` in a file not declared verbatim, or in a partial,
opens one. A placeholder
naming an optional `text` option must have its line to itself: no
placeholder for any other option may share it.

**A file holding a `{{` that is not a placeholder has to be declared
[verbatim](#verbatim-files).** There is no escape for a literal `{{` in text
that fills, and a partial can never be verbatim, so a partial can never hold
one. A malformed placeholder's refusal says which: in a file, that a `{{` which
is not a placeholder can stand only in a file declared verbatim; in a partial,
that a partial cannot hold one, so the text belongs in a file under `files/`
declared verbatim.

| Text | Result |
|---|---|
| `schedule: {{cadence}}` | fills with the chosen (or default) value |
| `assignees: ["{{assignee}}"]`, where `assignee` is a `text` option | fills with the wearer's text; with an optional `text` unset, the whole line is dropped |
| `["{{assignee}}", "{{assignee}}"]`, where `assignee` is an optional `text` | accepted — one optional twice is still one optional, and the line drops once |
| `{{assignee}}: {{cadence}}`, where `assignee` is an optional `text` | refused — dropping the line would drop `cadence`'s value with it |
| `{{ cadence }}` | refused — spaces inside the braces |
| `{{cadence` (nothing closes it on the line) | refused — unclosed |
| `${{ github.ref }}` | refused outside a verbatim file — a GitHub Actions expression, indistinguishable from a malformed placeholder; see [Verbatim files](#verbatim-files) |
| `{{{cadence}}}` | refused — an extra brace |
| `{{workflows}}`, where `workflows` is a `set` option | refused — names the wrong kind of option |
| `{{cadense}}` (nothing declared by that name) | refused — names no declared option |
| `text }} more text` (no preceding `{{`) | ordinary text — a lone `}}` opens nothing |

## Select grammar

A directive line is one whose text, after its leading spaces and tabs,
begins `# skeletons:`. It must then be exactly `# skeletons:partial `
followed by one well-formed option name and nothing else before its line
terminator, naming a declared `set` option.

A line is also checked *normalised*: with every leading whitespace character
dropped, every format or control character dropped wherever it sits (before
the marker or inside it), and any run of whitespace elsewhere read as at
most one space, does it begin `#`, then at most one space, then `skeletons`,
then at most one space, then `:`? Invisible means Unicode whitespace (the
White_Space property: a no-break space, an ideographic space, a vertical
tab, a form feed, as well as spaces and tabs), general category Cf (format:
a zero-width space, a word joiner, a byte-order mark), or general category
Cc (control: a stray control byte such as `\x01`) — the same set, whether it
sits in front of the marker, or inside it, after `#`, after `# `, or right
before `:`.

A line that reaches this normalised shape only by looking past an invisible
character other than a space or a tab is refused as hidden, on any line, in
a file and in a partial alike: read as text, such a line would drop a
directive its author may have meant; read as a directive, it would guess
what the invisible character was for. A line that reaches it with nothing
invisible in play — only ordinary ASCII spacing wrong, such as no space at
all after `#` — is refused as malformed instead: still a directive its
author meant, never treated as an ordinary comment.

None of this applies to a file declared [verbatim](#verbatim-files): no line of
one is a directive, and none is refused as a hidden one, however it reads.

| Text | Result |
|---|---|
| `  # skeletons:partial ecosystems` | selects `ecosystems`'s chosen partials, indented two spaces |
| `#skeletons:partial ecosystems` | refused — no space after `#`, but the line still normalises to the marker; only ordinary ASCII spacing is wrong |
| `# Skeletons:partial ecosystems` | ordinary text — wrong case, so it normalises to nothing the marker recognises |
| `# skeletons:partail ecosystems` | refused — misspelt keyword |
| `# skeletons:partial` | refused — missing the option name |
| `# skeletons:partial ecosystems docker` | refused — trailing text after the name |
| `# skeletons:partial ecosystems ` (trailing space) | refused — trailing text after the name |
| `# skeletons:partial ecosystems` followed by any invisible character (a no-break space, a control byte) | refused — trailing text after the name |
| `# skeletons:partial cadence`, where `cadence` is an `enum` option | refused — names the wrong kind of option |
| a directive as a file's last line, with no terminator | refused — see [Text rules](#text-rules) |
| a directive-shaped line inside a partial, well-formed or not | refused — a partial cannot hold a directive |
| a no-break space, zero-width space, a stray control byte, a byte-order mark or other invisible character, before the marker or inside it (after `#`, after `# `, or before `:`) | refused — the directive is hidden |

Selecting more than one value inserts each selected partial's file, one
after another, in the option's own declared order — never the order a
wearer supplied them in. Selecting nothing removes the directive line and
inserts nothing in its place, not even a blank line.

## Indentation and blank lines

Every line of a selected partial is prefixed with the directive line's own
leading spaces and tabs, verbatim — however deep or shallow that is,
including none at all. The one exception is a truly blank line (no bytes
before its terminator), which stays blank rather than gaining that
indentation. A line holding only spaces or tabs is not blank by this rule:
it is content, exactly like any other line, and does get the indentation.

## Text rules

A skeleton's files and partials are read as UTF-8; one that cannot be decoded
that way is refused rather than passed through. The exception is a file
declared [verbatim](#verbatim-files), which need not be UTF-8, so a binary file
can be claimed. Beyond that, render
normalizes nothing: bytes are kept exactly as written. A line ends at
`\n` and nowhere else, and every other byte is content; a missing final
newline stays missing, and a byte-order mark is
preserved as ordinary bytes, wherever it is, unless it hides a directive
(see [Select grammar](#select-grammar)). A file with no placeholder and no
directive, and every verbatim file, renders byte-for-byte identical to its
source.

A non-empty partial's last line must end with a line terminator — inserted
anywhere but the very end of a file, an unterminated last line would run
into whatever follows it. An empty partial is valid and inserts nothing. A
directive on a file's own final line, which by definition has no
terminator, is refused for the same reason a partial's own unterminated
last line is.

A file, or a directory, under `files/` or `partials/` that is a symbolic
link is refused, naming it — a skeleton is the bytes it ships, not a pointer to
bytes somewhere else. The same is true of `Cargo.toml` itself: a symbolic
link there could point outside the skeleton, so one skeleton version would read a
different manifest on each machine, and anything that is not a regular
file — a directory, a FIFO, a socket, a device — is refused rather than
opened.

## What a render yields

A successful render is every file under `files/`, at any depth, including
files and directories whose names begin with `.`, keyed by its path
relative to `files/` with `/` separators (`.github/dependabot.yml`,
`a/b/deep.yml`, `root.yml`). Iterating a render's files always yields them
in ascending order of that path string — never the order a filesystem
happened to list them in.

## Refusals

A render returns the *first* refusal it finds, in this order: the manifest
(its `options`, then the shape of its `verbatim` list and any path it lists
twice), the directory walk, the mapping between a `set` option's partials and
what `partials/` actually holds, each `verbatim` path in the order listed,
every file in path order, every partial in path
order, whether every declared option is used by something, whether the
largest render the skeleton can produce fits the render's size limit, then
the wearer's own choices, each in option-name order, then whether the render
they produce fits the size limit. Every refusal names the skeleton it is about;
most name a file, and some name a line within that file — the line the
skeleton's author actually wrote, never a line the assembled output would
occupy. Any text in a refusal from this table that a skeleton's author or a
wearer wrote, or a file system supplied, is printed escaped: a control or
invisible character shows as its escape, a backslash as `\\`, and a quote as
typed. A newline, tab and carriage return are `\n`, `\t` and `\r`; every other
escaped character is `\uXXXX` (four uppercase hex digits) inside the Basic
Multilingual Plane and `\UXXXXXXXX` (eight) above it, so a NUL is `\u0000`
and a zero-width space `\u200B`. Each escape reads the same way in `$'…'` in
bash 4.3 or later, or zsh, under a UTF-8 locale, except that bash cannot hold a
NUL and cuts the text off at `\u0000` where zsh keeps it; a whole message is
not a `$'…'` literal. A combining accent
is escaped only at the start of the text, where it has nothing to combine with.
So each of these refusals is one line, whatever the text it echoes. The same
escape prints the text from outside in every message `check` and `sync` print,
not only these: the names and paths a repository holds, and the words of git
and of cargo.

| What is wrong | Names |
|---|---|
| `Cargo.toml` cannot be read | the unreadable path |
| `Cargo.toml` is not valid TOML | `Cargo.toml`, and the line the parser names, when it names one |
| `Cargo.toml` has no string `package.name` | `Cargo.toml` |
| `Cargo.toml` has an empty `package.name` | `Cargo.toml` |
| `Cargo.toml` is a symbolic link, or not a regular file | `Cargo.toml` |
| `Cargo.toml` has no `[package.metadata.skeletons]` table | `Cargo.toml` |
| the schema holds a key it does not recognise | `Cargo.toml`, the dotted key path |
| a required key is missing | `Cargo.toml`, the dotted key path |
| a key holds the wrong shape of TOML value | `Cargo.toml`, the key and the expected shape |
| an option's own name breaks the name grammar | `Cargo.toml`, the option |
| an option declares a `type` other than `enum`/`set`/`text` | `Cargo.toml`, the option and the type given |
| an option declares an empty `values` | `Cargo.toml`, the option |
| a declared value, or a `text` default, is empty or holds a control character | `Cargo.toml`, the option and value |
| one option declares the same value twice | `Cargo.toml`, the option and value |
| a default names a value the option does not declare | `Cargo.toml`, the option and value |
| a `set` default lists one value twice | `Cargo.toml`, the option and value |
| `verbatim` lists one path twice | `Cargo.toml`, the path |
| a `set` value's `partial` names a file that does not exist | `Cargo.toml`, the option, value and partial |
| one partial is named by more than one value | `Cargo.toml`, the partial |
| a partial under `partials/` is named by no value | `Cargo.toml`, the partial |
| a `verbatim` path is a directory holding files | `Cargo.toml`, the path |
| a `verbatim` path names a partial | `Cargo.toml`, the path |
| a `verbatim` path names no file under `files/`, and no partial | `Cargo.toml`, the path |
| a declared option is used by nothing in the skeleton | `Cargo.toml`, the option |
| `files/` is absent, or holds no file at all | `files` |
| `files/` or `partials/` exists but is not a directory | `files` or `partials` |
| a file or directory under either tree is a symbolic link | the link itself |
| an entry is neither a file, a directory, nor a symbolic link | the entry |
| an entry's own name is not valid UTF-8 | the entry, named lossily |
| two entries in one directory are one name once case and Unicode normalization are ignored | the first of the two in path order, and the other |
| an entry under either tree is named `Cargo.toml`, or a name that is one name with it | the entry |
| the entries listed across both trees exceed the entry budget | the directory whose listing went past it |
| reading a file (including `Cargo.toml`) takes the skeleton past its shared read budget | the file being read when the budget ran out |
| a file not declared verbatim, or a partial, cannot be decoded as UTF-8 | the file |
| `{{` opens without a well-formed name and `}}` on the same line | the file and line |
| a well-formed placeholder does not name a declared `enum` or `text` option | the file and line |
| a placeholder for an optional `text` shares its line with a placeholder for another option | the file and line |
| a `# skeletons:` line is not exactly `# skeletons:partial <name>` | the file and line |
| a well-formed directive does not name a declared `set` option | the file and line |
| a directive line appears inside a partial | the partial and line |
| a line reads as a directive only once invisible characters before it are set aside | the file or partial, and line |
| a directive sits on a file's last, unterminated line | the file and that line |
| a non-empty partial's last line has no terminator | the partial and its last line |
| the largest render any choice could produce exceeds the size limit | the first file, in path order, at which it does |
| the wearer set a value for an undeclared option | neither — about the choice, not a file |
| the wearer's choice is the wrong shape for the option's kind | neither |
| the wearer's text for a `text` option is empty or holds a control character | neither |
| the wearer chose a value outside the option's declared set | neither |
| the wearer's `set` choice named the same value twice | neither |
| the render the wearer's choices produce exceeds the size limit | neither — the `text` option the wearer set that contributes the most bytes, the first by name on a tie |

Two of these deserve a note. The byte budget is one pool shared across
`Cargo.toml`, every file, and every partial in a single render — a refusal
names whichever one was being read when the render's total allowance ran
out, not a per-file limit. And the two rows for a well-formed
placeholder or directive that does not name a declared option of the right
kind cover two distinct cases: a name that matches nothing declared at
all, and a name that matches a real option of the *other* kind (a
placeholder naming a `set`, a directive naming an `enum` or a `text`) —
both are refused the same way, since neither is what the skeleton's author
asked for.
