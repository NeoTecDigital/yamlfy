// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 4 — linking: the pass that makes an interned but still separate set of
//! files one graph (§9, pass 4).
//!
//! The definition table, every path resolved with its operator role and its
//! `!ref` flag (D4.3), every clause validated for operand shape (D1.6), every
//! `!ref` binding that shadows a definition of its own file (`E0219`, D4.12),
//! the stratified inheritance graph pass 5 runs SCC over (D4.10), and every
//! extended-reference contribution (D4.5, D4.11).
//!
//! **Nothing is resolved here.** No view is flattened, no cycle is detected, no
//! ancestry is walked. Addressability (§4), the canonical `namespace/name`, and
//! why `own` vertices are sinks are all decided elsewhere; what this pass adds
//! is that a base YAML file's `<<` edges *are* in the graph even though its
//! anchors are unaddressable, because merge is YAML's and is governed in both
//! classes (D6.6).
//!
//! # Example
//!
//! ```no_run
//! use yfi_core::{discover, intern, link, DiscoverOptions};
//!
//! let project = discover("projects/link-graph-shapes", &DiscoverOptions::default());
//! let interned = intern::intern(&project);
//! let linked = link::link(&project, &interned);
//! assert!(!linked.graph().vertices().is_empty());
//! ```

mod clause;
mod contrib;
mod failed;
pub(crate) mod graph;
mod kernel;
pub(crate) mod keys;
pub(crate) mod path;
mod refs;
mod shadow;
mod table;
mod value;

use std::collections::HashSet;

use tracing::debug;
use yfi_syntax::{Diagnostics, FileId, NodeId, SeverityMap};

use crate::discover::Project;
use crate::intern::Interned;

pub use clause::{Clause, ClauseKind, Operand, OperandForm};
pub use contrib::{ContributedKey, Contribution};
pub use graph::{Direction, Edge, EdgeId, EdgeKind, Graph, Stratum, Vertex, VertexId};
pub(crate) use kernel::Ctx;
pub use kernel::{source_order, SourceOrder};
pub use path::Path;
pub use refs::{RefRole, Reference};
pub use table::Definition;

/// Everything pass 4 built, and everything it found.
pub struct Linked {
    diagnostics: Diagnostics,
    table: table::Table,
    references: Vec<Reference>,
    clauses: Vec<Clause>,
    graph: Graph,
    contributions: Vec<Contribution>,
    space: path::Space,
}

impl Linked {
    /// Everything linking found. Diagnostics accumulate; the pass never bails.
    #[must_use]
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Every addressable node, in discovery order.
    #[must_use]
    pub fn definitions(&self) -> &[Definition] {
        self.table.definitions()
    }

    /// The definition a canonical path names.
    #[must_use]
    pub fn definition(&self, path: &str) -> Option<&Definition> {
        self.table.get(path)
    }

    /// The canonical path of an addressable node.
    #[must_use]
    pub fn path_of(&self, file: FileId, node: NodeId) -> Option<&str> {
        self.table.path_of(file, node)
    }

    /// The shape of the project a path walks. Built once per link, because it
    /// is a pure function of the project and every later pass that resolves a
    /// path asks it the same two questions.
    pub(crate) fn space(&self) -> &path::Space {
        &self.space
    }

    /// The definition table, for the one other pass that walks a path.
    ///
    /// Crate-internal on purpose: pass 6 resolves a query path with the *same*
    /// walk pass 4 resolves an operand with, and handing it the table is what
    /// keeps "what is addressable" written once.
    pub(crate) fn table(&self) -> &table::Table {
        &self.table
    }

    /// Every path reference written in a Yamlfication source file, resolved or
    /// not, each carrying the role its operator gave it and whether it declared
    /// mutation intent with `!ref`.
    #[must_use]
    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    /// Every inheritance clause, with the operands that survived validation.
    #[must_use]
    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    /// The stratified inheritance graph.
    #[must_use]
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Every extended-reference contribution, in document order.
    ///
    /// A contribution carries `own(A)` — never `R(A)` — and is **additive
    /// unless it was written `override`**: it ranks below everything the base
    /// already has (D4.5), or, with the keyword, above it (D4.14). Pass 5 obeys
    /// that ranking; pass 4 records what it needs to, including which keys are
    /// inert and which contributions override.
    #[must_use]
    pub fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }
}

/// Link `project`, using the default severity for every code.
#[must_use]
pub fn link(project: &Project, interned: &Interned) -> Linked {
    link_with(project, interned, SeverityMap::new())
}

/// Link `project` with per-code severity overrides — `--deny W0303` and the
/// like, which D4.11 promises a project may ask for.
#[must_use]
pub fn link_with(project: &Project, interned: &Interned, severities: SeverityMap) -> Linked {
    let ctx = Ctx { project, interned };
    let mut diagnostics = Diagnostics::with_severities(severities);
    keys::check_member_names(&ctx, &mut diagnostics);
    let table = table::build(&ctx, &mut diagnostics);
    let space = path::Space::build(&ctx);
    let endpoints = endpoint_sequences(&ctx, &table, &space);
    let references = refs::resolve(&ctx, &table, &space, &endpoints, &mut diagnostics);
    let clauses = clause::collect(&ctx, &references, &mut diagnostics);
    let contributions = contrib::collect(&ctx, &table, &clauses, &mut diagnostics);
    let reference_count = references.len();
    let references = references.into_items();
    shadow::check(&ctx, &table, &references, &mut diagnostics);
    let graph = graph::build(&clauses, &references);
    debug!(
        definitions = table.definitions().len(),
        references = reference_count,
        clauses = clauses.len(),
        vertices = graph.vertices().len(),
        edges = graph.edges().len(),
        contributions = contributions.len(),
        "linked project"
    );
    Linked { diagnostics, table, references, clauses, graph, contributions, space }
}

/// The sequences whose items some `!edge` ends up reading as endpoints (D4.13).
///
/// Not answerable from the holder's tag — an edge inherits the member from
/// bases carrying no tag that says so, and making every `!type`'s `connections`
/// a reach would reserve the name across the language — so it is read off the
/// **inheritance relation** by [`crate::edge::endpoint_holders`], followed one
/// step to the sequence the member names, dereferencing an alias standing as
/// that value. The items may therefore sit in a file holding no edge at all.
///
/// The relation is built from clauses whose operands may themselves be paths,
/// which is why [`refs::probe`] runs first; [`refs`] documents why the two runs
/// agree.
fn endpoint_sequences(
    ctx: &Ctx,
    table: &table::Table,
    space: &path::Space,
) -> HashSet<(FileId, NodeId)> {
    let probe = refs::probe(ctx, table, space);
    let clauses = clause::collect(ctx, &probe, &mut Diagnostics::new());
    crate::edge::endpoint_sequences(ctx, &clauses)
}
