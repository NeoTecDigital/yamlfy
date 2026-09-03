// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 5 — checking. Pass 4 answers *what points at what, and is that legal to
//! write*; pass 5 answers *what does it mean*, and *is what it means allowed*.
//!
//! `E0212` over the stratified inheritance graph (D1.8, D4.10), resolution in
//! D4.7's precedence order, `E0217` (D4.12), the member gates (D4.12, D6.5),
//! `E0220`/`E0221`/`W0301` against declared ancestor views, the three `!edge`
//! codes over a resolved `connections` (D4.13), and the half of `W0303` that
//! needs a resolved base (D4.11). `E0216` is pass 4's — see [`reach`] for why
//! the two axes live in different passes. `W0302` is deferred, not implemented.
//!
//! # The two `Diagnostics`
//!
//! D1.8 withholds every finding read off a recovered view, so the pass collects
//! them into a second [`Diagnostics`] and folds it in only when the graph was
//! acyclic; findings that read no view go into the first unconditionally. The
//! views and the [`Edges`] are still **built** either way — a caller that wants
//! to inspect a broken project can, and [`Checked::is_cyclic`] tells it what it
//! is looking at — only nothing is *reported* from them.
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
    // Everything below reads a **resolved** view, so everything below goes into
    // the second `Diagnostics`.
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
