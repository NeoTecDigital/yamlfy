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
//! * **`E0216`/`E0217`** — what a path is allowed to reach, and what a `!ref`
//!   is allowed to change;
//! * **member gates** — each member's two axes, declared by its `pub`/`mut`
//!   prefix and composed with its scope's (D4.12, D6.5);
//! * **`E0220`, `E0221`, `W0301`** — every concrete node validated against its
//!   abstract ancestors' *declared* views, never against its own flattened one;
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
//! a defined view and the later checks report their own findings instead of
//! cascading into a wall of unrelated errors. **That recovered value is not a
//! language semantic and is never emitted: compilation fails whenever `E0212`
//! was raised**, which [`Checked::is_cyclic`] is how a caller asks.
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
//! use yamlfy_core::{check, discover, intern, link, DiscoverOptions};
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
mod inert;
mod names;
mod reach;
mod resolve;
mod scc;
mod validate;
pub mod view;

use tracing::debug;
use yamlfy_syntax::{Diagnostics, FileId, NodeId, SeverityMap};

use crate::discover::Project;
use crate::intern::Interned;
use crate::link::{Ctx, Linked};

pub use view::{Acquisition, Field, FieldGate, View};

/// Everything pass 5 resolved, and everything it found.
pub struct Checked {
    diagnostics: Diagnostics,
    views: resolve::Views,
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

    /// A node's literal keys, its inheritance clauses removed (D4.9).
    #[must_use]
    pub fn own(&self, file: FileId, node: NodeId) -> Option<&View> {
        self.views.own((file, node))
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
    let mut diagnostics = Diagnostics::with_severities(severities);
    let walk = cycles::walk_order(&ctx, linked.graph());
    let components = scc::components(linked.graph(), &walk);
    cycles::report(&ctx, linked, &components, &mut diagnostics);
    let dropped = scc::back_edges(linked.graph(), &walk);
    let order = resolve::every_holder(&ctx);
    let views = resolve::resolve(&ctx, linked, &dropped, &order);
    reach::reach(&ctx, linked, &mut diagnostics);
    inert::inert(&ctx, linked, &views, &mut diagnostics);
    validate::validate(&ctx, linked, &views, &dropped, &order, &mut diagnostics);
    debug!(
        components = components.len(),
        dropped = dropped.len(),
        resolved = views.len(),
        "checked project"
    );
    Checked { diagnostics, views, cyclic: cycles::any_cyclic(&components) }
}

/// Graph fixtures for the unit tests of [`scc`], built without a project so the
/// algorithm can be exercised on shapes rather than on files.
#[cfg(test)]
pub(crate) mod testing {
    use crate::link::graph::build;
    use crate::link::{Clause, ClauseKind, Graph, Operand, OperandForm};
    use yamlfy_syntax::{FileId, NodeId, Pos, Span};

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
