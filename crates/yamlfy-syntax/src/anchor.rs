// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! Anchor definitions and positional resolution.
//!
//! An anchor name may be defined any number of times in a document. An alias
//! binds to the definition with the greatest source position *strictly before*
//! the alias. That is the whole rule, and it is why the table is keyed by
//! definition identity rather than by name.

use std::collections::HashMap;

use crate::ast::NodeId;
use crate::span::Span;

/// Handle to an anchor *definition* — not to a name. Two definitions of the
/// same name are two different `AnchorId`s.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AnchorId(pub u32);

impl AnchorId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One `&name` occurrence.
#[derive(Clone, Debug)]
pub struct AnchorDef {
    /// This definition's handle.
    pub id: AnchorId,
    /// The name written after `&`.
    pub name: Box<str>,
    /// The node the anchor names.
    pub node: NodeId,
    /// Span of the `&name` token itself.
    pub span: Span,
    /// Zero-based index of the document the definition occurs in.
    pub document: u32,
    /// The definition of the same name this one shadows, if any.
    pub shadows: Option<AnchorId>,
}

/// Every anchor definition in a file, in source order.
#[derive(Default)]
pub struct AnchorTable {
    defs: Vec<AnchorDef>,
    /// Raw parser anchor id, namespaced by parser segment, to our handle.
    by_raw: HashMap<(u32, usize), AnchorId>,
    /// Most recent definition of each name within the current document.
    latest: HashMap<Box<str>, AnchorId>,
}

impl AnchorTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every definition, in source order.
    #[must_use]
    pub fn defs(&self) -> &[AnchorDef] {
        &self.defs
    }

    /// Look up a definition by handle.
    #[must_use]
    pub fn get(&self, id: AnchorId) -> Option<&AnchorDef> {
        self.defs.get(id.index())
    }

    /// The definition currently in scope for `name`, which is the most recent
    /// preceding one in the current document.
    #[must_use]
    pub fn current(&self, name: &str) -> Option<&AnchorDef> {
        self.latest.get(name).and_then(|id| self.get(*id))
    }

    /// Forget every name binding. Called at each document boundary, because
    /// YAML anchors do not survive one.
    pub fn end_document(&mut self) {
        self.latest.clear();
    }

    /// Record a `&name` occurrence and return its handle. `raw` is the
    /// `(segment, parser anchor id)` pair the alias events will refer to.
    pub fn define(
        &mut self,
        raw: (u32, usize),
        name: &str,
        node: NodeId,
        span: Span,
        document: u32,
    ) -> AnchorId {
        let id = AnchorId(u32::try_from(self.defs.len()).expect("anchor table overflow"));
        let shadows = self.latest.get(name).copied();
        self.defs.push(AnchorDef {
            id,
            name: name.into(),
            node,
            span,
            document,
            shadows,
        });
        self.latest.insert(name.into(), id);
        self.by_raw.insert(raw, id);
        id
    }

    /// Attach the arena node to a definition. Collections are anchored at their
    /// start event but only become nodes at their end event, so the definition
    /// is recorded first and pointed at its node afterwards. Recording it first
    /// is what keeps definition order equal to source order.
    pub(crate) fn set_node(&mut self, id: AnchorId, node: NodeId) {
        if let Some(def) = self.defs.get_mut(id.index()) {
            def.node = node;
        }
    }

    /// Resolve the raw anchor id carried by an alias event.
    #[must_use]
    pub fn by_raw(&self, raw: (u32, usize)) -> Option<AnchorId> {
        self.by_raw.get(&raw).copied()
    }
}
