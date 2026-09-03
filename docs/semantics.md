<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Yamlfication — Semantic Decisions

**Status:** normative for Phase 1. **Applies to:** the front end and all six passes —
`discover`, `parse`, `intern`, `link`, `check` and `emit` — every one of which is
implemented. `intern` and `emit` raise no diagnostic of their own, which is why an
earlier version of this line omitted them; they are still governed here, because
`intern` is where a member's flag prefix is taken off (D4.12) and a tag is classified
against the file's class (D6.6), and `emit` is where a member's gate is *applied*
(D4.12, D6.5) and where an edge's endpoints become an index (D4.13). A pass that
reports nothing still decides something.

Yamlfication is a graph database whose source language is **yfi**, written in `.yfy`
files and compiled by **`yamlfy`**. §11 states the language as a whole; this preamble
states only what kind of thing it is, because that decides how the rest is read.

**yfi is its own language, and `.yfy` is not YAML.** It began as one — the core of the
syntax is still a YAML superset, and §§1–3 govern that core in full — but a `.yfy` now
holds three constructs a YAML parser rejects outright: the `//` line comment, the
`<?-- … --!>` documentation block and the `<?-- … -->` code block (§10). They are
scanner errors, not extensions, which is why `.yfy` has a front end of its own and why
the file extension is a semantic declaration rather than a habit.

**`.yaml` and `.yml` are base YAML: the data the engine operates over, not the language
it compiles.** Nothing in §§6–11 is interpreted there — `extends:` is a field, `!node`
is a tag nobody reads, there is no header and there are no member flags — and that
separation is the subject of D6.6. The one thing the two classes share is what YAML
itself defines, which is exactly the scope of §§1–3.

So this document has two halves, and they answer different questions.

* **§§1–3 — what YAML leaves open.** A serialisation format does not define what a
  *graph database* should do with merge keys under cycles, with anchors redefined
  mid-document, or with positions in a stream that has already been tokenised. Those
  three gaps block the `link` pass, so they are settled here, fixture first. They hold
  for **both** file classes, because both are read by one parser over one event stream.
* **§§4–11 — what YAML says nothing about.** Diagnostics, the three inheritance
  operators, paths, the scope axes, projects, declarations, member flags and the `.yfy`
  front end are this system's own. They hold for `.yfy` alone.

Every decision below is backed by a file under `fixtures/` (one file, parsed alone) or
`projects/` (one directory, compiled as a project). The fixture is the specification;
this document explains it.

---

## 0. The premise everything else rests on

An **alias is a reference, never a copy.**

A document-level YAML loader returns an owned recursive enum. In Rust that type
*is* a tree: it cannot express a cycle without `Rc`/`RefCell` or arena indices, so
its only available behaviours are copy-on-resolve or divergence. Yamlfication is a
graph database; cycles are the subject matter, not an edge case. So the parser
consumes the **event stream**, where an alias arrives as an anchor id and nothing
else, and stores it as a reference record in a flat arena indexed by `u32`.

Consequences that hold everywhere in this document:

* A cyclic alias graph parses in bounded time and bounded memory.
  `fixtures/cycles/*` — `cycles::a_cycle_does_not_duplicate_nodes` asserts the exact
  node count.
* Any traversal of the node graph must carry a visited set. The arena's own
  `reachable_from` and `is_cyclic_from` do; anything built on top must too.
* Rendering the arena never follows an edge. `Ast::dump` prints the node *table*.

---

## 1. Merge keys (`<<`)

`<<` is **not part of core YAML 1.2.** It is a de-facto type from YAML 1.1, and the
event-level parser does not process it: it hands over an ordinary mapping entry
whose key is the plain scalar `<<`. Yamlfication therefore implements merge itself,
which means it must also decide the cases the de-facto spec never covered.

### D1.1 — What counts as a merge key (parser)

A mapping entry is a merge entry **iff** its key is

* an **untagged plain scalar** whose content is exactly `<<`, or
* any scalar explicitly tagged `!!merge`.

A quoted `"<<"` or `'<<'` is an ordinary string key. So is a `<<` carrying any other
explicit tag, `!!str <<` included.

*Why:* scalar style is the only signal the event stream gives, and it is a faithful
one — quoting is exactly how YAML says "this is data, not syntax". Without this
rule there is no way to store a literal `<<` key at all.

*Consequence:* a merge key and a literal `"<<"` key are **different keys** and may
coexist in one mapping without a duplicate-key error.

*Consequence:* the rule says **scalar**, so a collection carrying the tag — `!!merge [k]:
1` — is not a merge key. It is an ordinary complex key, and the tag on it means nothing;
that is `E0111`, argued in §5. What it is *not* is `E0211`: nothing here was read as a
merge, so there is no merge source to complain about.

Fixtures: `fixtures/merge/quoted-merge-key-is-literal.yml`,
`fixtures/merge/merge-tag-on-complex-key.yml`.
Implemented: `yfi_syntax::is_merge_key`; `Entry::merge` on every mapping entry.

### D1.2 — Resolution order within one mapping

For a mapping `m`, the resolved view `R(m)` is the **left-biased union**, highest
precedence first:

1. every key written **directly** in `m`, then
2. the merge sources of `m`, in the order they are written.

A key present at a higher precedence completely hides the same key at a lower one.

Fixtures: `fixtures/merge/own-key-wins.yml`, `fixtures/merge/sequence-precedence.yml`.

### D1.3 — Merge from an alias is transitive

`<<: *a` merges `R(a)`, the **resolved** view of `a`, not `a`'s literal keys.

*Why:* the alternative makes inheritance non-transitive — `c ← b ← a` would not give
`c` anything from `a` — which defeats the only reason to have merge at all.

Fixtures: `fixtures/merge/transitive.yml`, `fixtures/cycles/merge-acyclic-chain.yml`.

### D1.4 — Position of `<<` in the mapping is not significant

An own key wins over a merged key whether it is written before or after the `<<`.

*Why:* position-dependence here would be invisible in review and would make two
textually equivalent documents mean different things.

Fixture: `fixtures/merge/own-key-wins.yml` — the merge sits between the two own keys.

### D1.5 — Merge is shallow

Merging replaces whole values. If both `m` and its source define `nested`, `m`'s
`nested` is used as-is; the two mappings are **not** recursively combined.

*Why:* deep merge has no de-facto specification, is undefined for sequences, and is
expressible explicitly by nesting a `<<` inside the inner mapping. Shallow is the
behaviour every existing YAML merge implementation has.

Fixture: `fixtures/merge/shallow-not-deep.yml`.

### D1.6 — Legal merge sources

The value of a merge key must be

* a mapping, or
* an alias to one, or
* a `!ref` resolving to one (**added by D4.3**; unresolvable is `E0213`), or
* a **flat** sequence whose every element is one of those three.

Anything else — a scalar, a nested sequence, an alias to either — is `E0211`.

Fixtures: `fixtures/merge/inline-mapping-source.yml` (legal),
`fixtures/merge/merge-non-mapping.yml`, `fixtures/merge/nested-sequence-source.yml`.

### D1.7 — Multiple merge keys in one mapping are an error

A mapping may contain **at most one** merge key. Two are `E0210`, with a fix-it
pointing at `<<: [*a, *b]`.

*Why:* YAML requires mapping keys to be unique, and `<<: [a, b]` already exists for
merging several sources with defined precedence. Accepting two `<<` keys would mean
inventing an order rule for a construct the document is not entitled to write.

Fixture: `fixtures/merge/multiple-merge-keys.yml`.
Implemented: `E0210`, raised by the parser.

### D1.8 — Merge under a cycle is an error, not a fixed point

**A cycle in the merge graph is `E0212`.** It is reported once per strongly connected
component, naming every participating node with its span.

This is the decision the plan called out as having no inherited semantics, so here is
the argument in full.

The tempting alternative is to define `R` as the least fixed point of the merge
equations and let cycles simply converge. On *key sets* alone that works: the operator
is monotone, the lattice of key sets is finite, and Kleene iteration terminates. For
`a ↔ b` you would get `R(a) = own(a) ⊕ own(b)`, which looks reasonable.

It fails on **values**. Left-biased union does not merely add keys, it *chooses*
between competing values, and choice is not monotone. Consider — this is
`fixtures/cycles/merge-oscillating.yml`:

```yaml
c: &c { x: 1 }
d: &d { x: 2 }
a: &a
  b: &b
    <<: [*a, *d]
  <<: [*b, *c]
```

so `R(a) = R(b) ⊕ R(c)` and `R(b) = R(a) ⊕ R(d)`. `own(b) = {}`, and `own(a) = {b: …}`
— neither declares `x`, which is the only key the argument turns on.
Simultaneous Kleene iteration from the empty map gives

| iteration | `a.x` | `b.x` |
|---|---|---|
| 1 | 1 | 2 |
| 2 | 2 | 1 |
| 3 | 1 | 2 |

It **oscillates with period 2 and never reaches a fixed point.** Sequential iteration
converges instead — to whichever answer the visit order happens to produce. Both `1`
and `2` are equally defensible for `a.x`, and nothing in the document distinguishes
them.

An order-dependent answer is not a meaning. The entire purpose of compiling to a graph
image is that the same source produces the same graph, so a construct whose value
depends on the compiler's traversal order must be rejected rather than resolved.

The rule is uniform: **any** cycle in the merge graph is an error, including a
1-cycle (`&a { <<: *a }`), even though a self-merge happens to be a no-op. A rule that
fires only when the cycle is *observable* would require enumerating iteration orders,
and would make a document's legality depend on which values happened to collide.

**Recovery.** So that `E0212` does not cascade into a wall of unrelated errors, the
check pass makes the merge graph acyclic by depth-first search in document order and
**drops each back edge** — the merge edge whose target is already on the DFS stack.
Every node then has a defined resolved view, the walk terminates, and the pass never has
to bail. This recovery value is **not** a language semantic and is never emitted:
compilation fails whenever `E0212` was raised.

**Nothing derived from the recovered view is reported either, and that is stronger than
"never emitted".** A finding read off the repair is a claim about a program that does
not exist, and `W0303` is the proof. With `Base extends Patch` and
`Patch extends: !ref Base`, recovery drops one edge; the surviving one carries the
contributed key into the base; and the warning then reports the contribution as inert
*because the base already inherits it* — with a note pointing at the very line
contributing it. Nothing an author could write would satisfy that, because the
inheritance it names is the compiler's own repair. So once `E0212` has fired,
**`E0220`, `E0221`, `W0301`, `E0223`, `E0224`, `E0225`, `check`'s half of `E0213` and
`check`'s half of `W0303` are all suppressed for that compilation.**

The line is drawn at *reads a resolved view*, not at *is a warning* or *is pass 5's*, so
three things are deliberately outside it:

* **`E0212` itself**, which is the finding;
* **`E0217`** and the visibility gate in front of it (`E0216`), which ask a question of
  the scope tree and of a path, both of which are final before any view is composed;
* **the half of `W0303` that `link` raises** (D4.11), which compares a contribution
  against `own(base)` — the keys the base *writes* — and is decidable with nothing
  resolved. It is the same code and the same fault; it is simply not derived from
  anything the repair invented, so it survives. Only `check`'s half, which tests the
  base's `<<` and `extends` chain, is withheld — and that is exactly the half the
  example above is about.

**The views and the edge records are still built.** Suppression is about *reporting*,
not about computing: a caller holding a cyclic project can still inspect every resolved
view and every `!edge`'s endpoints, and ask whether the graph was cyclic. What it may
not do is emit — emission is refused outright — and what it will not receive is a
diagnostic about the repair.

**What is *not* an error.** A cycle in the *data* graph is legal and is the point of
the system. Only cycles **through merge edges** are rejected.

```yaml
base: &base { kind: base }
ring: &ring
  <<: *base      # merge edge, acyclic
  next: *ring    # data edge, cyclic — fine
```

Fixtures: `fixtures/cycles/merge-self-cycle.yml`,
`fixtures/cycles/merge-mutual-cycle.yml`, `fixtures/cycles/merge-deep-cycle.yml`,
`fixtures/cycles/merge-oscillating.yml`,
`fixtures/cycles/alias-cycle-with-merge-dag.yml` (legal),
`fixtures/cycles/merge-diamond.yml` (legal — a DAG, not a tree; `a` is reached twice
and left-biased union makes that idempotent).

### D1.9 — Merge sources are chosen positionally

The alias in `<<: *defaults` resolves under §2 like any other alias: the most recent
**preceding** definition of `defaults`. Fixture:
`fixtures/shadowing/shadow-in-merge.yml`.

---

## 2. Anchors

### D2.1 — Shadowing is positional

An anchor name may be redefined any number of times within a document. An alias binds
to the definition of that name with the greatest source position **strictly before**
the alias.

Redefining an anchor does **not** retarget aliases written earlier. They keep pointing
at the node they already named, which is a different arena node.

```yaml
first-def: &x 1
first-use: *x      # binds to `1`
second-def: &x 2
second-use: *x     # binds to `2`
```

*Why this is dangerous enough to specify:* get it wrong and links bind to the wrong
node with **no error at all** — a silently wrong graph, which is the worst failure
mode this system has.

*Implementation note.* The anchor table is keyed by **definition identity**, not by
name: `AnchorId` identifies one `&name` occurrence, and two definitions of `x` are two
different `AnchorId`s. Positional resolution then falls out of the data structure
rather than being enforced by a check that could be forgotten.

Fixtures: `fixtures/shadowing/basic-shadow.yml`,
`fixtures/shadowing/shadow-three-times.yml`,
`fixtures/shadowing/shadow-collection.yml`.

### D2.2 — Positional, not lexical

The scope of a redefinition is **the rest of the document**, not the block it was
written in. A redefinition inside a nested mapping governs aliases after that block
closes.

```yaml
outer-def: &v outer
inner:
  use-before: *v     # outer
  inner-def: &v inner
  use-after: *v      # inner
after-block: *v      # inner — the redefinition leaked out, and that is correct
```

*Why:* YAML anchors live in the character stream, not in a lexical scope tree. Any
block-scoped interpretation would be a different language.

Fixture: `fixtures/shadowing/shadow-across-nesting.yml`.

### D2.3 — Redefinition is legal, and warned about

Shadowing is valid YAML, so it is `W0300`, a **warning**, not an error. Its severity is
configurable per project — `--deny W0300` on the command line, or in `yamlfy.toml`:

```toml
[diagnostics]
severity = { W0300 = "error" }
```

The table header is part of the spelling. `severity` is a key of `[diagnostics]`, not a
top-level one, and a bare `severity = { … }` at the top of the file is rejected rather
than quietly ignored — configuration is TOML precisely so that a wrong shape is a
parse error at the line that wrote it (*an earlier draft of this decision, and of D5.3,
printed the spelling without its table and was wrong*).

The diagnostic carries **both** spans: the shadowing definition and the one it hides.

Fixture: `fixtures/shadowing/anchor-reused-not-shadowed.yml` proves distinct names do
not warn. Implemented: `W0300`.

### D2.4 — An anchor is registered when its node *starts*

`&a` takes effect at the node's start event, not at its end. A node may therefore
alias itself:

```yaml
--- !node &self
me: *self
```

This is exactly why cycles are expressible at all, and it is why the anchor table
records a definition at the collection's **start** event and attaches the node
afterwards: definition order must equal source order even when the collection closes
much later.

Fixture: `fixtures/cycles/self-alias.yml`.

### D2.5 — Forward references are errors

`*x` before any `&x` is an unknown anchor. There is no forward declaration.

*Consequence for the language:* a mutual reference must be written by nesting, so that
the outer anchor is already registered when the inner alias appears. Fixtures:
`fixtures/cycles/mutual-alias.yml`, `fixtures/cycles/deep-cycle.yml`,
`fixtures/malformed/alias-before-definition.yml`.

### D2.6 — Anchors do not cross a document boundary

Every document starts with an empty anchor table. The same name in two documents names
two unrelated nodes. An alias to an anchor defined in an earlier document is `E0130`.

*Why:* YAML 1.2 §3.2.2.2 says so, and it is what makes a document independently
compilable.

**This has a language consequence the plan does not state.** In the plan's §3 example,
`lines: [*line-a, *line-b]` appears in a different document from `&line-a`. That does
not work. Within a file, **cross-document edges must use `!ref`, and only
intra-document edges may use aliases.** Node-level inheritance via `<<` is therefore
also confined to a single document unless `!ref` is later given merge semantics.

**A cross-document alias is *rejected as an operand*, not merely reported.** An
`extends: *Base` or `<<: *Base` whose `Base` was anchored in an earlier document earns
`E0130` and then forms **no inheritance edge at all**: the operand is dropped, with no
second code. The parser's binding is real enough to index with, so keeping it would be
easy and would be wrong — the consequences print above the cause and blame a base the
compiler has just said this document cannot name. A required field of that base reports
as unsatisfied (`E0220`), an ancestry the reader cannot see appears on the `is_a` axis,
and the author is sent to fix a node that was never theirs. One illegal alias is one
diagnostic, and the graph is built as though the line said nothing.

Fixtures: `fixtures/shadowing/no-shadow-across-documents.yml`,
`fixtures/cycles/cycle-shared-across-documents.yml`,
`projects/cross-document-extends`.
Implemented: `E0130`.

**Upstream note.** `saphyr-parser` 0.0.12 clears its anchor table only on the
`Parser::load` path, not on the `next_event` path this crate uses; anchors leak across
documents there. Yamlfication enforces the boundary itself rather than relying on the
parser.

*What the crate does, once imports exist.* `AnchorTable::end_document` **restores the
imported baseline** rather than clearing the table. A header's imports are installed
once, into that baseline, before the file's first document event (D6.7); each document
then begins with exactly those bindings and no local ones, which is what D6.7 requires
and what keeps this decision intact — nothing a document *wrote* survives its end. A
file that imports nothing has an empty baseline, so restoring and clearing are the same
operation there and §2 reads exactly as written above. Implemented:
`AnchorTable::end_document`, `AnchorTable::begin_prelude` (which empties the baseline
ahead of each parser segment, so a restart after a syntax error re-installs the same
imports instead of shadowing the first set), and `parse_with_imports`.

**An alias binds through that baseline, not through the parser's anchor ids.** The two
answer different questions and the difference is load-bearing. `saphyr-parser`'s ids are
namespaced by the *stream* (see the upstream note above), so a name defined in one
document keeps answering with that document's node for the rest of the file; the
document's own table answers with the state the name is in **here** — the most recent
local definition, or the imported binding the document started with. Resolving an alias
through the parser first makes a *shadowed* import stop working: a file that imports
`&Name` and also writes `&Name` in an earlier document would get `E0130` for `*Name` in
a later one, against a name that is imported, in scope, and written correctly. So the
document's table is consulted first, and the parser's id is the **fallback** — which is
what still detects the real fault, because a name this document never bound and the
stream still knows is exactly an alias that crossed a boundary. Fixture:
`projects/import-reinstalled-after-shadow`. Implemented: `AnchorTable::in_document`.

---

## 3. The span model

Every diagnostic must be able to print `file:line:col`. That requires every arena node
to carry a position, and it requires those positions to be correct through multi-byte
text, CRLF line endings, byte-order marks and parser restarts.

### D3.1 — What a position is

```rust
struct Pos { byte: u32, line: u32, col: u32 }   // line and col are 1-based
struct Span { file: FileId, start: Pos, end: Pos }   // half-open
```

* `byte` is an offset into the **original file bytes, byte-order mark included**, so it
  can index the file on disk directly — which is what a reader comparing a diagnostic
  against `hexdump` or an editor's byte column needs. It is **not** an index into the
  text this compiler exposes: that text is BOM-stripped and therefore begins after the
  mark, so the two differ by the mark's length. The difference is published rather than
  left to be guessed at, as `byte_base` — zero for the overwhelming majority of files,
  and exactly the point of asking rather than assuming. Two offsets, one of which is not
  an index into the other, is the kind of thing that is only ever wrong once it is
  written down as one; see D3.3 and D8.4, which is where the second such pair lives.
* `line` and `col` are both **one-based**, and `col` counts **characters**, matching
  what an editor shows.

### D3.2 — Two conversions the event stream forces

`saphyr_parser::Marker` is not usable as-is, and its own documentation disagrees with
its behaviour. Measured, not assumed:

| Marker field | Documented | Actual |
|---|---|---|
| `index()` | "index (in bytes)" on the method, "in chars" on the field | **characters** |
| `col()` | "1-indexed" | **0-indexed** |
| `line()` | 1-indexed | 1-indexed |

So every marker is converted exactly once, in `SourceFile::pos`: `col + 1`, and
`index` mapped through a character-to-byte table. The table is built only when the file
is not ASCII; for ASCII the character index *is* the byte offset and the table is
`None`. This is checked by `span::tests::multi_byte_characters_shift_byte_offsets`.

### D3.3 — Byte-order marks

`saphyr-parser` does not strip a leading BOM; it becomes part of the first scalar and
usually derails the parse. Yamlfication strips it and records a `byte_base` of 3, which
is added back into every `Pos::byte`. Line and column are unaffected.

Fixture: `fixtures/spans/bom.yml`.

### D3.4 — What a node's span covers

* A **scalar**'s span is its content. Anchor and tag properties are *not* included —
  the event stream does not put them there, and a diagnostic about a value should point
  at the value.
* A **collection**'s span runs from its start event to its end event, so it strictly
  contains every child. Asserted for the whole corpus by
  `spans::a_collection_span_covers_all_of_its_children`.
* An **alias**'s span is the `*name` token exactly.
* An **anchor definition** has its own span, covering the `&name` token, held on
  `AnchorDef`. That is what `W0300` points at.
* A **document**'s span runs from its start to its end.

### D3.5 — Anchor names are recovered from the source, and that is checked

`saphyr-parser` reports an anchor as an opaque numeric id and **never yields the name
that was written**. The name is required — under the plan's §3 it is the node's
identifier within a document — so it is read back out of the source text.

The read is bounded rather than a guess. When an event carries a non-zero anchor id,
the region between the previous event and this node's content can hold only separation
white space, comments, a tag property and the anchor property. That region is scanned
forward for the last `&name` token, skipping quoted scalars and comments.

Two guards make the recovery honest rather than hopeful:

* Recovered anchor positions must be **strictly increasing**, because the parser
  assigns ids in definition order. A violation is `E0121`.
* If no token is found where one must exist, it is `E0120`. The parse continues; the
  anchor is recorded with an empty name so aliases still bind.

Neither has fired on the corpus or under fuzzing. They exist so that a future parser
change is caught rather than silently producing wrong node identifiers.

### D3.6 — Positions survive error recovery

A `saphyr-parser` event stream cannot be resumed after an error: the scanner's state is
gone. To still report more than one syntax error per file, the parser is **restarted**
on the remainder of the file from the next `---`, and every marker it then produces is
rebased by a character offset and a line offset onto the original file. Asserted by
`spans::spans_survive_a_parser_restart_after_a_syntax_error`.

---

## 4. Diagnostics

Diagnostics **accumulate**. A pass runs to completion and reports everything it found.
The one thing that cannot accumulate is a syntax error within a single document, since
the event stream is not resumable; recovery restarts at the next document boundary, so
each malformed document costs one `E0100` and the intact documents around it are still
parsed and kept.

| Code | Default | Meaning | Raised by |
|---|---|---|---|
| `E0100` | error | malformed YAML | parse |
| `E0101` | error | source is not valid UTF-8 | parse |
| `E0102` | error | source cannot be read | parse |
| `E0103` | error | recovery budget exhausted | parse |
| `E0104` | error | unterminated `<?--` block (D8.4) | parse |
| `E0110` | error | duplicate mapping key | parse |
| `E0110` | error | two keys naming one member (D8.5) | link |
| `E0111` | error | `!!merge` on a non-scalar key (D1.1) | parse |
| `E0120` | error | anchor name unrecoverable (D3.5) | parse |
| `E0121` | error | anchor recovery out of order (D3.5) | parse |
| `E0130` | error | alias crosses a document boundary (D2.6) | parse |
| `E0210` | error | more than one merge key (D1.7) | parse |
| `W0300` | warning | anchor enters a new state (D2.3, reframed by D5.3) | parse |
| `E0211` | error | illegal merge source (D1.6) | link |
| `E0212` | error | cyclic inheritance (D1.8, D4.10) | check |
| `E0213` | error | path names nothing (D4.3, D4.12) | link |
| `E0213` | error | an alias endpoint names a value, not a node (D4.13) | check |
| `E0214` | error | conflicting extended references (D4.11) | link |
| `E0215` | — | **retired, and never reused.** `!ref` into a file this file does not import | — |
| `E0216` | error | path into a scope this scope cannot see (D4.12) | link |
| `E0217` | error | `!ref` into a scope this scope may not write (D4.12, D6.5) | check |
| `E0218` | error | path addresses a member its target does not hold (D4.12) | link |
| `E0219` | error | a `!ref` binding shadows a definition of its own file (D4.12) | link |
| `E0220` | error | required field unsatisfied (D7.3) | check |
| `E0221` | error | declared-tag mismatch (D7.3, D4.8) | check |
| `E0222` | error | `!oneof` is reserved, not implemented (D7.4) | discover |
| `E0223` | error | an `!edge` relates nothing (D4.13) | check |
| `E0224` | error | an edge's `connections` or `definition` has the wrong shape (D4.13) | check |
| `E0225` | error | a `definition` handle names no position (D4.13) | check |
| `W0301` | warning | undeclared field on a concrete node (D7.3) | check |
| `W0303` | warning | inert extended-reference contribution (D4.11) | link, check |
| `E0230` | error | conflicting scope declarations (D6.1) | discover |
| `E0230` | error | duplicate *definition* in a directory (D6.1) | link |
| `E0231` | error | bad header axis value (D6.4) | discover |
| `E0240` | error | unresolved import (D6.7) | discover |
| `E0241` | error | import target not visible (D6.7, D6.5) | discover |

**This table is the whole vocabulary.** Every code the compiler can raise is listed
here and in `Code::all()`, which is what `--deny` validates a configuration against; a
code in one and not the other would be either a diagnostic no project can configure or
a configuration key that silences nothing. The numbering after `E0225` resumes at
`E0230`, which begins the scope block; the gap is a block boundary and holds nothing.
Two numbers appear elsewhere in this document and are deliberately **not** in the table:
`E0215`, retired below and never to be reused, and `W0302`, which §5 records as a
deferred idea and for which **no number is allocated** — the spelling there is a label
for a discussion, not a code the compiler knows.

Nothing from `E0211` down is implemented in `yfi-syntax`; each needs more than one
file. `E0211`, `E0213`, `E0214`, `E0216`, `E0218`, `E0219` and `E0230`'s
duplicate-definition condition are **raised by `link`** (pass 4), which walks every path
against the project's scope tree and builds the inheritance graph. **`E0212` is raised
by `check`** (pass 5), which runs SCC over that graph; `E0217`, `E0220`–`W0301` and the
three `!edge` codes are `check`'s, `E0217` because it is asked of a target that already
resolved and the rest because they need resolved or declared views.

**`E0216` is `link`'s and not `check`'s, and that placement is a semantic decision.**
Visibility is asked *inside path resolution*, in front of the lookup — see D4.12 — so an
invisible landing resolves to nothing rather than resolving and then being complained
about. A gate that only decorates a diagnostic is not a gate: with the lookup first,
`vault/Secret.password`, `vault/Secret.nosuch` and `vault/NoSuchNode` earn three
*distinguishable* answers, and between them an outsider enumerates a private scope's
node names and each node's member names. Its sibling `E0217` stays in `check` because it
is asked of a target that is already in view, which is exactly what makes the ordering
of the two axes structural instead of conventional.

**`E0213` is `link`'s with one exception.** An `!edge` endpoint written as an **alias**
to an anchored *scalar* names a value rather than a node (D4.13), and that is `E0213` —
the same fault a path naming the same anchor earns — but it is raised by `check`, because
an alias is not a path and never became a reference: pass 4 never saw it. One code, two
passes, over disjoint inputs, exactly as `W0303` is below.

**`E0213` and `E0218` are two codes because the fixes are opposite.** `E0213` says the
walk did not land — no such directory, no such peer, no such definition, or a `..` past
the root — and the author's next move is the path. `E0218` says the walk landed and the
`.` step did not, and the author's next move is the member name. One code would make the
commonest question about a reach — *did I get the address wrong, or the field?* — the
one thing the diagnostic did not answer.

**`E0215` is retired, not renumbered, and it stays retired.** It fired when a `!ref`
addressed a file the referencing file had not imported, and D4.12 no longer has that
rule: the path *is* the reach, so there is nothing left to be out of step with. The
number is burned rather than reused, because a project pinning `--deny E0215` in
configuration should get a clean "unknown code" rather than a silent redirection to an
unrelated rule. So the block runs `E0213, E0214, —, E0216, E0217, E0218, E0219, …` and
the hole stays a hole: `E0219`, the newest code here and the obvious candidate for
filling it, was allocated at the **end** instead. A code that once meant one thing and
now means another makes every configuration file and every archived report naming it
wrong without saying so, and it does that silently, which is the whole objection.
**No retired number is ever reused**, here or later.

**`W0303` is raised by both passes over disjoint inputs**, because D4.5's additivity
rule spans two things only one pass can see. `link` reports a contributed key the base
**writes directly**, which is decidable with nothing resolved and is the common case;
`check` reports one the base holds only **through its own `<<` or `extends` chain**,
which needs a resolved base. The sets do not overlap, so no contribution is warned about
twice.
`E0222`, `E0231`, `E0240` and `E0241` need only a file class or a project
tree, both of which `discover` holds, and are raised there in `yfi-core` — `E0241`
alongside `E0240`, in import resolution, because the scope tree is final by then and
the binding pass would report a cyclic component's import once per rebinding round.
The rest are listed so the numbering is stable. **No code is
allocated for an import cycle**, which is legal — D6.7 argues why, and why it must not
be folded into `E0212` if that ever changes. `E0212`'s message text changes with D4.10: it is **cyclic
inheritance**, not cyclic merge, because a user whose file contains no `<<` at all
should not be told their merge is cyclic.

**`E0230` is one code over three conditions, raised by two passes.** `discover` raises
it for the two *declaration* conflicts it can decide from headers and directories alone:
files in one directory disagreeing about an axis, and one namespace claimed by two
directories (D6.1). The **duplicate definition** — one *directory*, one name, two files,
whether or not either declares a namespace (D6.1) — is raised by `link`, for the same
reason `E0213` is: it is detected when the definition
table is built, and building that table is what resolving a path means. It could not be
answered earlier without first deciding *which* anchors are addressable, and that
decision is now made: **an anchored node that can be a parent scope — a collection — is
addressable; an anchored scalar is a value, not a type, and is addressable by nothing.**
Being addressable is not the same as being a model, so
D7.1 is untouched: `&line-a` in an invoice's `lines:` is referenceable without being
emitted as a model of its own.

**Why `W0301` is a warning and not an error.** An undeclared field on a concrete node
is how a field-name typo presents. `prot: 8080` next to an ancestor declaring
`port: !!int 80` produces a node that carries a junk key *and* silently keeps the
inherited `80` — a silently wrong graph, which D2.1 already names as the worst failure
mode this system has, reached here by the most ordinary mistake there is. Nothing else
in the design catches it: D1.2 is happy to carry an extra key, and D7.3's required
check is satisfied by the inherited default. So it must be reported. It is not an
error because the model is open-world — a concrete node carrying data its ancestors
did not anticipate is the normal case, not a violation — and a project that wants the
closed reading writes `--deny W0301`. `W0301` is raised only for a node that has a
declared view to be undeclared *against*, i.e. one with at least one abstract
ancestor; a concrete node with no ancestry declares its own shape and cannot deviate
from it. What counts as declared includes anything an extended reference has installed
on an ancestor, so the verdict is project-wide and can be changed by another file —
see D4.11, which is where that cost is argued.

**`E0110` is one code over two conditions, raised by two passes.** `parse` compares
keys by their **text**, which is the right rule there and the only one available: the
parser does not know the file's class and cannot know which text is a member flag and
which is a name. `link` does, so it raises the same code for two keys whose texts
differ and whose **member names** do not — `port:` beside `pub port:` (D8.5). One code
because it is one fault with one fix, in two places because only the second pass can
see the second spelling of it.

**Duplicate key identity.** Merge keys and ordinary keys are counted separately,
because D1.7 bounds merge keys by *role* while D1.1 identifies ordinary keys by
*text*.

- Two **ordinary** keys collide when they are both scalars with the same text.
- Two **merge-role** keys collide whatever their key text: `<<:` together with
  `!!merge zz:` is two merge keys in one mapping (`E0210`), even though the texts
  differ. Counting them by text would let that pair through, and the link pass
  would then receive a mapping with two merge sources and no defined precedence
  between them — the ambiguity D1.7 exists to forbid.

Holding the two apart is also what preserves D1.1's coexistence rule: a literal
`"<<"` key never meets the real merge key in the same table, so the two remain
different keys. **`extends` has no such shelter and no such coexistence**: it is bucketed
by nothing, so `extends:` beside `"extends":` is one text twice and is `E0110` — see
D4.2, which corrects the analogue it used to claim.

Non-scalar keys are not compared: deciding whether two mappings are the same key
needs resolved values, which the parser does not have. A merge tag on a
*non-scalar* key is not a merge key (D1.1) and is currently reported by nothing —
see §5.

Fixture: `fixtures/malformed/duplicate-key.yml`.

**Severity is decided once, by the pass that raises the finding.** Configuration is
handed to `discover`, `link` and `check`, and each applies it as it records; that
placement is forced rather than tidy, because `allow` suppresses **recording** and a
collection cannot un-record a diagnostic it never received. Everything downstream —
the command line merging three passes' collections into one — preserves the severity
each item already carries and re-decides nothing. So there is exactly one place that
answers *how serious is this*, and `--allow` costs nothing at all rather than costing a
filter at print time.

**A compilation renders one report, from one source map, ordered by position.** A
`FileId` is an index into that map, so a second map would restart at `FileId(0)` and
every span from the first project would then name the second project's files. Findings
arrive by *pass* rather than by position — every file's parse diagnostics, then
everything the project-wide passes found — so insertion order routinely prints a
consequence above its cause and interleaves files arbitrarily. The report is therefore
sorted by `(file, line, column)` across the **whole invocation**, which is D6.3's
`(file rank, document index, node index)` expressed in the terms a diagnostic carries: a
file's identifiers are assigned in discovery order, and within a file position ascends
with document and node index. A diagnostic with no span sorts last, because it belongs
to no position and putting it first would push the file it is about below it. Ties keep
the order they were found in; the sort is stable. Rendering once is also what makes the
warning count and the exit code cover the invocation rather than a fragment of it.

**Diagnostics derived from a recovered view are withheld.** When `E0212` fires, nothing
read off the repaired graph is reported at all — see D1.8, which owns the rule and the
exact list.

---

## 5. Open decisions

Recorded, not decided. Each needs an explicit answer before the pass that depends on it.

* ~~**File extension.**~~ **Answered twice, on both of its axes.** `.yfy` is yfi
  source; `.yaml`/`.yml` is base YAML the engine compiles or runs over.
  * On the **semantic** axis, D6.6: the same bytes need two readings, because
    `extends:` must be an operation in one class and an ordinary field in the other,
    and no signal inside the file decides that without guessing.
  * On the **syntax** axis, §10: **the trigger has fired.** This item recorded one —
    "the first construct a YAML parser would reject" — alongside the claim that it
    "has not fired and may never fire". It fired on `// comment`, `<?-- … --!>` and
    `<?-- … -->`, each of which is a `ScannerError` in a real YAML parser, verified
    against one rather than reasoned about. The earlier answer, that `.yfy` was
    unnecessary because everything was native YAML with zero custom lexing, is now
    **false in its premise as well as answering the wrong question**: there is custom
    lexing, it is `.yfy`-only, and it is specified in §10.
  * What did **not** change is the event-level foundation. §§1–3 still hold for both
    classes, because the pre-pass is a character-for-character substitution that hands
    the parser a text of the same length with the same line breaks (D8.4), so the two
    classes still meet one parser over one span model.
  `discover` still filters by a configurable extension list, so the spellings are
  configuration; what is normative is that there are **two classes with different
  readings**, and that only one of them is a language.
* **Which GNU licence.** The project ships GPL-3.0-or-later. That choice has a
  consequence the plan's Phase 3 will run into: a Go server layer over a C ABI into
  this core is a combined work under the GPL, so the server must be GPL too. If the
  core is meant to be embeddable in closed software, that wants **LGPL-3.0**; if it is
  meant to stay copyleft across a network service, that wants **AGPL-3.0**.
  **Needs your answer before Phase 3, not before Phase 1.**
* ~~**Does `!ref` participate in merge?**~~ **Answered: yes, and then superseded by
  the path amendment.** D2.6 confines `<<` to one document *by operand*, not by
  operator: `<<: *alias` stays document-local and `<<: ../peer/Base` crosses files. D1.6 gains `!ref` as a legal merge source, and
  **D1.8's cycle rule spans files** — one inheritance graph, one cycle rule, so a
  cycle formed half by `<<` and half by `!ref` is still `E0212`. This requires a
  normative total file order (root-relative path, compared by component — D6.2, which
  corrects both halves of the parenthesis this line first carried); without one,
  `readdir` order decides which back edge recovery drops, and therefore which
  *other* diagnostics get reported. **Written up as §6 (D4) and §8 (D6.2).**
* ~~**There is no closed cross-file inheritance.**~~ **Answered: the header import
  (D6.7).** The gap was real — an extension's operand is an alias, aliases do not cross
  a document (D2.6), and the only cross-document operand was `!ref`, which selects the
  global operation — so the design pushed users toward the largest blast radius exactly
  where the bounded operation was most wanted. The answer is not a fourth operator; the
  operator set is closed at three (§6). A header imports the other file, its definitions
  become definitions of this document, and `extends: *Service` is then an ordinary
  closed extension. **The file boundary is crossed by the import, not by the
  operation**, which is why D2.6 survives verbatim rather than being relaxed. See D4.4
  for the operation's side and D6.7 for the import's.
* **`W0302` inconsistent inheritance order.** Deferred, not rejected, and **not
  allocated**: the spelling is a label for this discussion and `W0302` is not a code the
  compiler knows, so a project cannot configure it and a reader should not go looking for
  it in §4. Two ancestors
  that appear in one relative order along one inheritance path and in the opposite
  order along another have no consistent linearisation — this is exactly the
  monotonicity failure Python 2's MRO had, and D1.2 resolves it silently by written
  order rather than reporting it. It is worth reporting because it goes quiet
  precisely where it is hardest to see: with both parents in one file a reader can
  compare the two source lists, and with the parents in *different* files reached by
  `!ref` (D4.3, D4.5) there is no single place the contradiction is visible at all.
  The
  check is cheap and needs no new machinery — a pairwise scan over each node's
  flattened source list, flagging any pair whose relative order contradicts a pair
  already seen. That is deliberately weaker than C3 linearisation: it reports the
  inconsistency and still resolves by D1.2, rather than adopting an algorithm whose
  failure mode is a hard compile error on a hierarchy the author considers
  reasonable. **Adopt only if cross-file diamonds appear in practice.**
  `fixtures/cycles/merge-diamond.yml` is consistent and would not fire; no fixture
  poses the inconsistent case.
* ~~**Nothing checks visibility on a `!ref`.**~~ **Answered: D4.12.** An import
  reaching a private scope was `E0241`, but a reach into one resolved silently, so the
  whole visibility system could be bypassed by spelling a reach `!ref` instead of an
  import. A path now reaches only a **visible** target (`E0216`), a `!ref` reaches only
  a **writable** one (`E0217`), and access to a member is a relation between the
  accessor and the holder rather than a flag on the node. **Written up as D4.12.** The
  import requirement that first answered this — `E0215` — was retired when the path
  became the reach; it was the right instinct wearing the wrong mechanism, because it
  asked a *declaration* to guard something a *syntax* can guard.
* ~~**A merge tag on a non-scalar key has no diagnostic.**~~ **Answered: `E0111`, and
  the other half of it is answered the other way, on purpose.** The item asked one
  question and there were two in it.

  **The tag is diagnosed.** D1.1 says a merge key is a *scalar* tagged `!!merge`, so
  `!!merge [k]: 1` is correctly not a merge key — and what it then becomes is an ordinary
  complex key, with the tag classifying nothing, resolving nothing and consumed by
  nothing. That is a **tag that means nothing**, which is the exact thing D7.4 refuses
  for `!oneof` and D4.13 refuses for `!edge`: a reservation with no diagnostic behind it
  is not a reservation, and a spelling that silently does nothing leaves the author
  believing they wrote a merge. The two candidate answers the item recorded are still the
  two candidates, and the second is the right one: `E0211` would read the tag as a
  *mistaken merge* and complain about the operand, which is wrong if the author meant a
  complex key, while a code of its own says only what is certainly true — the tag has no
  meaning in this position — and is right under either reading of the intent. It is
  `E0111`, next to `E0110` because it is the same concern at the same level: mapping keys,
  decided by the parser with nothing resolved.

  **The duplicate is not diagnosed, and that is now a decision rather than an omission.**
  Two `!!merge [k]: 1` entries in one mapping are still not reported as duplicates — and
  neither are two untagged `[j]: 3` entries, which is the tell: the silence never had
  anything to do with the tag. Key identity in this language is a key's **scalar text**,
  which is the only identity a parser holds; comparing complex keys needs resolved
  values, and the parser has none. Reporting the tagged pair and not the untagged one
  would make `!!merge` change what *duplicate* means. The rule is bounded — a non-merge
  key is absorbed as data, so this can only ever be silence and never a wrong graph — and
  it is **permanent** at this level: if complex-key identity is ever wanted it belongs to
  a later pass that has values, not to `check_keys`.

  *Fixture:* `fixtures/merge/merge-tag-on-complex-key.yml`, which writes both pairs and
  earns exactly two diagnostics, one per tagged key. The item's own objection — "deciding
  it needs a case, and the corpus has none" — was answered by writing the case.

---

## 6. Inclusion, extension, extended reference

§1 answers *what keys does this node end up with*. It does not answer *what is this
node*, and it does not answer *what does writing this node change elsewhere*. Those
are three questions, so there are three operations, and the language names them:

| written | name | meaning | what it changes besides A |
|---|---|---|---|
| `A` with `<<: P` | **inclusion** | A has P as one of its members. A makes no claim about what P is anywhere else. | nothing |
| `A` with `extends: P` | **extension** | A is a type of P, within the context of the parameter. | nothing |
| `A` with `extends: !ref P` | **extended reference** | A is a direct extension of P *itself*: **P depends on A**, and every P in the program carries A's definition. | every P |

`P` is a **path** (D4.12) — `../shared/Service`, `peer/Service`, `Service`,
`Service.port`. It may also be an alias or an inline mapping in the first two rows;
what it may never be is absent, because naming is how reaching happens.

Read the right-hand column first. **Two of the three are safe and one changes the
world.** Inclusion and extension both leave their operand exactly as it was; only the
extended reference reaches back into the base. Everything else in this section is a
consequence of that asymmetry.

**The tag selects the operation, and the path only says where.** `extends: P` and
`extends: !ref P` name the **same node** and are not the same operation. The natural
guess — that `!ref` is "the linked version", the same inheritance reaching across a
file boundary — is wrong, and it is wrong in the dangerous direction: the path already
crosses the boundary, so what `!ref` adds is not reach but **intent**. `!ref` is a
declaration that this context intends to *modify* the target, and it is checked as one
(`E0217`). This is stated first because it is the one misreading that produces a
silently wrong graph across an entire project.

**The set of *inheritance operators* is closed at exactly these three.** There is no
fourth — no `!use`, no `!from`, no `A extends B from C`. This is a specification of the
language, not a description of what happens to be implemented, so a later reader finding
a case the three do not cover should not read that as a gap awaiting a fourth spelling.
Every remaining question about reach — how a definition in another file becomes
available at all — is answered by the **path** (D4.12), which is a property of the
*name*, not of the operators. Keeping the operator set closed is what makes the table
above exhaustive and therefore learnable: three spellings, three blast radii, and
nothing else to check.

**Say exactly what is closed, because the language has grown since and will again.**
The claim is about the three *operators* and about nothing else. It is not a claim that
the tag vocabulary is closed, and reading it as one would be a mistake this document
invited: `!edge` shipped after the sentence was written, and it is a fourth **node
kind**, not a fourth operator. The difference is the whole point of the sentence. An
operator answers *what does writing this node change elsewhere*, and the three answers —
nothing, nothing, and every P — exhaust the blast radii there are. A node kind answers
*what is this node*, and `!edge` answers it with "a node whose content is what it
connects" (D4.13): it inherits with the same three operators, is validated by the same
checks, and adds no fourth thing to read a clause for. So the closure holds as stated
and the correct statement is:

> **Three inheritance operators, closed. Four node kinds — `!type`, `!node`, `!edge`
> and untagged — not closed, and never claimed to be.**

### D4.1 — The three operations

**Inclusion, `<<:`.** A has B as one of its properties. B's keys are absorbed into A;
B is unchanged; nothing else in the program observes anything. This is merge exactly
as §1 specifies — D1.1 through D1.9 apply verbatim, the operator is untouched, and
only the set of legal operands grows (D4.3).

Inclusion is **compositional, not definitional**, and this is worth saying plainly
because readers arriving from other languages will expect merge to be a kind of
inheritance. It is not. `A << B` says *A has a B in it*. It says nothing about what B
is outside of A, it creates no is-a relationship, and no query over the `is_a` axis
will ever return A for B. A node that includes `water` is not a water.

**Extension, `extends:` with a path, an alias or an inline definition.** A is a type of B,
**within the context of the parameter** — the claim holds where it is written and
nowhere else. A appends B's definition to itself and retains the ancestry as a
first-class, queryable `is_a` edge. B is untouched; no other node's resolved view
moves by so much as a key. The blast radius is exactly A.

That boundedness is the entire difference from the third operation, so it is worth
stating in the negative: an extension does **not** assert that A is a type of B
globally, does not register A with B, and does not make B aware that A exists. It is
a claim A makes about itself, in its own context.

**Extended reference, `extends:` with `!ref` on a path.** A is a direct extension of B itself.
**B depends on A.** Every node that is a B — the ones already written, the ones in
other files, the ones being written right now by someone who has never read A — now
carries A's definition. This is Swift's `extension` and Ruby's open class, not
subclassing. The blast radius is every B in the program.

*Why the language has this at all:* an open-class extension is how a family acquires a
property after the fact without editing the file that defines the family — which, in a
graph a whole organisation writes into, is often the only available move. It is
powerful for the same reason it is dangerous, and the design's answer is not to remove
it but to make it **look different from the safe operation at the point of writing**
and to make the base **agree in advance**. Since the path amendment the extended
reference is also *gated*: it is a write, so the base's scope must be `mutable` from
here or the operation is `E0217`. An organisation that does not want its families
reopened does not have to police reviews for a tag; it says nothing, because
`immutable` is the default (D6.4).

*Fixtures:* `projects/link-inert-contribution` and `projects/link-conflicting-extends`
write the extended reference; `projects/check-ref-reach` writes the gate that now
stands in front of it.

### D4.2 — What counts as an inheritance key

A mapping entry is an `extends` entry **iff** its key is an **untagged plain scalar**
whose content is exactly `extends`. A quoted `"extends"` or `'extends'` is an ordinary
string key, and so is `extends` carrying any explicit tag.

This is D1.1's rule with a different spelling, and the escape mechanism is the one
already fixtured at `fixtures/merge/quoted-merge-key-is-literal.yml`.

**The file class carries most of the weight the escape used to carry.** The obvious
worry — that `extends` is a plausible *field* name, where `<<` is a plausible field
name nowhere — is largely answered by D6.6 before the escape is reached: the operators
are interpreted only in `.yfy`, and a `.yaml` file's `extends:` is an ordinary field
by class, needing no quoting and no thought. Data whose schema happens to contain the
word lives in the file class where the word is data.

So the escape is retained for the residue, and the residue is real: a `.yfy` file
whose *subject matter* is a system that has its own `extends` — a compiled model of
another language's type system, a build tool's configuration schema — must be able to
write the field as a key. That is not an exotic case for a type-and-graph compiler; it
is one of the things people will point it at. Retaining the escape costs nothing,
because the rule is D1.1's and is already implemented for `<<`, and removing it would
make one English word unusable as a key in a language built to describe other systems'
vocabularies.

**The rule must not consult the operand, and that is the point.** It is tempting, now
that the operand selects the operation, to recognise `extends` only when its value is
a legal operand and treat it as data otherwise. That coupling is exactly wrong: it
would make a mistake in the *value* silently change whether the *key* is an operation
at all, and the failure it produces is a node that quietly stopped inheriting with no
diagnostic — D2.1's worst failure mode, reached by a typo. So the key is decided by
text and scalar style alone, and an `extends` entry whose operand is illegal is
`E0211`, reported, not reinterpreted.

***Not* a consequence, and this is where D4.2 stops being D1.1 with a different
spelling.** An earlier draft of this decision claimed the analogue of D1.1's coexistence
rule — that an inheritance key and a literal `"extends"` key are different keys and may
sit in one mapping. **They may not. `extends:` beside `"extends":` is `E0110`**, and the
compiler raises it.

The rules differ because the two keys are found by different means, and the escape does
different work in each:

* **`<<` is bucketed by *role* at parse time.** D1.1 classifies each entry as merge or
  ordinary *before* uniqueness is tested, and the two buckets are counted separately —
  which is what D1.7 needs, since a merge key must be bounded whatever its text (§4,
  "Duplicate key identity"). A literal `"<<"` therefore never meets the real merge key
  in the same table, and the coexistence falls out of the bucketing rather than being
  granted.
* **`extends` is an ordinary key.** No pass buckets it; the parser sees one mapping with
  two entries whose key text is `extends`, compares texts as §4 says it must, and
  reports a duplicate. It could not do otherwise without knowing the file's class, which
  is precisely what §4 says the parser does not know.

So the escape in D4.2 is narrower than D1.1's and is still worth having: it lets a
`.yfy` write `"extends":` as a **field**, which is the case the paragraphs above argue
for, and the field is then an ordinary key like any other. What it does not do is let
the same mapping hold the operation and the field at once. That is a real restriction
and it is stated rather than papered over — a node modelling another language's type
system writes its `extends` field in a mapping that does not also extend something, or
nests it, and neither is a hardship next to a duplicate-key error nobody can explain.

**D1.7 needs no analogue.** `extends` is an ordinary key by text, so two `extends:`
entries in one mapping are already `E0110`. D1.7 exists only because a merge key is
identified by *role* and therefore escapes text-based duplicate detection (§4,
"Duplicate key identity"); `extends` does not. Multiple operands are written
`extends: [*a, *b]`, exactly as D1.7's fix-it already directs for merge.

### D4.3 — Inclusion is unchanged, and a path under `<<` is not an extended reference

D1.6 gains one clause: a **path** resolving to a mapping is a legal merge source,
alongside a mapping and an alias, including as an element of the flat sequence form. A
path that resolves to nothing is `E0213`; one that lands and then misses a member is
`E0218`; one that resolves to a non-mapping is `E0211`. This is what §5's answered open
item promised, and it is the whole of the change to §1.

`<<: ../peer/Base` is **cross-file inclusion**, and it is safe. It absorbs the
referent's keys into A and changes nothing about the referent. Under `<<`, the operand
carries only *scope* — `*alias` is document-local (D2.6, unchanged), a path walks the
project — exactly as D2.6 already established for data edges.

**The argument that the operand selects the operation still holds; what the operand
ranges over has changed.** It used to be *alias versus `!ref`*, and that reading is now
wrong in a way worth naming, because it made `!ref` mean two unrelated things at once —
"this crosses a file" and "this reaches back into the base". Those came apart the moment
a path could cross a file on its own. The operand is now *path versus `!ref`-path*:

| written | operation | direction of dependency |
|---|---|---|
| `<<: *alias` | inclusion, document-local | A → B |
| `<<: P` | inclusion, anywhere in the project | A → B |
| `<<: !ref P` | inclusion, **plus** the declaration that A intends to modify B | A → B **and B → A** |
| `extends: *alias` | extension, document-local | A → B |
| `extends: P` | extension, anywhere in the project | A → B |
| `extends: !ref P` | **extended reference** | A → B **and B → A** |
| `key: P` | data edge | A → B |
| `key: !ref P` | data edge, **plus** the same declaration, bound to that key | A → B **and B → A** |

Every row carrying `!ref` has an edge pointing back into the operand, and those are the
rows that have to be spotted while reading. Only one of them — `extends: !ref P` —
*contributes keys*; the others declare the dependency without installing anything, which
is why `E0214` and `W0303` remain the extended reference's alone.

**`<<: !ref P` is a feature, and is meant to be written.** It is the one row of that
table a reader is most likely to take for an accident of a uniform rule — inclusion plus
a capability declaration, demanding write access to a base that inclusion does not
modify — so this states plainly that it is intended and what it is for.

It says two things at once, and they are two different things:

1. **include B's keys**, exactly as `<<: P` does, changing nothing about B;
2. **register a capability over B** — this context depends on B and intends to modify
   it — checked as one, so B's scope must be `mutable` from here or the line is
   `E0217`.

The pairing earns its place because *the site that composes something in is usually the
site that goes on to change it*, and it is the honest place to say so. A file that
includes `../core/Defaults` and later extends it by reference has a dependency on `core`
from its first line; writing `<<: !ref ../core/Defaults` declares that dependency **where
the composition happens**, answers the mutability gate at the top of the file rather than
halfway down it, and gives an audit one line to read. The alternative — include silently,
declare the intent somewhere else — splits one relationship across two places, which is
the drift `!ref` exists to prevent.

**The apparent oddity is the point.** Demanding write access for an operation that
performs no write is what keeps the *declaration* independent of the *installation*: an
intent is a claim about this context, not a description of what one line happens to do,
and D4.12 makes `!ref` legal wherever a path is precisely so that the claim can be made
where it is true. A context that only ever reads B writes `<<: P` and needs nothing from
B's header. That is the difference the two spellings carry, and it is one an author
chooses rather than one the operator forces.

*What it is not.* It contributes no keys to B (D4.5), so `E0214` and `W0303` have nothing
to say about it, and it creates no `is_a` edge, because inclusion never does (D4.1). The
blast radius of the inclusion is still exactly A. What crosses the boundary is the
declaration and the reverse dependency edge, and nothing else.

This also discharges D2.6's closing clause, which said node-level inheritance via `<<`
is confined to one document "unless `!ref` is later given merge semantics". It has them,
and then some: the *path* has them, and `!ref` is no longer what carries reach at all.
D2.6 itself is unchanged: `<<: *alias` reaching into another document is still `E0130`,
because that is an illegal *alias*, not an illegal inclusion.

### D4.4 — Extension crosses files by path; the import is one way, not the only way

An extension's operand is a **path**, an alias or an inline mapping. Aliases do not
cross a document boundary (D2.6), so an *alias-operand* extension is document-local by
construction, and that rule is not relaxed anywhere in this specification.

It used to follow that "A is a type of B" was unsayable when B lived in another file
unless B's file had been imported first, leaving the import as the sole boundary
crossing. That is no longer the structure the design rests on. The structure now is:

> **The path is the reach. Visibility permits it. `!ref` additionally requires
> writability.**

Two spellings therefore reach a definition in another file, and they are not
alternatives so much as different questions:

```yaml
--- !yfi/header
namespace: acme::web
imports:
  - core/service.yfy        # brings `&Service` in as an ordinary anchor
---
Frontend: !node
  extends: ../core/Service  # the path. No import needed; this line is the reach.
  port: !!int 8080

Backend: !node
  extends: *Service         # the alias. Ordinary anchor, because the import bound it.
  port: !!int 9090
```

Both are the second operation with all of its properties intact: each node is a type of
`Service`, an `is_a` edge is recorded, and **`Service` is untouched** — the file that
defines it is not modified, and no other node that is a `Service` moves by a key. The
blast radius is the node that wrote the clause, exactly as D4.1 says.

**What each mechanism is *for*.** A path is an **address**; an import is a **binding**.
The path says where something is and reaches it there; the import brings a whole file's
anchors into this document so that `*name` means something. They answer different
questions and neither subsumes the other:

* a path needs no header line, so nothing has to be kept in step with it, and a reach
  can be read without scrolling to the top of the file;
* an import is still the only way to write `*alias` across a file, still the only way to
  reach a **base YAML** file (which has no addressable definitions at all, D6.6), and
  still the thing whose *order* is authored rather than discovered when two files define
  one name (D6.7).

**They do not interfere.** An import does not change what a path means: it binds names
in the importing document and does not move any node into the importer's namespace or
directory (D6.7, point 2). A path does not install any binding: it names one node and
delivers that node, so a path in the body of a document cannot change what an alias
below it resolves to. That separation is what lets both exist without a precedence rule
between them.

**D2.6 is preserved verbatim, not weakened.** Anchors still do not cross a document
boundary; an alias to an anchor defined in an earlier document is still `E0130`. By the
time `*Service` is written, `Service` *is* a definition of this document — the import
put it there before the document's first event (D6.7). Nothing about §2 changes.

So each of the three operations reaches other files, and each reaches them in its own
character:

* **inclusion** — `<<: ../core/Base` directly, or `<<: *Base` after importing its file.
  Keys, no `is_a` edge, nothing changed anywhere else.
* **extension** — `extends: ../core/Base`, or `extends: *Base` after the import.
  Closed, bounded, `is_a`.
* **extended reference** — `extends: !ref ../core/Base`, changing every `Base` in the
  program, and legal only where `Base`'s scope is `mutable` from here.

The middle row is the ordinary case, which is why no fourth operator is needed and why
the set is closed.

*Fixtures:* `projects/import-alias` writes the alias form; `projects/link-graph-shapes`
writes the path form across a directory boundary; `projects/link-ref-binding` writes
both a path and an import in one project.

### D4.5 — An extended reference contributes `own(A)`, and only additively

Two rules make the extended reference implementable at all, and both are forced.

**It contributes `own(A)`** — the keys A writes directly — and not `R(A)`. Not the
keys A included, not the keys A inherited from its own bases, not anything reached
through a clause. *Why it is forced:* if B absorbed `R(A)` then `R(B)` would depend on
`R(A)`, which depends on `R(B)` because A is a type of B — every extended reference
would be a cycle, and D4.10 would reject the operation entirely. *Why it is also
right:* an extension should contribute what it declares, exactly as D4.9 says a clause
is discharged where it is written. The alternative injects A's unrelated ancestry into
B, which is action at a distance with no bound on it at all.

**It is additive.** A's contribution ranks **below everything B already has** — below
B's own keys, its inclusions and its extensions. An extended reference may *add* keys
to a base; it may not change one. *Why:* the operation already reaches every B in the
program, and the difference between adding a property to a family and silently
redefining one the family already has is the difference between a useful tool and an
untraceable bug. A contribution the base already defines is inert, and is `W0303`.

Multiple extended references to one base are installed in document order (D6.3) so the
resulting table is deterministic; but any *observable* disagreement between two of
them is `E0214` (D4.11), so that order is never load-bearing for meaning.

**A's own resolved view is provably unchanged by the third operation.** A is still a
type of B, so `R(A) = own(A) ⊕ … ⊕ R(B)`, and `R(B)` now ends with `own(A)` at its
lowest precedence — where it is shadowed by the copy of `own(A)` sitting at the top of
`R(A)`. The two spellings therefore produce **identical local views**. That is not a
curiosity; it is the reason the mistake is invisible in the file that makes it, and
the reason `W0303` and the visible difference in spelling are the only defences the
author gets.

### D4.6 — The Alchemist's Guild

**Two directories, because the difference this section teaches only exists across
one.** The project is rooted at `guild/`. The Guild's formulary is
`guild/stock/formulary.yfy`, `namespace: guild::stock`, `visibility: public`. The
apprentice writes at their own bench, `guild/bench/apprentice.yfy`,
`namespace: guild::bench`. That arrangement is not scene-setting:
D6.4's default is `immutable`, and the gate it puts in front of the third operation
**bites exactly on a reach that crosses a directory boundary**. Put both halves in one
file and there is no gate at all — see the last part of this decision, which says so
plainly, because a grimoire that claimed one would be teaching a defence the compiler
does not provide.

```yaml
# stock/formulary.yfy — the Guildmaster's
--- !yfi/header
namespace: guild::stock
visibility: public
---
water: &water
  solvent: spring-water
  volume_ml: !!int 250

BasePotion: !type &BasePotion
  vessel: vial
  cork: wax
  label: !!str            # required (D7.3): every potion must be labelled

SleepingTonic: !node      # the Guild's own potion, on the shelf, brewed daily
  extends: BasePotion
  label: Sleeping Tonic
```

**Inclusion — the apprentice's tonic *contains* water.**

```yaml
# bench/apprentice.yfy — the apprentice's
MoonTonic: !node
  <<: ../stock/water
  label: Moon Tonic
  volume_ml: !!int 100    # own key wins, before or after the clause (D1.2, D1.4)
```

```yaml
R(MoonTonic) = {solvent: spring-water, volume_ml: 100, label: Moon Tonic}
R(water)     = {solvent: spring-water, volume_ml: 250}      # untouched
is_a(MoonTonic) = { }                                       # a tonic is not a water
```

The tonic has water in it. Water has no opinion about tonics, and nothing anywhere
else in the Guild moved. Note that the path crossed a directory and nothing had to be
declared for it to: reading is what `public` grants, and inclusion only reads.

**Extension — the apprentice defines a proper new potion.**

```yaml
HealingDraught: !node
  extends: ../stock/BasePotion
  label: Healing Draught
  reagent: sunroot
```

```yaml
R(HealingDraught) = {label: Healing Draught, reagent: sunroot,
                     vessel: vial, cork: wax}
R(BasePotion)     = {vessel: vial, cork: wax, label: !!str}   # untouched
R(SleepingTonic)  = {label: Sleeping Tonic, vessel: vial, cork: wax}  # untouched
is_a(HealingDraught) = {BasePotion}
```

A healing draught is a type of potion. The Guild's definition of a potion is exactly
what it was that morning. The apprentice has changed one thing: their own draught.

One warning is raised here, and it is the right one. `reagent` is declared by no
ancestor, so it is `W0301` (§4) — an open-world model saying *this node carries data its
family did not anticipate*, which is true, is usually fine, and is exactly what a
field-name typo also looks like. Remember it; it is about to disappear.

**Extended reference — the same entry, one `!ref` slipped in.**

```yaml
HealingDraught: !node
  extends: !ref ../stock/BasePotion    # <- one token different
  label: Healing Draught
  reagent: sunroot
```

```yaml
R(HealingDraught) = {label: Healing Draught, reagent: sunroot,
                     vessel: vial, cork: wax}          # IDENTICAL to before
R(BasePotion)     = {vessel: vial, cork: wax, label: !!str,
                     reagent: sunroot}                 # <- the Guild's potion changed
R(SleepingTonic)  = {label: Sleeping Tonic, vessel: vial, cork: wax,
                     reagent: sunroot}                 # <- so did the sleeping tonic
```

Every potion in the Guild now has sunroot in it. The tonics on the shelf. The batch
three alchemists are halfway through brewing in the next room. Anything anyone defines
as a `BasePotion` tomorrow. The apprentice added a herb to their own recipe and
reformulated the Guild.

And their own draught looks **exactly right** — byte-identical resolved view to the
safe spelling (D4.5). Nothing in the entry they are reading is wrong.

**The warning set moves, and it moves the wrong way.** `W0301` on `reagent` is *gone*:
the contribution installed `reagent` on `BasePotion`, so the base now declares it and
every descendant of the base may carry it without comment. That is D4.11's second
consequence arriving in the smallest possible example — a typo'd contribution does not
add a junk key to one family, it makes the junk key legitimate vocabulary everywhere —
and it means the dangerous spelling is *quieter* than the safe one about the key that
actually crossed the boundary. What appears in its place is `W0303` on the `label` line:
`label: Healing Draught` was contributed to `BasePotion`, where `label: !!str` already
sits above it, so that part of the contribution is inert. A warning about a **different
key** is the only local signal there is.

That is why the two spellings must not look alike. `extends: ../stock/BasePotion`,
`extends: !ref ../stock/BasePotion` and — after a header importing the formulary —
`extends: *BasePotion` all name the **same node**, and the second is a different
operation. Since the path amendment the
difference is a single tag on an otherwise identical line, which is a thinner defence
than the old `ns::name/anchor` spelling gave — so the design stopped relying on the
reader alone. `!ref` is now a **declaration of intent**, and the Guild answers it:
`guild::stock` says nothing about mutability, so it is `immutable` (D6.4), and the
apprentice's line is

```text
error[E0217] `!ref ../stock/BasePotion` declares that this context intends to modify
             the target, and the target may not be written from here
  note: `guild/stock` is `immutable` and `guild/bench` is outside it; both axes
        compose over the whole path from the root
  note: drop the `!ref` if `../stock/BasePotion` is meant to be read rather than
        changed; a plain path asks for nothing
  note: <the definition, in the file that owns it>
```

— before anyone has to notice the tag. The apprentice gets an error at the character
they typed; the Guild gets its formulary back by writing nothing at all. `W0303` still
fires alongside it: the two are independent findings about one line, and a project that
drops the tag stops seeing both.

The blocking scope and the observer are named by their **qualified directory names**,
which is D4.12's rule for both axes. The answer was composed over the whole path from
the root (D6.5), so the scope that shut the reach out is frequently neither the target's
own directory nor one the author has ever opened, and naming the target alone would
point at a marking that is already correct.

A Guild that *wants* its potions reopened writes `mutability: mutable` in its header,
and is then back to `W0303` and a careful reader — which is the right place for that
trade to be made, in the file that owns the family rather than in the file that
extends it.

#### Inside one file there is no gate, and `W0303` is the whole defence

**A path naming a definition in its own file passes both gates unconditionally** — D4.12
says so, and it is right to: a file can always see, and always write, what it wrote.
`E0217` is a question about a *reach*, and a reach that never leaves the file has nobody
to ask.

So write the same three entries in one document, as an earlier version of this decision
did, and the third one is **clean but for `W0303`**. `extends: !ref BasePotion` beside
`BasePotion` in one grimoire compiles, reformulates every potion in it, and the compiler
says one thing: `label` was contributed inertly. That is not a defect and it is not a
hole to be closed — a file rewriting its own family is a file editing itself, which
needs no permission and no ceremony. It is simply not what this decision is about, and
claiming an `E0217` there would be claiming a defence that cannot fire.

Stated as a rule, so the two halves are never confused again:

| where the base lives | who may reopen it | what stops a mistake |
|---|---|---|
| the same file | that file, always | `W0303` alone |
| another directory, no `mutability:` | nobody outside | `E0217`, before the contribution is computed |
| another directory, `mutability: mutable` | anyone who can see it | `W0303` and a careful reader |

*Fixtures:* `projects/check-ref-reach` writes the middle row — `patch/patch.yfy` reaches
`../lib/Shared`, whose scope declares nothing, and earns `E0217`, and reaches
`../mut/Open`, whose scope declares `mutability: mutable`, and does not.
`projects/link-inert-contribution` and `projects/check-inert-inherited` write `W0303` at
each of its two passes.

### D4.7 — Precedence, consolidated

**D1.2 is unchanged in its ordering** and gains tiers below the two it already names.
For a node A, highest precedence first:

1. every key written **directly** in A,
2. A's inclusions (`<<`), in written order,
3. A's extensions (`extends:` with alias or inline operand), in written order,
4. the bases of A's extended references (`extends: !ref`), in written order,
5. `own(X)` for every X that holds an extended reference to A, in document order.

Tiers 1–4 are A's own dependencies, read child-to-base. Tier 5 is the reversed edge
and is deliberately last: an extended reference adds to a base and never overrides it
(D4.5).

*Why extensions rank below inclusions:* an inclusion is a deliberate, visible,
node-local statement about *this* node, while a base is a general statement about a
whole family. The specific must beat the general or the inclusion is unusable, and
this is the only ordering under which adding a `<<` to a node cannot be silently
ignored.

D1.3, D1.4, D1.5 and D1.9 apply to `extends` verbatim with the operator substituted:
inheritance is transitive over the resolved view, clause position in the mapping is
not significant, absorption is shallow, and an alias operand is chosen positionally
under §2.

### D4.8 — Validation reads declarations, never the flattened view

A concrete node is validated against **each abstract ancestor's declared view** — the
keys and tags that ancestor declares, including anything installed on it by an
extended reference (D4.5) — and never against the node's own resolved view.

Checking the flattened result is not merely weaker; it is incapable. An ancestor
declares `port: !!int`. A local inclusion supplies `port: !!str "8080"`. By D4.7 the
inclusion outranks the ancestor, so the flattened node holds a perfectly consistent
`port: !!str "8080"` — the violation has been overwritten by the very operation that
was supposed to be checked. Flattening first and checking after can only confirm that
the winner agrees with itself.

So `E0221` compares the concrete node's effective value for a key against the
**declared tag** at every abstract ancestor that declares it, and every such ancestor
must be satisfied.

**Its primary span is the effective value, and a note names the node it is reported
against.** The value is the token whose text has to change, so it is the primary span —
but that token is frequently **not the failing node's**. A shared `<<` mixin supplying a
bad value is one node, written once and resolved into many, so every node that includes
it fails at the same position. Without the note, two failures print byte for byte
identically, neither says which node to fix, and a reader who edits the mixin to satisfy
one may break the others. So the subject is stated with its own span, before the
declaration that was violated and before the origin note that explains why the flattened
node nevertheless looks consistent. `E0225` mirrors this for the same reason one level
down (D4.13). `E0220` likewise asks whether each ancestor's required keys (D7.3)
are supplied, by the node or by anything above it. The ancestors in question are those
on the `is_a` axis — which extensions and extended references create and inclusions do
not (D4.1). *No fixture yet.*

### D4.9 — An inheritance clause is consumed where it is written

`<<` and `extends` entries are resolved in the mapping that writes them and then
**cease to exist**. They appear in no resolved view, are not re-exported to anything
that inherits from that mapping, and are never re-applied through a further clause.
`own(A)` in D4.5 is therefore A's literal keys with its clauses removed.

This follows from D1.3 — a source contributes its *resolved* view, and its clause has
already been discharged in producing that view — but it has to be stated, because the
alternative is not visibly absurd and is quietly destructive. If `b` re-exported its
own `<<: *a`, a node writing `<<: [*b, *c]` would receive `a` twice: once at `b`'s
level, once at its own re-applied one. Whichever copy the resolver visited last could
change the winner for a key `a` and `c` both define, with nothing in the source to
point at. `fixtures/cycles/merge-diamond.yml` is legal precisely because reaching `a`
twice is idempotent under left-biased union, and that property survives only while
every clause is discharged exactly once, at its own level.

### D4.10 — One inheritance graph, two edge directions, one cycle rule

All three operations are edges of a **single inheritance graph**, and **D1.8's rule
runs over the union.** A cycle formed half by inclusion and half by extension — or
half by `*alias` and half by `!ref` — is `E0212`.

*Why the union and not three separate checks.* Take `A << B` and
`B extends: A`. Neither mechanism contains a cycle by itself; two independent checks
both pass. But resolution does not run on two graphs: `R(A)` needs `R(B)` needs
`R(A)`, and D1.8's oscillation argument applies unchanged — no fixed point, or one
chosen by traversal order. Separate rules would admit a construct whose meaning
depends on the compiler's visit order, which is the exact failure D1.8 exists to
reject.

**The extended reference reverses the edge, and a naive SCC pass is therefore wrong.**
In inclusion and extension, resolving A requires resolving B, so every edge points
child → base. An extended reference points **both** ways: A is a type of B, so
`R(A)` needs `R(B)`; and B depends on A, so `R(B)` needs A's contribution. If the
graph is built the obvious way — one vertex per node, every edge between resolved
views — then *every* extended reference is a two-cycle, and SCC rejects the operation
categorically. The pass would not miss cycles; it would hallucinate one every time the
feature is used, which is the worse failure because it looks like a working checker.

**Stratification fixes it, and is forced by D4.5.** Give every node two vertices:

* `own(N)` — N's literal keys with clauses removed. **It has no outgoing edges.**
* `R(N)` — N's resolved view.

Edges:

| from | to | source |
|---|---|---|
| `R(A)` | `R(B)` | `A << B` (inclusion) |
| `R(A)` | `R(B)` | `A extends: B` (extension) |
| `R(A)` | `R(B)` | `A extends: !ref B` — A is a type of B |
| `R(B)` | `own(A)` | `A extends: !ref B` — **B depends on A** |

Because `own` vertices are sinks, **a reverse edge can never lie on a cycle.** SCC over
this graph is therefore both sufficient and exact: it accepts every legal extended
reference and still finds every genuine cycle. This is not a convenience of the
encoding — it is D4.5's `own(A)`-not-`R(A)` rule, which had to hold anyway, showing up
as the property that makes the analysis decidable.

Test it against the case that motivates the question: `A extends: !ref B` together
with `B << A`. Edges: `R(A) → R(B)` (A is a type of B), `R(B) → own(A)` (the reverse
installation, a sink), and `R(B) → R(A)` (the inclusion). SCC finds `{R(A), R(B)}` —
`E0212`, correctly. Note *which* edges closed it: the two forward ones. The reverse
edge is not what makes it a cycle, even though the extended reference is what the
author will believe is at fault. `E0212`'s notes must therefore name every forward
edge in the component, or the report will point at the innocent half.

Mutual extended references (`A extends: !ref B`, `B extends: !ref A`) are likewise
`E0212`, via their forward halves. A self-extended reference (`A extends: !ref A`) is
a one-cycle and is an error under D1.8's uniform rule, even though its reverse edge is
a no-op — D1.8 already rejects unobservable self-cycles for the reason given there.

**The consequence, stated honestly.** `E0212` is a **whole-program** diagnostic. A file
that is acyclic alone can be the second half of a cycle completed by a file it has
never heard of, so a file can no longer always be decided in isolation, which weakens
the independent-compilability property D2.6 was defending. The property is not lost,
it is split — **one cycle rule, two compilation scopes**:

* **Document scope** decides everything that stays inside a document: parsing, anchor
  binding, shadowing, duplicate keys, D1.7, extensions (D4.4), and — **in a file whose
  header imports nothing** — any cycle made only of `*alias` edges. Every diagnostic in
  §4 down to `W0300` remains file-local.
* **Project scope** decides anything traversing a `!ref` **or an import**: `E0212`,
  `E0213`, `E0214`, `E0216`, `E0217`, `E0218`, `E0220`, `E0221`, `W0301`, `W0303`.

The import qualification is a correction, not a caveat. Before D6.7 an alias could only
name a node in its own document, so an inheritance cycle built purely from aliases was
necessarily document-local and decidable in document scope. An imported definition is a
definition of this document (D4.4), so `A extends: *B` and `B extends: *A` can now sit
in two different files and still form an alias-only cycle. The clause as first written
was therefore false once imports exist, and the repair is exact: **imports and `!ref`
are the two things that move a file from document scope to project scope**, and a file
with neither is still independently compilable in the full sense D2.6 intended. That is
a better statement of the property than the original, because it is a syntactic
condition a reader can check by looking at one header.

By D6.1 this is less of a change than it looks — a single file *is* a project of one
file, so the two scopes are one operation at two sizes. What is genuinely lost is the
guarantee that a clean file stays clean as a project grows around it. That is the
price of cross-file inheritance, and every module system pays it.

**Diagnostic wording.** `E0212`'s message becomes **cyclic inheritance**. A user whose
file contains no `<<` anywhere, reading "cyclic merge" about their `extends` chain, has
been told about a construct they did not write. Each note names its own edge kind —
`via <<`, `via extends`, `via extends !ref` — and, for a cross-file edge, the file it
lands in, because the participating spans alone do not say which operator closed the
cycle.

**An import adds no edge to this graph, and that is checked rather than assumed.** An
import is a *binding* operation: it decides which node a name denotes, exactly as an
anchor definition does, and it is performed by `discover` before any resolution
happens. It creates no dependency between resolved views, so it contributes neither an
`R → R` edge nor an `own` edge, and the vertex set and edge set above are unchanged by
its presence. What an import changes is *which node* an existing alias edge lands on —
and that node is an ordinary member of the project graph, over which the SCC pass
already runs project-wide. Two further facts complete the check: imports are not
transitive (D6.7), so the import relation carries no dependency of its own and needs no
traversal; and building a document's import bindings requires only the *parsed*
definitions of the imported file, never its resolved views, so the two phases cannot
interleave. The stratified argument therefore survives an import verbatim. Its one
visible effect on cycles is the scope correction above: cycles that were previously
confined to one document can now span files, and the same pass finds them.

**Recovery** is D1.8's, over the stratified union graph: depth-first search in document
order (D6.3), dropping each back edge. As in D1.8 the recovered view is not a semantic
and is never emitted.

**What is *not* an error**, restating D1.8 over the larger graph: cycles in the *data*
graph remain legal and remain the point of the system, whether the edge is `*alias`
(`fixtures/cycles/self-alias.yml`, `fixtures/cycles/mutual-alias.yml`) or `!ref`. Only
cycles through **inheritance** edges are rejected. A DAG with repeated ancestors is
also still legal at any width — `fixtures/cycles/merge-diamond.yml` is the fixtured
case and nothing here affects it.

*Fixtures owed:* a mixed inclusion/extension cycle in one document; a cross-file cycle
closed by `!ref`; the `A extends: !ref B` + `B << A` case above, which is the one that
distinguishes a correct implementation from a plausible wrong one; and an alias-only
cycle spanning two files joined by an import, which is the case the scope correction
exists for.

### D4.11 — Diagnostics the extended reference requires

An operation whose blast radius is the whole program needs its own reporting, and two
codes are allocated.

**`E0214` — conflicting extended references.** Two extended references to the same
base contributing the same key with different values. This is an error, not a
resolution by order: nothing in the source ranks two files' claims on a base except
their path names (D6.2), and a graph whose values depend on a filename is precisely
what D1.8 refuses. Two contributions of the *same* value are idempotent and legal, for
`merge-diamond.yml`'s reason.

**`W0303` — inert extended reference contribution.** A contributed key the base
already defines. By D4.5 it loses, so it does nothing; and by D4.5's identity result
the author's own node looks correct either way. `W0303` is the only local signal that
someone wrote `!ref` where they meant an extension, which is what earns it a code of
its own rather than silence. It is a warning because a contribution partly inert and
partly effective is legitimate — an extension may reasonably restate a key it also
depends on — and `--deny W0303` is available to projects that disagree.

**"Already defines" is D4.5's three things, and takes two passes to test.** A
contribution loses to the base's own keys, **its inclusions and its extensions**. Only
the first is decidable without resolving anything, so `link` tests it there and `check`
tests the other two against the resolved base. The two inputs are disjoint — `link`
marks a key not-inert precisely when the base does not write it — so one contributed key
is never warned about twice, and the earlier, commoner case is still reported as early
as it can be.

**An extended reference that resolves to nothing** is `E0213`, the ordinary
unresolved-path error; **one that lands and then misses a member** is `E0218`; **one
resolving to a non-mapping** is `E0211`, the ordinary illegal-source error. None needs a
special case.

**An extended reference into a scope this one may not write** is `E0217` (D4.12), and it
is the only one of these that is about the *operation* rather than the operand. It is
also the only defence in this section that acts before the contribution is computed,
which is why D6.4's default matters more than any of the warnings here: `W0303` tells an
author they made a mistake, and `immutable` tells them they may not.

**`W0301` still makes sense, with its input set redefined.** The undeclared-field
warning (§4) asks whether a concrete node's key is declared by any ancestor. Under
D4.5 an ancestor's declared view includes everything installed on it by extended
references, so the test is unchanged in shape but must be evaluated **after all
extended references are installed**, project-wide. Two consequences follow and both
are real costs:

* whether `prot:` is an undeclared field on a node in one file can be decided by a
  file in another — `W0301` joins project scope;
* an extended reference that contributes a key **silences `W0301` for that key on
  every descendant of the base**. A typo'd contribution does not merely add a junk key
  to one family; it makes the junk key legitimate vocabulary everywhere.

That second consequence is not fixable within the warning — it is the extended
reference doing exactly what it is for — and it is the strongest argument in the design
for why the third operation's spelling must be visibly different from the second's, and
now also for why the base has to have agreed to it (D6.4).

*Fixtures:* `projects/link-conflicting-extends` is `E0214`,
`projects/link-inert-contribution` and `projects/check-inert-inherited` are `W0303` at
each of its two passes, and `projects/check-extref-silences` is the `W0301` case whose
verdict changes because of an extended reference in another file.

### D4.12 — Reach and access

Three questions the operators raise and the earlier decisions did not answer: **how is
a definition elsewhere named**, **who may read a member of a resolved node**, and **who
may change one**. The first is syntax; the other two are the two reserved keyword pairs.

#### The path is the reach

**A reach is spelled the way a filesystem is spelled**, because the scope tree *is* the
directory tree (D6.4):

```text
../shared/Service        up one directory, exactly like `..`
../../core/Base          up two
sibling/Service          a peer file in this directory
Service                  this file
Service.port             a member of it
```

`..` walks up the scope tree the way it walks up directories. A bare segment names a
peer **directory** or, failing that, a peer **file**; naming a peer file is what brings
its contents into reach, so there is nothing to declare. `.` addresses members, and
chains: `Service.tls.port` is the `port` of the `tls` of `Service`.

The grammar is small and total, which is the point:

```text
path    := prefix segment ("/" segment)* ("." member)*
prefix  := "./" | "../"+ | ε
segment := name
member  := name
name    := [A-Za-z_] [A-Za-z0-9_-]*
```

`name` excludes digits-first, `:` and `.`, so `7`, `acme::billing/invoice` and
`http://host/thing` are not paths and never become one by accident.

**What each form resolves against, and why the bare form is the file.**

| written | resolved against |
|---|---|
| `Name` | a `!ref` binding of this document, else a definition of **this file** |
| `./Name` | a definition of **this directory** |
| `dir/Name` or `file/Name` | a child directory of this one, else a peer file of it |
| `../…` | the same, one scope higher per `..` |

A bare name is *this file* rather than *this directory* because a directory holds
several files by design (D6.1) and nothing ranks two of them but their filenames — the
thing D1.8 refuses. Making the bare form file-local means a name never silently starts
resolving somewhere else when a sibling file is added. Reaching a sibling is one
segment of typing, and that segment says which file.

**The guarantee held against a new *file* and not against a new *line*, and `E0219`
closes that.** The resolution order above is unchanged — a bare name finds a `!ref`
binding of this document first, and a definition of this file only if there is no
binding — but **a binding whose key is also a definition of the same file is `E0219`**:

```text
--- !type &Widget
near: !!int 1
--- !node &Use
Widget: !ref other/Widget    # <- add this one line
child: !node
  extends: Widget            # <- unchanged; now names another directory
```

A binding outranks the file's own definitions, so one added line silently retargets
every bare `Widget` already written. With matching keys on the two nodes there is no
shape change, no value change and no diagnostic — the strongest form of the fault D1.8
refuses, because nothing about the program says it happened.

*Reversing the precedence was considered and rejected.* Making local definitions outrank
bindings closes the retarget and opens a quieter hole: the binding would still be
written, still carry the capability, and still be what `Widget.member` addresses
through, while every bare `Widget` meant something else. **One spelling would denote two
things depending on whether a `.` followed it** — the same fault wearing the opposite
sign, and one no reader could see. So the precedence is left exactly where it is and
**the ambiguity itself is refused**: there is no resolution order to learn, no line whose
meaning depends on a line elsewhere, and the fix is a rename in one place — or `./Widget`,
which is this directory's and is never captured by a binding.

*Scope of the comparison.* A binding is a **document's**; a definition is a **file's**,
which is what a bare path resolves against. So the comparison is **file-wide**: a binding
in the second document of a file still makes one spelling mean two things in one file,
and that is what the rule is about. **Imported names do not collide** — an import binds
into the document's anchor table and is not a definition of this file, so a name that
arrives by `imports:` and is also bound by a `!ref` is the ordinary two-state sequence
`W0300` already governs (D2.3, D5.1, D6.7).

Fixture: `projects/ref-binding-shadow`.

**A segment naming both a directory and a file resolves to the directory.** The
alternative would let adding a directory move a path that already worked, and a
directory is the more public address of the two, being what a namespace is claimed on.
Only Yamlfication source answers a path at all, so a `service.yaml` beside a
`service.yfy` poses no question: base YAML has no addressable definitions (D6.6).

**Where a plain scalar is read as a path.** Three positions, not two:

| position | read as a path when |
|---|---|
| operand of `<<:` or `extends:` | it parses as a path at all |
| item of a `connections` some `!edge` reads (D4.13) | **always**: it is a reach or it is `E0213` |
| anywhere else — a data edge | it parses **and** was written `./…` or `../…` |

The asymmetry between the first row and the last is deliberate and it is the D6.6
argument applied one level down: a reading must not be decided by an incidental signal,
so where a value has always been data, the reach has to say so. A scalar under `<<:` or
`extends:` was `E0211` in every previous version of this language, so reading it as a
path cannot change the meaning of anything that used to be legal; a scalar in a data
position has always been data, so `region: eu-west` stays a string no matter what the
project happens to contain.

The middle row is stronger than the first, and D4.13 owns the argument for it. In
summary: the position is declared by the **language**, which is as explicit a signal as
`extends:` is; there is no prefix in a sequence item for a quote to escape, so a quoted
endpoint is still an endpoint and quoting escapes nothing there; and *which*
`connections` this is is a question about the inheritance relation, not about the
holder's tag.

**A failed path is two codes, because it has two fixes.** `E0213` if the walk did not
land — no such directory, no such peer, no such definition, `..` past the root, or not a
path at all. `E0218` if the walk landed and a `.` step did not. Being told the address
is wrong when the field name is wrong sends an author to the wrong file.

**`E0213` names where the walk got to.** "No definition called `X` was found" is
unanswerable without it: for `Nowhere` the place is *this file*, for `dir/Nowhere` that
directory, and the author's next move differs — write the definition here, or correct the
segment. So the message carries the landing, named as a file relative to the project root
or as a scope, whichever the walk reached.

#### The epistemic gate

**The path grants the reach syntactically; the keywords decide whether it is
permitted.** There are two pairs and they are consulted in a fixed order:

* `private` / `public` — may this be seen? Composed over the whole `root → target`
  path (D6.5). Failure is **`E0216`**, and there is no fix: you may not have this.
* `mutable` / `immutable` — may this be changed? Composed the same way. Failure is
  **`E0217`**, and it is asked only of a `!ref`.

**Visibility is decided first, and the ordering is structural rather than conventional.**
A `!ref` into a scope that is both `private` and `immutable` is `E0216`, not `E0217`:
reporting the mutability gate would send the author to change a keyword that is not what
stopped them, and the `public` they actually need would still be missing when they came
back. But an ordering maintained by remembering to ask two questions in the right order
is one that can be got wrong, so it is not maintained that way.

**Visibility is decided *during path resolution*, in pass 4, before the final segment is
sought and before any `.` member is addressed.** A path that lands in a scope the
referencing scope cannot see **resolves to nothing**. It names no definition, addresses
no member, contributes no `is_a` edge, installs nothing, and is an absent endpoint if it
was one. `E0216` is the only answer it earns, and by pass 5 there is no reference left
for `E0217` to be asked about — which is why `E0217` cannot be reported against a scope
whose `visibility:` was the real obstacle, and why nothing downstream ever sees an
invisible target.

*Why the gate must stand in front of the lookup and not behind it.* Resolve first and
the diagnostic is merely decorated: against one private `vault`, the three paths
`vault/Secret.password`, `vault/Secret.nosuch` and `vault/NoSuchNode` earn three
**distinguishable** answers — `E0216` naming the definition it found, `E0218` saying the
node does not hold `nosuch`, `E0213` saying there is no definition called `NoSuchNode`.
Between them an outsider enumerates a private scope's node names and each of those
nodes' member names, which is precisely the access this decision says it has none of. A
gate that only changes the wording of a refusal is not a gate. So the answer's **shape
does not vary** with whether the node exists or the member exists; only the path the
author wrote varies. Fixture: `projects/private-opacity`.

**`E0216`'s note names the blocking and observer scopes by qualified directory name, and
carries no source location.** Every other note in this document points at something the
author can read. That one would point at a `file:line:col` *inside the scope the gate
just refused*, which is the disclosure the gate exists to prevent — one span, and the
outsider knows the private file's name and how long it is. Naming the two scopes
discloses nothing, and this is provable rather than judged: every possible blocker lies
strictly between the root — which encloses every observer and therefore never blocks —
and the landing, so it is a directory the author named in the path or an ancestor of
one. It is a directory they already wrote.

**If B is private and outside A's scope, A has no access to B at all** — not its
members, not its public surface, not its name. **A private B in A's own scope is
entirely ordinary**: privacy is a boundary against the outside, not secrecy from
siblings, and a scope is open to an observer sitting inside it (D6.5).

**A path naming a definition in its own file passes both gates unconditionally.** A file
can always see, and always write, what it wrote. This is the reason D4.6 needs two
directories to teach `E0217` at all: a grimoire that reopens its own family is a file
editing itself, and there is nobody to ask.

#### `!ref` is a mutation declaration, and mutability is checked at compile time

Until this amendment the specification said Phase 1 "records and propagates mutability
but ships no writer". **That is no longer true, and it was never quite honest.** An
extended reference *is* a write: it installs `own(A)` on a base, and every node that is
a B in the program then carries it. The write is performed by the compiler rather than
by a runtime, which makes it earlier and more total than a runtime write, not less of
one. So the axis is checked, at the only time it can be checked, by the only construct
that performs the write.

**`!ref` is what declares the intent.** It is not the way to write a reference — a plain
path is — and it is not restricted by position. It is legal wherever a path is, and
wherever it is written it says three things at once:

1. **mutation** — the target must satisfy `writable(target, referencing scope)`, composed
   over the whole `root → target` path exactly as `visible` is (D6.5). Otherwise
   `E0217`. The predicate is the same walk, so the two axes cannot disagree about who
   blocked what;
2. **dependency direction** — the target depends on this context. That is the direction
   an extended reference already establishes, so it contributes the same reverse edge
   into `own(A)` (D4.10) and no second edge kind is invented. `own` is a sink, so the
   extra edges can never make `E0212` fire on something acyclic;
3. **epistemic access** — written at a mapping entry, `!ref` **binds that key** as a
   name carrying the capability. Access is granted to *that member*, not to the file,
   which is this decision's own rule that access is a relationship rather than a flag.

```yaml
service: !ref ../core/Service     # this member has access to ../core/Service,
                                  # and ../core/Service depends on this context
myClass:
  - a: !ref service.member_one    # addressing within the binding
  - b: !ref service.tls.enabled   # and chaining
```

**The capability is established at the binding; the `.` steps address within it.**
Visibility and writability are checked once, where the binding is written, and not again
at each member. That is the reading the model forces: a member is not a scope, it has no
axes of its own to consult (see D6.4), and re-deriving the target's scope at every step
would answer the same question with the same inputs and could only ever agree. The cost
is that a binding is a single grant covering everything under it — which is what
"capability" means, and what makes the binding line the one an audit reads.

**Only the bare one-segment form reaches a binding.** `service.port` finds the binding;
`./service/port` and `../service/port` address the tree. Letting a local name capture an
anchored path would make adding a `!ref` silently redirect a path that names a directory.

**A plain path binds nothing.** `service: ../core/Service` is a data edge, and
`service.member_one` elsewhere in the document will not find it. That is the difference
the two spellings are for, stated once more from the other side: `extends: ../core/Service`
is read-only instantiation; `service: !ref ../core/Service` is a capability-bearing
binding. Same target, different declaration about what this file intends to do to it.

**Contribution stays the extended reference's alone.** `extends: !ref P` installs
`own(A)` on P (D4.5) and is the only spelling that does. A `!ref` in any other position
declares the dependency and the intent without contributing keys, so `E0214` and `W0303`
have exactly the inputs they had before.

#### Access is a relationship, not a flag

Visibility is **not one flag per node**. Whether a member may be read depends on which
relationship brought it into the node holding it, and the three operators are three
different relationships:

| relationship | written | what happens to a private member of B |
|---|---|---|
| **containment** | `A << B` | comes in and stays **B's**, addressed through A, gated as it was. Containment neither republishes nor absorbs. |
| **instantiation** | `A extends: B` | becomes **A's own** private member, re-gated onto A. This is the one step privacy travels. |
| **descent** | A is a descendant of B, transitively | arrives only where A can **read** it at that level: a `pub` member always, a private one only when A sits inside the scope gating it. |

**Privacy crosses one inheritance step, and then stops at the first scope boundary.** In
`A extends B extends C`, C's private member becomes B's own private member and travels no
further as B's; A receives it only if A can read it where B holds it. Without that bound
a private field would propagate down an unbounded descendant chain, re-gated at each step
onto a scope further from the one that wrote it, which is republishing by instalments.

*The bound is the reader's question, not the flag.* An earlier statement of this rule
said a descendant receives an ancestor's member "only if it was public", which was
indistinguishable from this one while a member's gate was derived from its scope —
private then meant "written somewhere the project cannot reach". It stopped being
indistinguishable when members gained declarations, because a bare member is now private
by default: read literally, the old rule would drop every unflagged member from every
node two `extends` steps from its author, **including a chain written entirely inside one
directory**, where no boundary is crossed and nothing is republished. The fixture that
catches it is `projects/check-diamond`, three files in one namespace whose leaf would
silently lose the base's keys. So the rule asks what it always meant to ask: *can this
descendant read it?* — the same predicate `E0216` and the member table use, so the three
cannot disagree. Across a real boundary the two readings still coincide, which is why
`projects/check-access` is unchanged.

**A public B's private member is not reachable by reference.** A path to B yields B's
**public surface** and nothing under it. Being able to name a node is not being able to
reach into it; private members are reachable only through the two inheritance
relationships above. This is the sharp consequence and it is the reason access cannot be
a property of the field alone: the same member of the same node is readable from one
scope and not another, and the answer is a relation between the accessor and the holder.

#### What a member is

**A member is anything nested inside something else, exactly as YAML nests.** There is
no spelling rule and no reserved key: the discriminator is the **file class**, which
D6.6 already establishes.

A `.yfy` file is **not a data store**. Everything nested in it is a member of its
parent, and the data is what gets *evaluated from* that structure — sourced from a
`.yaml`, or written inline as a default. A `.yaml` file is base YAML data and declares
no members at all.

*This supersedes an interim rule* under which a bare scalar sequence item was a member
and a quoted or tagged one was data. That rule let a signal written **inside** the file
decide a semantic question, which is precisely what D6.6 refuses one level up:
`tags: [one, two]` and `tags: ["one", "two"]` would have been two different kinds of
thing on the strength of two quotation marks, with nothing reported either way. They are
one kind of thing. Quoting keeps the only job it ever had — it escapes the **prefix**,
so `"pub literal"` is a member called `pub literal` and not a public one called
`literal` — and it was never a claim about membership.

So the mapping spelling and the sequence spelling are two spellings of one thing
throughout: `port:` and `- port` both name a member of the node holding them, and only
the mapping form can also state a declaration (D7.3), which is why the sequence form is
the shorter one rather than a second model. A nested collection is nested the same way;
it simply has no name of its own to be addressed by.

#### Member flags: the two axes, written one level down

**A member declares its own axes, with a prefix on its name.** This supersedes an
earlier statement of this decision which said per-member gates were "derived, never
declared" and told a reader looking for a member-level spelling to stop looking. There
is one, it is not a tag, and it is this:

```yfy
ClassA:
  - private_member                     // private and immutable
  - pub public_member
  - public public_member_two
  - mutable mutable_member
  - mut member_two
  - pub mut public_mutable_member
  - mutable public mutable_public_member
```

`pub`/`public` and `mut`/`mutable`, either or both, **in any order**, as a prefix on the
member name. The same prefix goes on the mapping spelling of a member, where it sits in
front of the key: `pub port: !!int 8443`.

**They are prefixes on a plain scalar, not tags.** `- pub mut name` already parses as
the ordinary YAML string `"pub mut name"`, so nothing about the parse changes, no new
tag is introduced, and there is no collision with `!type`, `!node`, `!edge` or `!ref` —
the tag vocabulary is untouched and stays closed. An earlier analysis concluded that
member flags would have to be tags; that conclusion was wrong for this syntax, because
this syntax needs nothing from the tag position at all.

**The escape is the one D4.2 already uses.** The prefix is read only from a **plain,
untagged** scalar, so `"pub literal":` is a member genuinely called `pub literal` and
`!!str "mut x"` is one called `mut x`. A reader who knows how to write a literal
`extends` key or a literal `<<` key already knows how to write these.

**A bare member is `private` and `immutable`.** Both axes are opt-in, exactly as they
are for a scope (D6.4): a member that says nothing grants nothing. `- pub` on its own
declares a member *called* `pub`, because a prefix with nothing to qualify is a name.

**Composition needs no new rule, and there is no second predicate.** A member's gate is
its own declaration composed with its scope's, by the same `ScopeTree` walk D6.5
already specifies:

```
readable(m, o) = m is `pub` and visible(scope(m), o), or o is inside scope(m)
writable(m, o) = m is `mut` and writable(scope(m), o), or o is inside scope(m)
```

A `pub` member inside a `private` scope is therefore public *within* that scope and
invisible outside it — which is D6.5's "public node inside a private scope" case one
level down, giving the same answer for the same reason. A `mut` member under an
`immutable` scope is likewise often inert, and that is correct rather than a mistake to
diagnose.

**Two members of one node may not share a name**, however differently their prefixes are
written; `port:` beside `pub port:` is `E0110` (D8.5).

**Base YAML has no member flags**, because it has no yfi syntax at all (D6.6). A member
of a `.yaml` file therefore carries no declaration, and its gate is its scope's alone —
which is what the whole of this subsection said before it, and is not a special case for
data but the direct consequence of the flags not being interpreted there. Gating a data
file on a prefix it has no way to write would make every imported `.yaml` permanently
private.

**What the mutability axis does with this, today.** Nothing writes to a member at
compile time — the only compile-time write is an extended reference, and that is gated
on the *scope* (`E0217`, above). A member's `mut` is therefore carried, composed and
exposed, and applied by `emit`. That is a smaller claim than the visibility axis makes
and it is made deliberately: this document specifies a language and its compiler, and
a member's `mut` is carried for whatever reads the image. Enforcing it against a live
write belongs to a runtime, which is a separate artifact and out of scope here (D6.5).

**One earlier clause of this decision is now wrong and is withdrawn**: "a member is not
a scope, it has no axes of its own to consult". It has two. What survives unchanged is
the sentence that mattered — the capability is established **at the binding**, and the
`.` steps address within it — because a member still is not a *scope*, has no path
composed through it, and re-deriving anything at each step would answer the same
question with the same inputs.

*How this is carried.* Every member of a resolved view records **where it was written**,
**how it arrived** (own, included, extended, descended, installed) and **what gates it**
— a visibility plus the scope that visibility is measured against. A public member is
readable from anywhere; a private one is readable only from inside its gating scope,
which is the holder's for a member written there or taken across one `extends`, and the
source's for one merely included. `check` computes that per member; `emit` and the
runtime **apply** it while they walk, filtering as they go so that scoping never leaks
through result shape.

*Fixtures:* `projects/member-flags` writes both spellings of a member list, both
spellings of both flags in both orders, the quoted escape, a `pub` member of a `private`
scope, a quoted list whose items are members like any other, and a member path that
addresses a flagged member; `projects/imports-data/services.yaml` writes the same shape
one class over, where it declares nothing; `projects/member-collision` writes `port:` beside `pub port:`;
`projects/check-access` writes all five relationships in one project;
`projects/check-ref-reach` writes the reach outcomes — a path with no import, a path into
a private scope (`E0216`), a `!ref` into a visible but `immutable` scope (`E0217`), a
`!ref` into a `public mutable` one, and a `!ref` into a scope that is shut on both axes,
which is what fixes the order of the two checks. `projects/link-ref-binding` writes the
binding, the chained member, a missing member (`E0218`), and the directory-beats-file
tie-break; `projects/ref-binding-shadow` writes the binding that shadows a definition of
its own file (`E0219`); and `projects/private-opacity` writes the three paths into one
private scope that must not be told apart.

### D4.13 — `!edge`

**An edge is a node whose content is what it connects.** It carries a tag, `!edge`; it
has identity; it is addressable by the path syntax; it inherits with the same three
operators; it is validated by the same checks; it is emitted. There is no second
construct here and no second set of rules. This decision names the **two members the
language owns on such a node** and says what they mean, and that is the whole of it.

| member | is | absent or malformed |
|---|---|---|
| `connections` | a **sequence** of the nodes the edge relates, in written order. An alias to one is dereferenced | none at all, an unsatisfied declaration, or written in base YAML: `E0223`. Not a sequence: `E0224` |
| `definition` | optional. A **mapping** of *handles*: a name for a position in `connections` | not a mapping: `E0224`. A handle naming no position: `E0225` |

Both are read from the node's **resolved** view, not from its own keys, so an edge that
inherits its connections from a base has them. That is not a rule about edges; it is
what extension already means (D4.7), arriving at the right answer with nothing added.
They are read **independently**: an edge may inherit `connections` from one base and
`definition` from another, or write one and inherit the other, because each is a member
and each is resolved as a member. Nothing couples them but the check that a handle names
a position of the sequence the *same node* ends up holding — which is what `E0225`'s
inherited-`definition` note exists for.

**`connections` is a sequence, so an edge is n-ary.** A three-way relation is one edge,
never three binary ones, and nothing anywhere assumes two endpoints. "These three
services share an on-call rotation" is one fact, and encoding it as three pairwise edges
is a lie about its shape that no query can undo.

**An edge may be an endpoint of another edge**, because an edge is a node and a node may
be an endpoint. That is how a relation over relations is written and it is intended. It
follows that the **connection graph may cycle** — only *inheritance* cycles are illegal
(D4.10) — so every traversal over it carries a visited set, exactly as §0 requires of
every traversal in this system.

**Everything else on an edge is an ordinary member.** "A node whose content is what it
connects" says what makes a node an edge, not what a node may hold. An edge sitting
between two nodes and carrying its own members is **middleware**, and it is reachable
here precisely because nothing precludes it: the members are members, the validation is
D7.3's, and no feature had to be added to allow it.

**`!edge` is concrete, exactly as `!node` is** (D7.1): it is emitted. **There is no tag
for an abstract edge and none is needed.** The abstract counterpart of a concrete node is
whatever the author wrote — a `!type`, an untagged mixin, or nothing at all — and a
family of edges is a `!type` that declares `connections`. D7.2 is untouched: no rule says
which kinds may inherit from which.

#### `connections` is not a reserved word

This is the first thing to get right, because the obvious reading of the table above is
wrong in both directions.

**The two names are owned *on an edge*, not globally.** A sequence item under
`connections` is a **reach position** — read as a path whatever its scalar style, with no
anchoring prefix required, and quoting escapes nothing there — **iff the node holding the
key is an `!edge`, or is a node some `!edge` reads that member from.** Everywhere else
`connections` is an ordinary member name and its items are ordinary values.

**What the compiler carries is the set of such *sequences*, not the set of holders.** The
two would be the same set if the member's value were always written in place, and it is
not: an alias standing there is dereferenced, so the items may be written in a file that
holds no edge and names no `connections` at all. Reach-ness attaches to the sequence the
edge ends up reading. One consequence is worth stating rather than discovering: a
sequence an edge reads is a reach position **for every reader of it**, so an anchor
aliased once as `connections` and once as an ordinary member has items that are nodes in
both places. That is the same one-node-one-reading rule aliasing has everywhere in YAML,
and it is the reason to write a shared endpoint list as a sequence that is only ever
that.

*Why it cannot be a question about the holder's tag*, in either direction:

* **Narrow it to `!edge` and inheritance breaks.** An edge inherits `connections` from an
  **untagged mixin** — D7.1's ordinary form, which carries no tag saying so — or from a
  `!node`, or from a `!type` family, or through a `<<`. Reading those items as data
  leaves every one of them resolved to nothing, and the edge relates nobody with no
  diagnostic to say why.
* **Widen it to `!type` and the name is reserved across the language.** Every `!type` in
  every project would have a reserved member called `connections`, **with no escape**.
  Quoting cannot be the escape, because quoting has exactly one job in this language and
  it is not this one: it escapes a member-name **prefix** (D4.12), so `"pub literal"` is
  a member called `pub literal`. It has never been a way to say *this item is data*, and
  a reach position is decided by the position rather than by the style. So
  `["eth0", "eth1"]` would be two unresolved paths with nothing the author could write
  instead. A router type listing its interfaces is not an exotic case.

**Reach-ness belongs to the consumer.** A `connections` member holds an edge's endpoints
exactly when an `!edge` ends up holding it, which is a question about the **inheritance
relation** and is answered from the relation. The set is the **reverse closure of D4.7's
contribution edges, beginning at every `!edge`**: for each clause, an operand's target
contributes to the clause's owner (`<<`, `extends`, and `extends: !ref`), and for
`extends: !ref` the owner additionally contributes `own(owner)` to the target — which is
tier 5, the reversed edge, and it is followed too, because a base that an extended
reference installs keys onto is a node an edge reads keys from. Transitively, to
fixpoint.

Two consequences, both stated because both are load-bearing:

1. **An `!edge` inheriting `connections` from an untagged base, a `!node` base, a `!type`
   base or a `<<` mixin relates that base's endpoints.** No tag anywhere says "these
   items are nodes"; the relation says it.
2. **A node no `!edge` inherits from holds `connections` as an ordinary member.**

   ```yaml
   Router: !type &Router
     pub connections: ["eth0", "eth1"]     # legal, silent, two strings
   ```

   Nothing is reserved about it, nothing is resolved from it, and nothing is reported.

*Fixtures:* `projects/edge-mixin` writes three of the four bases — untagged, `!node`,
and a `<<` inclusion of an untagged mixin; `projects/edge-extends` writes the fourth, a
`!type` family; and `projects/edge-not-a-reach` writes the `!type &Router` above and a
`!node` that also has a `definition` member holding prose.

#### The pass runs twice, and that is sound

Deciding the set above needs the inheritance clauses; a clause operand may itself be a
path; resolving paths is what the reference pass does. So **pass 4 resolves every
reference twice**: once as a silent **probe**, with no `connections` item read as a reach
and nothing reported, whose only consumer is the clause collection the relation is
derived from; then once for real, with the answer, raising the diagnostics.

The second run is not a re-decision of the first. **No clause operand's meaning depends
on the set** — the set decides exactly one position, the sequence item, and no clause
operand is ever in it — so the two runs produce the *same clauses*, and only the third
row of the path table moves between them. That is why one silent pass and one reporting
pass is a fixpoint reached in two steps rather than an iteration with a termination
argument owed. Withholding the probe's diagnostics is required for the same reason: they
are the real run's, and reporting them twice would double every `E0213` in the project.

#### Positions are what is written

**`connections` writes N positions, where N is the length of the sequence as written.**
An item that resolves to nothing is `E0213` — the ordinary unresolved-path error, because
a `connections` item is a reach and a reach that names nothing is the same failure a
clause operand naming nothing is — and it **contributes no endpoint**. It does **not**
renumber the items after it, and it does **not** reduce N.

A handle is checked against **N, the written count**. A handle is matched to an endpoint
by **written position**. A handle naming a position whose item failed therefore binds to
nothing and answers `None`; it does not quietly acquire the next surviving endpoint.

```yaml
--- !edge &Gapped
pub connections: [Alpha, Nope, Gamma]
pub definition:
  first: 0
  second: 1
  third: 2
```

Exactly one diagnostic: `E0213` for `Nope`. `first` names `Alpha`. `second` names
nothing. `third` names `Gamma`. `third: 2` is legal because the sequence writes three
positions, not because two of them resolved.

*Why written and not surviving.* The alternative — compact the sequence, renumber what is
left — makes **one bad item silently move every handle after it**, so a typo in an
endpoint changes what `target` means without saying so, and the author reading `target: 1`
against a three-item list has no way to know. One fault costs one diagnostic and moves
nothing. The same rule is what lets a *filtered* read be honest: an endpoint an observer
may not see is absent from the result and the positions beside it are unchanged, so a
gated walk and an ungated one agree about which position is which.

#### What may stand as an endpoint

Four forms, and the fourth is a failure rather than a form:

| written | names |
|---|---|
| a **path** — `Alpha`, `../peer/Alpha`, `Alpha.tls` | what the path resolves to (D4.12). Unresolvable is `E0213` |
| an **alias** — `*Alpha` | the node it binds to, document-locally as §2 says. A binding to a scalar names no node and is `E0213` |
| an **inline collection** — `{host: localhost}`, `[1, 2]` | itself. A node like any other; it simply has no name to be addressed by |
| any other **plain scalar** — `7`, `not-a-def` | nothing, and it is `E0213` |

**An endpoint is a node, and that is decided once for all three spellings.** D6.1 makes
only an anchored **collection** addressable — an anchored scalar is a value, not a type
— and an edge relates nodes, so `&limit 30` is not one of them however it is reached:
`connections: [limit]` is `E0213` because the path names no definition, `connections:
[*limit]` is `E0213` because the alias binds a value, and `connections: [7]` is `E0213`
because a scalar written inline is not a node either. This used to be two answers to one
question: the path spelling refused and the alias spelling accepted, on the reasoning
that the two walk different structures — the definition table and the arena. They do,
but *what may be related* is not a question about the walk, and two spellings of one
question with two answers is exactly what a second implementation cannot reproduce.

The alias case is raised by `check` rather than by `link`, because an alias is not a
path and never became a reference: pass 4 never saw it. It is still `E0213` and not a
new code — one fault, one number — and its message names the anchored scalar it found.

**The `connections` member's own value is dereferenced by the same rule.**
`connections: *Endpoints`, where `&Endpoints` is a sequence, relates what that sequence
names. An alias is dereferenced for the member's value exactly as it is for an item, so
two edges may share one endpoint list without either of them extending the other, and
the value's *shape* is judged after the dereference: an alias to a mapping or a scalar
is `E0224` like any other wrong shape. What this does **not** loosen is where the items
may be written — they are read as reaches wherever the sequence is, which is why the
reach position is the **sequence** and not the key holding it.

*Fixture:* `projects/edge-shared-sequence`, whose two edges alias one imported sequence;
and `projects/edge-errors`, whose `&ToAValue` writes both spellings of the anchored
scalar and earns `E0213` for each.

#### A position has one spelling

A handle's value is written **canonical decimal**: ASCII digits, no sign, no padding, no
surrounding white space, and no leading zero unless the value is `0`. `" 0 "`, `"+0"` and
`"00"` are each `E0225`.

Leniency here buys nothing and costs the one thing it always costs. A handle's value is
written by the author, beside the sequence it indexes, in a file this language owns;
there is no wire format to be tolerant of and no producer to accommodate. Accepting
several spellings of one position means two `definition` mappings can differ textually
and not at all, which is a thing a reviewer must then know is a non-difference.

#### A handle may not take an owned name

`connections` or `definition` written as a handle is `E0225`. The two names are the
language's on this node, and a handle that took one would be a name for a position that
also names a member, in a mapping whose whole job is to name positions.

#### `E0225`'s shape

Three conditions, one code: **not a position**, **past the end**, **an owned name**. They
share a code because they share a fix — the handle's value, or the handle's name — and
because a reader who has learned "a handle names a position in `connections`" has learned
all three at once.

* **The primary span is the handle's value**, which is the token that must change.
* **The message names its subject** — the edge the handle failed against — and not only
  the handle.
* **When `definition` was inherited, a note points at the subject and names the origin**,
  mirroring `E0221`'s treatment of an effective value that is not the failing node's.

The last two are one requirement seen twice. A `definition` is a member like any other,
so a family may declare its handles once and be extended by several edges that each
narrow `connections`; the primary span is then the base's line, which is **correct for
every edge of the family that reads the sequence whole**. Without the subject in the
message and the origin in a note, three edges produce three byte-identical errors on one
line that is not wrong, and none of them says which node to fix.

**Two handles naming one position is not an error.** The mapping is **many-to-one on
purpose**: a self-loop is written `from: 0` and `to: 0` over a single endpoint, and both
of its ends really are the same node. Where an index of endpoints has to carry *a* name,
the **first** handle naming a position labels it, so the index does not silently depend on
which handle was written last; the rest are not lost, because every handle is kept on the
edge and a lookup by handle reads that list.

**A malformed `connections` removes the bound and nothing else.** The two members are
read independently, so a `connections` that earned `E0223` or `E0224` does not excuse
`definition` from being read: its own shape is checked (`definition: 1` is `E0224`
whatever sits above it), and so are the two rejections that never asked how long the
sequence is — a handle taking an owned name, and a value that is not a position at all.
Only **past the end** is withheld, because there is no length to be past the end of;
raised against zero it would print one fault once per handle, and every one of those
would vanish when the member above was fixed. The edge then records **no** handles, which
is the same statement as its recording no endpoints: a sequence that was never read
writes no positions, so there are none to name. Without this rule a `definition` full of
nonsense was silently accepted whenever `connections` was malformed, which is a silence
about the very member the author was in the middle of getting wrong.

#### `E0223` and `E0224` are two codes because they have two fixes

* **`E0223` — the edge relates nothing.** Three conditions, one code, one fix: *write the
  endpoints*.
  * There is no `connections` in the resolved view at all.
  * There is one whose value is empty or null. Two situations reach this and they are
    **worded apart**, because the author's next move differs. An **unsatisfied
    declaration** — `pub connections:` written in a base and never supplied by the
    concrete edge — earns a note at the declaration, so the author is not sent to a line
    that is correct for every edge that does supply them. A node that emptied the member
    it wrote **itself** inherited nothing, and telling it about a declaration would send
    it looking for a base there is none of; its note points at its own key.
  * There is one written in a **base YAML** file, where nothing is a reach — argued
    under *An edge cannot take its endpoints from a base YAML file* below.
* **`E0224` — the wrong shape.** A `connections` that is neither a sequence nor null, or a
  `definition` that is not a mapping. The fix is the value's shape, which is a different
  thing to look at.

Answering an unsatisfied declaration with `E0224` would send the author looking for a
sequence they never wrote, which is why the empty and null spellings are `E0223`'s: a
tagged-empty declaration parses to an empty scalar and a bare one to plain null, and the
two are one statement (D7.3). The spellings that count are the **plain** ones the core
schema resolves to null — the empty scalar, `~`, `null`, `Null` and `NULL` — and only
plain: a quoted `"null"` is the string, and a string is not a sequence, so it is `E0224`
like any other wrong shape.

**`connections: []` is a legal degenerate edge and is neither.** A relation with no
endpoints *yet* is a shape, not a mistake — it is still an `!edge`, still addressable,
still emitted. An edge with no `connections` **member** is the mistake.

**A tag that means nothing is worse than one that does not exist**, which is the whole
argument for `E0223` having a code at all. `!edge` on a node with no endpoints would
otherwise be a decorative tag: it would classify, it would print, and it would be consumed
by nothing.

#### `!ref` on an endpoint

`!ref` on a `connections` item is **the same declaration of intent it is anywhere else**
(D4.3). It is legal wherever a path is, it says that this context intends to modify the
target, and it is checked as one: the target's scope must be writable from here or the
item is `E0217`.

**The flag is kept in the image.** An endpoint the edge declared it intends to modify and
an endpoint it merely relates are two different claims, and an image that recorded them
alike would say two endpoints are the same when the compiler had just reported that they
are not. So a connection record carries its `capability`, and *which endpoints does this
edge intend to modify* is a question the compiled graph can answer.

What `!ref` does **not** do here is contribute keys. `extends: !ref P` installs `own(A)`
on P and is the only spelling that does (D4.5); a `!ref` in any other position — including
this one — declares the dependency and the intent and installs nothing, so `E0214` and
`W0303` have exactly the inputs they had before.

*Fixture:* `projects/edge-capability`, whose edge relates one node in an `immutable` peer
directory with `!ref` (`E0217`) and one in its own scope without.

#### An edge cannot take its endpoints from a base YAML file

**A `connections` the resolved view sites in a `.yaml` is `E0223`.** `.yaml` is data, not
language (D6.6): its scalars are never read as paths, its anchors are not addressable,
and no pass resolves anything in it. So an `!edge` that includes or extends a `.yaml`
mapping holding `connections: [Alpha]` ends up with the member — the resolved view is
composed across file classes, because `<<` is YAML's and is governed in both — and with
**no endpoints**, because none of its items was ever a reach.

This was silent until it was not, and the silence was the argument. There was no `E0223`,
because the member was present and was a sequence; there was no `E0213`, because no path
had been written where the compiler was looking; and the author got a wrong graph with
nothing said about it, which is the failure mode D2.1 names as the worst this system has.
The rule it follows from was never in doubt — a base YAML file declares nothing and is
reached by import alone, so it cannot carry an edge's endpoints any more than it can
carry a canonical name — and all that was missing was saying so.

**The condition is the member's file class, not what the member holds.** `connections`
in a `.yaml` is `E0223` whether it holds three items, one, or none: base YAML cannot
carry endpoints, so an empty sequence there is not the deliberate degenerate edge
`connections: []` is in a `.yfy` — it is the same member in the same place it may not be.
One condition, no subcases, and a note pointing at the sequence in the `.yaml` beside the
message naming the edge.

**Only `connections` is refused, and `definition` is read like any other member.** The
two are not alike here: a `connections` item is a **reach**, and reaching is the thing
base YAML does not do, while a handle's value is a **position** — a small whole number
indexing a sequence — and nothing about reading one depends on the file class. Refusing
`definition` too would be a rule about file classes rather than a rule about reaching,
and it would say that data cannot hold a number.

*Fixture:* `projects/edge-base-yaml`, whose `defaults.yaml` writes both members and whose
edge earns exactly one diagnostic for the one of them that is a reach.

#### The two owned members are never data edges

One written relationship, one record. Everything under `connections` and `definition` is
the **language's**, on both sides, and none of it is also read as a member naming a node:

* a `connections` **item** is a `Connection` record and nothing else. An alias standing
  there is an endpoint, not an endpoint *and* a data edge — which is a real hazard rather
  than a hypothetical, because a data edge leaves the *collection* a value sits in, and
  for a sequence element that is the sequence rather than the edge node, so a duplicate is
  invisible to any check made on the edge's own run;
* the `connections` **value** is the language's too, on both outcomes. An alias standing
  there is the sequence of endpoints and its items are already recorded once each, so
  reading the alias as data would say the same relationships a second time under a second
  kind; and a value of the **wrong shape** records no relationship at all — it has earned
  `E0224` and relates nothing, and turning it into a data edge would put in the image the
  relationship the compiler had just refused;
* a **handle's value is a position**, so `owner: ./Alpha` is `E0225` and is *not* also a
  member of the edge that names `Alpha`. It indexes a sequence; it never names a node.

**And the two names are never `W0301`.** They are written by the *language*, not by the
family, so an edge extending any abstract family that does not itself declare them is not
warned about for writing the two members its tag requires it to write. Without the
exemption the compiler reports its own vocabulary as a misspelled field.

#### Visibility, in both directions

An edge's endpoints are gated by the `connections` **member**, which is a member like any
other and is therefore private by default (D4.12). Two accessors exist and both are
needed:

* **`connections_readable_from(observer)`** — the endpoints an observer may see. Two
  gates, both already written and neither restated: the `connections` member must be
  readable from there, and the endpoint must be a node that observer can see at all.
  There is no third predicate. **Filtered as it walks**, so an endpoint the observer may
  not see is absent by *shape* rather than present as a hole — and **positions never
  renumber**, so a handle still answers over the written positions and a filtered result
  never moves the endpoints beside the one it dropped.
* **`incident_edges_visible_from(observer)`** — *what relates this node*, gated. It must
  exist, because "what relates this public node", asked without a gate, hands the asker a
  **private** edge and hands it to them by name. It is the same two predicates read from
  the other end: the edge must be a node this observer can see, **and** the edge's
  `connections` must be readable from there — because **an edge whose endpoints are
  undisclosed has not disclosed that this node is one of them.**

*Why both gates, when in a clean project one implies the other.* In a project that raised
no `E0216`, the member gate is the one that bites and the node gate can never fire behind
it: `E0216` forbids an edge from naming a target its own scope cannot see, so every closed
scope on an endpoint's path encloses the edge's scope, and the member gate opens either
because the observer sits inside the edge's scope — in which case it sits inside every
closed scope above it — or because the edge's scope is reachable from the root, in which
case no closed scope but the root is on the endpoint's path either. The node gate is kept
for the case where that premise is **false**: a project that raised `E0216` still emits,
because only a *cycle* refuses emission (D1.8), and a broken project must not become the
way to read what a scope declined to publish.

*Fixtures:* `projects/edge-visibility` writes a `pub connections`, a bare one and a
private edge, read from a peer scope in both directions;
`projects/edge-invisible-connection` is the `E0216` case, where the image is asked the
question the compiler refused and gives the same answer.

#### Extending an edge

`connections` is **one member**, absorbed by the ordinary left-biased, shallow rule (D1.5,
D4.7). A child either restates the sequence whole or inherits it whole. **Extension never
appends endpoints**, and the operators are untouched — there is no `!edge`-specific
composition rule, because inventing one would make one member of one node kind obey a
merge law nothing else in the language obeys.

That is also why `E0223`, `E0224` and `E0225` are `check`'s and not `link`'s. Pass 4 has
resolved every path and knows what each item names, but only pass 5 knows **which
`connections` a node ends up holding**; reporting `E0223` against `own(A)` would fire on
every concrete edge of a family that declares its endpoints once in the base.

#### Fixtures

* `projects/edge-binary` — the ordinary case, and an edge carrying its own member.
* `projects/edge-nary` — three endpoints in one edge, an edge over an edge, and
  `connections: []`.
* `projects/edge-handles` — handles, and the many-to-one self-loop.
* `projects/edge-positions` — the gapped sequence and its worked example, an inline
  endpoint, the three near-misses of a position's one spelling, a handle taking an owned
  name, an inherited `definition` narrowed by a concrete edge, an unsatisfied inherited
  declaration, and a handle whose value looks like a path.
* `projects/edge-errors` — `E0223` for a node with no member and for one that emptied its
  own, both shapes of `E0224`, four conditions of `E0225`, an endpoint naming nothing,
  both spellings of an anchored scalar as an endpoint, and a `definition` read beside a
  `connections` that was never read.
* `projects/edge-base-yaml` — a `connections` and a `definition` inherited from a `.yaml`:
  one of them is a reach and is `E0223`, and the other is a position and is not.
* `projects/edge-shared-sequence` — two edges aliasing one imported sequence as the
  `connections` **value**.
* `projects/edge-extends` — an abstract edge family as a `!type`, restating and inheriting
  `connections`, and the `W0301` exemption.
* `projects/edge-mixin` — endpoints inherited from an untagged base, a `!node` base and a
  `<<`.
* `projects/edge-not-a-reach` — `connections` as an ordinary member on nodes no edge reads.
* `projects/edge-capability` — `!ref` on an endpoint, and `E0217`.
* `projects/edge-visibility`, `projects/edge-invisible-connection` — the two gated walks.
* `projects/tagged` — a **nested, anchorless** edge: `owns: !edge` as a member of `Api`,
  relating `Api` to itself and to `Service`. Addressable by nothing, and a node all the
  same.
* `fixtures/valid/tags.yfy` — the corpus file, which writes `!edge &owned-by` with both
  members.

---

## 7. Anchor state sequences

### D5.1 — An anchor is a position-scoped binding; redefinition is a state transition

An anchor name is not a variable and not a symbol-table entry. It is a **binding
scoped to a position in the stream**, and a redefinition is a **transition to the next
state of that name**.

```yaml
&t {port: 80}      # state 0
use_a: *t          #   -> sees port 80
&t {port: 443}     # state 1
use_b: *t          #   -> sees port 443
```

**Nothing here is new.** This is precisely what D2.1 and D2.2 already specify and
precisely what the parser already does: `AnchorId` identifies one `&name` occurrence,
two definitions of `t` are two `AnchorId`s, and an alias binds to the definition with
the greatest position strictly before it. The parser is not being asked for anything
it does not already provide. D5 is a **reframing**, and it is written down because the
frame decides D5.2, and the wrong frame there is not recoverable.

Fixture: `fixtures/shadowing/shadow-three-times.yml` — three definitions of `t`, three
aliases, three distinct targets. That file is a three-state sequence.

### D5.2 — A global name denotes the final state

A namespaced name, `ns::t`, denotes the **last** state of the sequence of definitions
of `t` — the one in effect at the end of the document.

*Why this settles the design's largest silent-wrong-model risk.* Global naming had to
answer what `ns::t` means when `t` is defined three times, and the tempting answers
are all bad. "It is an error" outlaws `shadow-three-times.yml`, which is legal YAML
and legal under D2.3. "It is ambiguous" makes a cross-file reference resolve to
something the compiler cannot name, and an unresolvable-by-construction reference is
worse than no reference. "It is the first" contradicts D2.1, under which the trailing
part of the document already sees the last.

Under D5.1 the question dissolves, because a sequence has a defined end. A path ending
in `t` is the final state — the same node every alias written after the last `&t` already binds
to, and the state the document leaves behind. A repeated name is not an ambiguity to
be resolved; it is a sequence with a well-defined last element. Had it been framed as
ambiguity, the model would have been silently wrong in exactly D2.1's dangerous way:
a path `peer/t` binding to a node other than the one a local `*t` binds to, with no
diagnostic anywhere.

**Earlier states remain addressable**, by index within the sequence. Nothing needs to
be retained to make this possible — `AnchorDef.shadows` already links each definition
to the one it hides, so the whole chain is in the arena and reachable in source order.
Index 0 is the first definition; the final state is the last index, and is what the
bare name denotes.

The **surface spelling** of an indexed reference is not settled here and is owed a
decision before indexed access ships. The path grammar (D4.12) has no room for it —
`name` excludes digits and `@` — so it is a grammar extension, not a convention. What is settled is that the states
exist, are ordered, and are addressable — which is the part that would have been
expensive to add later.

### D5.3 — `W0300` is reframed, not re-severitied

`W0300`'s wording read as a suspected error: the anchor "shadows an earlier
definition". Under D5.1 that is a mischaracterisation of a construct the language
supports on purpose. The diagnostic reports a **state transition** — this name now has
a new state, aliases after this point bind to it, and the global name will denote the
last one. Implemented: the message names the new state and the note names the state it
supersedes.

Everything else about it is unchanged from D2.3. It stays a warning, because
shadowing is valid YAML and is the intended way to express a name's evolution through
a document. Its severity stays configurable per project — `--deny W0300`, or
`severity = { W0300 = "error" }` under a `[diagnostics]` table, which is D2.3's spelling
and is written out in full there. It still
carries both spans, the new definition and the one it supersedes; that pairing is what
makes the sequence readable in the terminal, and it is why a reader can tell state 1
from state 2 in `shadow-three-times.yml` without counting lines.

`fixtures/shadowing/anchor-reused-not-shadowed.yml` still proves distinct names do not
warn: two names are two sequences of one state each, and a one-state sequence has
nothing to report.

---

## 8. The project

### D6.1 — A project is the compilation unit

**A project is a root directory whose tree is one namespace hierarchy.** `discover`
takes that root, walks it under the configured extension list (§5), and produces the
namespace tree the whole rest of the pipeline resolves against. Directories nest,
namespaces nest with them, and a header document's `namespace:` key names the
namespace its file contributes to — as in `fixtures/valid/header-document.yfy`, which
declares `namespace: acme::billing`.

**Checking one file is a project of one file.** `yamlfy check <file>` and
`yamlfy check <dir>` are the same operation at two scopes, not two operations. This is
stated as a definition rather than derived, because it is what makes "cross-file"
unremarkable: there is no special cross-file mode and no linking step distinct from
resolution. A path resolves through the scope tree of the project it is in; a project of
one file simply has a very short one, and a path that leaves it is `E0213` for the
ordinary reason that nothing in the project answers to that name — a `..` past the root
is the sharpest case, and it is reported as such. A file may still declare what it wants
bound, with a header `imports:` (D6.7), but an import is a binding operation over one
file's anchors, not a compilation mode and no longer a precondition for reach.

**`E0230` is a duplicate *definition*, not a duplicate namespace.** Several files
contributing to one namespace is the ordinary arrangement — it is how a namespace is
grown without one enormous file — and must not be an error. Within a document a repeated
name is a state sequence with a defined last state (D5.2), and across two imports the
order is authored in a header (D6.7); across two files that answer to one address there
is no authored order at all, so the winner would be decided by D6.2's path ranking, which
is to say by a filename. A graph whose values depend on a filename is what D1.8 refuses,
so this one is an error rather than a warning. *The wording in an earlier draft of this
decision — "two files declaring the same namespace" — was wrong and would have outlawed
the normal case.*

**The condition is one *directory*, one name, one definition — not one namespace — and it
applies whether or not a header is present.** A later draft said "the same canonical
path", and that was the second wrong wording: it made the check conditional on a declared
namespace, so two headerless files of one directory both defining `&Widget` would collide
in fact and pass in silence. They are reached by one `./Widget` (D4.12) and only their
filenames rank them, which is the identical fault. Making the rule depend on a header
would let *adding* one change the graph while changing no definition, and would let
removing one hide a collision.

**Addressability and canonical identity are two questions.** A **canonical path** —
`namespace/name` — exists only for a directory that claims a namespace, and it is the
project's identity for a definition. **Addressability by file and by directory exists
regardless**, because a path addresses a file or a directory and never a namespace
(D4.12). A definition can therefore be reachable without being canonically named, and
that is correct: a name is for identity, and a path is for arriving. For a file that
*does* declare a namespace the two conditions coincide — a namespace is claimed by one
directory and one only, which `discover` already enforces — so exactly one `E0230` is
raised either way, and the directory reading is the one that is right in both cases.

Fixtures: `projects/duplicate-namespace` for the declared case,
`projects/headerless-collision` for the same collision in a directory that claims
nothing.

**Raised by the link pass.** What `discover` raises `E0230` for is the pair of
*declaration* conflicts it can decide — two headers in one directory disagreeing about an
axis, and one namespace claimed by two directories — and both stay. The
duplicate-definition rule above needs the definition table, which the link pass builds:
an anchored **collection** in a source file is addressable, an anchored **scalar** is not
addressable at all, and a base YAML file contributes nothing (D6.6). Addressable is not
the same as emitted, so D7.1's exclusion of a nested anchored node from being a model of
its own is untouched. See §4.

**Addressability and canonical identity are two questions, and the path amendment parted
them.** A path addresses a *file* or a *directory* (D4.12), so a `.yfy` in a directory
whose headers claim no namespace is still reachable as `sibling/Name`. The canonical path
`namespace/name` remains the project's identity for a definition and is what `E0230`
compares; it is no longer what a reach resolves against. A definition can therefore be
reachable without being canonically named, which is correct: a name is for identity, and
a path is for arriving.

The engine is agnostic to what is being modelled — invoices, service topologies, type
lattices — because a namespace tree and an inheritance graph are all it knows about.

*Fixtures.* Project fixtures are directories, a shape `fixtures/` does not hold, so
they live in a sibling tree: `projects/<name>/`, one directory per project.
`projects/nested-namespaces` is the namespace tree of D6.1, `projects/duplicate-namespace`
is `E0230`, `projects/inherited-header` and `projects/scope-matrix` are D6.4 and D6.5,
and D6.7's are cited there.

### D6.2 — Discovery order is normative: root-relative path, compared by component

Files are ordered by their **path relative to the project root, compared component by
component**, and that order is part of the specification rather than an implementation
detail. Canonicalization is used to establish file *identity* — so that two routes to one
file are recognised as one file and read once — and **not** to establish order.

**Component order is normative, and it is not byte order.** An earlier draft of this
decision said "lexicographically", which reads as a comparison of the path's *text* and
is a different order. Compare `a.yfy` with `a/b.yfy`:

| comparison | first |
|---|---|
| **components** — `["a.yfy"]` against `["a", "b.yfy"]`, so `"a"` against `"a.yfy"` | `a/b.yfy` |
| **bytes** — `.` is `0x2E` and `/` is `0x2F` | `a.yfy` |

The two disagree, and they disagree in the ordinary case of a file beside a directory of
the same stem. **Component order is what this specification means**, and it is what the
compiler does.

*Why the distinction is worth a table rather than a footnote.* File rank is the first
element of D6.3's tuple, and D6.3 fixes **`E0212`'s primary span** — the diagnostic points
at the minimum member of the strongly connected component under that order. So the two
readings put the error on different lines of different files for the same cycle, which is
observable, reproducible under each reading, and impossible to reconcile after the fact.
An ordering rule that two implementers can read two ways is not an ordering rule; it is
the `readdir` problem with extra steps, and the whole of this decision is that the
`readdir` problem must not exist.

*Why component order and not byte order.* Both are total and both are deterministic, so
the choice is between two defensible answers and this one picks the one a reader already
holds. A path is a sequence of names, not a string that happens to contain separators;
comparing it as a string makes the separator character participate in the comparison, so
the order depends on where `/` sits in the encoding relative to the characters people put
in filenames — which is a fact about ASCII, not about the project. Component order also
keeps a directory's contents contiguous with the directory, which is what makes the walk's
own guarantee (parents before children) and the file rank agree instead of merely
coinciding.

*Why it has to be normative.* D1.8's recovery makes the inheritance graph acyclic by
dropping back edges in a depth-first search in document order. Which edge is a back
edge depends on where the search starts and in what order it proceeds. So without a
total order over files, `readdir` — which returns entries in filesystem order, varying
by filesystem, by directory history, and by machine — decides which edge is dropped.
That would be tolerable if the only casualty were the recovery value, which is never
emitted anyway (D1.8). It is not: the recovered view is what every later pass reads, so
it determines which `E0220`, `E0221` and `W0301` findings those passes produce. The
same source would print a **different set of diagnostics on a different machine**, with
the same exit status. A compiler whose reported errors depend on directory iteration
order is not reproducible in the only sense that matters to a user trying to fix them.

*Why the order is root-relative and not canonical.* Ordering by canonicalized absolute
path defeats the purpose it was introduced for. Canonicalization resolves symlinks, so
a tree that links a file in from elsewhere is ranked by wherever the target happens to
live on that machine — reintroducing exactly the machine-dependence the rule exists to
remove, and additionally making the order depend on the absolute location of the
checkout. The root-relative path is a property of the project's own contents, so two
clones of the same tree in different directories, on different machines, with different
symlink layouts, produce the same ranking. *An earlier draft of this decision specified
the canonical path; that was wrong for this reason.*

Canonicalization is still required, for identity: a file reachable by two routes must be
discovered once, or it would appear twice in the order and its definitions would collide
with themselves under `E0230`. Identity is a set question and order is a sequence
question, and they are answered by different keys.

### D6.3 — Document order is a tuple

"Document order", used unqualified by D1.8 and §2, is defined project-wide as

```
(file rank, document index, node index)
```

compared left to right, first difference deciding — file rank being the position from
D6.2, document index
the zero-based index of the document within its file (already carried on
`AnchorDef.document`), and node index the arena position within the document. Within a
single file this is exactly the order §1 and §2 already use, so no existing decision
changes meaning.

**`E0212`'s primary span is the minimum SCC member under that order**, with one note
per remaining member. D1.8 already requires the diagnostic be reported once per
strongly connected component naming every participant; the tuple is what makes *which*
participant is named first deterministic. Picking the minimum rather than, say, the
node the search happened to enter by means the same cycle prints identically no matter
where the traversal began.

### D6.4 — Two orthogonal axes, inherited from the enclosing scope, closed by default

Visibility (`private` / `public`) and mutability (`immutable` / `mutable`) are two
independent axes, and those four words are the reserved keywords of the access model.
On each axis, a scope that does not state a value **inherits its parent's**; a scope
that states one governs itself and all its descendants. The root scope has no parent and
therefore states both: **`private`** and **`immutable`**.

`fixtures/valid/header-document.yfy` carries both keys on a header document
(`visibility: public`, `mutability: immutable`). Any other value on either key is
`E0231`.

The axes are orthogonal because they answer unrelated questions — who may *see* a
node, and who may *change* it — and coupling them would make `public immutable`, the
single most useful combination in a graph database, inexpressible.

**Both defaults are the closed value, and that is a deliberate reversal.** An earlier
draft of this decision had the root `mutable`, on the reasoning that mutability was
recorded but unenforced and a closed default would be ceremony with nothing behind it.
Two things changed. Mutability is now enforced — an extended reference is a compile-time
write and `E0217` refuses one into a scope that has not opened (D4.12) — so the default
is no longer inert. And the path amendment made reaching *easy*: naming is reaching,
with no import to write, which removes the accidental friction that used to stand in for
a policy. With reach that cheap, an open default would mean every family in a project is
reopenable by any file that can spell its directory, and the first anyone would know is
a resolved view that changed under them.

So both axes are **opt-in**: a scope that says nothing grants nothing. Being reachable
and being writable are claims a file makes about itself, in its own header, and a
reviewer reading that header sees the whole of what it has agreed to. The cost is one
line in the files that mean to be open, which is the right place for that line to be —
in the definition, not in the reach.

*What this does not do:* it does not make a project harder to write inside itself. Root
is `private` and every scope in the project is inside root, so intra-directory reach is
unaffected (D6.5), and a `!ref` inside one file or one directory needs nothing declared.
The default bites exactly where it should — on a reach that crosses a directory
boundary into a scope that never said it was open.

**The same two axes exist one level down.** A member declares them with a `pub`/`mut`
prefix on its name, they default to the same closed values for the same reason, and they
compose with the scope's by D6.5's walk. See D4.12; the axes are one system read at two
granularities, and both granularities are writable.

### D6.5 — Resolution is path-composed, and needs no narrowing rule

```
visible(n, o)  = every scope on path(root -> n) is visible to o
writable(n, o) = every scope on path(root -> n) permits writing to o
```

A node is visible to an observer only if **every** scope from the root down to it is,
and writable only if every scope from the root down to it permits it.

*Why no narrowing restriction is needed on either axis.* The obvious worry is a
`public` node inside a `private` scope: it looks like a leak, and an access-control
system would answer it with a rule forbidding a child from widening its parent. No
such rule is required, because composition already gives the right answer. The
`public` node is reachable from **inside** the private scope, where the private gate is
already passed, and unreachable from **outside**, where the enclosing scope stops the
path before the node is ever reached. The `public` marking is not a lie; it means
"visible to everyone who can get here", and getting here is the enclosing scope's
business. These are Rust's `pub` semantics exactly, applied to both axes rather than
one, and Rust needs no widening rule for the same reason.

The mutability axis behaves identically, with the same reading: a `mutable` node
inside an `immutable` scope is writable by anything that can already write into that
scope, and by nothing else. `mutable` under `immutable` is therefore often inert, and
that is correct rather than a mistake to be diagnosed — it is what lets a subtree be
frozen without editing every node inside it.

**The implementation consequence is load-bearing.** Evaluating either axis
**node-locally** — reading the node's own marking, or its nearest explicit ancestor's
— makes an `immutable` parent mean nothing at all, because any descendant marked
`mutable` escapes it. Path composition is the whole mechanism; it is not an
optimisation detail and it cannot be replaced by resolving each node's effective
marking once and consulting that.

**The mutability axis is enforced, and `!ref` is what enforces it.** An earlier draft of
this decision said Phase 1 "records and propagates mutability but ships no writer".
That is superseded: an extended reference is a **write performed at compile time**, so
`writable(target, referencing scope)` must hold for one, composed over the whole path
exactly as `visible` is, and `E0217` is the failure (D4.12). The predicate is the same
walk as `visible`'s, with the same outermost-blocker reporting, so the two axes cannot
give contradictory accounts of who shut a reach out.

**Where this stops, and why that is a boundary and not a debt.** The axis is enforced
at compile time, against the one write the language performs: an extended reference.
Enforcing it at *query* time is a different thing, and it belongs to a runtime — which
is a separate artifact and is deliberately not specified here. What this document owes
such a runtime is the composition rule, and D6.5 is it: the same walk, the same
outermost blocker, so a runtime that follows it cannot disagree with the compiler
about who may write what.

### D6.6 — Two file classes, and why there are two

A project holds two kinds of file, and the difference is semantic **and** syntactic.

| class | extension | what the engine reads |
|---|---|---|
| **yfi source** | `.yfy` | the full language: the `//` comment and the two block forms (§10), a header, the three operations (§6), `!ref`, `!type` / `!node` / `!edge` (§9, D4.13), namespaces, scope axes, member flags |
| **base YAML** | `.yaml`, `.yml` | ordinary YAML. Objects and data the engine compiles or runs over |

*The syntactic half arrived after the semantic one, and does not replace it.* When this
decision was written the two classes were the same syntax read two ways, and the
argument below is entirely about the reading. `.yfy` has since acquired constructs YAML
rejects (§10), so the classes now differ before the parser as well as after it — but the
extension still decides the *reading*, and would still have to even if the syntaxes had
stayed identical. A `.yfy` holding not one construct from §10 is still the full language,
and that is the case the argument below is about.

**In a base YAML file the operators are not interpreted.** `extends:` is an ordinary
field with an ordinary string key. `!ref` is an unrecognised tag on a value, not a
reference. `!type` and `!node` say nothing about abstractness or emission, `!edge` says
nothing about relations, and `connections` is a field name like any other. There is no
header, so the file declares no namespace and no scope axes. `<<` is the only construct
that behaves the same in both classes, and only because it is not ours: it is YAML
1.1's de-facto merge key, and §1 governs it in both file classes because §1's rules
were written to match what every existing YAML implementation already does. A `.yaml`
file's `<<` edges are in the inheritance graph for D4.10's cycle rule — the engine has
to resolve them to run over the data, and an oscillating merge is as meaningless there
as anywhere — but they create no `is_a` edge, because inclusion never does (D4.1).

*Why two classes rather than one and a heuristic.* The engine's subject matter is other
people's data. That data is already YAML, it is frequently generated, vendored, or
owned by another team, and it contains whatever field names it contains. A single class
would force the engine to guess whether `extends:` in a service definition is an
operation or a field, and every available heuristic — look at the operand, look for a
header, look for a tag — decides a semantic question from an incidental signal. The
file class decides it by declaration instead: **the extension states whether the file is
written in this language.** That is also the answer to §5's file-extension item, which
was closed on the wrong axis; see there.

*Consequence for membership.* A `.yfy` is not a data store: everything nested in it is a
member of its parent, and the data is what is evaluated from that structure. A `.yaml`
is the data, and declares no members at all. That is the same declaration-not-heuristic
rule read one level down, and it is D4.12's membership rule (see there).

*Consequence for the extended reference.* A base YAML file declares nothing, so it holds
**no addressable definitions**, so no path can name anything in it and it cannot be
extended by reference. It is reachable only by import (D6.7),
after which its nodes may be included and may be extended locally like any other
mapping. This is deliberate: an extended reference is a global, retroactive edit to a
family, and the engine must not be able to perform one on a file the language does not
own. A team that wants a base YAML's data to carry a canonical name imports it into a
`.yfy` and declares it there, where the name belongs to a file that opted in.

*Fixtures.* The two files this decision reclassified have been renamed and are now
`fixtures/valid/header-document.yfy` and `fixtures/valid/tags.yfy`; they write
`!yfi/header`, `!node`, `!edge` and `!ref`, which D6.6 makes meaningless in a `.yml`
file, and they carried the wrong extension only because they predate the split. The rest
of `fixtures/` is genuinely base YAML — `fixtures/merge/*`, `fixtures/shadowing/*` and
`fixtures/cycles/*` exercise `<<`, anchors and aliases, all of which §1 and §2 govern in
both classes — and stays as it is.
`projects/nested-namespaces/net/edge.yaml` is the case that catches a wrong
implementation: it writes a `!yfi/header` document, a nonsense axis value and an
`extends` key, all of which are inert because the file is base YAML.
`projects/reserved-tag/objects.yaml` is the same test for the tag vocabulary.

*Fixture owed:* an operator spelling used as an ordinary **field name** in a base YAML
file, distinct from the tag and header cases already covered.

### D6.7 — The header import

A header may import other files. This is the only mechanism by which a definition
written in one file becomes available in another **under an ordinary alias**, and the
only way to reach a base YAML file at all.

**It is no longer the only way to reach across a file, and that is the one clause of
this decision the path amendment rewrote.** A path performs its own reach (D4.12), so
`extends: ../core/Service` needs no header line. The two mechanisms coexist because they
answer different questions and neither can be expressed in the other:

| | what it is | what it delivers | what it costs |
|---|---|---|---|
| **path** | an address | one definition, at the point of use | nothing to declare, nothing to keep in step |
| **`imports:`** | a binding | every anchor of a whole file, installed before the document's first event | one header line, and an authored order when names collide |

Three consequences, each of which is a rule elsewhere read from this side:

1. **An import does not change what a path means.** It writes into the importing
   *document's* anchor table only; it does not move a node into the importer's namespace
   or directory (point 2 below). So a path written in a file that imports and a path
   written in a file that does not resolve identically.
2. **A path does not install a binding.** It names one node and delivers that node, so a
   path in the body of a document cannot change what an alias below it resolves to, and
   D2.6 is untouched by the amendment.
3. **Both are gated, by the same two axes, in different places.** An unreachable import
   is `E0241` at the header line; an unreachable path is `E0216` at the path; a `!ref`
   into an unwritable scope is `E0217`. Reach is never a grant, whichever spelling asked
   for it.

The import survives because it earns its place: `*alias` across a file is only sayable
this way, base YAML is only reachable this way (D6.6), and when two files define one
name the header is the only place an *authored* order exists.

```yaml
--- !yfi/header
namespace: acme::web
imports:
  - core/service.yfy        # definitions
  - vendor/defaults.yaml    # data
```

**The key is `imports:`, plural**, and it is the only spelling. *An earlier draft of this
decision wrote the example with `import:`, which is not a header key at all* — and the
compiler answered the difference with silence, because an unrecognised header key is
ignored rather than reported. A file that wrote `import:` therefore imported nothing,
bound nothing, and then failed at every alias with `E0100 unknown anchor`: the wrong code,
in the wrong place, about lines that were written correctly. **That an unknown header key
is silent is itself a gap** — the header's key set is small, closed and entirely the
language's, so a misspelling there has no legitimate reading the way a misspelled *member*
does (`W0301`) — but it is a gap in the header and not in this decision, and no code is
allocated for it here.

`imports:` is a flat sequence; each entry names one file, resolved relative to the
project root (D6.1). An entry naming a file outside the project, or naming nothing, is
`E0240`. *The selective form — importing named definitions rather than a whole file —
is not specified, because nothing yet needs it; the whole-file form is what the
operator set depends on.*

**Importing a `.yfy` brings definitions; importing a `.yaml` brings objects.** That is
the entire reason two classes exist (D6.6). From a `.yfy` you receive nodes that carry
declarations, tags, ancestry and axes, and which can therefore be extended, validated
against, and queried on the `is_a` axis. From a `.yaml` you receive nodes that carry
keys and values and nothing else: they are legal inclusion sources and legal extension
operands, but their declared view is empty, so `E0220` and `E0221` have nothing to say
about anything descending from them, and `W0301` will not fire on a node whose only
ancestor is base YAML data. That is not a defect — importing data is asking for data.

**What an import does, precisely.** It is a *binding* operation over the importing
file's anchor table, performed by `discover` before the file's first document event:

* Every anchor definition of the imported file is registered in the importing
  document's anchor table, in import order, before any definition written locally.
* An imported binding is an ordinary `AnchorDef` as far as §2 is concerned. Positional
  resolution (D2.1) and the state-sequence model (D5.1) apply to it unchanged.
* Each document of the importing file starts with the same imported bindings and
  nothing else, so **D2.6 is untouched**: nothing leaks between documents; the imports
  are re-installed identically at every document start.

Three things fall out of that and need no new machinery:

1. **Name collisions are already specified.** A local `&Service` after an import
   shadows the imported one — `W0300`, both spans, the local definition winning for
   aliases after it (D2.1, D2.3). Two imports both defining `Service` shadow in import
   order, and import order is **authored in the header**, not discovered from the
   filesystem, so the outcome is deterministic and reviewable. Under D5.1 this is a
   two-state sequence and the last state is what the bare name denotes; no new rule and
   no new code is required.
2. **An import does not change any namespace.** The imported node keeps its canonical
   path in the namespace it was written in. An import writes only into the importing
   *document's anchor namespace*, which is document-local by D2.6; it does not move a
   node into the importer's namespace, does not re-export it, and does not make
   `acme::web/Service` name it. The project's namespace tree is built from
   headers and directories (D6.1) and is unaffected by who imports whom.
3. **An import is not a visibility grant.** Reach is path-composed on the imported
   node's *canonical* path (D6.5), so importing a node the importer cannot see is
   `E0241`, not a way around the private marking. Note that root's `private` (D6.4)
   bounds the project against the outside, not its interior — a private root is visible
   to everything beneath it, so ordinary intra-project imports are unaffected, and
   `E0241` fires only for a genuinely nested private scope.

**Imports are not transitive.** You receive the imported file's *own* definitions, not
the things it imported. This is D4.9's rule — a clause is consumed where it is written —
applied to the file level, and it has the same justification: a re-exporting import
would deliver names the importing file never asked for and cannot see the source of,
and would make the set of names in scope a function of an unbounded transitive closure.
Definitions still work when they arrive: if the imported `Service` was written
`extends: *Base` against a `Base` its own file imported, its clause was discharged
there (D4.9), so you receive a resolved `Service` complete with `Base`'s keys while
having no name for `Base` at all.

**An import cycle is legal, and no code is allocated for it.** This is the answer the
model forces rather than the one the question offers. `E0212` exists because a cycle in
the *inheritance* graph has no defined value — D1.8's oscillation. An import carries no
value and no dependency: because imports are not transitive, a document's bindings are
the union of `own(f)` over the files its header names, never those files' imports and
never any resolved view. The **meaning** of a mutual import is therefore a one-step
union — `a` sees `own(a) ∪ own(b)`, `b` sees `own(b) ∪ own(a)` — with no fixed point in
it and the same answer whichever file is read first. Rejecting them would forbid a
construct with a perfectly defined meaning, which is the opposite of D1.8's reasoning,
and folding them into `E0212` would report a cyclic-inheritance error against files
that may contain no inheritance at all.

**Obtaining `own(f)` is a different question, and it does iterate.** `own(f)` is read
off a parse, and a parse stops at the first alias it cannot bind: an unknown alias is a
*scan* error, so recovery resumes only at the next document boundary and every anchor
written below that alias is lost. Every member of a cycle is in that state until the
others are bound, and none of them can go first, so an unbound parse is not a source of
`own(f)` for a cycle member. The binding pass therefore **seeds** a cyclic component
from the `&name` tokens read out of each member's *text*, installs the seed, re-parses,
and repeats until the exported names stop moving; the seed is a prelude and never an
answer, being replaced by each member's own parsed definitions in the first round. The
loop is bounded by the component's size, and the seed is what makes the first round
already complete in every ordinary case. *An earlier draft of this decision claimed
mutual imports "terminate in one pass with both documents fully bound". That is true of
the meaning and false of the computation: one pass sufficed only for a member whose
anchors happen to be written above its first cross-file alias, which made whether a
cycle compiled depend on the order of lines inside a file.*

*This is conditional on non-transitivity and the condition should be stated.* If
re-exporting imports are ever added, the import relation becomes a dependency, name
resolution acquires a fixed point, and an acyclicity rule becomes necessary. It must
then get **its own code**: the failure would be non-termination of name resolution, a
different condition from value oscillation, and a user told "cyclic inheritance" about
two headers would be looking for the wrong thing.

**An imported definition can still be extended by reference, and importing does not
protect against one.** `extends: !ref ../core/Service` is the same global operation
whether or not anyone imported that file; a path addresses the tree and has no interest
in local bindings. And because an import binds a *name to a node* while resolution
happens later, an import never snapshots: the `Service` you imported resolves at link
time with every extended reference installed on it, including ones added afterwards in
files you have never read. Import is reach, not insulation.

**What does protect against one is the mutability keyword**, and since D6.4's defaults
flipped it protects by default. A file that never writes `mutability: mutable` cannot be
extended by reference from outside its own directory, whoever imported it. That is the
guarantee an import was never able to give, and it is given by the definition rather
than by the reach — which is the only place it could be given honestly.

*Fixtures*, under `projects/` (D6.1):

* `projects/imports-source` — a `.yfy` importing a `.yfy`: definitions cross, and the
  same file named twice is one import.
* `projects/imports-data` — a `.yfy` importing a `.yaml`: objects cross, and the data
  file has no header and cannot import. The data file sits beside its importer rather
  than in a subdirectory, because a data file declares nothing (D6.6) and so a
  directory holding only data inherits `private` and is unreachable from a sibling —
  which is now `E0241` rather than a silent no-op.
* `projects/import-alias` — closed cross-file extension: `extends: *Service` against an
  imported definition, the case D4.4 exists for.
* `projects/import-cross-document` — the same bindings re-installed at every document
  start, so D2.6 is untouched.
* `projects/import-shadowing` — two imports of one name, shadowing in the order the
  header authors rather than the order discovery ranks them.
* `projects/import-shadowed-locally` — a local definition after the import supersedes
  it, with `W0300` pointing at each state in the file that wrote it.
* `projects/import-not-transitive` — `a` imports `b` imports `c`; `a` has no name for
  `c`.
* `projects/import-cycle` — a mutual import, legal, no code.
* `projects/import-cycle-late-anchor` — the same shape with every anchor written
  *below* the alias that needs the other side, as a 2-cycle and as a 3-cycle, both
  clean. This is the case no unbound parse can start, and it is what the text seed
  exists for.
* `projects/import-reinstalled-after-shadow` — an imported name also defined locally in
  an **earlier document**: that local state died with its document (D2.6), the next
  document starts with the imported bindings again, and the alias there is an ordinary
  alias to the import rather than `E0130`.
* `projects/import-private` — an import is not a visibility grant: the edge is recorded,
  nothing is installed, the definition keeps its own scope, and the import is `E0241`.
* `projects/import-private-alias` — the same unreachable import, *aliased*: the
  diagnosis is `E0241` at the import entry, not `E0100` at the alias.
* `projects/import-missing` — one path naming nothing and one leaving the project, each
  `E0240`.

**Where `E0241` points, and why.** Its primary span is the **import entry in the
importing header**, which is the text the author wrote and the only one they can
change; `E0240` points at the same place, so the two import faults read alike. Its note
carries the second span, the shape `W0300` and `E0110` already use, and names **the
scope that blocked the reach** at the `visibility:` that closed it — because reach is
path-composed (D6.5), so the blocking scope is often neither the target's directory nor
one the importing author has ever opened, and naming the target alone would point at a
marking that is already correct. The exporting definition is not the primary span:
the import form is whole-file, so there is no one definition to blame, and the location
the author must act on is their own header.

`projects/import-private-alias` is the fixture that makes the change matter. Without
`E0241` an unreachable import that is also aliased reports only `E0100 unknown anchor`
at the alias — the wrong code, in the wrong file, about a construct that is written
correctly. The alias still fails, and is still reported, because `E0241` diagnoses what
an import does not do rather than changing it; what the code adds is the cause,
reported where the fix is.
---

## 9. Declarations

### D7.1 — Abstract and concrete, and what untagged means

* **`!type`** is **abstract**: inheritable, validated against, and **never emitted**
  as a model in the compiled output.
* **`!node`** is **concrete**: emitted.
* **`!edge`** is **concrete**: emitted, exactly as `!node` is. It is a fourth node kind
  and not a third state of this axis — what it adds is D4.13's two members, not a new
  answer to *abstract or concrete*. **There is no tag for an abstract edge and none is
  needed**: the abstract counterpart of a concrete node is whatever the author wrote, so
  a family of edges is a `!type` that declares `connections`, and an untagged mixin
  carrying the member is one too.
* **An untagged node is abstract.**

The last is the one that needs an argument, and the corpus supplies it.
`fixtures/merge/*` and `fixtures/cycles/merge-diamond.yml` are built out of untagged
anchored mappings — `&base`, `&a`, `&b`, `&c` — that exist solely to be merged into
something else. They are mixins. If untagged defaulted to concrete, every one of those
fixtures would emit half a dozen junk models, and the ordinary act of factoring shared
keys into a named mapping would pollute the graph. Abstract-by-default makes the
common case correct with no annotation, and makes emission an explicit act: a model
appears in the output because someone wrote `!node`.

This does **not** make plain data disappear. An untagged mapping or sequence appearing
as a *value inside* a concrete node is that node's data, not a candidate model — the
question of abstract versus concrete only arises for a node that could be emitted in
its own right. `fixtures/valid/header-document.yfy` is the case to read against: the
document is `!node &invoice-001`, and the untagged `&line-a` and `&line-b` inside its
`lines:` sequence are values of that invoice. They are emitted as part of it, and not
as models of their own. `fixtures/valid/tags.yfy` writes the other reading explicitly,
tagging its line items `!node` so that they *are* models.

This also settles what a base YAML file contributes. `!node` is not interpreted in that
class (D6.6), so no node in a `.yaml` file is ever concrete, and an imported base YAML
emits no models of its own — its nodes are data, emitted as part of whatever `.yfy`
node includes or extends them. Nothing has to be added for that; it is D7.1's default
arriving at the right answer from the other direction.

### D7.2 — Inheritance across the abstract/concrete boundary is unrestricted

`!node` may extend `!node`. `!type` may extend `!node`. `!node` may extend `!type`, an
untagged mixin, or both. There is no rule about which kinds may inherit from which.

*Why not.* Every restriction available here is a rule from class-based OO — abstract
cannot descend from concrete, only abstract may be inherited from, a concrete class is
final. This is a **prototypal** model: a node is a node, some are emitted and some are
not, and inheritance is key absorption plus an `is_a` edge. Adding kind restrictions
would reinvent classes on top of prototypes, which means importing their failure modes
(the abstract-base-class ceremony, the "make it abstract just to inherit from it"
refactor) in exchange for preventing constructs that are useful. `!type` extending
`!node` is exactly how a family is generalised out of a working instance after the
fact, which is the normal direction of discovery in a graph that already has data in
it.

The restriction that does apply is D4.10's: the inheritance graph must be acyclic,
whatever the tags on its nodes.

### D7.3 — The three-state declaration rule

This is the rule validation is built on; without it there is nothing to check.

| written | state | meaning |
|---|---|---|
| `port: !!int` | tagged, empty | **required** — a descendant must supply a value |
| `port: !!int 3` | tagged, with value | **optional**, defaulting to `3` |
| `region:` | untagged, empty | **declared, unconstrained** — the key exists, anything may fill it |

A concrete node that leaves a required key unsupplied — by its own keys or by anything
it inherits — is `E0220`. A value whose tag contradicts the declared one is `E0221`,
checked against declarations and not the flattened view (D4.8).

**What `E0221` compares, exactly.** The supplied value's **explicit tag**, by
`(is core schema, suffix)` — never by handle, because a `%TAG` directive rewrites the
handle and leaves the suffix alone, and because `!node` must not compare equal to the
core schema's `tag:yaml.org,2002:node`. An **untagged scalar is not resolved against the
core schema** and is therefore never `E0221`: YAML's own resolution would call plain
`8443` an `!!int`, which reads well until a field declared `!!str` is given plain
`8443` — legal YAML, and rejecting it would be the compiler inventing a rule this
section does not state. What *is* compared without a tag is the **kind**: a mapping or a
sequence supplied where a core scalar tag is declared is a mismatch no schema resolution
can explain away.

*Why the empty/valued distinction has to carry the meaning.* Without a bottom marker
there is no way to say "must be supplied". The candidate spellings all fail: `port:`
alone is null, and **null is a legitimate inherited value** — a descendant may
perfectly well want to inherit "no port" — so "unsupplied" and "supplied as nothing"
would be the same text. A sentinel string (`port: REQUIRED`) is data, indistinguishable
from a default of that string. A separate `required: [port]` list duplicates the field
names and goes stale the moment one is renamed. The tag-without-value spelling is
already legal YAML, is already what the tag means (a type with no inhabitant chosen),
and puts the requirement on the same line as the field it constrains, where a reader
looking at the field cannot miss it.

The third state is what keeps the model open. `region:` declares that the key is part
of this family's shape without saying anything about its contents — enough for
`W0301` to know the key is not a typo, and not enough to constrain it.

*No fixture yet.* No file under `fixtures/` writes a tagged-empty declaration;
`fixtures/valid/tags.yfy` exercises tags on values only. Each of the three states owes
a fixture, and `E0220`/`E0221` owe one each.

### D7.4 — `!oneof` is reserved and unimplemented

The spelling `!oneof` is reserved now, and writing it is `E0222` — reserved, not
implemented. A reservation with no diagnostic behind it is not a reservation: the tag
would otherwise be an unrecognised one on an ordinary value and would silently do
nothing, which is the failure mode reserving it was meant to prevent. It is not
implemented in Phase 1 and is not scheduled.

*Why reserve rather than ignore.* Enumerations are the one constraint the prototypal
model cannot express with what it has. Everything else a declaration says is said by a
tag and a value, but `mode: [tcp, udp]` is **textually identical** to a sequence-valued
field whose default is those two elements, and no amount of reading the value tells
the two apart. Expressing an enum therefore requires new syntax, and adding new syntax
to a language that has already shipped documents is a breaking change unless the
spelling was held back. Reserving costs one rejected name today and preserves the
option.

Fixture: `projects/reserved-tag` — `modes.yfy` raises `E0222`, and `objects.yaml`
writes the same spelling and hears nothing, because the tag vocabulary is not
interpreted in base YAML (D6.6). Implemented: `TagKind::OneOf`, raised by `discover`,
which is where the tag and the file's class are both already known.

### D7.5 — Range, regex and length constraints are out of scope permanently

Numeric ranges, string patterns and collection length bounds will not be added. This
is a decision, not an omission, and it is recorded here so it is not re-litigated as
an open item.

They are a different kind of thing from everything in D7.3. A declaration in this
system says what a field *is* — its type, and whether it must be present — which the
graph needs in order to know the shape of what it stores. A range says what a field's
value must *satisfy*, which is a predicate language, and a predicate language does not
stop at ranges: it acquires cross-field conditions, then quantifiers, then a solver,
and the compiler's job stops being compilation. Yamlfication compiles a graph; the
application that consumes the graph owns its business rules and is better placed to
express them in a language that has functions.

---

## 10. The `.yfy` front end

Three constructs, one pre-pass, and one promise about positions. This section exists
because `.yfy` stopped being YAML: each construct below is a `ScannerError` in a real
YAML parser, verified against one, which is the trigger §5 recorded and never expected
to fire.

**The pre-pass runs over `.yfy` only.** A `.yaml`/`.yml` reaches the parser exactly as
it was written — no comment rewriting, no blocks, nothing — because base YAML is data
the engine operates over and not a language it compiles (D6.6). Which front end a file
gets is decided by its class, which is decided by its extension, which is a declaration
the author made.

### D8.1 — `// …` is a line comment, with `#`'s rule

`// note` is a comment to the end of the line. It is rewritten to `# ` before the parser
sees it, and `//` and `# ` are both two characters, which is why this spelling and not
another: the substitution is free and moves nothing (D8.4).

**A comment opens where a `#` opens: at the start of a line, or after white space.**
Nowhere else. That is not a simplification of YAML's rule, it *is* YAML's rule, and it
is what keeps `url: http://host/thing` a URL — the `//` there follows a `:` — and
`note: "a // b"` a string. Inside a quoted scalar, inside an existing comment and inside
a block scalar, `//` is text.

The cost is the one `#` already has and is inherited knowingly: `path: //server/share`
is a null value and a comment, exactly as `path: #server/share` would be. An author who
knows where a `#` may appear knows where a `//` may, which is the whole reason to give
the new construct the old rule instead of a better one.

*Why have it at all.* `#` is not going away and both are legal. `//` is what the
language's users write in every other file they open that day, and a comment syntax is
the cheapest possible place to be familiar.

### D8.2 — `<?-- … --!>` is documentation

The region is captured for documentation generation and **emits no node**. To the
parser it is white space, so `doc: <?-- why 8443 --!>` leaves `doc` null: documentation
is not a value.

Its contents are never parsed. Each block is recorded with its kind, its text verbatim
and its span, and `emit` is what attaches one to what it documents — which is a pass-6
question and is deliberately not answered here.

### D8.3 — `<?-- … -->` is a code block, and it is a value

The contents reach the model **as a string carrying a flag that says it is code**:
`ScalarStyle::Code`, a scalar like any other, addressable and inheritable like any other
member's value.

**This language never parses the contents.** Any syntax may appear inside — `[unbalanced`,
a bare `"`, a tab-indented `--- x`, another language entirely — and none of it is read,
because it is compiled or executed by something else. The contents are the text between
the delimiters, verbatim, with nothing trimmed and nothing interpreted.

**The first terminator closes the block.** A `-->` inside the contents ends it, because a
language that does not read the contents has no basis on which to decide that one was
meant as text. `<?-- x --> y --!>` is a code block holding ` x `.

*How it reaches the arena.* Not by injection. The pre-pass replaces the region with
filler chosen so that **the parser builds the node itself** — a plain scalar occupying
exactly the region's first line, with the remaining lines blanked — and the block's text
and style are then written into that scalar. So a code block is a node with the span the
author wrote, in the place the grammar puts it, with the parentage the parser gave it.
The alternative considered and rejected was to blank the region entirely and splice a
node in afterwards, which would have required this language to invent a node's position
in a mapping the parser had already finished building, and to guess which null value it
was meant to replace.

*The one constraint this places on an author.* A multi-line code block's terminator line
must hold nothing after the `-->` but white space, and the block's first line must sit
where a value may sit. Both are how anyone writes one; neither is checked, because the
failure is an ordinary YAML syntax error at a position the author can see.

### D8.4 — The rewrite is a character-for-character substitution

**One character out for one character in, and a line break is never touched.** This is
the whole design of the pre-pass and it is what lets §3's span model survive it: every
node's line, column and byte offset is a position in the file the **author wrote**, not
in the text the parser read.

The property that has to hold, and is tested as a property rather than asserted as a
rule: *a `.yfy` file with these constructs and the same file with plain YAML written in
the same space yield identical `(kind, span)` for every node that exists in both.* The
corpus is swept the same way — every readable fixture parsed under both front ends, node
for node, kind for kind, span for span, diagnostic for diagnostic — because the pre-pass
over a file that uses none of the constructs must be exactly the identity. (One fixture
is deliberately not UTF-8 and has no text to compare; the other 46 are all swept.)

**Byte offsets need two tables and get them.** A substitution preserves characters and
lines but not bytes: an ASCII space replacing a two-byte character is one byte shorter.
So a rewritten file keeps the offsets of both texts, resolves every position against the
text **as written**, and slices only against the text the **parser read**. Without that
the span of a node after a block holding non-ASCII text would index the wrong byte, and
the guarantee D3.4 makes — that a span slices its own text out of the file — would hold
everywhere except where it was least obvious.

**An unterminated block is `E0104`, and costs its own line.** The line is blanked, the
rest of the file is read exactly as written, and one diagnostic is reported at the `<?--`
that opened it. This is `E0100`'s bargain (§4): report the cause once, keep everything
that can still be understood, and do not let one bad construct decide the fate of the
file around it.

### D8.5 — Two keys naming one member are `E0110`

`port:` and `pub port:` in one mapping are two key *texts* and one *member*. The parser
compares texts, which is the right rule there and the only one available to it — it does
not know the file's class and so cannot know which text carries a flag — so the
collision is raised by `link`, where the member names are known, under the **same code**
because it is the same fault with the same fix.

Without it the second entry would be dropped by left-biased absorption (D1.2) in
silence, which is the failure mode D2.1 names as the worst this system has.

*Fixtures:* `projects/member-flags` for the constructs in a compiled project, and
`projects/member-collision` for the collision. The front end's own behaviour — the
identity sweep, the URL that is not a comment, the block scalar that keeps its `//`, the
code block that holds unparseable text — is fixtured in the syntax crate's tests rather
than in `fixtures/`, because each case is about *two* readings of one text and the corpus
holds one file per reading.

---

## 11. The language, end to end

Everything above is a decision. This is the shape they add up to, for a reader who
wants to know what yfi *is* before reading why. It states nothing new; every claim
cites the decision that owns it.

### The four names

| name | is |
|---|---|
| **Yamlfication** | the ecosystem, and the runtime engine |
| **yfi** | the syntax |
| **`.yfy`** | the file format |
| **`yamlfy`** | the runtime execution — what you invoke to run the engine |

You `yamlfy` the `yfi`.

### What a `.yfy` file is

A file of **definitions**, in a **project** whose directory tree is its scope tree
(D6.1, D6.4). It may open with a header document declaring its namespace, its two scope
axes and its imports (D6.7); everything after that is definitions.

A definition is a named node, and there are **four kinds of node** (D7.1, D4.13):

| written | is |
|---|---|
| `!type` | abstract — inheritable, validated against, never emitted |
| `!node` | concrete — emitted |
| `!edge` | concrete — emitted. A node whose content is what it connects: `connections` names the nodes it relates, in written order and in any number, and `definition` names positions in that sequence so an endpoint can be addressed as `source` rather than as `0` |
| untagged | abstract, which is what keeps an ordinary mixin from polluting the graph |

There is no tag for an *abstract* edge and none is needed: a family of edges is a `!type`
that declares `connections`, and an edge inherits it by the ordinary operators. What a
node declares about its members is written as a key, a tag and possibly a value — three
states, of which the useful one is the tag with no value, meaning *required* (D7.3).

A member is anything nested inside something else, exactly as YAML nests — a `.yfy` is
not a data store, and the file class is the whole of the discriminator (D4.12). A node's
members may be written as a mapping, where each key names a member and its value
declares it, or as a sequence of names, which declares that the members exist and
constrains nothing. Each member may carry `pub`/`public` and `mut`/`mutable` as a prefix
on its name; both are opt-in and a bare member is private and immutable (D4.12).

### The three operators, and nothing else

| written | name | what it changes besides A |
|---|---|---|
| `A` with `<<: P` | inclusion — A **has** a P | nothing |
| `A` with `extends: P` | extension — A **is a** P | nothing |
| `A` with `extends: !ref P` | extended reference — **every P** carries A | every P |

The set of **inheritance operators** is closed at three and no fourth will be added
(§6). Two are safe and one changes the world, which is why they must not look alike.
That closure is about the operators and about nothing else: the **node kinds** are not
closed and were never claimed to be — `!edge` is a fourth one, added after the sentence
was first written, and it inherits with these same three and adds no way to read a
clause.

`P` is a **path**, spelled the way a filesystem is spelled because the scope tree is the
directory tree: `../shared/Service`, `peer/Service`, `Service`, `Service.tls.port`.
Naming is reaching; there is no import to keep in step with a path (D4.12). `!ref` is
not how a reference is written — a plain path is — it is a declaration that this context
intends to **modify** the target, legal wherever a path is, and checked (D4.3, D4.12).

### Who may see and who may change

Two orthogonal axes, `private`/`public` and `immutable`/`mutable`, at two granularities:
on a **scope**, in a header (D6.4), and on a **member**, as a prefix (D4.12). Both are
closed by default at both granularities, and both compose the same way — over the whole
path from the root, never node-locally (D6.5). Failure to see is `E0216`; failure to
write is `E0217`, and it is asked only of a `!ref`, which is the only construct that
writes at compile time.

**Visibility is asked first, and it is asked inside path resolution.** A path landing
where the referencing scope cannot see resolves to **nothing** — no definition, no
member, no `is_a` edge, no endpoint — so `E0216` is the only answer it can earn and its
shape does not vary with whether the node or the member exists. That is what stops a
refusal from becoming an oracle over a private scope's contents (D4.12).

### Relations are nodes

An `!edge` is a node whose content is what it connects (D4.13). `connections` is a
sequence, so a relation is **n-ary** — three services sharing a rotation is one edge, not
three — and an endpoint may itself be an edge, which is how a relation over relations is
written. `definition` names positions in that sequence, many-to-one, which is how a
self-loop is written. Positions are what is **written**: an endpoint that resolves to
nothing costs one `E0213` and renumbers nothing beside it, and a filtered read leaves the
same gap rather than closing it.

`connections` is **not a reserved word**. It is a reach position on an `!edge` and on
whatever an `!edge` reads it from, and an ordinary member name everywhere else — so an
edge inherits its endpoints from an untagged mixin, and a `!type &Router` listing
`["eth0", "eth1"]` is two strings and nothing else.

### `.yaml` is the other half, and is not this language

Base YAML is the data the engine compiles and runs over. None of the above is
interpreted in it: `extends:` is a field, `!node` is a tag nobody reads, `//` is text,
there is no header and there are no member flags (D6.6). It is reached by `imports:`
and by nothing else, and what it brings is objects rather than definitions (D6.7). The
one thing shared is `<<`, because that is YAML's and not ours (§1).

### The pipeline

| pass | name | what it does |
|---|---|---|
| 1 | `discover` | walk the project, classify each file, read headers, resolve imports, build the scope tree and both axes (D6.1, D6.2, D6.4, D6.7) |
| 2 | `parse` | the `.yfy` front end (§10), then events into one arena per file, with spans (§§0–3) |
| 3 | `intern` | symbols, member names with their flags taken off, tags, documents, parents, scopes (D4.12, D7.1) |
| 4 | `link` | the definition table, every path resolved, every clause validated, the stratified inheritance graph (D4.10, D4.11) |
| 5 | `check` | cycles, resolved views in precedence order, the epistemic gate, declarations (D4.7, D4.12, D7.3) |
| 6 | `emit` | the compiled image: a node per resolved holder, each one's ancestor chain, an edge index in both directions over the three operators, data edges and every `!edge`'s connections (D4.13), each node's scope path — and the point at which member gates are **applied** while walking |

**All six passes are implemented.** *An earlier version of this line said pass 6 was "not
written, and deliberately absent rather than stubbed"; that has not been true since
`emit` shipped.* What remains true is the reason it was said: emission is **refused
outright** when the inheritance graph held a cycle, so a recovered view never reaches
output, and a refused image holds nothing rather than holding a repair (D1.8).

Diagnostics accumulate at every pass; no pass stops at the first problem (§4). **Two
passes raise none** — `intern` and `emit` — which is not the same as being absent: pass 3
is where a member's flag prefix is taken off and a tag is classified against its file's
class, and pass 6 is where a member's gate is applied. Severity is decided once, by the
pass that raises the finding, and one report is rendered from one source map in position
order across the whole invocation (§4).
