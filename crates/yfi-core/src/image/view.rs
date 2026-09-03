// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two borrowed views the runtime reads an image through.
//!
//! Both are `Copy` handles of two words: a borrow of the [`Image`] and an
//! index into it. Nothing is copied out of the image to answer a question, so
//! there is one owner of every fact and no view can go stale against it.
//!
//! [`ModelView`] answers what a node *is* — its kind, its ancestry, its edges,
//! its scope path. [`FieldView`] answers what one member *is* and, crucially,
//! **who may read or write it**, by calling pass 5's predicates rather than
//! restating them. Filtering is done as the caller walks
//! ([`ModelView::fields_readable_from`]) so that an unreadable member is absent
//! from a result by shape rather than present as a hole.
//!
//! # Gated and ungated accessors
//!
//! Every walk that can disclose something comes in two forms, and the naming is
//! a convention rather than a coincidence:
//!
//! | ungated | gated |
//! |---|---|
//! | [`ModelView::view`] | — (there is no partial `View`; ask for fields) |
//! | [`ModelView::fields`] | [`ModelView::fields_readable_from`] |
//! | [`ModelView::connections`] | [`ModelView::connections_readable_from`] |
//!
//! **The `_readable_from` form is the default.** Anything answering on behalf
//! of an observer — a query, a serialisation, an API response — must take that
//! one, because the ungated form returns the compiler's whole answer and the
//! compiler is allowed to know things the observer is not. The ungated forms
//! exist for the two callers that legitimately have no observer: the compiler's
//! own tests, which assert what was *resolved*, and tooling that is the project
//! rather than a reader of it.
//!
//! They keep the shorter names because they are the primitives the gated ones
//! are built from — `fields_readable_from` is `fields` plus a filter — and a
//! reader following the definition should not have to step through a rename to
//! see that the predicate is pass 5's and is applied in exactly one place.

use yfi_syntax::{FileId, NodeId, Span};

use crate::check::{Acquisition, Field, FieldGate, View};
use crate::link::SourceOrder;
use crate::scope::ScopeId;
use crate::symbol::Symbol;

use crate::edge;

use super::{Edge, EdgeKind, Image, Model, ModelId, ModelKind};

/// One node of the image, with everything known about it.
#[derive(Clone, Copy)]
pub struct ModelView<'a> {
    image: &'a Image<'a>,
    id: ModelId,
}

impl<'a> ModelView<'a> {
    /// A handle on one node of `image`. Crate-internal: a view is obtained
    /// from the image, never built beside it.
    pub(super) fn new(image: &'a Image<'a>, id: ModelId) -> Self {
        ModelView { image, id }
    }

    /// This node's handle.
    #[must_use]
    pub fn id(self) -> ModelId {
        self.id
    }

    fn model(self) -> &'a Model {
        &self.image.models[self.id.index()]
    }

    /// Whether the node is emitted as a model.
    #[must_use]
    pub fn kind(self) -> ModelKind {
        self.model().kind
    }

    /// Whether the node is emitted — `!node` or `!edge` in Yamlfication source.
    #[must_use]
    pub fn is_concrete(self) -> bool {
        self.kind().is_concrete()
    }

    /// Whether the node is an `!edge`, and therefore declares a relation of its
    /// own on top of everything every other node has (D4.13).
    #[must_use]
    pub fn is_edge(self) -> bool {
        self.kind().is_edge()
    }

    /// Where the node is written.
    #[must_use]
    pub fn place(self) -> (FileId, NodeId) {
        self.model().place
    }

    /// The file that wrote it.
    #[must_use]
    pub fn file(self) -> FileId {
        self.model().place.0
    }

    /// The arena node.
    #[must_use]
    pub fn node(self) -> NodeId {
        self.model().place.1
    }

    /// The directory scope holding it.
    #[must_use]
    pub fn scope(self) -> ScopeId {
        self.model().scope
    }

    /// Its stored `root → scope` path.
    #[must_use]
    pub fn scope_path(self) -> &'a [ScopeId] {
        self.image.paths.slice(self.id.index())
    }

    /// Its span, which is what a diagnostic or a `show` points at.
    #[must_use]
    pub fn span(self) -> Span {
        self.model().span
    }

    /// Where it sits in the project's user-visible order.
    #[must_use]
    pub fn order(self) -> SourceOrder {
        self.model().order
    }

    /// The anchor name it was written with, if it carries one.
    #[must_use]
    pub fn name(self) -> Option<&'a str> {
        let ast = &self.image.project.file(self.file())?.ast;
        let anchor = ast.nodes().get(self.node().index())?.anchor?;
        Some(&ast.anchors().get(anchor)?.name)
    }

    /// Its canonical `namespace/name`, when its directory claims a namespace.
    /// Identity, not reach: a definition can be reachable without one (D6.1).
    #[must_use]
    pub fn canonical(self) -> Option<&'a str> {
        self.image.linked.path_of(self.file(), self.node())
    }

    /// Its resolved view — all five tiers of D4.7, **ungated**.
    ///
    /// The whole answer, including members an observer may not read. Answering
    /// on behalf of somebody? Walk [`ModelView::fields_readable_from`] instead:
    /// there is no such thing as a partially readable `View`, which is why this
    /// has no gated twin.
    #[must_use]
    pub fn view(self) -> Option<&'a View> {
        self.image.checked.resolved(self.file(), self.node())
    }

    /// Every member, highest precedence first, **unfiltered**.
    ///
    /// Discloses members an observer may not read. Use
    /// [`ModelView::fields_readable_from`] for anything answering on behalf of
    /// one; this is the primitive that one is built from, and is for callers
    /// with no observer at all.
    pub fn fields(self) -> impl Iterator<Item = FieldView<'a>> {
        let image = self.image;
        self.view()
            .into_iter()
            .flat_map(move |view| view.fields().iter().map(move |field| FieldView { image, field }))
    }

    /// The members an observer in `observer` may **read**, in precedence order.
    ///
    /// Filtered as it walks: an unreadable member is absent by shape, never a
    /// hole or a count, or the scoping leaks through the result. The predicate
    /// is pass 5's ([`Field::is_readable_from`]) and is not restated.
    pub fn fields_readable_from(self, observer: ScopeId) -> impl Iterator<Item = FieldView<'a>> {
        let scopes = self.image.scopes();
        self.fields().filter(move |held| held.field.is_readable_from(scopes, observer))
    }

    /// One member by name.
    #[must_use]
    pub fn field(self, name: Symbol) -> Option<FieldView<'a>> {
        self.view()?.get(name).map(|field| FieldView { image: self.image, field })
    }

    /// The `is_a` axis, nearest first.
    #[must_use]
    pub fn ancestors(self) -> &'a [ModelId] {
        self.image.ancestry.slice(self.id.index())
    }

    /// The edges leaving this node. O(degree).
    #[must_use]
    pub fn out(self) -> &'a [Edge] {
        self.image.out.slice(self.id.index())
    }

    /// The edges arriving at this node. O(degree).
    #[must_use]
    pub fn inc(self) -> &'a [Edge] {
        self.image.inc.slice(self.id.index())
    }

    /// Whether an observer in `observer` may see this node at all.
    #[must_use]
    pub fn is_visible_from(self, observer: ScopeId) -> bool {
        self.image.is_visible_from(self.id, observer)
    }

    // ---------------------------------------------------------------- edges

    /// The incidence records of this edge's endpoints, in written order
    /// (D4.13). Empty for a node that is not an `!edge`.
    ///
    /// The position of a record in this run **is** the endpoint's index, which
    /// is what a `definition` handle names, and it never renumbers.
    pub fn connection_edges(self) -> impl Iterator<Item = &'a Edge> {
        self.out().iter().filter(|held| held.kind == EdgeKind::Connection)
    }

    /// The nodes this edge relates, in written order, **unfiltered**. Empty for
    /// a node that is not an `!edge`.
    ///
    /// Discloses endpoints an observer may not see. Use
    /// [`ModelView::connections_readable_from`] for anything answering on
    /// behalf of one.
    ///
    /// An endpoint may itself be an edge: an edge is a node, so an edge over
    /// edges is a legal and intended shape, and it is what composing relations
    /// out of relations looks like. The connection graph may therefore cycle —
    /// only *inheritance* cycles are illegal — so a traversal built on this
    /// carries a visited set (spec §0).
    pub fn connections(self) -> impl Iterator<Item = ModelView<'a>> {
        let image = self.image;
        self.connection_edges().filter_map(move |held| image.model(held.to))
    }

    /// The endpoint a `definition` handle names.
    ///
    /// Answered from the edge's handle list rather than by matching the edge
    /// index's `key`, because handles are **many-to-one**: a self-loop names one
    /// position twice (`from: 0`, `to: 0`) and an index record carries only the
    /// first of those names. Reading the list answers both.
    #[must_use]
    pub fn connection(self, handle: Symbol) -> Option<ModelView<'a>> {
        let held = self.image.checked.edges().get(self.place())?.connection(handle)?;
        self.image.at(held.target.0, held.target.1)
    }

    /// The endpoints an observer in `observer` may see, in written order.
    ///
    /// Two gates, both already written and neither restated: the `connections`
    /// **member** must be readable from there ([`FieldView::is_readable_from`],
    /// pass 5's predicate), and the endpoint must be a node that observer can
    /// see at all ([`ModelView::is_visible_from`], the scope-level gate). There
    /// is no third predicate.
    ///
    /// Filtered as it walks, so an endpoint the observer may not see is absent
    /// by **shape**. Its position is not reused: [`ModelView::connection`]
    /// still answers by handle over the *written* positions, and a filtered
    /// result never renumbers the endpoints beside the one it dropped.
    ///
    /// # Why both, when one implies the other in a clean project
    ///
    /// In a project that raised no `E0216`, **the member gate is the one that
    /// bites and the node gate can never fire behind it.** `E0216` forbids an
    /// edge from naming a target its own scope cannot see, so every closed
    /// scope on an endpoint's path encloses the edge's scope; and the member
    /// gate opens either because the observer sits inside the edge's scope —
    /// in which case it sits inside every closed scope above it too — or
    /// because the edge's scope is reachable from the root, in which case no
    /// closed scope but the root is on the endpoint's path either. Both
    /// branches make the endpoint visible.
    ///
    /// The node gate is kept for the case where that premise is *false*: a
    /// project that raised `E0216` still emits (only a cycle refuses emission),
    /// and a broken project must not become the way to read what a scope
    /// declined to publish. `projects/edge-invisible-connection` is that case.
    pub fn connections_readable_from(
        self,
        observer: ScopeId,
    ) -> impl Iterator<Item = ModelView<'a>> {
        let readable =
            self.connections_field().is_some_and(|field| field.is_readable_from(observer));
        self.connections().filter(move |held| readable && held.is_visible_from(observer))
    }

    /// The `!edge` nodes that name this node as an endpoint. O(degree).
    ///
    /// This is the reverse of [`ModelView::connections`] and the reason the
    /// incidence encoding earns its keep: "what relates this node" is one
    /// contiguous slice, whatever the arity of the relations involved.
    pub fn incident_edges(self) -> impl Iterator<Item = ModelView<'a>> {
        let image = self.image;
        self.inc()
            .iter()
            .filter(|held| held.kind == EdgeKind::Connection)
            .filter_map(move |held| image.model(held.from))
    }

    /// The `!edge` nodes an observer in `observer` may be told relate this
    /// node, in the reverse index's order.
    ///
    /// The counterpart of [`ModelView::connections_readable_from`], and it must
    /// exist: "what relates this public node" asked without a gate hands the
    /// asker a **private** edge, and hands it to them by name. It is the same
    /// two predicates read from the other end — the edge must be a node this
    /// observer can see, and the edge's `connections` must be readable from
    /// there, because an edge whose endpoints are undisclosed has not disclosed
    /// that this node is one of them.
    ///
    /// Filtered as it walks, so an edge the observer may not be told about is
    /// absent by shape.
    pub fn incident_edges_visible_from(
        self,
        observer: ScopeId,
    ) -> impl Iterator<Item = ModelView<'a>> {
        self.incident_edges().filter(move |held| {
            held.is_visible_from(observer)
                && held.connections_field().is_some_and(|field| field.is_readable_from(observer))
        })
    }

    /// This node's `connections` member, as a member — which is what carries
    /// the member-level half of the access question.
    fn connections_field(self) -> Option<FieldView<'a>> {
        self.field(self.image.interned.symbols().get(edge::CONNECTIONS)?)
    }
}

impl std::fmt::Debug for ModelView<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("ModelView")
            .field("id", &self.id)
            .field("kind", &self.kind())
            .field("name", &self.name())
            .finish()
    }
}

/// One member of one node, with its gate and how it arrived.
#[derive(Clone, Copy)]
pub struct FieldView<'a> {
    image: &'a Image<'a>,
    field: &'a Field,
}

impl<'a> FieldView<'a> {
    /// The member's interned name, its flag prefix already taken off.
    #[must_use]
    pub fn symbol(self) -> Symbol {
        self.field.name
    }

    /// The member's name as written, prefix removed.
    #[must_use]
    pub fn name(self) -> &'a str {
        self.image.interned.symbols().resolve(self.field.name).unwrap_or_default()
    }

    /// The key node, at the site that wrote it.
    #[must_use]
    pub fn key(self) -> (FileId, NodeId) {
        self.field.key
    }

    /// The value node, at the site that wrote it.
    #[must_use]
    pub fn value(self) -> (FileId, NodeId) {
        self.field.value
    }

    /// The mapping that wrote the entry.
    #[must_use]
    pub fn origin(self) -> (FileId, NodeId) {
        self.field.origin
    }

    /// How the member arrived in the node holding it.
    #[must_use]
    pub fn acquired(self) -> Acquisition {
        self.field.acquired
    }

    /// What gates it, on both axes.
    #[must_use]
    pub fn gate(self) -> FieldGate {
        self.field.reach
    }

    /// Pass 5's field record, whole.
    #[must_use]
    pub fn field(self) -> &'a Field {
        self.field
    }

    /// Whether an observer in `observer` may read it (pass 5's predicate).
    #[must_use]
    pub fn is_readable_from(self, observer: ScopeId) -> bool {
        self.field.is_readable_from(self.image.scopes(), observer)
    }

    /// Whether an observer in `observer` may change it (pass 5's predicate).
    #[must_use]
    pub fn is_writable_from(self, observer: ScopeId) -> bool {
        self.field.is_writable_from(self.image.scopes(), observer)
    }

    /// The member's value as written, when it is a scalar. A code block's
    /// contents are verbatim, indentation and leading newline included.
    #[must_use]
    pub fn text(self) -> Option<&'a str> {
        let ast = &self.image.project.file(self.field.value.0)?.ast;
        ast.scalar(self.field.value.1).map(|scalar| &*scalar.value)
    }

    /// The node this member's value names, when it names one — a path, a `!ref`
    /// or an alias. This is the data edge, read from the member's side.
    #[must_use]
    pub fn target(self) -> Option<ModelView<'a>> {
        let holder = self.image.at(self.field.origin.0, self.field.origin.1)?;
        let edge = holder
            .out()
            .iter()
            .find(|edge| edge.kind == EdgeKind::Data && edge.key == Some(self.field.name))?;
        self.image.model(edge.to)
    }
}

impl std::fmt::Debug for FieldView<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("FieldView")
            .field("name", &self.name())
            .field("acquired", &self.field.acquired)
            .field("gate", &self.field.reach)
            .finish()
    }
}
