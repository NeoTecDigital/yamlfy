<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Yamlfication

A graph database whose source is a language. You `yamlfy` the `yfi`.

| name | is |
|---|---|
| **Yamlfication** | the ecosystem, and the runtime engine |
| **yfi** | the syntax |
| **`.yfy`** | the file format |
| **`yamlfy`** | the runtime execution — what you invoke to run the engine |

## Two kinds of file

**`.yfy` is yfi source** — the language the engine compiles. Its core is a YAML
superset, so anchors, aliases and merge keys mean what they mean in YAML, and it adds
what a graph needs: named definitions, inheritance, paths between files, and two access
axes. It also holds three constructs a YAML parser would reject — a `//` line comment,
a `<?-- … --!>` documentation block and a `<?-- … -->` code block — so it has a front
end of its own.

**`.yaml` and `.yml` are base YAML** — the data the engine operates over. None of the
language is interpreted there: `extends:` is an ordinary field, `!node` is a tag nobody
reads, `//` is text. That split is a declaration, not a heuristic; the extension is how
a file says which of the two it is, so the engine never has to guess whether a field
name was meant as an operation.

## Three operators

```yfy
# stock/formulary.yfy — the Guild's
--- !yfi/header
namespace: guild::stock
visibility: public
---
// A base every potion shares. `!type` is abstract: inheritable, never emitted.
BasePotion: !type &BasePotion
  pub vessel: vial
  pub label: !!str untitled   <?-- a tag with a value: optional, with a default --!>

water: &water
  pub solvent: spring-water
```

```yfy
# bench/apprentice.yfy — someone else's
--- !yfi/header
namespace: guild::bench
visibility: public
---
MoonTonic: !node                // `<<` — a tonic *has* water in it
  <<: ../stock/water
  pub label: Moon Tonic

HealingDraught: !node           // `extends` — a draught *is a* potion
  extends: ../stock/BasePotion
  pub label: Healing Draught

Patch: !node                    // `extends: !ref` — *every* potion gains a reagent
  extends: !ref ../stock/BasePotion
  pub reagent: sunroot
```

* `<<: P` — **inclusion.** A has a P in it. P is unchanged; nothing else moves.
* `extends: P` — **extension.** A is a type of P. P is unchanged; the blast radius is A.
* `extends: !ref P` — **extended reference.** P depends on A, and *every* P in the
  program now carries A's definition. This is Swift's `extension`, not subclassing.

Two of the three are safe and one changes the world, which is why they do not look
alike. `P` is a path, spelled the way a filesystem is spelled — `../shared/Service`,
`peer/Service`, `Service`, `Service.tls.port` — and naming is reaching: there is
nothing to import first.

The third one above is an **error**, and that is the point of writing it in two
directories. `guild::stock` never said `mutability: mutable`, so it is `immutable`
(the default), and reopening its family from another directory is `E0217` at the `!ref`
— before the contribution is computed and before anyone has to notice the tag. **Inside
one file there is no gate**: a file may always rewrite what it wrote, so the same three
entries in one document compile, and the only signal is `W0303` on a contribution the
base already defines. The specification's D4.6 works the whole example through.

## Two access axes, closed by default

`private`/`public` and `immutable`/`mutable`, on a **scope** in a file's header and on
a **member** as a prefix on its name:

```yfy
Service:
  - private_member              // a bare member is private and immutable
  - pub public_member
  - pub mut open_member
```

Both axes are opt-in at both levels: a scope that says nothing grants nothing, and
neither does a member. They compose over the whole path from the project root, so a
`public` member inside a `private` directory is public *within* it and invisible
outside — the same rule Rust's `pub` follows, applied to two axes instead of one.

`!ref` is what declares an intent to modify, and it is checked: a reference into a
scope that never said `mutable` is an error at the character you typed.

## Build and run

```sh
cargo build --workspace
cargo test --workspace
cargo run -- check path/to/project
```

`yamlfy check` takes a file or a directory and runs the whole front half of the
compiler — `discover`, `parse`, `intern`, `link`, `check` — so what the compiler finds is
what you are told, not only what the file readers found. Of those, `intern` raises no
diagnostic of its own; it is in the list because the passes after it cannot run without
it, not because it has anything to report. A directory is one
project: its tree is the namespace and scope hierarchy, and checking one file is a
project of one file.

Configuration is `yamlfy.toml` and `YAMLFY_*`; every diagnostic code can be set to
`allow`, `warning` or `error`, so a project that wants a closed world writes
`--deny W0301` and gets it.

## Examples

[`examples/`](examples) holds four complete projects, each with a `README.md` and each
compiled by an integration test so it cannot rot:

| | |
|---|---|
| [`01-three-operators`](examples/01-three-operators) | inclusion, extension and extended reference side by side, and what each one changes besides the node you are writing |
| [`02-scopes`](examples/02-scopes) | the directory tree as the scope hierarchy; both axes closed by default; an import that is not a visibility grant |
| [`03-edges`](examples/03-edges) | an edge as a node whose content is its connections — n-ary, addressable, extendable |
| [`04-cycles-and-states`](examples/04-cycles-and-states) | cyclic data as ordinary subject matter, and an anchor redefined as a state transition |

```sh
yamlfy check examples/01-three-operators
```

## The specification

[`docs/semantics.md`](docs/semantics.md) is normative. It is not a tutorial: it records
every decision, what was rejected and why, and which fixture pins it. Start at §11,
which states the language as a whole, and follow the decision numbers from there.

Licensed GPL-3.0-or-later.
