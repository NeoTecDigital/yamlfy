<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# How a `yamlfy` run works

One command, traced end to end. `docs/semantics.md` says what the language *means*;
this says what the code *does*, in order.

## Crates

`yfi-syntax` is the sink — it depends on no other workspace crate and everything
depends on it. There are no back-edges between passes.

```mermaid
graph TD
    S["yfi-syntax<br/><i>arena · spans · diagnostics</i>"]
    CORE["yfi-core<br/><i>the six passes</i>"]
    CFG["yfi-config<br/><i>toml · YAMLFY_*</i>"]
    CLI["yfi-cli<br/><b>bin: yamlfy</b>"]
    UMB["yamlfication<br/><i>umbrella — nothing imports it</i>"]
    CORE --> S
    CFG --> S
    CLI --> CORE
    CLI --> CFG
    CLI --> S
    UMB -.-> CORE
    UMB -.-> CFG
    UMB -.-> S
```

## The run

Four ways out, marked ⏹. Three places something runs **twice** — each is a real
decision, explained below.

```mermaid
graph TD
    A["main()"] --> B["Cli::parse()"]
    B --> C{"build_config"}
    C -->|error| X2["⏹ exit 2"]
    C -->|ok| D["logging::init"]
    D --> E{"--root a directory?"}
    E -->|no| X2
    E -->|yes| F["group_by_root<br/><i>paths bucketed by project</i>"]

    F --> G{{"for each root"}}
    G --> H{"root is a directory?"}
    H -->|no| L1["Run::loose<br/><i>read the path alone</i>"]
    H -->|yes| P1

    subgraph pipeline["one project"]
        P1["<b>1. discover_in</b><br/>walk · scopes · headers"]
        P1 --> P2["<b>2. parse</b> ✱<br/><i>per file</i>"]
        P2 --> P1B["<b>1b. bind</b> ✱✱<br/><i>imports, to a fixed point</i>"]
        P1B --> P3["<b>3. intern</b><br/>symbols · parents · scope paths"]
        P3 --> P4["<b>4. link</b> ✱<br/>paths · clauses · graph"]
        P4 --> P5["<b>5. check</b> ✱<br/>cycles · inheritance · validation"]
    end

    P5 --> COL["Run::collect<br/><i>merge 3 diagnostic sets</i>"]
    L1 --> COL
    COL --> G
    G -->|done| FIN["Run::finish<br/><i>sort by position, render once</i>"]
    FIN --> Z{"any errors?"}
    Z -->|no| X0["⏹ exit 0"]
    Z -->|yes| X1["⏹ exit 1"]

    P5 -.->|"never called<br/>from the binary"| P6["<b>6. emit</b><br/>the Image"]
```

## The three double-runs

**✱ `parse`, once or twice per file.** `discover` parses every file to read its
header — which is the only way to learn what it imports. A file that imports
something is then parsed *again* with those definitions installed, and only the
second arena is kept. Files importing nothing are parsed once. The survey parse's
diagnostics are **replaced**, not merged: it reports unknown anchors that the real
parse resolves.

**✱✱ `bind`, to a fixed point.** Mutually importing files have no imports-first
order, so their members are re-parsed until their exported name sets stop moving,
bounded by `members + 1` rounds. An acyclic chain converges in one.

**✱ `link`, resolving references twice.** Whether a `connections:` item is a *path*
or a *value* depends on whether some `!edge` reads that sequence — which is known
only from the inheritance relation, which is built from clauses, whose operands are
themselves paths. The circle is broken by resolving once silently with no endpoints
known (correct for every clause operand, since no operand is ever an endpoint),
deriving the endpoint set, then resolving again for real.

**✱ `check`, two diagnostic sets.** Findings that read a *resolved view* go into a
second collection, folded in only when the inheritance graph was acyclic. After a
cycle the resolved view is the compiler's own repair, so a diagnostic derived from it
would blame an edge nobody wrote. The views are still built — a caller may inspect a
cyclic project.

## Pass interfaces

| pass | entry | consumes | produces |
|---|---|---|---|
| 1 `discover` | `discover_in(sources, root, opts)` | a `SourceMap`, by value | `Project` |
| 2 `parse` | `parse_with_imports(sources, file, opts, imports)` | one file | `Parsed { ast, diagnostics }` |
| 3 `intern` | `intern(&project)` | `&Project` | `Interned` |
| 4 `link` | `link_with(&project, &interned, sev)` | the two above | `Linked` |
| 5 `check` | `check_with(&project, &interned, &linked, sev)` | the three above | `Checked` |
| 6 `emit` | `emit(&project, &interned, &linked, &checked)` | all four, borrowed | `Image<'a>` |

Data flows strictly forward. No pass mutates an earlier pass's output; each is a side
table keyed by `(FileId, NodeId)`. The one exception is `Ast::rebind_import`, the
only `&mut` on a finished arena, used solely to point an imported anchor at the node
it names once every file has been parsed.

## Where the shared kernel lives

`link::Ctx` is the most-called thing in the compiler — `Ctx::ast()` from 33 sites in
15 files — and it is used by passes 4, 5 and 6. It lives in `link.rs` for historical
reasons rather than good ones: pass 4 owns it, and the two passes after it borrow it.

## Pass 6 is not on the product path

`emit` is called from tests and from nothing else. The `check` subcommand stops after
pass 5, because every diagnostic is raised by then and the `Image` is what a *runtime*
would consume — and the runtime is deliberately a separate artifact. The image is a
library surface, complete and tested; it is simply not what `yamlfy check` is for.
