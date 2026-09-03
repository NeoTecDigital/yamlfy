<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# The three operators

```sh
yamlfy check examples/01-three-operators
```

Yamlfication has exactly three ways to build one node out of another, and they differ
in **what they change besides the node you are writing**.

| written | name | what else moves |
|---|---|---|
| `<<: B` | inclusion | nothing |
| `extends: B` | extension | nothing |
| `extends: !ref B` | extended reference | **every B in the program** |

Read the right-hand column first. Two of the three are safe; one changes the world.

## What the file does

An apprentice writes three entries in the Guild's grimoire.

**`MoonTonic` includes `water`.** The tonic *has* water in it. Water has no opinion
about tonics, and no other potion moved. Inclusion is compositional: a node that
includes water is not a water, and no `is_a` query will ever say otherwise.

**`HealingDraught` extends `BasePotion`.** A healing draught *is a kind of* potion —
so it inherits the vessel and the cork, and the ancestry is retained as a queryable
relationship. The Guild's definition of a potion is exactly what it was that morning.

**`Sealed` extends `!ref BasePotion`.** One token different. This does not define a new
potion that has a seal; it adds a seal to **`BasePotion` itself**. Every potion in the
building now has one — including `SleepingTonic`, which nobody edited.

That is why the two spellings had to look different.

A note on the mutability axis, because it is easy to over-claim: `extends: !ref` is a
compile-time **write**, so the target must be writable from where the reference is
written. Here it always is — `Sealed` and `BasePotion` share one scope, and a scope is
open to an observer sitting inside it. Removing `mutability: mutable` from this file
changes nothing. The gate bites across a **directory boundary**, which is what
[`02-scopes`](../02-scopes) shows.

## The warning is deliberate

The project compiles with one warning:

```
W0301  `reagent` is declared by no ancestor of this node
```

`HealingDraught` adds a key its family never declared. That is legal — the world is
open — but it is also exactly what a typo looks like, and a misspelled field name would
otherwise add a junk key while silently keeping the inherited value. A project that
wants a closed world writes `--deny W0301`.

## Also shown

`BasePotion` uses all three declaration states: `vessel: !!str vial` is optional with a
default, `label: !!str untitled` likewise, and `notes:` is declared but unconstrained.
A tag with **no** value would make the field required of every descendant.

`water` is untagged, so it is abstract — inheritable, never emitted as a model. `!type`
is abstract and says so; `!node` is concrete and is what the compiler emits.
