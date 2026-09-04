// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The compiled image: what pass 6 produces and what the runtime queries.
//!
//! A **side table**, exactly as every pass before it is. It borrows the
//! project, the interned indexes and the resolved views rather than copying
//! them, and adds only what those cannot answer in the time a traversal needs:
//! a model per resolved node marked abstract or concrete; its ancestor chain,
//! because flattening destroys the `is_a` axis; a **CSR edge index, forward and
//! reverse**, so inbound traversal is O(degree) rather than O(V+E); its scope
//! path; and a name index keyed by the path syntax of D4.12.
//!
//! `!node` and `!edge` are concrete and are emitted; `!type` and untagged are
//! abstract and are not (D7.1, D4.13). [`Image::models`] is the compiled output and
//! yields concrete models alone, while [`Image::nodes`] yields everything the
//! image holds — an ancestor chain whose links were dropped is not a chain, so
//! an abstract base is retained as a *vertex of the `is_a` axis*, which is not
//! the same act as emitting it as a model.
//!
//! Pass 5 computed each member's gate and wrote the two predicates. This module
//! **applies** them and does not restate them:
//! [`ModelView::fields_readable_from`] filters as it walks, so an unreadable
//! member is absent from the result by *shape* rather than present as a hole, a
//! null or a count. Two implementations of one rule is how the axes come to
//! disagree about who blocked what.
//!
//! The edge index carries data edges as well as inheritance edges, and a data
//! cycle is an ordinary shape in it, so every traversal over [`Image::out`] or
//! [`Image::inc`] must carry a visited set (§0).

mod view;

use std::collections::HashMap;

use yfi_syntax::{FileId, NodeId, Span};

use crate::check::Checked;
use crate::discover::Project;
use crate::intern::Interned;
use crate::link::path;
use crate::link::{Ctx, Linked, SourceOrder};
use crate::scope::{ScopeId, ScopeTree};
use crate::symbol::Symbol;

pub use view::{FieldView, ModelView};

/// Handle to one node of the image, abstract or concrete.
///
/// Ids are assigned in the project's **source order** — `(file rank, document
/// index, source position)` — so iteration is stable across machines and a
/// lower id was written earlier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ModelId(pub u32);

impl ModelId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Whether a node is emitted as a model or retained only for its ancestry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelKind {
    /// `!type`, or untagged, or anything in a base YAML file (D7.1, D6.6).
    /// Inheritable, validated against, and never emitted.
    Abstract,
    /// `!node` in a Yamlfication source file. Emitted.
    Concrete,
    /// `!edge` in a Yamlfication source file. Emitted, and additionally the
    /// **incidence** of the relation it declares: its `connections` are a run
    /// of [`EdgeKind::Connection`] records leaving it (D4.13).
    ///
    /// A separate variant rather than a flag beside [`ModelKind::Concrete`],
    /// because one field answering "what is this node" cannot disagree with
    /// itself; and a variant of *this* enum rather than a second enum, because
    /// an edge is a node and the abstract/concrete question is asked of it in
    /// exactly the same words.
    Edge,
}

impl ModelKind {
    /// Whether this kind is emitted as a model. An edge is: a relation nothing
    /// holds is not a relation.
    #[must_use]
    pub fn is_concrete(self) -> bool {
        matches!(self, ModelKind::Concrete | ModelKind::Edge)
    }

    /// Whether this kind declares a relation of its own.
    #[must_use]
    pub fn is_edge(self) -> bool {
        self == ModelKind::Edge
    }
}

/// What relationship an edge record states.
///
/// The first three are the language's three operators, and they mirror
/// [`crate::link::EdgeKind`]. The last two have no counterpart there and cannot
/// have one: pass 4's graph is the *inheritance* graph, over which a cycle is
/// an error, and both a data edge and a connection are legally cyclic
/// (spec §0). They meet only here.
///
/// `!ref` is deliberately **not** a kind. It is a declaration of intent that is
/// legal wherever a path is (D4.12), so it qualifies these rather than standing
/// beside them — which is [`Edge::capability`]. Making it a kind would say
/// `<<: !ref P` is not an inclusion, and it is one.
///
/// # There is one notion of "edge" here, and this is it
///
/// [`Edge`] is an **incidence record**: a labelled ordered pair the CSR index
/// is built from. A `!edge` *node* is not a second one — it is a
/// [`ModelId`] like any other node, and what it contributes to this index is a
/// run of [`EdgeKind::Connection`] records, one per endpoint, all leaving the
/// edge node itself. That is the standard incidence encoding of a hypergraph,
/// and it is the encoding "an edge is a node" forces: an n-ary relation cannot
/// be a pair, so the relation becomes a vertex and each endpoint becomes a
/// pair. Traversal from one endpoint to another is therefore two hops through
/// the edge node, which is what makes the edge's own members — its middleware —
/// reachable on the way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    /// `<<` — containment. A has a B in it, and is not a B.
    Inclusion,
    /// `extends:` — instantiation. A is a type of B.
    Extension,
    /// `extends: !ref` — B carries A's definition. The one spelling whose
    /// blast radius is every B in the program.
    ExtendedReference,
    /// A value naming another node — `key: ../peer/Thing`, `key: *alias`.
    /// Carries no `is_a` claim and may lie on a cycle.
    Data,
    /// One endpoint of an `!edge` node's `connections`, leaving that edge node
    /// (D4.13). Not an [`EdgeKind::Data`] reference: a data edge says *this
    /// member names that node*, and a connection says *this edge relates that
    /// node*, so a query for what is connected to a node must not collect every
    /// node that happens to point at it.
    Connection,
}

impl EdgeKind {
    /// The spelling a diagnostic or a dump uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Inclusion => "<<",
            EdgeKind::Extension => "extends",
            EdgeKind::ExtendedReference => "extends !ref",
            EdgeKind::Data => "data",
            EdgeKind::Connection => "connection",
        }
    }

    /// Whether an edge of this kind puts its source on the `is_a` axis.
    /// Inclusion does not: a node that includes `water` is not a water (D4.1).
    #[must_use]
    pub fn is_ancestry(self) -> bool {
        matches!(self, EdgeKind::Extension | EdgeKind::ExtendedReference)
    }
}

/// One edge of the image's graph.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    /// The node that wrote the relationship.
    pub from: ModelId,
    /// The node it names.
    pub to: ModelId,
    /// Which relationship it is.
    pub kind: EdgeKind,
    /// Whether it was written `!ref`: the declaration that this context intends
    /// to **modify** the target, and therefore that the target depends on it.
    /// Every edge carrying it has a dependency running the other way, and those
    /// are the ones an audit reads.
    pub capability: bool,
    /// Whether it was written `override` (D4.14).
    ///
    /// On an [`EdgeKind::ExtendedReference`] this is a record of a compile-time
    /// decision the image has already applied: the contribution outranked the
    /// base, so the views here hold the result and nothing downstream re-derives
    /// it. On an [`EdgeKind::Inclusion`] it is the **runtime claim** — *this
    /// node reserves the right to modify the target's global state* — and it is
    /// the only thing that spelling leaves behind, because it moves no resolved
    /// value at compile time. The compiler records it, gates it like any write
    /// and emits it here; **executing it belongs to a runtime**, which is a
    /// separate artifact (D6.5).
    ///
    /// It rides the edge it qualifies rather than standing in a table of its
    /// own, for the reason [`Edge::capability`] does: it is one relationship
    /// with a second claim on it, and both indexes already reach it — the
    /// claimants of a node are `inc(node)` filtered by this flag.
    pub overrides: bool,
    /// The member that carries a data edge, when one does; for an
    /// [`EdgeKind::Connection`] it is the **handle** `definition` gives that
    /// endpoint, when it gives it one. `None` for an inheritance edge, whose
    /// site is the operator rather than a key.
    pub key: Option<Symbol>,
    /// Where it is written.
    pub span: Span,
}

/// What a path named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    /// The path named a node the image holds.
    Model(ModelId),
    /// The path landed on a node and a trailing `.member` addressed a field of
    /// it — the holder, and the member's name.
    Field(ModelId, Symbol),
}

/// One node of the image.
pub(crate) struct Model {
    /// Where it is written.
    pub(crate) place: (FileId, NodeId),
    /// Whether it is emitted.
    pub(crate) kind: ModelKind,
    /// The directory scope holding it.
    pub(crate) scope: ScopeId,
    /// Its span.
    pub(crate) span: Span,
    /// Its position in the project's user-visible order.
    pub(crate) order: SourceOrder,
}

/// A flat, offset-indexed table: one contiguous slice per model, in one
/// allocation. Used for the edge indexes, the ancestor chains and the scope
/// paths alike, because all three are "a run of items per model" and a
/// `Vec<Vec<_>>` would allocate once per model for each of them.
pub(crate) struct Flat<T> {
    offsets: Vec<u32>,
    items: Vec<T>,
}

impl<T> Flat<T> {
    /// An empty table over `count` models.
    pub(crate) fn empty(count: usize) -> Self {
        Flat { offsets: vec![0; count + 1], items: Vec::new() }
    }

    /// Build from one run per model, in model order.
    pub(crate) fn build(runs: Vec<Vec<T>>) -> Self {
        let mut offsets = Vec::with_capacity(runs.len() + 1);
        let mut items = Vec::new();
        offsets.push(0);
        for run in runs {
            items.extend(run);
            offsets.push(u32::try_from(items.len()).expect("image overflow"));
        }
        Flat { offsets, items }
    }

    /// The run belonging to `at`, or empty when `at` is out of range.
    fn slice(&self, at: usize) -> &[T] {
        let Some(end) = at.checked_add(1).and_then(|next| self.offsets.get(next)) else {
            return &[];
        };
        &self.items[self.offsets[at] as usize..*end as usize]
    }
}

/// Everything pass 6 emitted.
///
/// Borrows the four earlier passes rather than copying them: the views, the
/// spans and the scope tree all already exist, and one owner of each is what
/// keeps the answers consistent.
pub struct Image<'a> {
    pub(crate) project: &'a Project,
    pub(crate) interned: &'a Interned,
    pub(crate) linked: &'a Linked,
    pub(crate) checked: &'a Checked,
    pub(crate) models: Vec<Model>,
    pub(crate) index: HashMap<(FileId, NodeId), u32>,
    pub(crate) concrete: Vec<ModelId>,
    pub(crate) out: Flat<Edge>,
    pub(crate) inc: Flat<Edge>,
    pub(crate) ancestry: Flat<ModelId>,
    pub(crate) paths: Flat<ScopeId>,
    pub(crate) refused: bool,
}

impl<'a> Image<'a> {
    /// The project the image was built from.
    #[must_use]
    pub fn project(&self) -> &'a Project {
        self.project
    }

    /// The project's scope tree, which every access question composes over.
    #[must_use]
    pub fn scopes(&self) -> &'a ScopeTree {
        self.project.scopes()
    }

    /// The project's symbols, for turning a [`Symbol`] back into text.
    #[must_use]
    pub fn interned(&self) -> &'a Interned {
        self.interned
    }

    /// Whether emission was **refused** because pass 5 found an inheritance
    /// cycle.
    ///
    /// A refused image holds nothing. The views pass 5 composed over a graph
    /// made acyclic by dropping back edges are a recovery, not a meaning, and
    /// must never reach output — which is what [`Checked::is_cyclic`] is for.
    #[must_use]
    pub fn is_refused(&self) -> bool {
        self.refused
    }

    /// How many concrete models the image holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.concrete.len()
    }

    /// Whether the image emits no models at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.concrete.is_empty()
    }

    /// The compiled output: every **concrete** model, in source order.
    pub fn models(&self) -> impl Iterator<Item = ModelView<'_>> + '_ {
        self.concrete.iter().filter_map(|id| self.model(*id))
    }

    /// Every node the image holds, abstract ones included, in source order.
    /// An abstract node is a vertex of the `is_a` axis and is not output.
    pub fn nodes(&self) -> impl Iterator<Item = ModelView<'_>> + '_ {
        (0..self.models.len())
            .filter_map(|at| self.model(ModelId(u32::try_from(at).expect("image overflow"))))
    }

    /// Every `!edge` node the image holds, in source order (D4.13).
    ///
    /// An edge is a node, so each of these is also in [`Image::models`]; this
    /// is the projection onto the ones that declare a relation.
    pub fn edges(&self) -> impl Iterator<Item = ModelView<'_>> + '_ {
        self.nodes().filter(|held| held.is_edge())
    }

    /// One node of the image.
    #[must_use]
    pub fn model(&self, id: ModelId) -> Option<ModelView<'_>> {
        self.models.get(id.index()).map(|_| ModelView::new(self, id))
    }

    /// The node written at `(file, node)`, if the image holds one.
    #[must_use]
    pub fn at(&self, file: FileId, node: NodeId) -> Option<ModelView<'_>> {
        self.index.get(&(file, node)).map(|id| ModelView::new(self, ModelId(*id)))
    }

    /// The edges **leaving** a node, in written order. O(degree).
    #[must_use]
    pub fn out(&self, id: ModelId) -> &[Edge] {
        self.out.slice(id.index())
    }

    /// The edges **arriving** at a node, in written order. O(degree) — which
    /// is the reason the reverse index is materialised rather than derived by
    /// scanning, and the reason it is here rather than in the runtime.
    #[must_use]
    pub fn inc(&self, id: ModelId) -> &[Edge] {
        self.inc.slice(id.index())
    }

    /// A node's `is_a` axis, nearest first, each ancestor once however many
    /// paths reach it. Inclusions contribute nothing to it (D4.1).
    #[must_use]
    pub fn ancestors(&self, id: ModelId) -> &[ModelId] {
        self.ancestry.slice(id.index())
    }

    /// A node's stored `root → scope` path, so an access question needs no
    /// walk of the scope tree to find what it composes over.
    #[must_use]
    pub fn scope_path(&self, id: ModelId) -> &[ScopeId] {
        self.paths.slice(id.index())
    }

    /// Whether an observer in `observer` may see a node at all.
    ///
    /// This is the scope-level gate ([`ScopeTree::visible`]), composed over the
    /// whole `root → scope` path. It is the *node's* question; whether a
    /// particular member may be read is the member's, and is
    /// [`FieldView::is_readable_from`].
    #[must_use]
    pub fn is_visible_from(&self, id: ModelId, observer: ScopeId) -> bool {
        let Some(model) = self.models.get(id.index()) else { return false };
        self.scopes().visible(model.scope, observer)
    }

    /// Resolve a path written in `origin`, in the syntax of D4.12: `..`
    /// ascents, a peer directory or peer file segment, a bare name for this
    /// file, and `.member` suffixes.
    ///
    /// The grammar and the walk are pass 4's — the same ones an operand is
    /// resolved with — so a path cannot mean one thing to the compiler and
    /// another to a query. A trailing member yields [`Named::Field`]; the
    /// member is read from the node that *writes* it, which is the rule
    /// `E0218` states from the other side.
    ///
    /// Document-local `!ref` bindings are not consulted: a binding is a name in
    /// one document, resolved where it is written, and the image addresses the
    /// project's tree.
    #[must_use]
    pub fn resolve(&self, origin: FileId, text: &str) -> Option<Named> {
        let parsed = path::parse(text)?;
        let ctx = Ctx { project: self.project, interned: self.interned };
        let table = self.linked.table();
        let landed = path::resolve(&ctx, self.linked.space(), table, origin, &parsed).ok()?;
        let Some(last) = parsed.members.last() else {
            return self.index.get(&landed).map(|id| Named::Model(ModelId(*id)));
        };
        let holder = self.holder_of(&ctx, origin, &parsed)?;
        let name = self.interned.symbols().get(last)?;
        Some(Named::Field(holder, name))
    }

    /// The node a path's **last** member is addressed on: everything the path
    /// names, less that final step.
    fn holder_of(&self, ctx: &Ctx, origin: FileId, parsed: &path::Path) -> Option<ModelId> {
        let trimmed = path::Path {
            members: parsed.members[..parsed.members.len() - 1].to_vec(),
            segments: parsed.segments.clone(),
            ..*parsed
        };
        let base =
            path::resolve(ctx, self.linked.space(), self.linked.table(), origin, &trimmed).ok()?;
        self.index.get(&base).map(|id| ModelId(*id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_table_hands_back_one_run_per_model_and_nothing_out_of_range() {
        let flat = Flat::build(vec![vec![1u32, 2], Vec::new(), vec![3]]);
        assert_eq!(flat.slice(0), [1, 2]);
        assert_eq!(flat.slice(1), []);
        assert_eq!(flat.slice(2), [3]);
        assert_eq!(flat.slice(3), [], "past the end is empty, never a panic");
    }

    #[test]
    fn an_empty_table_is_addressable_for_every_model_it_covers() {
        let flat: Flat<u32> = Flat::empty(2);
        assert_eq!(flat.slice(0), []);
        assert_eq!(flat.slice(1), []);
    }
}
