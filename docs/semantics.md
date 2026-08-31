<!-- Written by Richard Christopher, Copyright 2026 Richard Christopher -->

# Yamlfication — Semantic Decisions

**Status:** normative for Phase 1. **Applies to:** `yamlfy-syntax` (implemented) and
the `link` / `check` passes (specified here, implemented in Phase 1 steps 3–4).

YAML 1.2 defines a serialisation. It does not define what a *graph database* should
do with merge keys under cycles, with anchors redefined mid-document, or with
positions in a stream that has already been tokenised. Those three gaps block the
`link` pass, so they are settled here, fixture first.

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
* a **flat** sequence whose every element is a mapping or an alias to one.

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

so `R(a) = R(b) ⊕ R(c)` and `R(b) = R(a) ⊕ R(d)`, with `own(a) = own(b) = {}`.
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
| `W0300` | warning | anchor shadows an earlier definition (D2.3) | parse |
| `E0211` | error | illegal merge source (D1.6) | link — Phase 1 step 4 |
| `E0212` | error | cyclic merge (D1.8) | link — Phase 1 step 4 |

`E0211` and `E0212` are specified and fixtured here but are **not** implemented in
`yamlfy-syntax`; they need resolved aliases, which is the link pass's job. They are
listed so the numbering is stable.

**Duplicate key identity.** Two keys collide when they are both scalars, have the same
text, and have the same merge role (D1.1). Non-scalar keys are not compared: deciding
whether two mappings are the same key needs resolved values, which the parser does not
have. Fixture: `fixtures/malformed/duplicate-key.yml`.

---

## 5. Open decisions

Recorded, not decided. Each needs an explicit answer before the pass that depends on it.

* **File extension.** Everything above is native YAML, which is the plan's stated
  advantage: zero custom lexing. A distinct `.yfy` extension is therefore a *naming and
  discovery* decision, not a syntax one, and is cheap — `discover` (Phase 1 step 3)
  would accept `.yml`, `.yaml` and `.yfy` alike. It only becomes a syntax decision if
  Yamlfication later adds constructs that a YAML parser would reject, at which point the
  event-level foundation here stops applying. **Needs your answer before `discover`.**
* **`<ORG>` for the copyright header.** Files currently carry
  `Written by Richard Christopher, Copyright 2026 Richard Christopher`. The plan's own
  open item. **Needs your answer.**
* **Does `!ref` participate in merge?** D2.6 confines `<<` to one document. If
  cross-document inheritance is wanted, `<<: !ref ns::path` needs its own decision —
  including whether D1.8's cycle rule then spans files.
