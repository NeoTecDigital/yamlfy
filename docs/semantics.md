<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Yamlfication — Semantic Decisions

**Status:** normative for Phase 1. **Applies to:** `yamlfy-syntax` (implemented) and
the `discover` / `link` / `check` passes (specified here, implemented in Phase 1
steps 3–4).

YAML 1.2 defines a serialisation. It does not define what a *graph database* should
do with merge keys under cycles, with anchors redefined mid-document, or with
positions in a stream that has already been tokenised. Those three gaps block the
`link` pass, so they are settled here, fixture first — §§1–3. YAML defines nothing at
all about nominal inheritance, projects, scope axes or declarations, which are this
system's own and are settled in §§6–9.

Every decision below is backed by a file under `fixtures/`. The fixture is the
specification; this document explains it.

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

Fixture: `fixtures/merge/quoted-merge-key-is-literal.yml`.
Implemented: `yamlfy_syntax::is_merge_key`; `Entry::merge` on every mapping entry.

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
link pass makes the merge graph acyclic by depth-first search in document order and
**drops each back edge** — the merge edge whose target is already on the DFS stack.
Every node then has a defined resolved view and later passes can report their own
findings. This recovery value is **not** a language semantic and is never emitted:
compilation fails whenever `E0212` was raised.

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
configurable per project (`--deny W0300`, or `severity = { W0300 = "error" }`).

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

Fixtures: `fixtures/shadowing/no-shadow-across-documents.yml`,
`fixtures/cycles/cycle-shared-across-documents.yml`.
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

* `byte` is an offset into the **original file bytes**, so it can index the file on
  disk directly.
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
| `E0110` | error | duplicate mapping key | parse |
| `E0120` | error | anchor name unrecoverable (D3.5) | parse |
| `E0121` | error | anchor recovery out of order (D3.5) | parse |
| `E0130` | error | alias crosses a document boundary (D2.6) | parse |
| `E0210` | error | more than one merge key (D1.7) | parse |
| `W0300` | warning | anchor enters a new state (D2.3, reframed by D5.3) | parse |
| `E0211` | error | illegal merge source (D1.6) | link — Phase 1 step 4 |
| `E0212` | error | cyclic inheritance (D1.8, D4.10) | link — Phase 1 step 4 |
| `E0213` | error | unresolved `!ref` (D4.3, D4.11) | link — Phase 1 step 4 |
| `E0214` | error | conflicting extended references (D4.11) | link — Phase 1 step 4 |
| `E0220` | error | required field unsatisfied (D7.3) | check |
| `E0221` | error | declared-tag mismatch (D7.3, D4.8) | check |
| `E0222` | error | `!oneof` is reserved, not implemented (D7.4) | discover |
| `W0301` | warning | undeclared field on a concrete node (D7.3) | check |
| `W0303` | warning | inert extended-reference contribution (D4.11) | link — Phase 1 step 4 |
| `E0230` | error | conflicting scope declarations (D6.1) | discover |
| `E0230` | error | duplicate *definition* in a namespace (D6.1) | link — Phase 1 step 4 |
| `E0231` | error | bad header axis value (D6.4) | discover |
| `E0240` | error | unresolved import (D6.7) | discover |
| `E0241` | error | import target not visible (D6.7, D6.5) | discover |

Everything from `E0211` down is specified here but **not** implemented in
`yamlfy-syntax`. `E0211`–`E0214` and `W0303` need resolved aliases and refs, which is
the link pass's job; `E0220`–`W0301` need declared views, which is `check`'s.
`E0222`, `E0231`, `E0240` and `E0241` need only a file class or a project
tree, both of which `discover` holds, and are raised there in `yamlfy-core` — `E0241`
alongside `E0240`, in import resolution, because the scope tree is final by then and
the binding pass would report a cyclic component's import once per rebinding round.
The rest are listed so the numbering is stable. **No code is
allocated for an import cycle**, which is legal — D6.7 argues why, and why it must not
be folded into `E0212` if that ever changes. `E0212`'s message text changes with D4.10: it is **cyclic
inheritance**, not cyclic merge, because a user whose file contains no `<<` at all
should not be told their merge is cyclic.

**`E0230` is one code over two conditions, and only one of them is implemented.**
`discover` raises it for the two *declaration* conflicts it can decide from headers and
directories alone: files in one directory disagreeing about an axis, and one namespace
claimed by two directories (D6.1). The **duplicate definition** — one namespace, one
name, two files — is **owed by the link pass**, for the same reason `E0213` is: it is
detected when the canonical-path table is built, and building that table is what
resolving a `!ref` means. It cannot be answered earlier without first deciding *which*
anchors carry a canonical path, and no decision here has made that one — D7.1 already
holds that an anchored node nested inside another (`&line-a` in an invoice's `lines:`)
is not a model of its own, so grouping every `&name` in a namespace would report a
duplicate against two nodes that are not addressable at all, which is exactly the false
positive D6.1's own errata warns against. Until link raises it, two files declaring one
canonical path are accepted silently, and that is a gap rather than a decision.

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
different keys.

Non-scalar keys are not compared: deciding whether two mappings are the same key
needs resolved values, which the parser does not have. A merge tag on a
*non-scalar* key is not a merge key (D1.1) and is currently reported by nothing —
see §5.

Fixture: `fixtures/malformed/duplicate-key.yml`.

---

## 5. Open decisions

Recorded, not decided. Each needs an explicit answer before the pass that depends on it.

* ~~**File extension.**~~ **Superseded by D6.6: both, and they mean different
  things.** `.yfy` is Yamlfication source; `.yaml`/`.yml` is base YAML the engine
  compiles or runs over. The earlier answer here — that `.yfy` is unnecessary because
  everything is native YAML with zero custom lexing — was **right on its own axis and
  answering the wrong question.** It treated the extension as a *syntax* decision, and
  the revisit trigger it recorded (the first construct a YAML parser would reject) has
  not fired and may never fire; the event-level foundation of §§1–3 still applies to
  both classes. What forced the split is *semantics*: the same bytes need two readings,
  because `extends:` must be an operation in one class and an ordinary field in the
  other, and no signal inside the file decides that without guessing. `discover` still
  filters by a configurable extension list, so the spellings are configuration; what is
  normative is that there are **two classes with different readings**.
* **Which GNU licence.** The project ships GPL-3.0-or-later. That choice has a
  consequence the plan's Phase 3 will run into: a Go server layer over a C ABI into
  this core is a combined work under the GPL, so the server must be GPL too. If the
  core is meant to be embeddable in closed software, that wants **LGPL-3.0**; if it is
  meant to stay copyleft across a network service, that wants **AGPL-3.0**.
  **Needs your answer before Phase 3, not before Phase 1.**
* ~~**Does `!ref` participate in merge?**~~ **Answered: yes.** D2.6 confines `<<` to
  one document *by operand*, not by operator: `<<: *alias` stays document-local and
  `<<: !ref ns::path` crosses files. D1.6 gains `!ref` as a legal merge source, and
  **D1.8's cycle rule spans files** — one inheritance graph, one cycle rule, so a
  cycle formed half by `<<` and half by `!ref` is still `E0212`. This requires a
  normative total file order (canonicalized-path lexicographic); without one,
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
* **`W0302` inconsistent inheritance order.** Deferred, not rejected. Two ancestors
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
* **A merge tag on a non-scalar key has no diagnostic.** D1.1 says a merge key is a
  *scalar* tagged `!!merge`, so `!!merge [k]: 1` is correctly not a merge key — but
  it then becomes an ordinary non-scalar key, which duplicate detection also skips,
  so two of them in one mapping are silent. Needs either `E0211` or a new code.
  **Needs an answer before `link`.**

---

## 6. Inclusion, extension, extended reference

§1 answers *what keys does this node end up with*. It does not answer *what is this
node*, and it does not answer *what does writing this node change elsewhere*. Those
are three questions, so there are three operations, and the language names them:

| written | name | meaning | what it changes besides A |
|---|---|---|---|
| `A` with `<<: B` | **inclusion** | A has B as one of its members. A makes no claim about what B is anywhere else. | nothing |
| `A` with `extends: B` | **extension** | A is a type of B, within the context of the parameter. | nothing |
| `A` with `extends: !ref B` | **extended reference** | A is a direct extension of B *itself*: **B depends on A**, and every B in the program carries A's definition. | every B |

Read the right-hand column first. **Two of the three are safe and one changes the
world.** Inclusion and extension both leave their operand exactly as it was; only the
extended reference reaches back into the base. Everything else in this section is a
consequence of that asymmetry.

**The operand selects the operation.** `extends: *base` and
`extends: !ref ns::base` may name the *same node* and are not the same operation.
The natural guess — that `!ref` is merely "the linked version", the same inheritance
reaching across a file boundary — is wrong, and it is wrong in the dangerous
direction: the `!ref` form is the global one. This is stated first because it is the
one misreading that produces a silently wrong graph across an entire project.

**The operator set is closed at exactly these three.** There is no fourth operator and
none will be added — no `!use`, no `!from`, no `A extends B from C`. This is a
specification of the language, not a description of what happens to be implemented, so
a later reader finding a case the three do not cover should not read that as a gap
awaiting a fourth spelling. Every remaining question about reach — how a definition in
another file becomes available at all — is answered by the **header import** (D6.7),
which is a property of the *file*, not of the operators. Keeping the operator set
closed is what makes the table above exhaustive and therefore learnable: three
spellings, three blast radii, and nothing else to check.

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

**Extension, `extends:` with an alias or an inline definition.** A is a type of B,
**within the context of the parameter** — the claim holds where it is written and
nowhere else. A appends B's definition to itself and retains the ancestry as a
first-class, queryable `is_a` edge. B is untouched; no other node's resolved view
moves by so much as a key. The blast radius is exactly A.

That boundedness is the entire difference from the third operation, so it is worth
stating in the negative: an extension does **not** assert that A is a type of B
globally, does not register A with B, and does not make B aware that A exists. It is
a claim A makes about itself, in its own context.

**Extended reference, `extends:` with a `!ref`.** A is a direct extension of B itself.
**B depends on A.** Every node that is a B — the ones already written, the ones in
other files, the ones being written right now by someone who has never read A — now
carries A's definition. This is Swift's `extension` and Ruby's open class, not
subclassing. The blast radius is every B in the program.

*Why the language has this at all:* an open-class extension is how a family acquires a
property after the fact without editing the file that defines the family — which, in a
graph a whole organisation writes into, is often the only available move. It is
powerful for the same reason it is dangerous, and the design's answer is not to remove
it but to make it **look different from the safe operation at the point of writing**.

*No fixture yet.* Every case in this section owes one; `fixtures/merge/` covers
inclusion only.

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

*Consequence, as in D1.1:* an inheritance key and a literal `"extends"` key are
different keys and may coexist in one mapping without a duplicate-key error.

**D1.7 needs no analogue.** `extends` is an ordinary key by text, so two `extends:`
entries in one mapping are already `E0110`. D1.7 exists only because a merge key is
identified by *role* and therefore escapes text-based duplicate detection (§4,
"Duplicate key identity"); `extends` does not. Multiple operands are written
`extends: [*a, *b]`, exactly as D1.7's fix-it already directs for merge.

### D4.3 — Inclusion is unchanged, and `!ref` under `<<` is not an extended reference

D1.6 gains one clause: a `!ref` resolving to a mapping is a legal merge source,
alongside a mapping and an alias, including as an element of the flat sequence form. A
`!ref` that resolves to nothing is `E0213`; one that resolves to a non-mapping is
`E0211`. This is what §5's answered open item promised, and it is the whole of the
change to §1.

`<<: !ref ns::path` is **cross-file inclusion**, and it is safe. It absorbs the
referent's keys into A and changes nothing about the referent. Under `<<`, the operand
carries only *scope* — `*alias` is document-local (D2.6, unchanged), `!ref` crosses
files — exactly as D2.6 already established for data edges.

So `!ref` does not have one meaning. It is a cross-document reference whose effect is
set by the operator it is an operand of:

| written | operation | direction of dependency |
|---|---|---|
| `<<: *alias` | inclusion, document-local | A → B |
| `<<: !ref B` | inclusion, cross-file | A → B |
| `extends: *alias` | extension, document-local | A → B |
| `extends: !ref B` | **extended reference** | A → B **and B → A** |

The last row is the only one with an edge pointing back into the operand, and it is
the row that has to be spotted while reading. This also discharges D2.6's closing
clause, which said node-level inheritance via `<<` is confined to one document "unless
`!ref` is later given merge semantics". It now has them. D2.6 itself is unchanged:
`<<: *alias` reaching into another document is still `E0130`, because that is an
illegal *alias*, not an illegal inclusion.

### D4.4 — Extension is document-local; the import is what crosses files

An extension's operand is an alias or an inline mapping. Aliases do not cross a
document boundary (D2.6), so **an extension is document-local by construction**, and
that rule is not relaxed anywhere in this specification.

It would appear to follow that "A is a type of B" is unsayable when B lives in another
file, leaving only `extends: !ref B` — the operation that extends B for everybody — as
the cross-file is-a spelling. It does not follow, and the reason is the one structural
idea this design rests on:

> **The file boundary is crossed by the import, not by the operation.**

A header **imports** another file (D6.7). The import brings that file's definitions
into *this* document, where they are ordinary anchors. An alias then reaches them
because they are here — not because the alias learned to travel:

```yaml
--- !yamlfy/header
namespace: acme::web
import:
  - core/service.yfy        # brings `&Service` into every document of this file
---
Frontend: !node
  extends: *Service         # closed. Ordinary alias, ordinary extension.
  port: !!int 8080
```

`extends: *Service` here is the second operation with all of its properties intact:
`Frontend` is a type of `Service`, an `is_a` edge is recorded, and **`Service` is
untouched** — the file that defines it is not modified, and no other node that is a
`Service` moves by a key. The blast radius is `Frontend`, exactly as D4.1 says, even
though `Service` was written somewhere else entirely.

**D2.6 is preserved verbatim, not weakened.** Anchors still do not cross a document
boundary; an alias to an anchor defined in an earlier document is still `E0130`. By the
time `*Service` is written, `Service` *is* a definition of this document — the import
put it there before the document's first event (D6.7). Nothing about §2 changes, which
is why this is the mechanism rather than a new operand form.

So each of the three operations reaches other files, and each reaches them in its own
character:

* **inclusion** — `<<: !ref B` directly, or `<<: *B` after importing B's file. Keys,
  no `is_a` edge, nothing changed anywhere else.
* **extension** — `extends: *B` after importing B's file. Closed, bounded, `is_a`.
* **extended reference** — `extends: !ref B`, needing no import at all, and changing
  every B in the program.

The middle row is the ordinary case and it is now available, which is why no fourth
operator is needed and why the set is closed.

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

One grimoire, one document, `namespace: guild::stock`. The Guildmaster wrote the top
half; an apprentice writes the bottom half.

```yaml
--- !yamlfy/header
namespace: guild::stock
---
water: &water
  solvent: spring-water
  volume_ml: !!int 250

BasePotion: !type &BasePotion
  vessel: vial
  cork: wax
  label: !!str            # required (D7.3): every potion must be labelled

SleepingTonic: !node      # the Guild's own potion, on the shelf, brewed daily
  extends: *BasePotion
  label: Sleeping Tonic
```

**Inclusion — the apprentice's tonic *contains* water.**

```yaml
MoonTonic: !node
  <<: *water
  label: Moon Tonic
  volume_ml: !!int 100    # own key wins, before or after the clause (D1.2, D1.4)
```

```yaml
R(MoonTonic) = {solvent: spring-water, volume_ml: 100, label: Moon Tonic}
R(water)     = {solvent: spring-water, volume_ml: 250}      # untouched
is_a(MoonTonic) = { }                                       # a tonic is not a water
```

The tonic has water in it. Water has no opinion about tonics, and nothing anywhere
else in the Guild moved.

**Extension — the apprentice defines a proper new potion.**

```yaml
HealingDraught: !node
  extends: *BasePotion
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

**Extended reference — the same entry, one `!ref` slipped in.**

```yaml
HealingDraught: !node
  extends: !ref guild::stock/BasePotion    # <- one token different
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
safe spelling (D4.5). Nothing in the entry they are reading is wrong. The single local
signal is `W0303` on the `label` line: their `label: Healing Draught` was contributed
to `BasePotion`, where `label: !!str` already sits above it, so that part of the
contribution is inert. A warning about the wrong key is the only thing standing
between the apprentice and the entire Guild.

That is why the two spellings must not look alike. `extends: *BasePotion` and
`extends: !ref guild::stock/BasePotion` name the **same node** and are different
operations, and the `!ref` — which everywhere else in the language is the ordinary,
unremarkable way to point at something in another file — is the one that reaches back.

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
must be satisfied. `E0220` likewise asks whether each ancestor's required keys (D7.3)
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
  `E0213`, `E0214`, `E0220`, `E0221`, `W0301`, `W0303`.

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

**An extended reference that resolves to nothing** is `E0213`, the ordinary
unresolved-`!ref` error; **one resolving to a non-mapping** is `E0211`, the ordinary
illegal-source error. Neither needs a special case.

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
for why the third operation's spelling must be visibly different from the second's.

*Fixtures owed:* `E0214`, `W0303`, and a `W0301` case whose verdict changes because of
an extended reference in another file.

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

Under D5.1 the question dissolves, because a sequence has a defined end. `ns::t` is
the final state — the same node every alias written after the last `&t` already binds
to, and the state the document leaves behind. A repeated name is not an ambiguity to
be resolved; it is a sequence with a well-defined last element. Had it been framed as
ambiguity, the model would have been silently wrong in exactly D2.1's dangerous way:
`!ref ns::t` binding to a node other than the one a local `*t` binds to, with no
diagnostic anywhere.

**Earlier states remain addressable**, by index within the sequence. Nothing needs to
be retained to make this possible — `AnchorDef.shadows` already links each definition
to the one it hides, so the whole chain is in the arena and reachable in source order.
Index 0 is the first definition; the final state is the last index, and is what the
bare name denotes.

The **surface spelling** of an indexed reference is not settled here and is owed a
decision before `!ref` resolution is written. What is settled is that the states
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
a document. Its severity stays configurable per project (`--deny W0300`). It still
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
`yamlfy build <dir>` are the same operation at two scopes, not two operations. This is
stated as a definition rather than derived, because it is what makes "cross-file"
unremarkable: there is no special cross-file mode and no linking step distinct from
resolution. A `!ref` resolves through the scope path of the project it is in; a project
of one file simply has a very short scope path, and a `!ref` that leaves it is `E0213`
for the ordinary reason that nothing in the project answers to that name. A file does
declare what it wants in scope, with a header `import:` (D6.7), but an import is a
binding operation over one file's anchors, not a compilation mode.

**`E0230` is a duplicate *definition*, not a duplicate namespace.** Several files
contributing to one namespace is the ordinary arrangement — it is how a namespace is
grown without one enormous file — and must not be an error. `E0230` is *defined* to
fire when two files declare the **same canonical path**: one namespace, one name, two definitions,
in two files. Within a document a repeated name is a state sequence with a defined last
state (D5.2), and across two imports the order is authored in a header (D6.7); across
two files in a namespace there is no authored order at all, so the winner would be
decided by D6.2's path ranking, which is to say by a filename. A graph whose values
depend on a filename is what D1.8 refuses, so this one is an error rather than a
warning. *The wording in an earlier draft of this decision — "two files declaring the
same namespace" — was wrong and would have outlawed the normal case.*

**Not implemented; the link pass owes it.** What `discover` raises `E0230` for today is
the pair of *declaration* conflicts it can decide — two headers in one directory
disagreeing about an axis, and one namespace claimed by two directories — and both stay.
The duplicate-definition rule above needs the canonical-path table, and which anchors
enter that table is a `!ref` question the link pass owns (D7.1 excludes a nested
anchored node from being a model of its own, and nothing here has yet said whether it is
nevertheless addressable). See §4.

The engine is agnostic to what is being modelled — invoices, service topologies, type
lattices — because a namespace tree and an inheritance graph are all it knows about.

*Fixtures.* Project fixtures are directories, a shape `fixtures/` does not hold, so
they live in a sibling tree: `projects/<name>/`, one directory per project.
`projects/nested-namespaces` is the namespace tree of D6.1, `projects/duplicate-namespace`
is `E0230`, `projects/inherited-header` and `projects/scope-matrix` are D6.4 and D6.5,
and D6.7's are cited there.

### D6.2 — Discovery order is normative: root-relative path, lexicographic

Files are ordered by their **path relative to the project root, compared
lexicographically**, and that order is part of the specification rather than an
implementation detail. Canonicalization is used to establish file *identity* — so that
two routes to one file are recognised as one file and read once — and **not** to
establish order.

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

compared lexicographically — file rank being the position from D6.2, document index
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

### D6.4 — Two orthogonal axes, inherited from the enclosing scope

Visibility (`private` / `public`) and mutability (`readonly` / `mutable`) are two
independent axes. On each axis, a scope that does not state a value **inherits its
parent's**; a scope that states one governs itself and all its descendants. The root
scope has no parent and therefore states both: **`private`** and **`mutable`**.

`fixtures/valid/header-document.yfy` already carries both keys on a header document
(`visibility: public`, `mutability: readonly`). Any other value on either key is
`E0231`.

The axes are orthogonal because they answer unrelated questions — who may *see* a
node, and who may *change* it — and coupling them would make `public readonly`, the
single most useful combination in a graph database, inexpressible.

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
inside a `readonly` scope is writable by anything that can already write into that
scope, and by nothing else. `mutable` under `readonly` is therefore often inert, and
that is correct rather than a mistake to be diagnosed — it is what lets a subtree be
frozen without editing every node inside it.

**The implementation consequence is load-bearing.** Evaluating either axis
**node-locally** — reading the node's own marking, or its nearest explicit ancestor's
— makes a `readonly` parent mean nothing at all, because any descendant marked
`mutable` escapes it. Path composition is the whole mechanism; it is not an
optimisation detail and it cannot be replaced by resolving each node's effective
marking once and consulting that.

**Phase 1 records and propagates mutability but ships no writer.** The axis is
computed, carried on every scope and queryable; nothing yet acts on it. It is
specified now because retrofitting an access axis onto an existing graph format is a
breaking change, and because `header-document.yfy` already writes the key.

### D6.6 — Two file classes, and why there are two

A project holds two kinds of file, and the difference is semantic, not cosmetic.

| class | extension | what the engine reads |
|---|---|---|
| **Yamlfication source** | `.yfy` | the full language: header, the three operations (§6), `!ref`, `!type` / `!node` (§9), namespaces, scope axes |
| **base YAML** | `.yaml`, `.yml` | ordinary YAML. Objects and data the engine compiles or runs over |

**In a base YAML file the operators are not interpreted.** `extends:` is an ordinary
field with an ordinary string key. `!ref` is an unrecognised tag on a value, not a
reference. `!type` and `!node` say nothing about abstractness or emission. There is no
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

*Consequence for the extended reference.* A base YAML file has no header, therefore no
namespace, therefore **no canonical path**, therefore it cannot be the target of a
`!ref` and cannot be extended by reference. It is reachable only by import (D6.7),
after which its nodes may be included and may be extended locally like any other
mapping. This is deliberate: an extended reference is a global, retroactive edit to a
family, and the engine must not be able to perform one on a file the language does not
own. A team that wants a base YAML's data to carry a canonical name imports it into a
`.yfy` and declares it there, where the name belongs to a file that opted in.

*Fixtures.* The two files this decision reclassified have been renamed and are now
`fixtures/valid/header-document.yfy` and `fixtures/valid/tags.yfy`; they write
`!yamlfy/header`, `!node`, `!edge` and `!ref`, which D6.6 makes meaningless in a `.yml`
file, and they carried the wrong extension only because they predate the split. The rest
of `fixtures/` is genuinely base YAML — `fixtures/merge/*`, `fixtures/shadowing/*` and
`fixtures/cycles/*` exercise `<<`, anchors and aliases, all of which §1 and §2 govern in
both classes — and stays as it is.
`projects/nested-namespaces/net/edge.yaml` is the case that catches a wrong
implementation: it writes a `!yamlfy/header` document, a nonsense axis value and an
`extends` key, all of which are inert because the file is base YAML.
`projects/reserved-tag/objects.yaml` is the same test for the tag vocabulary.

*Fixture owed:* an operator spelling used as an ordinary **field name** in a base YAML
file, distinct from the tag and header cases already covered.

### D6.7 — The header import

A header may import other files. This is the only mechanism by which a definition
written in one file becomes available in another under an ordinary alias, and it is
what makes the operator set closable at three (§6).

```yaml
--- !yamlfy/header
namespace: acme::web
import:
  - core/service.yfy        # definitions
  - vendor/defaults.yaml    # data
```

`import:` is a flat sequence; each entry names one file, resolved relative to the
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
   `!ref acme::web/Service` mean anything. The project's namespace tree is built from
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
protect against one.** `extends: !ref core::svc/Service` is the same global operation
whether or not anyone imported that file; `!ref` addresses the canonical path and has
no interest in local bindings. And because an import binds a *name to a node* while
resolution happens later, an import never snapshots: the `Service` you imported
resolves at link time with every extended reference installed on it, including ones
added afterwards in files you have never read. Import is reach, not insulation.

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
* **An untagged node is abstract.**

The third is the one that needs an argument, and the corpus supplies it.
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
