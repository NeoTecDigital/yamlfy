// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 5 — checking.
//!
//! Pass 4 answers *what points at what, and is that legal to write*. Pass 5
//! answers *what does it mean*, and then *is what it means allowed*:
//!
//! * **`E0212`** — Tarjan's algorithm over the stratified inheritance graph,
//!   reported once per strongly connected component, primary span textually
//!   first and notes naming the **forward** edges that closed it;
//! * **resolution** — every node's view, composed in D4.7's precedence order
//!   with each clause consumed where it is written (D4.9);
//! * **`E0217`** — what a `!ref` is allowed to change. Its sibling `E0216` —
//!   what a path is allowed to *reach* — is pass 4's, raised inside path
//!   resolution so that an invisible target resolves to nothing; see
//!   [`reach`] for why the two axes live in different passes;
//! * **member gates** — each member's two axes, declared by its `pub`/`mut`
//!   prefix and composed with its scope's (D4.12, D6.5);
//! * **`E0220`, `E0221`, `W0301`** — every concrete node validated against its
//!   abstract ancestors' *declared* views, never against its own flattened one;
//! * **`E0223`, `E0224`, `E0225`** — what each `!edge` node relates, read off
//!   its resolved view because `connections` may be inherited (D4.13);
//! * **`W0303`** over a resolved base, which pass 4 could only test against the
//!   base's own keys.
//!
//! `W0302` (inconsistent inheritance order) is deliberately deferred and is not
//! implemented here.
//!
//! # Recovery is not a semantic
//!
//! When `E0212` fires, the pass makes the graph acyclic by depth-first search in
//! the project's textual order and drops each back edge, so every node still has
//! a defined view, the walk terminates, and the pass never has to bail. **That
//! recovered value is not a language semantic and is never emitted: compilation
//! fails whenever `E0212` was raised**, which [`Checked::is_cyclic`] is how a
//! caller asks.
//!
//! Nothing read off it is emitted either. A finding derived from the recovered
//! view is a claim about a program that does not exist, and `W0303` was the
//! proof: with `Base extends Patch` and `Patch extends: !ref Base`, recovery
//! drops one edge, the surviving one carries the contributed key into the base,
//! and the warning then reported the contribution as inert *because the base
//! already inherits it* — with a note pointing at the very line contributing
//! it. Nothing an author could do would satisfy that, because the inheritance
//! it names is the compiler's own repair.
//!
//! So `E0212`, and the two gates that read no view (`E0217` and the pass-4
//! visibility gate ahead of it), are reported unconditionally; `W0303`,
//! `E0220`, `E0221`, `W0301` and the three `!edge` codes are collected into a
//! second [`Diagnostics`] and folded in only when the graph was acyclic. The
//! views and the [`Edges`] are still **built** either way — a caller that wants
//! to inspect a broken project can, and [`Checked::is_cyclic`] tells it what it
//! is looking at — only nothing is *reported* from them.
//!
//! # Cyclic data stays legal
//!
//! Only cycles through **inheritance** edges are rejected. A cycle in the data
//! graph is legal and is the point of the system;
//! `fixtures/cycles/alias-cycle-with-merge-dag.yml` is the fixtured case and is
//! clean through this pass.
//!
//! # Example
//!
//! ```no_run
//! use yfi_core::{check, discover, intern, link, DiscoverOptions};
//!
//! let project = discover("projects/check-diamond", &DiscoverOptions::default());
//! let interned = intern::intern(&project);
//! let linked = link::link(&project, &interned);
//! let checked = check::check(&project, &interned, &linked);
//! assert!(!checked.is_cyclic());
//! ```

mod ancestry;
mod cycles;
mod declare;
pub mod edges;
mod inert;
mod names;
mod reach;
mod resolve;
mod scc;
mod validate;
pub mod view;

use std::collections::HashSet;

use tracing::debug;
use yfi_syntax::{Diagnostics, FileId, NodeId, SeverityMap};

use crate::discover::Project;
use crate::intern::Interned;
use crate::link::{Ctx, EdgeId, Linked};

pub use ancestry::is_concrete;
pub use edges::{Connection, EdgeNode, Edges};
pub use view::{Acquisition, Field, FieldGate, View};

/// Everything pass 5 resolved, and everything it found.
pub struct Checked {
    diagnostics: Diagnostics,
    views: resolve::Views,
    edges: Edges,
    dropped: HashSet<EdgeId>,
    cyclic: bool,
}

impl Checked {
    /// Everything checking found. Diagnostics accumulate; the pass never bails.
    #[must_use]
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Take the diagnostics so the caller can fold them into the project's.
    pub fn take_diagnostics(&mut self) -> Diagnostics {
        std::mem::take(&mut self.diagnostics)
    }

    /// Whether the inheritance graph held a cycle.
    ///
    /// Every view below was still composed, over a graph made acyclic by
    /// dropping back edges — but that is a recovery, not a meaning. A caller
    /// that is about to emit must refuse when this is true.
    #[must_use]
    pub fn is_cyclic(&self) -> bool {
        self.cyclic
    }

    /// A node's resolved view: all five tiers of D4.7.
    #[must_use]
    pub fn resolved(&self, file: FileId, node: NodeId) -> Option<&View> {
        self.views.resolved((file, node))
    }

    /// What a node **declares**: its own keys plus everything an extended
    /// reference installed on it, and nothing it merely includes.
    #[must_use]
    pub fn declared(&self, file: FileId, node: NodeId) -> Option<&View> {
        self.views.declared((file, node))
    }

    /// Every `!edge` node the project writes, with the endpoints each one
    /// relates and the handles naming them (D4.13).
    ///
    /// An edge is a node, so it also has a view, an ancestry and a scope like
    /// any other; this is only the part no other node has.
    #[must_use]
    pub fn edges(&self) -> &Edges {
        &self.edges
    }

    /// A node's literal keys, its inheritance clauses removed (D4.9).
    #[must_use]
    pub fn own(&self, file: FileId, node: NodeId) -> Option<&View> {
        self.views.own((file, node))
    }

    /// Every node on `(file, node)`'s `is_a` axis, nearest first, each
    /// ancestor once however many paths reach it.
    ///
    /// Extensions and extended references create ancestry; **inclusions do
    /// not** (D4.1). The walk follows no edge the cycle recovery dropped, which
    /// is why it is answered here rather than re-derived from the graph: the
    /// dropped set is pass 5's and a second walk without it would report an
    /// ancestry the checker never validated against.
    #[must_use]
    pub fn ancestors(&self, linked: &Linked, file: FileId, node: NodeId) -> Vec<(FileId, NodeId)> {
        ancestry::ancestors(linked.graph(), &self.dropped, (file, node))
    }

    /// How many nodes were resolved.
    #[must_use]
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether the project resolved nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.views.len() == 0
    }
}

/// Check `linked`, using the default severity for every code.
#[must_use]
pub fn check(project: &Project, interned: &Interned, linked: &Linked) -> Checked {
    check_with(project, interned, linked, SeverityMap::new())
}

/// Check `linked` with per-code severity overrides — `--deny W0301` and the
/// like, which §4 promises a project that wants the closed-world reading.
#[must_use]
pub fn check_with(
    project: &Project,
    interned: &Interned,
    linked: &Linked,
    severities: SeverityMap,
) -> Checked {
    let ctx = Ctx { project, interned };
    let mut diagnostics = Diagnostics::with_severities(severities.clone());
    let walk = cycles::walk_order(&ctx, linked.graph());
    let components = scc::components(linked.graph(), &walk);
    cycles::report(&ctx, linked, &components, &mut diagnostics);
    let cyclic = cycles::any_cyclic(&components);
    let dropped = scc::back_edges(linked.graph(), &walk);
    let order = resolve::every_holder(&ctx);
    let views = resolve::resolve(&ctx, linked, &dropped, &order);
    reach::reach(&ctx, linked, &mut diagnostics);
    // Everything below reads a **resolved** view, so everything below is
    // collected apart and kept only when the graph was acyclic. See "Recovery
    // is not a semantic" above.
    let mut derived = Diagnostics::with_severities(severities);
    inert::inert(&ctx, linked, &views, &mut derived);
    validate::validate(&ctx, linked, &views, &dropped, &order, &mut derived);
    let edges = edges::collect(&ctx, linked, &views, &order, &mut derived);
    if !cyclic {
        diagnostics.extend(derived);
    }
    debug!(
        components = components.len(),
        dropped = dropped.len(),
        resolved = views.len(),
        edges = edges.len(),
        cyclic,
        "checked project"
    );
    Checked { diagnostics, views, edges, dropped, cyclic }
}

/// Graph fixtures for the unit tests of [`scc`], built without a project so the
/// algorithm can be exercised on shapes rather than on files.
#[cfg(test)]
pub(crate) mod testing {
    use crate::link::graph::build;
    use crate::link::{Clause, ClauseKind, Graph, Operand, OperandForm};
    use yfi_syntax::{FileId, NodeId, Pos, Span};

    fn span() -> Span {
        Span::empty(FileId(0), Pos { byte: 0, line: 1, col: 1 })
    }

    /// A graph over `(from, to, extended_reference)` triples in file 0.
    pub(crate) fn graph(edges: &[(u32, u32, bool)]) -> Graph {
        let clauses: Vec<Clause> = edges
            .iter()
            .map(|(from, to, extended)| Clause {
                file: FileId(0),
                owner: NodeId(*from),
                kind: if *extended { ClauseKind::Extension } else { ClauseKind::Inclusion },
                site: span(),
                operands: vec![Operand {
                    node: NodeId(*to),
                    form: if *extended { OperandForm::Ref } else { OperandForm::Alias },
                    target: (FileId(0), NodeId(*to)),
                    span: span(),
                }],
            })
            .collect();
        build(&clauses, &[])
    }
}
