# Skeletons — design

The design as it stands: the principles `skeletons` is built on, the shape they
give it, and the reason for each decision. The skeleton format itself — the
manifest schema, the fill and select grammars, every refusal — is in
[skeleton-format.md](skeleton-format.md), which a skeleton author works from directly.
This document is the reasons behind that format, and the parts of the design
that reach beyond this release.

## Purpose

Setting a repository up happens once: a toolchain pin is chosen, a lint
table is written, a CI workflow is copied in, a licence is dropped in place.
Drift happens across every repository that started that way, forever, and it
compounds — the toolchain pin is bumped in one place and not another, one
repository's dependabot cadence quietly diverges from its siblings', and
nothing notices until two repositories that were meant to agree plainly do
not. `skeletons` is for the forever part: it keeps a repository's
configuration from drifting away from what it is meant to look like.

## Scope and status

`skeletons` is a bundle of tasks for
[ritual](https://github.com/hexlace/ritual). It has no binary of its own: a
project adds it to its own ritual command line the way it would add any
other bundle, and depends on `rituals` from crates.io the same way any of
ritual's users does.

`skeletons` is the middle of three layers. **Ritual** is the runner. It
gives every project its own command line and knows nothing about drift.
**`skeletons`** is a bundle of rituals for skeletons: checking what a
repository wears, and making it true. The third layer is **the skeletons
themselves**, the actual content a team wears. That content lives wherever
its owners keep it: an organisation's shared repository, an open-source
crate, or a path dependency inside one project. Moving a skeleton between
those is a move, not a redesign; repoint the dependency and nothing else
changes.

`skeletons` stays in its own repository, apart from ritual, because the
larger promise would swallow the smaller one. It depends on `rituals` the
way any stranger does, which also makes it the best test ritual gets.

The render is the core: given a skeleton's directory and the option values a
wearer chose, it produces the exact bytes every file that skeleton ships should
hold, or refuses, naming what is wrong. `check` reads a repository's own
wearing tables and its lockfile, fresh on every run, and reports — for
every bone — whether its file matches that render or has drifted,
distinguishably missing or changed, and — for every worn skeleton — whether
it is current, behind a newer version, permanently pinned, or that could
not be determined. `sync` writes every drifted bone's file back to match,
refusing outright unless the work tree is clean and every file it would
replace is exactly what git would check out (see
[Check, behind and sync](#check-behind-and-sync)). `wear` is how a repository
starts: it adds a skeleton as a dependency and writes the empty wearing table
that makes it worn, and leaves the files to `sync` (see [Wear](#wear)). See
[wearing.md](wearing.md) for the wearer's own reference:
the wearing table, `wear`, where bones land, `check` and `sync`'s own output,
and every way each can refuse.

## A skeleton is a set of claims about a repository

Not a set of files: a set of claims. Each claim is a **bone**. *This file
is exactly this* is a bone. So, in the design, is *this table exists in
`Cargo.toml`, once, and looks like this*, and so is *this file has this
shape, and these sections belong to the wearer* (see
[Beyond this release](#beyond-this-release)). A whole file is only the
simplest bone, and it is the only kind this release has.

Everything else here is built around that. `check` evaluates a skeleton's
bones against a repository, and `sync` makes them true. Every kind of bone
can be checked; a kind that can't be applied mechanically is check-only.
A new kind comes to the sensor first and the actuator second, because the
sensor shows which kinds are really needed before anything pays for
applying them.

Kinds stay small and closed. A new one is added only when a real skeleton
needs it.

## A skeleton is a crate, and it is data

A skeleton is an ordinary Cargo crate that also carries a
`[package.metadata.skeletons]` table, marking it as one. Everything a skeleton
ships sits under two directories: `files/`, whole files a wearing repository
should hold, and `partials/`, fragments the metadata's `set` options select
between. `skeletons` builds no infrastructure for a skeleton to run code:
rendering reads only a skeleton's `Cargo.toml`, its `files/` tree and its
`partials/` tree, and runs nothing the skeleton ships. A skeleton that carries a
`build.rs` is not refused for it — that script runs, if at all, when Cargo builds
the wearing repository, which is outside anything `skeletons` does.

This is a design constraint, not a guarantee about what a skeleton can do. The
reason for it: sensing whether a repository's files match what a skeleton says
they should be is exact when the thing being compared is data — bytes in,
bytes out — and would be a much harder question if a skeleton could compute
its own output as part of being worn. A repository takes a skeleton the way it
takes any other dependency, and a skeleton carries the same risk as any other
crate it could add; this is a choice about what `skeletons` builds, not a
promise about what a skeleton can do, and it may change in a future release.

## It's just Cargo

A repository wears a skeleton the way it depends on any crate: by adding it to
its own manifest. The lockfile records exactly which version it took, the
same as any other dependency. `skeletons` adds nothing to that mechanism — it
reads what Cargo already gives it and builds provenance on top, rather than
inventing a second way to declare what a repository depends on.

Maintaining a skeleton works the same way. Its owner edits it and publishes
a new version, and every repository wearing it sees the difference the next
time `check` runs, first as *behind*. Once the repository takes the new
version, `check` shows which files it changes, and `sync` makes them match.
Drift arrives the way any dependency update does, as a version bump someone
chooses to take, never as a change that reached into a repository on its
own.

`wear` keeps to that. It writes the dependency by running `cargo add`, so
Cargo's own sources and version grammar apply, and the only thing it adds of
its own is the empty wearing table beside it. Each repository still ends with
its own dependency line, its own table and its own lockfile pin.

## Provenance is derived, not stored

Comparing a repository against a skeleton needs to know three things: what the
skeleton says now, what it said at the version the repository actually took, and
what the repository's files actually hold. The first and third are read
directly. The second — what the locked skeleton rendered at the time the
repository took it — is never recorded anywhere. It is recomputed instead:
the locked skeleton's directory, rendered with the repository's own option
values, run through exactly the same pure function described below.

A stored hash of what a skeleton once rendered can go stale without anyone
noticing — the hashing logic changes, or the hash was computed against the
wrong inputs, and it keeps looking valid regardless. A recomputed answer
cannot go stale in that way: it is either right, because it is derived fresh
from the same render every time, or the render itself is wrong, which is a
render bug, not a provenance bug.

## The render is the contract

Everything that decides whether a repository has drifted compares
the repository's files against this render's output — never against a
skeleton's internal representation, its partials, or how it happens to be
written. That means a skeleton's internals are free to change as long as its
render does not: a later release could restructure how a skeleton represents an
option internally, and every wearer would see no difference at all, because
what they are compared against never moved.

It also means adopting a skeleton costs nothing extra. A file a repository
already holds, hand-written, that happens to be byte-identical to what a
skeleton would render with its defaults, reads as already matching the day that
repository starts wearing the skeleton. Nothing has to be regenerated or
reformatted first.

**Skeletons guarantees the file matches the render. It does not guarantee the
render is valid YAML, TOML, or anything else.** Skeletons are for drift.
They do not check that anyone writes good configuration, and engineers still
have to be careful about that. A `text` option's value goes into the render
verbatim, so quoting it for the format of the file it lands in is the
template's job and the wearer's: the skeleton writes the quotes around the
placeholder, and the wearer supplies a value that suits them.

## Options

A skeleton declares its own options, in its manifest, each with a type and,
apart from an optional `text` option, a default. Setting a value for an option
the skeleton did not declare is refused, naming the option; setting an `enum`
or `set` option to a value outside its own declared set is refused the same
way, and so is empty text, or text holding a control character, for a `text`
option. Every value is stated, never inferred. An option either takes one of
the skeleton's declared values or takes the wearer's text verbatim, and its
declaration says which. A wearer that gets it wrong finds out from a refusal,
not from a rendered guess.

Options are the variation a skeleton's author anticipated. Variation nobody
anticipated is not an option waiting to be added; it is what extensions would
be for (see [Beyond this release](#beyond-this-release)). Without options, a
repository that wanted a different dependabot cadence would need a whole
skeleton of its own. With too many, a skeleton becomes a program.

Three option types exist, each needed by a real skeleton, and nothing here is
added speculatively:

- **`enum`** — a closed set of values, one chosen, filling a placeholder.
- **`set`** — a closed set of values, any number chosen, each selecting a
  partial.
- **`text`** — the wearer's own words, filling a placeholder. A value nobody
  could list in advance, such as who a dependabot entry is assigned to, is
  what it is for. It takes any non-empty text with no control character, and
  no pattern or length bound beyond that: what a value ought to look like
  is for the skeleton's template and the wearer to keep right, as
  [The render is the contract](#the-render-is-the-contract) says. With a
  `default` it is required like an `enum`; without one it is optional, and
  leaving it unset drops the lines that hold its placeholder (see
  [Fill](#fill)).

Options choose between alternatives a skeleton's author already wrote, or
carry a value the wearer stated. They add nothing but a value someone stated:
a declared value, or the wearer's own text. They never loop, and never
compute — the full grammar is fill and select, described below, and nothing
more. Only `text` can be optional; an `enum` keeps its required default.

**A skeleton is never a generator.** If something needs generating, that is
a ritual. Rituals are code, and they get reviewed like code. Skeletons are
data, and they stay checkable.

## Fill

A placeholder, `{{name}}`, is replaced with the value chosen for the `enum`
or `text` option it names, or with that option's default when the wearer chose
nothing. `{{cadence}}` becomes `weekly`, or whatever the wearer set
`cadence` to.

A `text` option with no default is optional. When the wearer has stated
nothing for it, every line holding its placeholder is dropped whole,
terminator included, and nothing takes its place. A line holding an optional
placeholder holds no placeholder for any other option, though it may hold the
same optional more than once. Dropping a line that also held a required value
would drop that value without saying so, and dropping it for one unset
optional would drop another optional's stated value the same way, so a line
that mixes them is refused instead. A list entry that must vanish when unset
therefore sits on a line of its own, in a form that fits on one line:

```yaml
assignees: ["{{assignee}}"]
```

The block form, an `assignees:` line above a `- "{{assignee}}"` line, would
leave a bare `assignees:` behind when the option is unset, because only the
line holding the placeholder goes. A group of lines that should appear
together is the case [Beyond this release](#beyond-this-release) describes.

Every `{{` in a skeleton's text opens a placeholder, without exception. It must
be followed, on the same line, immediately by a well-formed option name and
then `}}` — nothing else counts. A space inside the braces
(`{{ cadence }}`), a name that does not close before the line ends, a name
that does not match the grammar, or an extra brace, is refused as a
malformed placeholder rather than treated as literal text. There is no way
to write a literal `{{` that survives a render unchanged in text that fills. A
file that needs one is declared verbatim and never filled at all (see
[Verbatim files](#verbatim-files)).

This is a deliberate refusal to guess. A typo in a placeholder — a name that
is close to a real option but not quite it — has no correct rendered output;
the only two choices are to render something the skeleton's author did not mean,
or to refuse and say what is wrong. `skeletons` refuses. The same reasoning
extends to a well-formed placeholder that names something real but of the
wrong kind: `{{workflows}}` where `workflows` is a `set` option is refused
exactly like a name that matches nothing at all, rather than being filled
with some stand-in text nobody asked for.

A placeholder inside a partial is filled exactly the same way as one in a
main file — a partial is authored text like any other, and an unfilled
placeholder sitting in rendered output would not be a faithful render of
what its author wrote.

Fill is one pass over the text as its author wrote it. A placeholder's
replacement value is never itself scanned for a placeholder or a directive
of its own, so a value can never accidentally reopen the grammar it was
substituted into. That matters most for a wearer's text, which the skeleton's
author did not write: a `text` value of `{{cadence}}` or of
`# skeletons:partial ecosystems` is written into the render exactly as typed
(tested by `crates/skeletons/src/acceptance/optional_text.rs` →
`a_text_value_holding_a_placeholder_is_written_verbatim_not_filled_again` and
→ `a_text_value_that_is_a_directive_is_written_verbatim_not_selected`).

## Select

A directive line selects between a `set` option's partials. A line whose
text, after its leading spaces and tabs, begins `# skeletons:` must be
exactly `# skeletons:partial <option-name>`, naming a declared `set`
option, with nothing else on the line before its terminator. Anything else
of that shape — a misspelt keyword, a missing or extra argument, trailing
text — is refused rather than treated as an ordinary comment; the same "no
guessing" rule as fill applies here too, including for a directive that
names a real option of the wrong kind.

When the wearer's chosen values for that option select one or more
partials, each selected partial's own file is inserted at the directive's
position, in the order the skeleton declared the values — never the order the
wearer happened to list them in. Choosing `["release", "lint"]` when the
skeleton declared `lint`, `test`, `release` in that order still renders `lint`
before `release`. Selecting nothing removes the directive line and puts
nothing in its place, not even a blank line.

Partials go one level deep. A directive line found inside a partial is
refused rather than expanded a second time — that would make
`skeletons` a template engine with its own recursive expansion, which
is exactly the generality options are designed to avoid.

Every partial a `set` option's values name must exist, and every file under
`partials/` must be named by exactly one declared value — a partial nothing
selects, or a partial two values both claim, is refused. A skeleton's partials
and its declared values are meant to be a single, complete mapping, checked
in both directions.

An option the skeleton declares but nothing in it ever uses — an `enum` or a
`text` with no placeholder anywhere naming it, or a `set` with no directive
anywhere naming it — is refused the same way. Setting such an option would
change nothing a wearer could observe, which is the same closedness this format
insists on everywhere else: a declared option that reaches nothing is not a
real option, it is a promise the skeleton does not keep.

## Indentation

A partial is written flush-left, and its lines land at whatever indentation
the directive that selects it happens to have. Every non-blank line of a
selected partial is prefixed with the directive line's own leading spaces
and tabs, verbatim, however deep that directive sits.

"Blank" means a line with no bytes at all before its terminator. A blank
line inside a partial stays blank — rendering never adds indentation to
nothing. A line holding only spaces or tabs is not blank by this rule: it is
content, exactly like any other line, and it gets the directive's
indentation the same as a line of real text would.

## Text

A skeleton's files and partials are read as UTF-8 text, because finding
placeholders and directives is a text operation with no faithful way to perform
it on bytes that cannot be decoded as text. A file that is not valid UTF-8 is
refused rather than copied through unexamined, unless it is declared verbatim:
nothing in a verbatim file is searched for, so the requirement has no reason
left behind it (see [Verbatim files](#verbatim-files)).

Beyond that, render normalizes nothing about a file's own bytes: bytes are
kept exactly as authored. A line ends at `\n` and nowhere else; every other
byte, a control character included, is content like any other, kept where
it was written. A file whose last line has no trailing newline renders the
same way, with none added. A byte-order mark is bytes like any other and
survives untouched, with one exception, which it shares with every
invisible character, wherever it sits — in front of a directive's own
marker, or inside it, between `#` and `skeletons` or right before `:`: a
line that reaches the marker's shape once whitespace, format and control
characters are set aside, but not once only its spaces and tabs are, is
refused. A no-break space pasted in with text from a web page, a zero-width
space, a stray control byte, a byte-order mark: rendered as text, such a
line would drop a directive its author meant; read as a directive, it would
decide what an invisible character was for — and a render refuses rather
than guesses. A line that reaches the marker's shape with nothing invisible
in play at all, only ordinary ASCII spacing gone wrong, is refused too, but
as a malformed directive rather than a hidden one: still a directive its
author meant, never an ordinary comment. A file with no placeholder and no
directive in it at all renders byte-for-byte identical to its source.

A partial's own last line must end with a line terminator, because a
partial can be inserted anywhere in the middle of a file, and one without a
terminator on its last line would run straight into whatever follows it — an
empty partial is fine and inserts nothing, but a non-empty one without a
closing terminator is refused. The same reasoning extends to a directive
line itself: a directive on a file's own final line, which by definition has
no terminator, is refused rather than rendered with a terminator its
author never wrote — keeping "render adds no terminator that was not
already there" true without an exception carved out for this one shape.

## Verbatim files

Every `{{` opens a placeholder and there is no escape, so a GitHub Actions
expression, `${{ secrets.TOKEN }}`, is refused as a malformed placeholder. That
would make the design's own first example, a CI workflow copied in, impossible
to write, and a release workflow is the first skeleton worth having. So a
skeleton's manifest can list files as **verbatim**. A verbatim file gets no fill
and no select: its render is its bytes, exactly as written, and `check`,
`behind` and `sync` compare against it as against any other render.

```toml
[package.metadata.skeletons]
verbatim = [".github/workflows/ci.yml"]
```

With `files/.github/workflows/ci.yml` holding `TOKEN: ${{ secrets.TOKEN }}`, a
repository that wears the skeleton claims that file byte for byte. It reads as
matching while the bytes are identical, as changed once they are edited, and
`sync` writes it back exactly (tested by `ritual/tests/verbatim.rs` →
`a_byte_identical_verbatim_workflow_reads_as_matching`, and →
`an_edited_verbatim_workflow_reads_as_changed_and_the_templated_file_still_matches`,
and → `sync_restores_an_edited_verbatim_workflow_to_its_exact_bytes`; the render
itself, beside a file that still fills, in
`crates/skeletons/src/acceptance/verbatim.rs` →
`a_verbatim_file_is_claimed_byte_for_byte_beside_a_file_that_still_fills`).

**The declaration is stated, never inferred.** A file that holds `${{` could be
taken for verbatim without being told, but then one piece of text would mean two
things depending on what else the file held, and a mistyped placeholder such as
`{{cadense}}` would quietly become literal text: exactly the guess the fill
grammar refuses to make. The same principle as [Options](#options) applies, in
the same way: a value somebody stated, or a refusal. The text grammar gains
nothing. Fill and select stay the whole of it, and what changes is which files
they are applied to.

The shape follows from that. It is a flat list of paths in the manifest, not a
table per file: a path is an awkward quoted key inside a dotted table name, and
a table would open a namespace for each file to hold one boolean. Each path is
relative to `files/`, as a set value's `partial` is relative to `partials/` and
as a render keys its files, so every entry is spelled the way the file lands in
a wearing repository. It is compared exactly, without `./`, case folding or
Unicode folding: the walk already refuses two siblings that fold to one name,
and folding an entry would guess which file its author meant, so a path that
differs from a file's only in case is refused as naming no file.

Every entry must name a file, and each way it can fail to is refused as the one
thing it is, so the message can say what to write instead. A directory is never
taken as a subtree, since that would quietly make a file added there later
verbatim, which is an inference; the refusal says to list each file under it. A
partial is refused because a partial is text spliced into a file at an
indentation, never a file in its own right, so a partial never holds a `{{` that
is not a placeholder. A path listed twice is refused, as a default that lists a
value twice is.

**A declaration nothing needed is accepted.** It is not refused on the analogy
of an unused option, for three reasons. That rule exists because a wearer could
set something that changes nothing, and a verbatim declaration is no knob of a
wearer's. Deciding that a declaration was needed would mean scanning the file as
a template, which contradicts a verbatim file being interpreted as nothing at all. And
it would tie a declaration's validity to the text grammar, so that adding an
escape under [Beyond this release](#beyond-this-release) would turn a file that
is accepted as written into a refusal. The converse holds by construction: an option whose
only placeholder sits in a verbatim file is used by nothing, since the file is
never parsed, and is refused as unused.

Nothing in a verbatim file is read as text. No line of it is a directive or a
hidden one, and it need not be UTF-8, so a skeleton can claim a binary file
(tested by `crates/skeletons/src/acceptance/verbatim.rs` →
`everything_the_grammar_reads_is_inert_inside_a_verbatim_file`, and →
`a_verbatim_file_need_not_be_utf_8`). The UTF-8 requirement exists only because
finding placeholders is a text operation, and a verbatim file is never searched.
The bounds in [Limits](#limits) still apply to it: it is read against the same
budget, and its length is what it adds to a render, whichever choices are made.

A skeleton can be a member of a workspace that `check` reads, and `check` reads
every key of a member's `[package.metadata.skeletons]` as a wearing table.
So `verbatim` is reserved exactly as `options` is: it is the member's own
declaration and never a wearing table, and a member that also declares a
dependency under that key cannot wear it (see [wearing.md](wearing.md); tested by
`ritual/tests/verbatim.rs` →
`a_workspace_member_that_is_itself_a_verbatim_skeleton_draws_no_refusal`, and →
`a_dependency_keyed_verbatim_is_refused_as_a_reserved_key_naming_it`). The
alternative was telling the two apart by shape, an array for a declaration and a
table for a wearing table. One rule for both keys is easier to hold than a
second that depends on what a key holds.

## A skeleton is validated whole

Every file and every partial a skeleton ships is read and checked — every
placeholder, every directive, everything the manifest's metadata refers to
— before a single byte of anything is assembled, whatever the wearer's
option values turn out to be. A defect in a partial that nobody's choices
select is refused exactly as loudly as one in a partial everyone sees. A
verbatim file has no placeholder or directive to check, so what is checked of it
is the declaration that names it.

The alternative — checking only what the chosen options actually reach —
would make one version of a skeleton valid for one wearer and invalid for
another, purely as a function of which options they happened to set. The
render is the contract every wearer relies on, and a contract cannot be
sound for some option values and broken for others: a skeleton's validity is a
property of the skeleton, not of who is using it. The size limit described in
[Limits](#limits) below is part of this same whole-skeleton validation: the
skeleton's own text is checked against the largest render any choice could
produce, not the one a wearer actually chose, for exactly this reason. The one
thing that check cannot see is the length of a wearer's own `text`, which the
skeleton does not declare; that is checked separately, against the render the
wearer's choices produce.

## Refusals

Every refusal names the skeleton it is about, and, where one applies, the file
and the line. A refusal about the manifest's own schema or about a whole
directory names `Cargo.toml`, `files`, or `partials` with no single line at
fault. A refusal found while parsing a file or a partial names that file's
own path — relative to the skeleton's directory, `files/…` or `partials/…` —
and the 1-based line of that file the author actually wrote, never a line
number the assembled output would occupy. A refusal about the wearer's own
choices names neither a file nor a line, since it is not about anything the
skeleton's author wrote.

A render returns the first refusal it finds, in a fixed order: the
manifest (its options, then the shape of its `verbatim` list and any path it
lists twice), then the directory walk, then the mapping between a `set` option's
declared partials and the files actually shipped, then each `verbatim` path, in
the order listed, then every file in path order, then every partial in path
order, then whether every declared option
is used by something, then whether the largest render the skeleton could
produce fits the size limit, then the wearer's own choices, each in
option-name order, then whether the render they produce fits the size limit.
The order is fixed so that which refusal a skeleton author sees first never
depends on anything but the skeleton itself — the same defect always surfaces
the same way.

## Determinism

Rendering the same skeleton with the same option values has to produce the
same bytes, every time, in every process, on every machine — including when
an option's chosen values are, in the shape a wearer wrote them, an
unordered collection. Nothing about how those values happen to be stored or
iterated may be observable in the output.

"On every machine" includes machines whose filesystems disagree about
names. A default macOS volume ignores case and Unicode normalization, so
`Dependabot.yml` and `dependabot.yml` — or an `é` written precomposed and
one written as `e` and a combining accent — are one file there and two on
Linux. A skeleton holding two such names in one directory would render a
different set of files on each, so the render refuses it, deciding which
names are one name by the Unicode Standard's canonical caseless match: the
rule a case-insensitive macOS volume applies itself.

The only order that can ever appear in a render's bytes is the order a
skeleton's own author declared, in the manifest's `values` arrays. Every other
collection along the way — the wearer's own choices, the files and partials
a directory walk discovers — is either sorted by path or reduced, before
assembly, to a form that carries no order of its own to leak: a wearer's
chosen `set` values are checked in the order given (so a duplicate can be
refused) and then turned into a selection aligned with the skeleton's own
declared order, which has nothing left to leak.

As a guard at the point where an order-dependent collection could otherwise
be reached for by habit, the workspace's own `clippy.toml` marks
`std::collections::HashMap` and `HashSet` as disallowed types, with the
reason recorded alongside: their iteration order changes from one process
to the next, which is exactly the shape of bug this format cannot tolerate.
Reaching for one of them is a lint error that has to be argued past
explicitly, not merely a style preference.

## Limits

A render is bounded in three ways, each for a different part of the work:

- **At most 1,024 entries listed** across `files/` and `partials/`
  together — charged the moment a directory's listing yields an entry, so
  no more of a listing is ever held at once than the budget allows, rather
  than the whole of it.
- **At most 1 MiB read** in total across the manifest, every file, and
  every partial. Everything built from what is read — parsed lines,
  segments, TOML values — costs a roughly constant multiple of the bytes
  read, so bounding the read bounds what parsing it can cost too.
- **At most 8 MiB rendered**, checked twice. First against the largest
  render the skeleton's own text could produce, every set value selected,
  every enum at its longest declared value and every `text` at its default
  or at nothing, rather than the one a wearer actually chose — because a
  skeleton's validity must not depend on who wears it, and reuse (one
  partial behind several directives, one value behind many placeholders)
  can multiply what a skeleton ships far past what it read. Then against
  the render the wearer's choices produce, before any of it is assembled,
  because a wearer's `text` has no length the skeleton declared: a value too
  long is refused as the wearer's choice, and the refusal names the `text`
  option the wearer set that contributes the most bytes to the render, the
  first by name when two contribute the same (tested by
  `crates/skeletons/src/acceptance/optional_text.rs` →
  `a_render_made_too_large_by_the_wearers_text_names_the_option_contributing_most`,
  and → `a_tie_for_the_most_bytes_in_a_too_large_render_names_the_first_option_by_name`).
  An option that took its default is never named, since the wearer wrote
  nothing to shorten (tested by `crates/skeletons/src/skeleton.rs` →
  `a_heavy_default_does_not_take_the_blame_for_the_wearers_text`).

A skeleton is configuration text, and all three limits sit far above what any
real one holds. A verbatim file counts against them like any other file. They
apply to every render, whatever skeleton it is given, so
a mistake fails loudly and quickly instead of walking forever, exhausting
memory while being read, or exhausting memory while being assembled.
Raising any of them is a compatible change: nothing that renders under a
lower limit stops rendering under a higher one.

## Check, behind and sync

`check` and `sync` sit on top of everything the render's own
contract promises.

### What check reports

`check` compares a wearing repository's files against what the locked skeleton,
rendered with the repository's own option values, says they should hold.
One claim kind exists in this release — `file`, compared as bytes — so
reformatting a skeleton-owned file counts as drift the same as any other
change. It reports two things, and they are independent of one another:
each *bone* either **matches** the render or has **drifted** from it, and
each *skeleton* — once per dependency, never per bone — is current,
**behind** a newer version than the one locked, whatever its version
requirement allows, or **pinned** to a revision that nothing can be behind.
A bone can be
drifted while its skeleton is current, or matching while its skeleton is behind;
neither state implies the other, because they answer different questions —
"does this bone match what the locked skeleton says" and "is a newer skeleton available" —
and a repository can be wrong about one without being wrong about the
other. Filtering flags narrow which rows are shown but never change
whether the command succeeds or fails; only a `--fail-*` flag changes the
exit code, and the default fails on drifted so that a repository cannot
silently stop noticing its own drift by narrowing its own output. `--json`
gives the same information in a form meant for collecting across many
repositories at once, rather than being a separate feature from the
per-repository view.

A drifted file is, further, distinguishably **missing** or **changed**, and
a skeleton's behind state has a fourth value beyond current, behind and
pinned: **undetermined**, covered in full below. The filtering flags are
`--drifted` and `--behind`; the one `--fail-*` flag is `--fail-behind`,
which also fails a run on behind or undetermined.

### Claimed paths

A claimed path is accepted only under the exact spelling its skeleton gives
it, never a spelling a filesystem lookup merely folds to it. A name lookup
(`symlink_metadata`) answers for any spelling the filesystem folds to the
one asked for, but reading a directory's own listing never folds
anything — it names exactly what is there, byte for byte — and the walk
compares that listing by the one fold the render and overlap detection use
(case and Unicode normalisation together), so it names the same variants on
every platform, and the lookup stays as the backstop for anything a
filesystem folds beyond it. Put together, a
case-only spelling difference is one file on a case-insensitive filesystem
and two files on a case-sensitive one, and the untracked, unclaimed one of
those two files is exactly the content `sync` must never destroy. This is
decided in the walk `check` and `sync` share, not in `sync`'s own
positive-proof check (rule (b), below). There, a path present on disk is
looked up in the index by exact spelling (`--literal-pathspecs`), so an
index entry listed under a case or normalisation variant proves nothing
about the claimed spelling and is refused rather than trusted
(`crates/skeletons/src/sync/proof.rs` →
`a_case_variant_in_the_index_is_not_in_this_index_under_the_claimed_spelling`,
and →
`an_nfd_claim_whose_committed_entry_was_normalised_to_nfc_is_listed_as`).
Folding case there alone would stop `sync` overwriting the wrong file, but
`check` would still read that wrong file as though it were the claim, and
the two commands would keep disagreeing about the identical repository
state.

Two spellings that differ only in Unicode normalization display identically,
so a message that names both (`café.yml is spelled café.yml on disk; rename
café.yml to café.yml`) tells the reader nothing to act on. Whenever one
message names spellings of which any two are canonically equivalent but differ
in bytes, every spelling it names is shown escaped, as any text from outside
is, and followed by the same name with its non-ASCII characters as `\uXXXX`
(`\UXXXXXXXX` above U+FFFF) code points, which bash 4.3 or later, or zsh,
reads back as the same bytes in `$'…'` under a UTF-8 locale, with a note
saying why; case and full-fold variants
look different and stay as they are, escaped only as any text is. One helper
does this for every message that names variants side by side, and `--json` keeps
the exact strings (`crates/skeletons/src/survey/spelling.rs` →
`a_composed_and_a_decomposed_spelling_are_each_followed_by_their_code_points`,
→ `the_kelvin_and_angstrom_signs_look_like_their_letters_and_are_written_out`
and → `case_and_full_fold_variants_are_visibly_different_and_stay_readable`, and
`ritual/tests/sync_messages.rs` →
`a_refusal_over_two_unicode_spellings_shows_how_they_differ`).

The same escape is what keeps every other message one line. A name from the
wearer's repository, a word of git's or cargo's own, a path and a url all reach
`check` and `sync` messages, and a newline in any of them would split the
report and put a line break into `--json`'s `message` and `detail` fields. Each
such text is escaped once, where the message that names it is built, and a
remedy that repeats a path quotes it escaped, since it is the message that names
it. The types beneath the messages (`claim/`, `workspace/`) keep the raw text,
because a claimed path is also what git is asked about, so escaping there would
change the question. Text that is already message text (another message's output, a
render error's reason) is never escaped again, since that would double its
backslashes. `--json` fields that are data, not prose (`path`, `paths`, `at`,
`on_disk`, `manifest`, `dependency`, and a pin's `url`, `tag`, `branch`, `rev`
and `source`) keep the exact string
(`ritual/tests/check_one_line.rs`).

Comparing a directory's listing with a claim's component costs what the directory is large.
Two ASCII names are one name to that fold exactly when they are equal apart from ASCII case,
so an ASCII entry beside an ASCII component is compared without folding either, and any
other pair is folded, the component's fold once and not once per entry. The exact spelling
`verify` re-checks after a write is only ever compared byte for byte, and is found by a scan
that stops at the first match and folds nothing. Neither changes an answer: the folded ASCII
name is the name lowercased, which proptests hold to the full rule over every ASCII string
and over listings mixing ASCII with the characters that fold onto it
(`crates/skeletons/src/skeleton/folding.rs` →
`an_ascii_name_folds_as_the_full_rule_folds_it`, →
`colliding_gives_the_answer_folding_both_names_gives`, and
`crates/skeletons/src/claim/location.rs` →
`the_listing_scan_gives_the_answers_folding_every_entry_gives` and →
`listing_the_exact_spelling_finds_it_exactly_when_the_directory_lists_it`).

A claimed name also has to be a name a file can have, and one `sync` can stage. A filesystem holds
at most 255 bytes in a name (ext4's limit, in bytes; APFS counts 255 UTF-16 code units, which is
never more than the UTF-8 byte count, so 255 bytes is the one bound both honour), and `sync` stages
a claim as `.<name>.skeletons-sync`, 16 bytes longer, so a last component of more than 239 bytes
cannot be staged. Both are decided when the claim is built, from the name alone and not from any
one directory's own limit, so `check` and `sync` agree on every machine and nothing is created
before a claim is refused: a component of more than 255 bytes, or a last component of more than
239, is refused as `unsafe-path`, cause `name-too-long`, naming the name and its length
(`crates/skeletons/src/claim.rs` →
`a_last_component_of_239_bytes_is_the_longest_that_can_be_staged`, →
`a_component_of_more_than_255_bytes_is_refused_as_a_name_that_cannot_exist`, →
`a_directory_may_be_255_bytes_but_not_256` and → `the_limit_counts_bytes_not_characters`, and
`ritual/tests/sync_messages.rs` → `check_refuses_a_claim_whose_staging_name_would_be_too_long`, →
`sync_refuses_a_claim_whose_staging_name_would_be_too_long_before_it_stages_anything` and →
`a_claim_whose_staging_name_just_fits_is_still_written`).

A claim is also refused when git would refuse to track its name, decided at the
same place and for the same reason: from the name alone, so `check` and `sync`
agree and nothing is created first. `.git` in any ASCII case is `unsafe-path`,
cause `inside-git-directory`, because a write there is a write into git's own
directory. Every other spelling git reads as `.git` (`.git.`, `git~1`, either
followed by spaces, periods, a colon or a backslash, and `.git` with any of
sixteen invisible code points in it) is cause `untrackable-name`, naming the
component. The set is taken from git v2.53.0's source (`read-cache.c`,
`verify_path_internal`, and the readings in `path.c` and `utf8.c`), and
`ritual/tests/check_untrackable_names.rs` asks git itself, in a
scratch repository, about every name it tests. A backslash that is a component's
first character is not a separator to git, and so is not one here. The shapes
are in `crates/skeletons/src/claim.rs`, one test each (→
`dotgit_followed_by_a_period_is_untrackable` and its neighbours), with
`names_git_tracks_are_accepted` holding the names that must stay valid.

Claims overlap on the same fold, so an overlap is refused on every
filesystem, not only where the filesystem's own lookup happens to fold that
far: two claims that are one name once case and Unicode normalisation are
folded together, one claim inside another, or two claims under one
directory that they spell two ways (`a/x` and `A/y`). The last is an
overlap because a directory has one spelling on disk, so the two can never
both be written, and the claim walk refuses both once either directory
exists. All three are refused before anything is read or written, by
`check` and `sync` alike, so `sync` never leaves a state `check` refuses.
Two claims under a directory both spell the same way (`a/x`, `a/y`) are not
an overlap (`ritual/tests/sync_free_paths.rs` →
`a_success_never_leaves_a_claim_under_a_differently_spelled_directory`).

A path absent from the work tree is looked up the other way round, by git's
own case fold, asked of git itself: `ls-files --stage -z --
':(literal,icase)<claim>'`, run without the global `--literal-pathspecs`
(which disables every magic, `icase` too) and with `literal` magic keeping
`*`, `?` and `[` ordinary. The reason is what `sync` must not write, which
is a file git would ignore. Under `core.ignorecase` git takes a new `A.yml`
for a tracked `a.yml` hidden by `--skip-worktree`, and ignores it, so
`sync` would report `created` while `HEAD` kept the committed bytes. The
fold is git's own and not the one overlap detection uses for names: git
folds ASCII case only (`é` and `É` stay two names to it), so asking git
refuses exactly what git would ignore and nothing git keeps visible. It is
asked whatever `core.ignorecase` says, so it answers the same on every
platform, at the price of also refusing an index-only case variant on a git
that would have kept it visible, which is rare and names the entry
(`crates/skeletons/src/sync/proof.rs` →
`an_absent_path_git_folds_onto_a_tracked_entry_is_tracked_but_absent`, and →
`the_case_fold_is_asked_of_git_whatever_core_ignorecase_says`, and
`ritual/tests/sync_free_paths.rs` → `a_path_git_hides_under_another_case_is_refused_not_created` and
→ `a_path_git_hides_under_a_directory_of_another_case_is_refused_not_created`).

### What sync writes

`sync` writes the locked skeleton's render over the repository's own files. For
whole files, that is a plain copy, not a merge — there is nothing to merge
when the skeleton owns the whole file. It refuses outright unless the whole
work tree is clean and every file it would replace holds exactly what git
would check out for it (rules (a) and (b), below), so the repository's own
version control is the undo: what `sync` replaces is what `git checkout`
writes back, under the attributes and content filters in force when it ran,
and nothing it writes is the only copy of what was there before. That holds
for what `sync` saw immediately before each write landed: an edit made in the
instant between that last look and the write itself is the one thing it can
still replace (see [Known limits](#known-limits)). Rule (a)
alone is not enough for this — git can hold a path clean while the work
tree genuinely diverges from it (`--assume-unchanged`, `--skip-worktree`),
or while the file holds bytes a clean filter would drop on the way in —
which is what rule (b)'s own per-path proof exists to catch
(`crates/skeletons/src/sync/proof.rs` →
`an_assume_unchanged_edit_is_refused_as_hidden_from_the_work_tree` and →
`a_line_a_lossy_clean_filter_strips_is_not_what_git_checks_out`).

A file that already matches is never rewritten at all, since doing so
would still change its mtime and permissions for no reason. `sync` shares
`check`'s own claim resolution — the one `survey` both call
(`crates/skeletons/src/survey.rs` → `survey`) for the same symlink-free walk
from the workspace root and the same drift compare — so the two can never
disagree: a bone `check` reports drifted is always one `sync` would write
(tested by `ritual/tests/sync.rs` →
`sync_creates_a_missing_file_matching_the_render_and_then_check_reports_matches`),
and a path either one refuses as unsafe is refused by both, since both walk
it through the very same function (`crates/skeletons/src/claim/location.rs` →
`a_symlinked_intermediate_directory_is_refused_naming_it`).

That walk also refuses a claim that lies inside another repository. Every
directory it goes through below the workspace root is listed, and one that
holds a `.git` entry (a directory, a file as a submodule or a linked worktree
has, or a link, spelled in any ASCII case, the rule a claim's own name is
refused by) is another repository: a file written there is that
repository's to track, and this one's `git status` shows it as one changed or
untracked directory at most. It is refused as `unsafe-path`, cause
`inside-another-repository`, by `check` and `sync` alike, since both walk it
(`crates/skeletons/src/claim/location.rs` →
`a_directory_holding_a_dotgit_directory_below_the_root_is_another_repository`,
and `ritual/tests/sync_git_ignores.rs` →
`check_refuses_an_absent_claim_inside_a_submodule`, →
`sync_refuses_an_absent_claim_inside_a_submodule`, →
`check_refuses_an_absent_claim_inside_an_ignored_nested_repository` and →
`sync_refuses_an_absent_claim_inside_an_ignored_nested_repository`). The
workspace root's own `.git` is this repository's and is not counted, so a
workspace below the repository's root, and a claim beside a submodule rather
than under it, are untouched (`ritual/tests/sync_git_ignores.rs` →
`a_workspace_in_a_repository_with_nothing_between_root_and_claim_still_syncs`
and → `a_claim_beside_a_submodule_but_not_under_it_still_syncs`). A submodule
that is not checked out has no `.git` on disk to find; the index question
below finds its gitlink.

### All or nothing, and a clean work tree

`sync` is all or nothing until the first file lands: if any
worn skeleton is refused, any two bones overlap, or either of the two rules
below fails to hold, it writes nothing at all, and says so before failing.
Once files have started to land, a write that fails or is refused partway is
the one way it can leave some files written (below): a rename or link that
fails, or a file found changed by the re-verification each write gets right
before it lands. Both rules are checked only once there
is something to write — with nothing to write, nothing can be overwritten,
so an unrelated dirty tree never blocks a `sync` that would have done
nothing anyway. Neither is anything else about git: `sync` runs its
survey and its write plan first (`crates/skeletons/src/sync.rs` → `run`), and
opens the work tree — the redirecting-variable refusal below and the
`rev-parse` — only when the plan is not empty. So with nothing to write it
needs no git at all, and answers the same inside a git hook, outside a git
work tree, or with a redirecting variable set
(`ritual/tests/sync_worktree.rs` →
`sync_in_a_pre_commit_hook_with_every_bone_matching_needs_no_git_and_exits_zero`,
and →
`a_missing_git_dir_does_not_stop_a_sync_with_nothing_to_write`).

**(a) The whole work tree is clean**, as git itself defines clean: a
whole-tree `git status` (not scoped to the paths `sync` is about to write)
reports nothing at all — no tracked change (staged or unstaged, a
mode-only change included), untracked file, conflict, intent-to-add entry,
or change inside a submodule. Ignored files do not count. The status
question is asked under flags that defeat configuration that would
otherwise hide dirt from a plain `git status`
(`status.showUntrackedFiles`, `diff.ignoreSubmodules`,
`submodule.<name>.ignore`; `crates/skeletons/src/work_tree/clean.rs` →
`status_show_untracked_files_no_does_not_hide_an_untracked_file` and →
`a_submodule_edit_is_dirty_even_under_ignore_all_configuration`). A file
merely deleted in the work tree still counts as dirty — the whole-tree
question does not exempt it the way a question scoped to the paths `sync`
is about to write would.

### Positive proof, per path

**(b) Positive proof, per path `sync` is about to write, that the bytes
there are what git would check out for it.** The path is either absent —
nothing on disk (`symlink_metadata` finds nothing) *and* no entry in this
repository's index at or under the path, or at any directory above it —
or a regular file — never a symbolic link or a submodule's own gitlink —
tracked in this repository's index as one stage-0 entry, mode `100644` or
`100755`, not intent-to-add, that git reads from the work tree (never
flagged skip-worktree or assume-unchanged), whose on-disk bytes are exactly
what `git cat-file --filters --path=<prefix><path> <index-object>` writes:
the bytes `git checkout` would put there for that entry, its smudge side
included (`crates/skeletons/src/sync/proof.rs` → `a_clean_tracked_file_is_held`). The
`--path` is relative to the repository's top level, where git reads
attributes from, so from a workspace below it the workspace's own prefix
leads the claimed path; without it git applies no attribute and the raw blob
is compared (`ritual/tests/sync_checkout_side.rs` →
`a_clean_file_under_a_filter_in_a_workspace_below_the_repository_root_is_held_and_updated`
and →
`a_file_a_repository_path_attribute_would_expand_differently_is_refused_in_a_nested_workspace`,
which pin it from both sides). The file is read last, through the walk
`check` uses, no further than one byte past the checkout's own length, so
whatever a content filter did while git ran is what is compared.

Anything that is not what git would check out is refused, whatever the
cause. Asking whether git would *record* the file as unchanged
(`git hash-object`) proves less: a clean filter that is not the exact
inverse of its smudge maps bytes git never stored onto the committed
object, so they read as held, are replaced, and no object holds them any
more (`ritual/tests/sync_checkout_side.rs` →
`a_line_a_lossy_clean_filter_would_strip_is_never_overwritten`; a smudge
whose output varies from run to run is the same case, →
`a_file_whose_checkout_is_not_reproducible_is_never_overwritten`). A file
`git checkout` itself wrote under a filter that is a function of the blob
stays held (→ `a_checked_out_ident_file_is_held_and_updated`).

When the bytes differ, `cat-file` is asked a second time, on that refusal path
only, so the refusal can say whether there is one checkout to bring the file
back to. The same bytes twice mean there is, and `git checkout -- <path>`
writes it, so the refusal offers exactly that, after a copy is kept. Different
bytes mean the object and the path do not decide the checkout (a smudge that
adds the process id): `git checkout` would write yet another variant and the
next run would refuse again, so the refusal states that cause and names no
remedy (`crates/skeletons/src/sync/proof.rs` →
`a_second_checkout_that_differs_from_the_first_is_not_reproducible` and →
`a_file_whose_checkout_differs_every_time_git_is_asked_is_not_reproducible`,
and `ritual/tests/sync_messages.rs` →
`a_file_under_a_nondeterministic_filter_is_not_told_to_check_it_out`). A filter
whose output changes only between whole seconds can return the same bytes twice
and is refused once more after the remedy, with the same line.

Each refused path's line ends in a remedy that fits its cause, or in none:
committing a file git does not hold fits a path that is not in the index and
breaks a path git holds but cannot give back (a `required` filter that fails,
a file over 16 MiB) and one it hides. So the closing summary offers no remedy of
its own and points at the lines above it, and a line whose cause has no single
right command names the fact and nothing else. A path tracked but absent
from the work tree ends in the command that brings the entry back:
`git sparse-checkout add` for a sparse checkout that leaves it out, or
`git update-index --no-skip-worktree` and then `git checkout`, since clearing
the flag alone leaves the file deleted (`crates/skeletons/src/sync/message.rs` →
`each_remedy_a_line_names_fits_its_cause`, →
`the_lines_that_deliberately_name_no_remedy_name_none`, →
`the_unproven_summary_offers_no_remedy_of_its_own` and →
`tracked_but_absent_at_the_claimed_path_itself_omits_the_as_clause`, and
`ritual/tests/sync_messages.rs` →
`a_file_hidden_from_the_work_tree_is_told_how_to_bring_it_back`, →
`a_file_hidden_from_the_work_tree_is_not_told_to_commit_it_or_move_it_away`, →
`a_failing_required_filter_is_not_told_to_commit_or_move_the_file`, →
`an_oversized_tracked_file_is_not_told_to_commit_it_or_move_it_away` and →
`a_skip_worktree_file_that_differs_from_git_is_not_told_to_check_it_out`).

A file git does not read from the work tree at all is refused before that
comparison is made. Git's own whole-tree status cannot see a file flagged
`--skip-worktree` or `--assume-unchanged`, and a write there is never seen
by a commit: `git commit -a` records the old bytes, so `sync` would report
`updated` while `HEAD` kept the committed ones. That holds even when the disk
holds exactly what git holds, which is the case no comparison of bytes can
catch, so the same `ls-files -v --stage` question that names the one entry
(the `-v` is the flag letter, `S`, `h` or `s`) refuses a flagged one before
`cat-file` is ever asked
(`crates/skeletons/src/sync/proof.rs` →
`an_assume_unchanged_edit_is_refused_as_hidden_from_the_work_tree`, →
`a_skip_worktree_edit_is_refused_as_hidden_from_the_work_tree`, →
`a_flagged_file_whose_bytes_equal_git_s_is_refused_for_each_flag`, and →
`a_flagged_file_is_refused_before_git_is_asked_what_it_would_check_out`, and
`ritual/tests/sync_git_ignores.rs` →
`a_present_skip_worktree_file_equal_to_git_is_refused_not_updated`, →
`a_present_assume_unchanged_file_equal_to_git_is_refused_not_updated`; the
control is →
`a_clean_committed_file_with_no_index_flags_is_still_updated`).

Absence on disk alone proves nothing, because a path git was told not to
materialise — `--skip-worktree`, or a sparse checkout excluding it, cone
or not — is missing from the work tree while its index entry stays, and a
file `sync` wrote there would be ignored by git: `sync` would report
`created` while `HEAD` kept the committed bytes. So the absent branch asks
the same `ls-files --stage` question the present branch does, with git's
own case fold (above) in place of the exact spelling
(`crates/skeletons/src/sync/proof.rs` → `prove_absent`), and any entry at any
stage, at the path itself or beneath it (a literal pathspec matches a
directory prefix), or at or beneath another case of it, refuses the path as
tracked in git's index but absent from the work tree; the user runs
`git sparse-checkout add` or `git update-index --no-skip-worktree` and then
`sync` again
(`crates/skeletons/src/sync/proof.rs` →
`a_skip_worktree_path_absent_from_the_work_tree_is_tracked_but_absent`, →
`an_absent_path_with_a_tracked_entry_beneath_it_names_that_entry`, →
`an_absent_path_git_folds_onto_a_tracked_entry_is_tracked_but_absent`, →
`a_glob_character_in_an_absent_claim_stays_a_literal_character`, and
`ritual/tests/sync_worktree.rs` →
`a_skip_worktree_path_absent_from_the_work_tree_is_refused_not_created`, →
`a_non_cone_sparse_checkout_excluding_a_claimed_file_is_refused_not_created`,
→
`a_cone_sparse_checkout_excluding_a_claimed_file_in_a_subdirectory_is_refused_not_created`).

Nor does an entry at or under the claim exhaust what makes git ignore it. A
claim `a/b.yml` needs `a` to be a directory, and if the index tracks `a` as a
file, a symbolic link or a submodule's gitlink, hidden from the work tree,
creating `a/` and `a/b.yml` gives a tree git reads as `a` deleted and
`a/b.yml` untracked, and `git checkout -- a` puts the file back over the
bone. So the proof asks, before either branch, whether git tracks an entry at
each directory above any write: one `ls-files -v --stage` per *distinct*
directory, with git's own case fold, for exactly that path and nothing beneath
it (`':(literal,icase)<a>'` and an exclusion of what lies beneath `a`, since
`a` alone matches every file under it). The exclusion is a glob of the escaped
path followed by `/**`, and not a literal `<a>/`, because git reads a
trailing `/` as matching a gitlink of that name, which would hide exactly the
submodule the question is for. Any record refuses every write under it, as a
file or link where a directory is needed, or, for a gitlink, as a submodule
(`crates/skeletons/src/sync/proof/above.rs` →
`a_file_the_index_tracks_at_a_directory_above_the_claim_refuses_it`, →
`a_gitlink_at_a_directory_above_is_a_submodule_even_when_nothing_is_checked_out`, →
`a_directory_git_tracks_files_beneath_is_not_an_entry_above_the_claim`, →
`a_name_that_only_starts_like_the_directory_is_not_it` and →
`a_glob_character_in_a_directory_stays_a_literal_character`, and
`ritual/tests/sync_git_ignores.rs` →
`a_tracked_file_hidden_above_an_absent_claim_is_refused_not_created`).

### Files git ignores

A file `sync` would create can also be one git's ignore rules ignore: a
`.gitignore` above it, `.git/info/exclude`, or `core.excludesFile`. Then
`git status` never shows it, `git add` refuses it without `-f`, and the bone is
never committed; the next drift finds it untracked and `sync` refuses to update
it. `skeletons` does not serve one-shot files meant to stay out of git, so
`sync` refuses to create one, before it writes anything, naming the rule and the
two ways out: remove the ignore rule, or create the file by hand and
`git add -f` it. Which rule wins depends on every ignore file above the path and
on the order and negation of the patterns in them, so it is asked of git, never
read from them: `git check-ignore --no-index -q -- ./<claim>` gives the verdict
as its exit status (`0` ignored, `1` not ignored; a `!` pattern that undoes an
earlier one exits `1`, so that claim is created), and only when that says
ignored, `-v` prints the rule as `<source>:<line>:<pattern>`, which the message
quotes as git prints it (`crates/skeletons/src/sync/proof/ignored.rs`). Only a
write `sync` would create is asked. A file that is present is tracked, since
`sync` proves that first, and git does not ignore what it tracks, so a claim
added with `git add -f` is updated whatever a rule says; a claim the index holds
an entry for is refused for that entry, not for a rule.

The command is built without the global `--literal-pathspecs`, because
`check-ignore` refuses it (it exits 128 with
`pathspec magic not supported by this command: 'literal'`, and the same for
`GIT_LITERAL_PATHSPECS` and `GIT_GLOB_PATHSPECS`, which every builder here
removes). It reads its path without expanding a wildcard: `*`, `?` and `[` are
ordinary characters of the name, so a claim `*.log` is asked about as a file
called `*.log` and a rule `x.log` does not ignore it, while `--no-index` is what
keeps a tracked file that such a name reads like a glob of from answering for
the claim (without it the command matches the name against the index as a
pathspec, finds the tracked `kept.log`, and says the absent `*.log` is not
ignored). What it does read is a leading `:(…)` as pathspec magic, so the path
is passed after `./`, which makes `./:(top)a.yml` the file called `:(top)a.yml`,
and a leading `-` a name and not an option
(`crates/skeletons/src/git/command.rs` →
`a_check_ignore_path_is_the_claim_after_a_dot_slash`, and
`crates/skeletons/src/sync/proof/ignored.rs` →
`a_glob_character_in_a_claim_is_an_ordinary_character_of_its_name`, →
`a_claim_that_opens_like_pathspec_magic_is_a_file_of_that_name`, →
`a_claim_that_begins_with_a_dash_is_a_name_not_an_option` and →
`a_tracked_file_a_claim_reads_like_a_glob_of_does_not_answer_for_the_claim`).
Exit `128`, any other status, a rule line `skeletons` cannot read, and output
past the cap are each a refusal, never a claim created on an answer that was not
read (`crates/skeletons/src/sync/proof/ignored.rs` →
`exit_zero_is_ignored_and_exit_one_is_not_ignored`, →
`exit_128_is_a_failed_check_carrying_gits_own_detail`, →
`output_that_is_not_one_rule_line_parses_to_nothing`, and
`ritual/tests/sync_ignored_claims.rs` →
`a_claim_a_gitignore_rule_ignores_is_refused_naming_the_rule_and_the_way_out`, →
`a_claim_the_local_exclude_file_ignores_is_refused_naming_that_file`, →
`a_claim_a_later_negated_rule_un_ignores_is_still_created` and →
`a_claim_added_with_force_despite_an_ignore_rule_is_still_updated`).

### Spellings git cannot fold

Git's case fold is ASCII only, so it cannot say that an index entry hidden
under a different Unicode spelling of the claim, `café.yml` decomposed for a
claim of `café.yml` precomposed, or `É.yml` for `é.yml`, is one name with it.
A filesystem that folds them, APFS, does say so: creating the claim then
makes the hidden entry read as modified and the new file as untracked, or git
shows nothing, and the bone is never committed. Whether that can happen
depends on the directory being written into, which no platform test and no
git setting states, so it is asked of the filesystem itself. Every path
git's index holds at the depth of the writes and above is listed once
(`ls-files -z` with one glob per depth, `*`, `*/*`, and so on, paths only),
and each entry that is one name with a claim, or with a directory above it,
under the fold the render, overlap detection and the walk use, but is spelled
differently, is a candidate (`crates/skeletons/src/sync/fold_variant.rs` →
`an_entry_one_name_with_the_claim_but_spelled_otherwise_is_the_same_path`, →
`an_entry_at_a_directory_above_the_claim_spelled_otherwise_is_a_directory_above`, →
`an_entry_beneath_a_variant_of_the_claim_is_not_a_candidate` and →
`a_sibling_under_a_variant_directory_is_not_a_candidate`). Once the staging
file exists, each candidate's spelling is looked up on the filesystem: if it
finds the very file `sync` just created, or the very directory the claim
needs, the filesystem takes the two for one name and the write is refused
with nothing written; if it finds nothing, they are two names, and nothing is
refused (`crates/skeletons/src/sync/write/fold_probe.rs` →
`a_variant_the_filesystem_folds_is_refused_and_one_it_keeps_apart_is_not`, and
`crates/skeletons/src/sync/write.rs` →
`a_fold_variant_the_filesystem_takes_for_the_claim_is_refused_with_nothing_written`,
and `ritual/tests/sync_git_ignores.rs` →
`an_index_entry_hidden_under_the_other_unicode_spelling_is_refused_not_created`).
A filesystem that keeps the spellings apart, ext4, never refuses: git keeps
the new file visible there. An entry *beneath* a variant of the claim, and a
sibling under a variant directory, are not candidates, since git shows both
as untracked whatever the filesystem does.

### Git output and time

Git output is read whole or not at all: an answer past the cap `skeletons`
reads from git (16 MiB) is refused, never read as though it were complete,
since a `status` cut short may have dropped exactly the record that says
dirty. Each of the five commands' output is classified by a pure function
that takes the stream as `Result<&[u8], Truncated>`, so a stream that ran
past its cap cannot be reached as bytes without handling that
(`crates/skeletons/src/subprocess.rs` → `stdout`); a unit test hands each
one a truncated stream and the same bytes uncut
(`crates/skeletons/src/work_tree.rs` →
`truncated_stdout_reads_as_git_output_too_large`, →
`crates/skeletons/src/work_tree/clean.rs` →
`truncated_status_stdout_reads_as_git_output_too_large_never_clean`, →
`crates/skeletons/src/work_tree/index_records.rs` →
`truncated_ls_files_stdout_reads_as_too_large`, →
`crates/skeletons/src/sync/proof.rs` →
`truncated_cat_file_stdout_reads_as_output_too_large`,
`crates/skeletons/src/sync/proof/listing.rs` →
`a_truncated_listing_is_refused_as_too_large_never_searched_in_part`, and
`crates/skeletons/src/sync/proof/ignored.rs` →
`truncated_output_is_refused_whatever_the_exit_status_says`).

Every git command is also bounded in time (30 seconds), and one that runs
past it is killed and reported with what it was answering: the command, the
path (or the directory, or the listing) it was reading, what most likely held
it up, and how to find out. A `cat-file` names the content filter the
repository configures for the path and `git check-attr filter`, which names it;
a `status` names a filter or a slow filesystem and says to run it by hand; an
`ls-files` names another process holding git's index, and a `check-ignore`
names a slow filesystem (`crates/skeletons/src/work_tree.rs` →
`a_command_killed_for_running_too_long_is_a_timeout_naming_the_question`,
`crates/skeletons/src/work_tree/message/timed_out.rs` →
`a_checkout_that_timed_out_names_the_path_the_command_and_the_filter`, and
`ritual/tests/sync_messages.rs` →
`a_content_filter_that_times_out_is_named_with_its_path_command_and_a_remedy`).

### Where sync refuses to run

`sync` also refuses, naming the variable, when any of `GIT_DIR`,
`GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
`GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_COMMON_DIR` or `GIT_ATTR_SOURCE`
is set in its own environment, whatever its value — each one redirects
which repository, work tree, index, object store or attribute source git
would actually answer about, so a `sync` that ran anyway could be checking
one repository and writing into another
(`crates/skeletons/src/work_tree.rs` → `open`, called once there is
something to write and before either rule is asked). The refusal exists
because inside `git commit <paths>` or `git commit -a` the index git is
using may be a temporary one, and a file written mid-commit is not part of
that commit; git exports `GIT_INDEX_FILE` to every hook it runs, so `sync`
refuses there and says to run it outside a hook, while `check`, which
writes nothing to the repository, works inside one
(`ritual/tests/sync_worktree.rs` →
`sync_in_a_pre_commit_hook_with_a_drifted_bone_refuses_and_says_to_run_it_outside`,
and →
`check_in_a_pre_commit_hook_works_and_reports_drift`). It refuses the same way, naming git's own `safe.directory`
check, when git itself calls the repository's ownership dubious. Outside a
git work tree at all, `sync` refuses outright — with no version control
there is no undo for what it would replace.

### Staging and writing

Every write is staged beside its own target before any of them is committed. The staging
file is `.<name>.skeletons-sync`, created exclusively (`create_new`: it fails on anything
already there, a dangling symbolic link included, so it never truncates and never writes
through a link). Something already at that path is refused by name and left alone,
because a leftover is something a person should see (tested by
`crates/skeletons/src/sync/write.rs` →
`a_symlink_at_the_staging_path_is_refused_and_nothing_is_written_through_it`).
A directory this user may not create in (a `create_dir` or a staging file refused
with a permission error) is named, with `chmod u+w` for it, and not left as the
operating system's "Permission denied" (`crates/skeletons/src/sync/write/staging.rs` →
`creating_in_a_directory_this_user_cannot_write_is_not_writable_and_names_it`,
`crates/skeletons/src/sync/write.rs` →
`a_directory_that_cannot_be_written_is_reported_by_name_with_nothing_written`, and
`ritual/tests/sync_messages.rs` →
`a_directory_sync_cannot_write_into_is_named_with_a_remedy`).
Before anything is staged, each write's staging name is compared with every claim, drifted
or not, by the fold the render and overlap detection use (case and Unicode normalisation
together, component by component), on every platform alike: a staging name that is equal to
a claimed path, a directory above one, or beneath one is refused with nothing written, since
on a filesystem that ignores case the staged bytes of one bone would land on another's
path (`ritual/tests/sync_free_paths.rs` →
`a_staging_name_that_folds_onto_another_claim_is_refused_before_anything_is_written`;
`crates/skeletons/src/sync/write/staging_claim.rs` →
`a_staging_name_that_folds_onto_nothing_claimed_is_free`). Claims `a/b` and `a/.B.skeletons-sync`
are the case in point: on such a filesystem `a/b` would receive the other bone's bytes and
`verify` would then fail.
Every staging file and every directory `sync` creates is recorded in the same call that
creates it, so any failure, including one between creating a staging file and filling
it, removes everything this run created (`crates/skeletons/src/sync/write/staging.rs` →
`every_created_staging_file_is_removed_when_staging_fails_after_creation`). Each is
recorded with the device and inode it was created as, and every removal (a rollback, the
backstop that runs when a panic unwinds, and the removal of a staging name after its
link) walks the path from the workspace root again, the way `check` does, and removes an
entry only if it is still that one. A directory above it swapped for a link is never
followed: the file is left, and named with its reason, so a same-named file where the link
points is untouched (→ `rolling_back_never_removes_a_file_through_a_directory_swapped_for_a_link`
and → `dropping_the_ledger_never_removes_a_file_through_a_directory_swapped_for_a_link`).
Whatever is left in place, or cannot be removed, is named in the failure message with why
(→ `rolling_back_leaves_a_file_that_is_no_longer_the_one_it_staged_and_reports_it`).
An existing file's permissions are read with `symlink_metadata`, never through a link,
and set on the staging file through its open handle.

Between proving a path and writing it, `sync` runs every later path's proof, and git runs the
repository's content filters for each of them: arbitrary commands that may touch any file in the
work tree. So each write is re-verified against what was proven, three times, by one function that
walks the path from the workspace root the way `check` does and reads the file again: every
directory above the target must still be a real directory under its claimed spelling, and a file
`sync` replaces must still hold exactly the bytes git would check out for it (one `sync` creates
must still be absent). The first look is while the write is staged, before anything is created for
it, so a directory swapped for a link never has a staging file written through it
(`ritual/tests/sync_ancestor_swap.rs` →
`a_directory_swapped_for_a_link_to_an_outside_file_with_the_proven_bytes_never_writes_through_it`).
The second is at the start of the commit, over every write, before the first lands, so a change made
while later files were staged still writes nothing. The third is immediately before each write's own
rename or link (`ritual/tests/sync_reverify.rs` →
`a_file_touched_while_a_later_path_is_proven_is_not_overwritten`;
`crates/skeletons/src/sync/write.rs` →
`a_change_found_after_the_first_write_landed_leaves_it_and_names_both_lists`). A file `sync`
replaces is renamed over its target. A file it creates is hard-linked to its target and the staging
name removed, and a link fails with `AlreadyExists` where a rename would silently replace, so a file
that appears after the last look is refused, never overwritten (`crates/skeletons/src/sync/write.rs`
→ `a_file_that_appears_after_prepare_is_not_replaced_by_commit` and →
`a_link_into_a_path_that_is_taken_is_refused_and_replaces_nothing`). A staging name that survives
its own link is named after the writes are reported and fails the run, since it is a second link to
the new file and an untracked file that would stop the next `sync`.

Every file `sync` creates lands before any file it replaces, each group in path order.
A filesystem that refuses `link(2)` refuses it for
every file, so with the links first the first one that fails does so with nothing
written; a rename that had already landed could not be taken back. Every `hard_link`
failure other than `AlreadyExists` is one cause, and the error number is not read: what
a filesystem without hard links answers differs by platform, and reading it would claim
to know why the link failed, which `sync` does not. The message says that `sync` creates
new files with hard links and carries the operating system's own error, unchanged, which
says the rest. Nothing is added to support such a filesystem, and there is no separate
probe, since the first link is the probe (`crates/skeletons/src/sync/write/landing.rs` →
`landing_order_puts_every_absent_write_before_any_held_one`;
`crates/skeletons/src/sync/write.rs` → `a_link_that_fails_leaves_every_replacement_unwritten`,
→ `a_link_that_fails_says_sync_creates_new_files_with_hard_links_and_carries_the_os_error`,
→ `a_link_that_fails_for_any_other_reason_says_the_same` and →
`a_failure_after_absent_writes_landed_names_them_in_path_order`). Only a workspace
that spans two filesystems can fail a later link after an earlier one landed, and
that reads as any partial failure does. A refusal or a failure after the first has
landed leaves the earlier ones in place, since they are correct, finished writes,
removes the rest, and names both, each list in path order. So `sync` writes every drifted bone's file or none of them *unless a write
fails or is refused after another has landed*, and then it says exactly which.
Each written file is read back afterwards through `symlink_metadata` (a regular file, not
a link), its bytes compared with its render, its spelling confirmed by the directory
listing, and a carried permission confirmed. A file that no longer reads back as written
was changed by something else in the instant after the commit: that is reported with its
path and a non-zero exit, never a panic (`crates/skeletons/src/sync/write.rs` →
`a_file_changed_after_commit_is_reported_and_not_a_panic`).
`sync` never writes outside the workspace root, including through a symbolic link at or
above a claimed path: the walk `check` uses refuses such a claim before either command
touches it (`crates/skeletons/src/claim/location.rs` →
`a_symlinked_intermediate_directory_is_refused_naming_it`), every path `sync` itself
creates is created exclusively, and each write's path is walked again immediately before
it lands, as above. The instant after that last walk is the one limit (see
[Known limits](#known-limits)).

### What sync leaves alone

`sync` makes no network request of its own: whether a skeleton is behind
has no bearing on what it writes, so `behind` is never computed for it, and
it reads the workspace with `cargo metadata --offline`, so a clone whose
skeleton sources were never fetched aborts with cargo's own offline message rather
than fetching anything on `sync`'s behalf (tested by
`ritual/tests/sync.rs` →
`sync_makes_no_network_request_as_observed_through_the_remote_query_log`,
which reads the test-only remote-query log the same workspace's `check`
does append to). The git it runs is told never to fetch a missing object
(`GIT_NO_LAZY_FETCH=1`, which git honours from 2.44; `cat-file --filters`
itself needs 2.11), so git makes no request of its own either; a content
filter that downloads is the filter's own doing (see
[Known limits](#known-limits)). It never stages, commits or
stashes anything either. It runs exactly five kinds of git command —
`rev-parse`, `status`, `ls-files`, `cat-file` and `check-ignore`, each
built with `--no-optional-locks` (`crates/skeletons/src/git/command.rs` →
`command`) — and none of them writes git's own index, refs, config or
object database (tested by snapshotting every byte under `.git/` before
and after, outside `lfs/objects/`:
`crates/skeletons/src/sync/proof.rs` →
`dot_git_is_byte_identical_before_and_after_check_clean_and_prove`). A
content filter the wearer configured in the repository runs while
`cat-file` writes a file's checkout — its smudge side, exactly as it runs
whenever git itself checks that file out — and `status` may run the clean
side when it has to compare content rather than trust a cached stat match.
git-lfs's filters are the one exception to ".git/ is unchanged": a clean
filter stores the file's content in `.git/lfs/objects/` if it is not
already there, and a smudge that has to download an object it does not hold
stores that there too, adding content-addressed files and never changing an
existing one (a failed download also leaves a log under `.git/lfs/logs/`) —
which is why `lfs/objects/` is excluded from the snapshot above, not
because `sync` itself ever writes there.

### When a skeleton is behind

Whether a skeleton counts as **behind** is read from the dependency declaration
a wearer already wrote, rather than adding a setting nobody asked for:

- a registry dependency is behind when a non-yanked, non-prerelease
  version above the one locked is published, whatever the requirement
  allows: with `rust-base = "0.3"` locked at 0.3.2 and 0.4.0 published, the
  skeleton is behind;
- a dependency pinned with `tag =` is behind the newest version tag on the
  remote past the one locked, reading `<skeleton>-v<version>` tags —
  cargo-release's own workspace default — whenever the remote holds any of
  that shape for this skeleton, and only falling back to reading the
  repository's plain `vX.Y.Z`/`X.Y.Z` tags when it holds none (tested by
  `crates/skeletons/src/behind/version_tag.rs` →
  `release_tags_on_a_many_crate_remote_reads_only_this_packages_own_prefixed_tags`).
  The prefixed shape is read whatever the repository's crate count:
  `git ls-remote` lists tags, not crates, so the count cannot be read from
  the remote, and the prefixed shape is unambiguous in a single-crate
  repository too;
- a dependency pinned with `branch =` is behind only when there is newer
  content in *this skeleton's own package directory*, past the commit
  locked — not merely a newer commit anywhere in the repository. The
  question is not whether a commit past the locked one touches the
  directory: comparing the directory's tree at the locked commit and at the
  head answers what `cargo update` would bring, so a touch a later commit
  reverts reads current, and a backward force-push that changes the
  directory reads behind;
- a dependency pinned with `rev =` is never behind — it reads as **pinned**,
  since a fixed revision was chosen on purpose and has nothing to be behind
  of.

The four cases above are not the whole rule. A `tag =` pin's
"newest version tag" excludes a prerelease tag, the same reason a
prerelease version does not count for a registry dependency, just above: a
prerelease is not a release a wearer is behind on. An unqualified `git =`
dependency naming none of `tag`/`branch`/`rev` is judged the same way as
`branch =`, against whichever branch the remote's own `HEAD` points at,
since Cargo itself follows that branch's moving head, so there is a real
"newer" to compare against. A local `path =` dependency — including one a
`[patch]` table redirects to a local directory — is never behind, for the
same reason a `rev =` pin is not: there is no remote to ask. A skeleton
taken from a registry other than the default one reads **undetermined** and
is never queried at all — see [Known limits](#known-limits). And when the
network answer needed to decide behind cannot be reached at all — a DNS
failure, a timeout, a git remote that no longer exists — the skeleton reads
**undetermined** too, never **current**: every arm of `determine` that
cannot answer returns `Behind::Undetermined` rather than falling through to
`Current`, so no path reads a failure as "not
behind" by omission (`crates/skeletons/src/behind.rs`; tested by
`ritual/tests/check_behind_registry.rs` →
`network_unreachable_reads_undetermined_and_does_not_change_the_default_exit_status`).

### Branch pins

A branch or default-branch pin is decided in two steps. First, the cheap
question every pin kind shares: `git ls-remote` reads the branch's own
remote head. When that head already equals what is locked, the pin is
**current**, and nothing further is asked — no checkout is read, no
temporary directory is created, no fetch runs (tested by
`ritual/tests/check_behind_branch_directory.rs` →
`a_branch_pin_whose_head_equals_locked_reads_current_without_ever_fetching_a_snapshot`).
Only when the head differs does the directory-scoped comparison run: the
locked side is read from Cargo's own checkout — the same checkout
`cargo metadata` already resolved this skeleton's own directory from — with
one local `git rev-parse --show-prefix HEAD HEAD:./`, giving the skeleton's
own directory prefix and its tree object at the locked commit
(`crates/skeletons/src/behind/cargo_checkout.rs` → `locked_directory`). The
remote side is the head commit alone, fetched (`--depth=1
--filter=blob:none`) into a temporary bare repository this crate creates
and removes (`crates/skeletons/src/behind/snapshot.rs` → `directory_trees`;
`crates/skeletons/src/behind/temporary_directory.rs`), never the wearer's
repository or Cargo's own checkout, both of which are only ever read. The
two directories' tree objects are compared: equal is **current**, whatever
commit history separates the locked commit from the remote head — a commit
whose own change a later commit reverts leaves the two tree objects
identical, and `cargo update` would bring nothing new, so it must read
current, never behind (tested by
`ritual/tests/check_behind_branch_directory.rs` →
`a_branch_pin_after_a_touch_to_this_crates_directory_that_a_later_commit_reverts`).
A force-push that moves a branch backwards reads behind exactly when the
content the directory would hold has genuinely changed, which is what
`cargo update` would bring, not merely because the two shas differ. A directory absent at the remote
head reads **undetermined**, never behind or current: Cargo finds a git
package by name anywhere in the repository, so a moved directory may be
unchanged, and guessing either answer would be wrong as often as it was
right. `check`'s own git removes every repository-redirecting variable
(`crates/skeletons/src/git/command.rs`), so `GIT_DIR` in a wearer's own
environment can never redirect this fetch into their own repository; `sync`
never asks any of this at all. `git` itself (already required for `sync`'s
own work-tree checks) is what asks a git remote, so it reads the running
user's own credentials and `insteadOf` rewrites exactly as their own git
does; nothing here reimplements them.

A branch or default-branch pin can read **undetermined** for two more
reasons that are neither the network nor a missing directory. Cargo's own
checkout must be readable as a git repository and sit at exactly the commit
the pin locked: one that cannot be read, or whose `HEAD` is some other
commit, reads `checkout-unreadable`, never a comparison of tree objects
taken from the wrong commit. The one command that reads it returns a
prefix, a commit and a tree, and anything else — two lines, a malformed
object id — is refused (`crates/skeletons/src/behind/cargo_checkout.rs` →
`locked_directory`, tested by `two_lines_instead_of_three_fails_to_parse`,
→ `a_bad_object_id_line_fails_to_parse`, → `a_directory_that_is_not_a_repository_is_unreadable`
and → `a_checkout_at_another_commit_than_the_locked_one_is_refused_naming_its_own_head`,
with the wording of both refusals tested in `crates/skeletons/src/behind/branch_directory.rs`
→ `a_checkout_at_another_commit_reads_checkout_unreadable_naming_both_commits`
and → `an_unreadable_checkout_reads_checkout_unreadable_carrying_the_diagnostic`). A failure creating or
initialising the temporary repository, before any network request was made,
reads `local-failure`, so a local fault is not reported as an unreachable
remote (`crates/skeletons/src/behind/branch_directory.rs` →
`finalize_branch_directory_reads_each_snapshot_failure_as_its_own_undetermined_reason`).
The temporary directory is `skeletons-behind-<pid>-<slot>-<attempt>` under the
operating system's temporary directory, created exclusively (owner-only on
Unix) so a name already taken by anything, a symbolic link included, is
never reused, and removed when its owner is dropped, whichever way the
comparison ends (`crates/skeletons/src/behind/temporary_directory.rs` →
`a_name_already_taken_by_something_else_is_retried_under_the_next_attempt`
and → `drop_removes_the_directory_and_everything_under_it`; end to end,
`ritual/tests/check_behind_branch_directory.rs` →
`a_branch_pin_reads_behind_when_a_commit_past_the_lock_touches_this_crates_own_directory`
asserts that none survives the run and that Cargo's own home is unchanged).

## Wear

Wearing a skeleton takes two pieces written into a manifest: the skeleton as a
dependency, and a `[package.metadata.skeletons.<key>]` table named for the
dependency's key, which is not the package's name when the dependency is
renamed. Both are boilerplate and the second is easy to get slightly wrong.
`wear <crate>[@<version>] [<key>]` writes both and nothing else: no option
value, which is the wearer's to state, and no file, which is `sync`'s. Nothing
about how wearing works changes. A repository that wears a skeleton by hand and
one that used `wear` are the same repository.

**Cargo does the dependency half.** `wear` runs
`cargo add --dev --package <package> …`, so a registry, a git repository and a
directory all work, the version grammar is Cargo's own, and `skeletons` reads no
source and resolves no version. Every flag that takes a value is handed over as
`--flag=value`, one argument, so a url or directory that begins with `-` cannot
be read as an option of `cargo add`
(`crates/skeletons/src/wear/cargo_add.rs` →
`a_value_that_begins_with_a_hyphen_stays_inside_its_own_flag`).

**The package it writes into is the command line's own.** A task built to
receive its command line is told which package built it, and `wear` locates that
package through ritual's workspace metadata, as ritual's own `add` does. There is
no search for a manifest, no flag to choose one, and no case of two candidates
to refuse: the command line that is running always knows which package it is.
It is also the one manifest a project is certain to have that can hold
dependencies, since a project's root manifest may be a `[workspace]` with no
`[package]`. The dependency is a dev-dependency because the skeleton is not part
of that command line's build. A command line outside the workspace it runs in
is refused, writing nothing
(`crates/skeletons/src/wear/prospect.rs` →
`a_package_that_is_no_member_is_refused_naming_the_package_and_the_root`).

**A refusal puts the files back; it does not run `cargo remove`.** `cargo add`
changes `Cargo.lock` as well as the manifest, and `cargo remove` restores
neither the lockfile nor the manifest's own formatting and comments. So both
writes run inside ritual's rollback, which records the manifest and `Cargo.lock`
before the first change and, when the run returns a failure, writes both back as
they were, byte for byte
(`ritual/tests/wear_refusals.rs` →
`a_crate_that_is_not_a_skeleton_is_refused_after_cargo_ran_and_nothing_changes`,
and `crates/skeletons/src/wear.rs` →
`a_crate_that_is_not_a_skeleton_puts_the_manifest_and_lockfile_back_byte_for_byte`).
Rollback undoes a failure and does not undo a panic, so every condition the
wearer can cause or act on, found after `cargo add`, is a returned failure,
including the two that are defects in `skeletons`
(`crates/skeletons/src/wear/confirm.rs` →
`a_workspace_that_does_not_report_the_wearing_at_all_is_a_defect`, and
`crates/skeletons/src/wear/refusal.rs` →
`a_table_that_changed_something_else_is_called_a_defect`): a panic there would
leave the project half-written. A violated invariant is still a defect that
panics, and a few assertions in the workspace reader and the process runner can
be reached after `cargo add`; a panic leaves whatever was already written.
Rollback cannot give back a change made around
it, and does not check that a file still holds what `wear` last wrote before
restoring the original, so git is the second undo, and git has to hold what it
would give back: a clean work tree is required first, and then the two files
are checked.

The clean tree is the same whole-tree question `sync` asks, through the same
code, counting every uncommitted path rather than only the two files `wear`
changes, because a person's own edits to them would otherwise be mixed into
what `wear` wrote (`crates/skeletons/src/work_tree.rs` → `open_clean`, and
`ritual/tests/wear_refusals.rs` →
`a_work_tree_with_uncommitted_changes_is_refused_and_nothing_changes`).

A clean tree says nothing about a file git is told not to read, so `wear` then
asks git's index about the manifest and `Cargo.lock`, the way `sync`'s rule (b)
asks about every path it writes: the same `ls-files -v --stage -z` question,
the same record parser and the same line for a hidden file. The manifest has to
be tracked and read from the work tree, tagged `H` and not `h`, `S` or `s`, and
a tracked `Cargo.lock` has to be too; otherwise `wear` refuses before writing,
because a dependency and a table written into a file git does not watch would
leave a lockfile that no committed manifest explains
(`crates/skeletons/src/wear/hand_back.rs` →
`a_file_flagged_so_git_does_not_read_it_is_refused_for_each_flag_and_each_file`
and →
`a_manifest_git_holds_nothing_for_is_refused_and_a_lockfile_is_allowed`;
`ritual/tests/wear_refusals.rs` →
`a_manifest_marked_skip_worktree_with_a_local_edit_is_refused_and_nothing_changes`
and →
`a_manifest_that_git_ignores_and_does_not_track_is_refused_and_nothing_changes`).
An untracked or ignored `Cargo.lock` is allowed: git never held it, Cargo
regenerates it, and refusing it would shut out every project that ignores its
lockfile (`ritual/tests/wear_refusals.rs` →
`a_lockfile_that_git_ignores_and_does_not_track_is_worn_and_then_synced`).

Last of the checks before a write, `wear` asks the operating system whether it
can open the manifest, and `Cargo.lock` when there is one, for writing in place.
`cargo add` writes through a temporary file and a rename, so it succeeds on a
read-only file, while `wear`'s table write and `rollback`'s restore both write in
place and would both fail, leaving the dependency added: neither a worn project
nor the files as they were. The question is an open for append, which creates
nothing, truncates nothing and writes no byte, rather than a read of the
permission bits, so an access control list, an immutable flag and a read-only
mount answer as they would for the real write, and a process that can write any
file passes, which is right because it can also restore. A missing `Cargo.lock`
is the `--locked` read's to refuse
(`crates/skeletons/src/wear/writable.rs` →
`a_path_that_cannot_be_opened_for_writing_is_refused_for_either_file_whoever_runs_it`,
and `ritual/tests/wear_refusals.rs` →
`a_read_only_manifest_is_refused_before_anything_is_written` and →
`a_read_only_lockfile_is_refused_before_anything_is_written`).

Every path inside the project that `wear` puts in a message is relative to the
workspace root (the root itself is shown whole, and what Cargo or git print in
their own words is theirs). `rollback` prints a path exactly as it is handed it,
so `wear` moves the process into the workspace root and hands `rollback` the
two files as relative to it. Nothing inside the rollback run depends on the
working directory: `cargo add`, the read back and every `git` question are given
the directory they run in, which is where the command was run, so a relative
`--path` still means what the wearer typed
(`crates/skeletons/src/wear.rs` → `enter_the_workspace_root`).

**Everything the workspace already answers is refused before `cargo add` runs.**
The workspace is read `--locked` first, so a stale lockfile is refused before
`cargo add` could rewrite more of it than the skeleton, and what that read holds
settles a skeleton already worn, a key a dependency already holds and a wearing
table with no dependency under it, each in `wear`'s own words and with nothing
changed. A key is compared as `rustc` names a dependency, with `-` and `_` the
same, because two dependencies that differ only there are one name to the
compiler (`crates/skeletons/src/wear/prospect.rs` →
`a_hyphen_and_an_underscore_collide_because_rustc_names_them_alike`), and a
dependency under a target-specific table holds its key like any other
(`crates/skeletons/src/wear/prospect.rs` →
`a_target_specific_dependency_holds_its_key_like_any_other`). A wearing table
is compared the same way: one at another spelling of the key is refused, since
`wear` would write a second table beside it and `sync` would refuse the pair
(`crates/skeletons/src/wear/prospect.rs` →
`a_wearing_table_at_the_other_spelling_of_the_key_is_refused_naming_it_as_written`).
The work tree is
asked last, so a request that is wrong is refused as wrong whether or not the
tree is clean.

**The key grammar is narrower than Cargo's.** A key, and the crate name that
becomes one when none is given, start with an ASCII letter or `_` and continue
with ASCII letters, digits, `-` and `_`, at most 64 bytes. Cargo accepts more
than a person means to type there, and checks a rename only after it has
written it, so a key it would then refuse leaves a broken manifest. The
narrower grammar is checked first, and it also keeps a crate spec from ever
reaching `cargo add` as an option
(`crates/skeletons/src/wear/request.rs` → `a_key_outside_the_grammar_is_refused`,
and → `a_crate_name_is_held_to_the_grammar_a_key_is_and_is_refused_as_a_crate_name`).
A key outside it can still be written by hand. The crate name and the key are
two types sharing the one grammar, so one cannot be passed for the other.

**A key that names a crate the compiler provides is refused.** `std`, `core`,
`alloc`, `proc_macro` and `test` are crates `rustc` supplies to every build, and
a dependency renamed to one shadows it in the command line's test build: `std`
replaces the standard library's prelude and `test` the harness's
`test_main_static`, so `cargo check --tests` then fails in `rustc`'s words, which
never mention skeletons, after `sync` and `check` have both passed. The key is
compared as `rustc` names it, with `-` and `_` the same, so `proc-macro` is
refused as `proc_macro` is, whether it is typed or defaulted from a crate's
name (`crates/skeletons/src/wear/request.rs` →
`the_crates_the_compiler_provides_are_refused_as_keys_explicit_or_defaulted`,
and `ritual/tests/wear_refusals.rs` →
`a_key_that_names_a_crate_the_compiler_provides_is_refused_and_nothing_changes`).
A crate of that name is fine under another key
(`crates/skeletons/src/wear/request.rs` →
`a_crate_named_for_the_compiler_is_a_fine_crate_under_another_key`).

**The table goes where `toml_edit` puts it.** The empty table is written in
place, so every comment and blank line the wearer wrote stays, after the last
`[package…]` table with one blank line around it. That placement is
`toml_edit`'s own and is pinned by tests rather than decided here
(`crates/skeletons/src/wear/table.rs` →
`with_no_metadata_the_table_goes_right_after_package`, and →
`a_trailing_comment_and_a_missing_final_newline_are_kept`). The edited manifest
is compared against an independent parse of the original before it is kept: it
must be the original plus the one empty table.

**What `wear` writes is read back by what reads it.** Before it keeps anything,
the workspace is read again through the reader `check` and `sync` use, and the
wearing at the key must come back as a worn skeleton, so a table named for the
wrong key, or a crate that is no skeleton, is refused and undone rather than
left to be found by the next `check`
(`ritual/tests/wear.rs` →
`wearing_from_a_path_adds_the_dependency_and_an_empty_table_then_sync_and_check_agree`).

**The next step names the `sync` task alone.** A task is never told the key it
is mounted under, so `wear` cannot say how its command line reaches `sync`, and
the wearer recognises the subcommand on their own command line. The same holds
for every message that asks for another run. The line opens by telling the
wearer to commit the manifest and `Cargo.lock`, because `sync` writes only into
a clean work tree and `wear` has just changed both; it names `Cargo.lock` only
when git tracks it, since `git add` refuses a lockfile git ignores.

`wear` adds a skeleton and nothing takes one off: by hand that is two
deletions, with no order to get wrong.

## Known limits

Several things about this format are limits rather than defects, recorded
here so a skeleton author or a reader of a drifted repository is not left
guessing why.

**`cargo package` decides which dotfiles ship from where a skeleton was
packaged.** When a crate is packaged from inside a git checkout, Cargo lists
its files from git, which includes dotfiles and dot-directories. When a crate
is packaged from a directory that git does not track, Cargo walks the
directory itself and skips dotfiles and dot-directories — this is Cargo's own
behaviour, not something `skeletons` controls. A skeleton whose only file
lives under a dot-directory, published from outside a git checkout, would
silently lose that file, and be left with an empty `files/`, which the render
refuses loudly rather than rendering nothing. What else Cargo leaves out, and
the check an author runs before publishing, are in the
[skeleton format's layout](skeleton-format.md#layout).

**A nested `Cargo.toml` never reaches the package.** Cargo treats any
directory holding its own `Cargo.toml` as a nested package and leaves the
whole directory out of the crate it packages — inside a git checkout and
outside one alike, unlike the dotfile case above. A skeleton with a
`Cargo.toml` under `files/` or `partials/` would render one way from its
author's checkout and another from the published crate, so the render
refuses a file or directory named `Cargo.toml` there, or any name a
case-insensitive filesystem would take for it, at any depth.

**Not every invisible-looking character hides a directive.** The set this
treats as invisible is Unicode whitespace, general category Cf (format) and
general category Cc (control) — the same general-category table this crate
already carries for other purposes, so recognising a hidden directive needs
no dependency of its own. Default-ignorable characters outside that set —
the combining grapheme joiner, a variation selector, a Hangul filler, a
Mongolian free variation selector — are not recognised, because nothing
realistic puts one of them in front of a directive, and recognising them
would need another Unicode table this crate does not otherwise need.

**What a wearer renders is the skeleton as its dependency delivers it, not the
author's working tree.** A registry dependency delivers the packaged crate,
exactly what `cargo package` produces; a git dependency delivers the
committed tree at the locked revision; only a path dependency delivers the
directory as it stands. So a skeleton repository's CI should check the
package rather than the source tree: that is what every registry wearer
receives, and it catches the dotfile limit above before a wearer does. The
check is the comparison of `cargo package --list` with what `files/` and
`partials/` hold, given in the [skeleton format's
layout](skeleton-format.md#layout), run from the skeleton's directory on a
committed tree.

**A name collision is refused only where both names can exist.** A default
macOS volume holds one of two names that differ only in case or Unicode
normalization, so the same published skeleton is refused on Linux and
renders on a Mac. Run a skeleton repository's CI on Linux, where the pair is
visible.

**`sync` narrows the time between checking a path and acting on it; it cannot close it.**
Every step `sync` takes on disk (creating a directory or a staging file, renaming or
linking a file into place, and removing what it created) resolves every directory above
its path by name at the moment it runs, and POSIX has no way to take such a step only if
those directories are still the ones `sync` checked, or to replace a file only if it still
holds given bytes. So immediately before each one, `sync` walks the path again from the
workspace root: before it creates a staging file, before it lands each write, and before
it removes anything it created. Every directory above must still be a real directory, not
a symbolic link, spelled as claimed; a file it replaces must still hold exactly the bytes
it proved git would check out, a file it creates must still be absent, and a file or
directory it removes must still be the very one it created. What is left is the instant
between each of those last looks and the step itself, across the whole time from the first
staging file to the last removal: an edit that lands in such an instant is replaced, and a
directory above swapped for a symbolic link in such an instant is followed, so a staging
file can be created, or a file of the same name removed, where the link points. Whatever
`sync` finds changed when it looks is left in place and named, never removed through a
link. Every git process `sync` runs has exited before its first look, but a content filter
can start a process of its own that outlives git, and such a process, like any other on
the machine, can act at any moment; each look catches what it did before that look, not
after it.

**`sync` creates a missing file with a hard link, and a link that fails fails the run.**
It hard-links the staging file into place, because a link fails where a rename would
silently replace a file that appeared in the meantime, and no other step gives that
guarantee. A workspace on a filesystem that refuses hard links can be checked but
not synced where a file is missing. `sync` does not tell such a
filesystem from any other cause of a failed link: it says that it creates new files
with hard links and gives the operating system's error.

**The proof holds under the attributes and filters in force when `sync`
runs.** "What git would check out" is asked of git with the work tree's own
`.gitattributes`, as git reads them for a file in the work tree. A
`.gitattributes` that is itself hidden from git (`--assume-unchanged`,
`--skip-worktree`) and differs from the committed one can make that answer
differ from what a later `git checkout`, which reads the index's first,
writes. A filter whose configuration later changes can do the same. Either
way the bytes `sync` replaced were a function of the committed object under
the configuration in force when it ran.

**Proving a file runs its smudge filter, as `git checkout` would, and
git-lfs's smudge downloads an object that is not stored locally** (a file
checked out with `GIT_LFS_SKIP_SMUDGE`, for example). That request is the
filter's, not `sync`'s: `skeletons` has no way to stop the wearer's own
filter doing what `git checkout` does, and every way of switching
git-lfs's download off changes which files it refuses. `sync` tells git
itself never to fetch a missing object (`GIT_NO_LAZY_FETCH`, honoured from
git 2.44).

**A claimed file larger than 16 MiB, as git would check it out, cannot be
proven**, since that is the most `skeletons` reads from git. `sync`
refuses it, naming the limit.

**A repository whose index lists more than 16 MiB of paths at the depth of the
files `sync` would write, or above, cannot be searched for a spelling variant,
and `sync` refuses it, naming the limit.** The listing that finds an index
entry hidden under another Unicode spelling of a claim is read whole or not at
all, and a listing cut short may have dropped exactly the entry that matters.
It holds paths only, at the depth of the deepest write and above, so this
takes a repository with several hundred thousand shallow paths.

**A `.gitignore` shipped inside `files/` applies inside `files/` too.**
Rendering never consults git, but committing and packaging do: a file under
`files/` that a skeleton's own `.gitignore` ignores renders from the author's
working tree, is never committed, and so never reaches a registry or git
wearer at all.

**A render carries bytes only; it has no file mode.** What a render yields
for a file is its content, never its permissions. The format has no way to
declare that a file should be executable, and neither `check` nor `sync`
reads or compares a wearing repository's file modes. This is a limit of the
format, not a design position: a way to carry a mode could be added. While
there is none, `check` and `sync` never have to guess whether a mode
difference is drift or a wearer's own deliberate `chmod`, which is exactly
the kind of guess this format refuses to make everywhere else.

**`options` can never be a worn dependency key.** A workspace member's
`[package.metadata.skeletons.options]` is read as that member's own declared
skeleton schema (see [skeleton-format.md](skeleton-format.md)), not as a wearing table,
whether or not the member actually is a skeleton. A member that depends on some
other crate renamed to `options` can therefore never wear it: the table is
refused, naming the rename that would fix it — see
[The wearing table](wearing.md#the-wearing-table) in the wearing reference.

**A skeleton cannot itself wear skeletons.** A wearing table is only ever
read from a workspace member's own manifest; a skeleton dependency itself
sits outside the workspace, so any
`[package.metadata.skeletons.<dependency>]` table its own manifest carries
is never seen. A skeleton that itself depends on another skeleton gets no
`check`/`sync` behaviour for that dependency — only the wearing
repository's own workspace members can wear anything.

**A skeleton taken from a registry other than crates.io always reads
`behind` as undetermined, and is never queried at all.** Nothing about how
that answer would be verified against a real second registry extends past
crates.io itself, and an answered-but-unverified case is exactly the kind
of guess this format elsewhere refuses to make. The limit is that the answer
cannot be verified, not a position that it should never be given: `behind`
could read another registry once it can be tested against one.

**A branch or default-branch pin's `behind` reads only this skeleton's own
package directory.** A commit that changes what Cargo builds without
touching that directory — a workspace-inherited field in the repository's
root manifest, most notably — is invisible to it. Read
[Check, behind and sync](#check-behind-and-sync) for the mechanism (tree
objects compared, not commit history); by the same mechanism a
backward-moving force-push reads behind only when the directory's own
content has genuinely changed.

**A tag pin's `behind` only ever counts a tag that parses as a version**, in
one of two shapes: `<skeleton>-v<version>` (the skeleton's own package
name, one `v`, then a full semver — cargo-release's own workspace default),
when the remote holds any tag of that shape for this skeleton, or plain
`vX.Y.Z`/`X.Y.Z` otherwise. There is no setting for which shape applies:
the prefixed shape is preferred whenever the remote has any tag of it for
this skeleton, whatever the repository's own crate count, since it is
unambiguous even in a single-crate repository, and the fallback reads the
plain tags of a repository that does not use it (tested end to end by
`ritual/tests/check_behind_many_crates.rs` →
`a_tag_pin_reads_behind_naming_the_newer_prefixed_tag_for_this_skeleton`
and →
`a_tag_pin_falls_back_to_whole_repository_tags_when_none_are_prefixed_for_this_skeleton`).
A release tagged some other way — `release-candidate`, `latest`, a date —
is never compared against and never counts as newer, whether or not it was
made after the one locked; a repository whose remote tags releases outside
both shapes will never see `behind` for a genuinely newer one.

**`sync` needs the locked sources already fetched.** It reads the workspace
with `cargo metadata --offline`, making no network request of its own, so a
clone whose skeleton dependencies were never fetched aborts with cargo's own
offline message rather than fetching anything on `sync`'s behalf; running
`cargo fetch` first is the wearer's own responsibility, the same as for
building the workspace at all.

## Beyond this release

Directions the design has an answer for, which the render described above
does not include and no release is promised to carry:

- **Structural claims.** A claim kind that understands a file's own
  structure — a TOML, YAML or JSON key path compared by its parsed value
  rather than by text — for the cases a whole-file claim is too coarse for,
  such as one table inside an otherwise wearer-owned manifest.
- **Extensions. Divergence is an extension.** A repository that differs
  from a shared skeleton on purpose wears a skeleton that extends it: every
  repository in one project might share a `project-base` that extends an
  organisation's `rust-base`. A one-off in a single repository is a small
  skeleton of its own, a path dependency that extends the base. There is no
  waiver file. A one-off that costs a small crate is a one-off that gets
  questioned. An extension replaces specific claims of its parent
  explicitly, each override carrying its own stated reason. Two unrelated
  skeletons making the same claim is a loud error; an extension replacing
  its own parent's claim is not.
- **Optional groups of lines.** An optional `text` option drops the one line
  that holds its placeholder. A group of lines that should appear together,
  or not at all, has a shape already in the design: a
  `# skeletons:partial` directive naming an optional `text` option, whose
  partial is inserted only when the option is set, and which may use the
  placeholder itself. It adds no syntax and no conditions, only presence,
  partials stay one level deep, and fill and select remain the whole
  grammar. It sits close to templating, so it waits for a skeleton that
  needs it.
- **An escape for a literal `{{`.** A verbatim file can hold a `{{` but cannot
  also be filled, so a file that needs a literal `{{` and options at the same
  time, a workflow holding `${{ secrets.TOKEN }}` beside a `{{cadence}}`, has no
  way to say so. An escape sequence would be the answer, and only if a file ever
  needs both at once. More templating than that stays out of scope.

None of this is part of the render described above, and nothing here depends
on it existing.
