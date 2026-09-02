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
--- !yfi/header
namespace: guild::stock
visibility: public
---
// A base every potion shares. `!type` is abstract: inheritable, never emitted.
BasePotion: !type &BasePotion
  pub vessel: vial
  pub label: !!str            <?-- required: a tag with no value --!>

water: &water
  pub solvent: spring-water

MoonTonic: !node                // `<<` — a tonic *has* water in it
  <<: *water
  pub label: Moon Tonic

HealingDraught: !node           // `extends` — a draught *is a* potion
  extends: *BasePotion
  pub label: Healing Draught

Patch: !node                    // `extends: !ref` — *every* potion gains a reagent
  extends: !ref BasePotion
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

`yamlfy check` takes a file or a directory. A directory is one project: its tree is the
namespace and scope hierarchy. Configuration is `yamlfy.toml` and `YAMLFY_*`; every
diagnostic code can be set to `allow`, `warning` or `error`, so a project that wants a
closed world writes `--deny W0301` and gets it.

## The specification

[`docs/semantics.md`](docs/semantics.md) is normative. It is not a tutorial: it records
every decision, what was rejected and why, and which fixture pins it. Start at §11,
which states the language as a whole, and follow the decision numbers from there.

Licensed GPL-3.0-or-later.
