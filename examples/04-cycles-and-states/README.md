<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Cycles in data, and anchors as state

```sh
yamlfy check examples/04-cycles-and-states
```

## Cyclic data is legal

`ring.yfy` is a ring: each node names the next, and the last names the first. That
compiles, and it is meant to — a graph database's subject matter is graphs, and refusing
cycles would refuse the point.

What is refused is a cycle through **inheritance**, because inheritance has to be
*resolved* and a cyclic resolution has no answer that does not depend on the order the
compiler happened to visit things in. So the two graphs are held apart: the data graph
here is a ring, the inheritance graph is a tree, and both are fine.

This is also why an alias is a **reference and never a copy**. A loader that resolves an
alias by copying cannot express a ring at all — it either duplicates forever or gives up.

## A path may name something written later

```yfy
--- !node &Third
pub next: First      # `First` is written below this
```

A path resolves by **name**, so it can close a ring. An alias could not: `*First` binds
to a definition that already exists at the point the alias is written, which is why a
mutual reference written with aliases has to be nested.

## An anchor is a position-scoped binding

`states.yfy` writes `&state` twice in one document. That is not a mistake and is not
treated as one — it is a **state transition**, and an alias binds to whichever state is
in force where the alias is written:

```yfy
first: &state
  port: 80
early_reader: *state     # sees 80
second: &state
  port: 443
late_reader: *state      # sees 443
```

The compiler says so rather than complaining:

```
W0300  anchor `&state` enters a new state; aliases after this point bind to this
       definition, and the bare global name denotes the last state
  note: the state it supersedes is here
```

Both spans are carried, so the sequence is readable from the diagnostic. The severity is
configurable: a project that wants redefinition to be an error writes `--deny W0300`.

Anchors do not cross a `---`, so a sequence of states lives inside **one** document. Two
documents that both write `&state` name two unrelated things.
