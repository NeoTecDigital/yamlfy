<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Edges

```sh
yamlfy check examples/03-edges
```

**An edge is a node whose content is what it connects.** There is no second construct
and no second set of rules: an edge has identity, is addressable, extends, and is
validated exactly like any other node.

```yfy
--- !edge &Owns
extends: Relation
pub connections: [Platform, Api]
pub definition:
  owner: 0
  owned: 1
pub since: 2026-01-01
```

## `connections` is a sequence, so edges are n-ary

A relation is not required to be binary. `SharesRotation` connects three services with
one edge, because "these three share an on-call rotation" is one fact and encoding it as
three pairwise edges would be a lie about its shape.

## `definition` names positions

Handles let an endpoint be addressed by name rather than by index — `owner` rather than
`0`. The mapping is **many-to-one**, which is not a degenerate case: it is how a
self-loop is written.

```yfy
--- !edge &DependsOnItself
pub connections: [Billing]
pub definition:
  from: 0
  to: 0
```

One endpoint, two handles naming it. Recording a handle on the position it names would
let only the last survive and silently lose `from`.

## An abstract edge is a `!type` that declares `connections`

There is no separate tag for one, because `!edge` is concrete exactly as `!node` is. So
a family fixes what every relation has, and concrete edges extend it:

```yfy
--- !type &Relation
pub connections: []
pub since: !!str unknown
```

`Owns` and `SharesRotation` both extend `Relation`, so both are declared to carry a
`since` — and both write their own, which is why the inherited default is not what you
see in the resolved view. An edge that
supplies no `connections` of its own would inherit its family's endpoints, by nothing
more than what extension already means.

## Why this is a node and not a labelled link

Because an edge can then be typed, extended, validated and reached like anything else —
and because a relation with its own members is a place to put behaviour. An edge sitting
between two nodes with its own definition is middleware, without middleware having to be
built as a feature.
