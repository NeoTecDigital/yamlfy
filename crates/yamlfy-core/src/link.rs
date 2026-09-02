// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 4 — linking.
//!
//! Pass 3 leaves every file interned and indexed but still alone. Linking is
//! the pass that makes the project one graph:
//!
//! * the **definition table** — every addressable node, indexed by the file and
//!   by the directory that holds it (which is what a path walks, D4.12) and by
//!   its canonical `namespace/name` (which is what `E0230` compares);
//! * every **path resolved**, recorded with the *role* the operator it is an
//!   operand of gives it and with whether it carried `!ref` (D4.3);
//! * every **inheritance clause** validated for operand shape (D1.6, `E0211`);
//! * the **stratified inheritance graph** pass 5 runs SCC over;
//! * every **extended-reference contribution**, with `E0214` and `W0303`.
//!
//! Nothing is *resolved* here. No view is flattened, no cycle is detected, no
//! ancestry is walked. Pass 4 answers "what points at what, and is that legal
//! to write"; pass 5 answers "what does it mean".
//!
//! # Addressability
//!
//! An anchored node that **can be a parent scope — a collection — is
//! addressable**: it is a member of its file and referenceable as a type. An
//! anchored *scalar* is a value, not a type, and carries no canonical path, so
//! two files may both write `&limit 30` without colliding.
//!
//! The canonical path is `namespace/name` and is therefore namespace-qualified,
//! which is what makes an ordinary local `&defaults` mixin safe: two files in
//! two namespaces defining one name are two paths, not a collision. **Reach
//! does not go through it** — a path addresses a file or a directory (D4.12),
//! so a file whose directory claims no namespace is still reachable while
//! carrying no canonical path at all.
//!
//! A base YAML file declares nothing (D6.6), so its anchors are unaddressable
//! and its `extends:` is an ordinary field. Its `<<` edges *are* in the graph,
//! because merge is YAML's and is governed in both classes.
//!
//! # Why the graph is stratified
//!
//! Every node gets two vertices, `own(N)` and `R(N)`. Inclusion and extension
//! contribute `R(A) → R(B)`. An extended reference contributes `R(A) → R(B)`
//! *and* `R(B) → own(A)`, because B depends on A. `own(A)` is a **sink**, so a
//! reverse edge can never lie on a cycle and SCC over this graph is exact.
//!
//! Built the obvious way — one vertex per node — every extended reference is a
//! two-cycle and pass 5 hallucinates a cycle on every use of the feature, which
//! is worse than missing cycles because it looks like a working checker.
//!
//! # Example
//!
//! ```no_run
//! use yamlfy_core::{discover, intern, link, DiscoverOptions};
//!
//! let project = discover("projects/link-graph-shapes", &DiscoverOptions::default());
//! let interned = intern::intern(&project);
//! let linked = link::link(&project, &interned);
//! assert!(!linked.graph().vertices().is_empty());
//! ```

mod clause;
mod contrib;
pub(crate) mod graph;
pub(crate) mod keys;
pub(crate) mod path;
mod refs;
mod table;
mod value;

use tracing::debug;
use yamlfy_syntax::{Ast, Diagnostics, FileId, NodeId, SeverityMap};

use crate::discover::{FileClass, Project};
use crate::intern::Interned;

pub use clause::{Clause, ClauseKind, Operand, OperandForm};
pub use contrib::{ContributedKey, Contribution};
pub use graph::{Direction, Edge, EdgeId, EdgeKind, Graph, Stratum, Vertex, VertexId};
pub use path::Path;
pub use refs::{RefRole, Reference};
pub use table::Definition;

/// What every pass-4 step reads. Two borrows, carried together so no step has
/// to be handed a different subset of the project than its neighbour.
pub(crate) struct Ctx<'a> {
    pub(crate) project: &'a Project,
    pub(crate) interned: &'a Interned,
}

impl<'a> Ctx<'a> {
    /// One file's arena.
    pub(crate) fn ast(&self, file: FileId) -> Option<&'a Ast> {
        self.project.file(file).map(|f| &f.ast)
    }

    /// Whether `file` is read as Yamlfication source. In base YAML the
    /// operators are not interpreted (D6.6), so almost every rule here asks.
    pub(crate) fn is_source(&self, file: FileId) -> bool {
        self.interned.class_of(file) == Some(FileClass::Source)
    }

    /// The namespace the file's directory scope claims, if it claims one.
    pub(crate) fn namespace_of(&self, file: FileId) -> Option<&'a str> {
        let scope = self.project.file(file)?.scope;
        self.project.scopes().get(scope)?.namespace.as_deref()
    }
}

/// A position in the project ordered by where it is **written**.
///
/// [`NodeOrder`] is the project's total order and its node component is the
/// arena index, which is *post-order*: the lowest-indexed member of a set is
/// its deepest-leftmost leaf, not the one written first. Every user-visible
/// choice — which member of a cycle `E0212` points at, which of two
/// contributions is "the first" — must be the textually first one, so it is
/// ordered by this instead. Provided here rather than left to pass 5, which
/// would otherwise have to guess.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SourceOrder {
    /// Rank of the file in relative-path order.
    pub file: u32,
    /// Zero-based document index within that file.
    pub document: u32,
    /// Byte offset of the node's first character.
    pub byte: u32,
}

/// Where `node` is written, as `(file rank, document index, source position)`.
#[must_use]
pub fn source_order(
    project: &Project,
    interned: &Interned,
    file: FileId,
    node: NodeId,
) -> Option<SourceOrder> {
    let rank = project.rank(file)?;
    let ast = &project.file(file)?.ast;
    let span = ast.nodes().get(node.index())?.span;
    Some(SourceOrder {
        file: rank,
        document: interned.document_of(file, node).unwrap_or(u32::MAX),
        byte: span.start.byte,
    })
}

/// Everything pass 4 built, and everything it found.
pub struct Linked {
    diagnostics: Diagnostics,
    table: table::Table,
    references: Vec<Reference>,
    clauses: Vec<Clause>,
    graph: Graph,
    contributions: Vec<Contribution>,
}

impl Linked {
    /// Everything linking found. Diagnostics accumulate; the pass never bails.
    #[must_use]
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Take the diagnostics so the caller can fold them into the project's.
    pub fn take_diagnostics(&mut self) -> Diagnostics {
        std::mem::take(&mut self.diagnostics)
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
    /// only**: it ranks below everything the base already has (D4.5). Pass 5
    /// obeys that rule; pass 4 records what it needs to, including which keys
    /// are inert.
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
    let references = refs::resolve(&ctx, &table, &space, &mut diagnostics);
    let clauses = clause::collect(&ctx, &references, &mut diagnostics);
    let contributions = contrib::collect(&ctx, &table, &clauses, &mut diagnostics);
    let reference_count = references.len();
    let references = references.into_items();
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
    Linked { diagnostics, table, references, clauses, graph, contributions }
}
