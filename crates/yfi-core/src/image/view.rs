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

use yfi_syntax::{FileId, NodeId, Span};

use crate::check::{Acquisition, Field, FieldGate, View};
use crate::link::SourceOrder;
use crate::scope::ScopeId;
use crate::symbol::Symbol;

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

    /// Whether the node is `!node` in Yamlfication source.
    #[must_use]
    pub fn is_concrete(self) -> bool {
        self.kind().is_concrete()
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

    /// Its resolved view — all five tiers of D4.7.
    #[must_use]
    pub fn view(self) -> Option<&'a View> {
        self.image.checked.resolved(self.file(), self.node())
    }

    /// Every member, highest precedence first, unfiltered.
    pub fn fields(self) -> impl Iterator<Item = FieldView<'a>> {
        let image = self.image;
        self.view().into_iter().flat_map(move |view| {
            view.fields().iter().map(move |field| FieldView { image, field })
        })
    }

    /// The members an observer in `observer` may **read**, in precedence order.
    ///
    /// Filtered as it walks: an unreadable member is absent by shape, never a
    /// hole or a count, or the scoping leaks through the result. The predicate
    /// is pass 5's ([`Field::is_readable_from`]) and is not restated.
    pub fn fields_readable_from(
        self,
        observer: ScopeId,
    ) -> impl Iterator<Item = FieldView<'a>> {
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

