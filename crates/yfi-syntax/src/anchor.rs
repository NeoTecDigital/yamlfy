// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Anchor definitions and positional resolution.
//!
//! An anchor name may be defined any number of times in a document. An alias
//! binds to the definition with the greatest source position *strictly before*
//! the alias. That is the whole rule, and it is why the table is keyed by
//! definition identity rather than by name.

use std::collections::HashMap;

use crate::ast::NodeId;
use crate::span::{FileId, Span};

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

/// How a definition entered a document's anchor table.
///
/// A header import installs the exporting file's definitions into the importing
/// document before its first event (D6.7), where they are ordinary anchors as
/// far as §2 is concerned. They differ in exactly one way, and it is the reason
/// this enum exists rather than a `bool`: the node an imported definition names
/// lives in **another file's arena**, so it cannot be reached from this
/// [`Ast`](crate::ast::Ast) and is not known until that file's parse is final.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// Written in this file.
    Local,
    /// Installed by a header import, and not yet pointed at the node it names.
    Imported,
    /// Installed by a header import and bound to the node it names in the file
    /// that wrote it.
    Bound(NodeId),
}

/// One `&name` occurrence.
#[derive(Clone, Debug)]
pub struct AnchorDef {
    /// This definition's handle.
    pub id: AnchorId,
    /// The name written after `&`.
    pub name: Box<str>,
    /// The node the anchor names **in this file's arena**. Meaningless for an
    /// imported definition, whose node is carried by [`AnchorDef::source`].
    pub node: NodeId,
    /// Span of the `&name` token itself, always in the file that wrote it — so
    /// `span.file` is the defining file for an imported definition too.
    pub span: Span,
    /// Zero-based index of the document the definition occurs in. Always `0`
    /// for an import, which is re-installed at the start of every document.
    pub document: u32,
    /// The state of the same name this one supersedes, if any (D5.1). This
    /// is the chain `W0300` walks, and it is what keeps every earlier state
    /// addressable rather than lost.
    pub shadows: Option<AnchorId>,
    /// Where the definition came from.
    pub source: Source,
}

impl AnchorDef {
    /// Whether a header import put this definition here rather than this file.
    #[must_use]
    pub fn is_imported(&self) -> bool {
        !matches!(self.source, Source::Local)
    }

    /// The file and node this definition names, once that is known.
    ///
    /// `None` for an imported definition that has not been rebound yet, which
    /// is the only honest answer: the node it names is in another arena and the
    /// importing parse has no way to reach it.
    #[must_use]
    pub fn target(&self) -> Option<(FileId, NodeId)> {
        match self.source {
            Source::Local => Some((self.span.file, self.node)),
            Source::Imported => None,
            Source::Bound(node) => Some((self.span.file, node)),
        }
    }
}

/// Every anchor definition in a file, in source order.
#[derive(Default)]
pub struct AnchorTable {
    defs: Vec<AnchorDef>,
    /// Raw parser anchor id, namespaced by parser segment, to our handle.
    by_raw: HashMap<(u32, usize), AnchorId>,
    /// Most recent definition of each name within the current document.
    latest: HashMap<Box<str>, AnchorId>,
    /// The bindings every document of this file starts with — the header's
    /// imports, in authored order (D6.7). Re-installed rather than cleared at a
    /// document boundary, which is precisely why D2.6 survives: nothing leaks
    /// *between* documents, the same imports are simply installed into each.
    imported: HashMap<Box<str>, AnchorId>,
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

    /// Forget every locally written name binding, and re-install the header's
    /// imports. Called at each document boundary, because YAML anchors do not
    /// survive one and an import is re-installed into every document.
    pub fn end_document(&mut self) {
        self.latest.clone_from(&self.imported);
    }

    /// Drop every binding ahead of installing a fresh set of imports. Called
    /// once per parser segment, so a parser restarted after a syntax error
    /// re-installs the same imports rather than shadowing the first set.
    pub(crate) fn begin_prelude(&mut self) {
        self.latest.clear();
        self.imported.clear();
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
        self.push(raw, name, node, span, document, Source::Local)
    }

    /// Install a definition a header import brought into this document.
    ///
    /// `span` is the `&name` token in the file that *wrote* it, so every
    /// diagnostic about the definition points at the exporting file. The node
    /// it names is not known yet; [`crate::ast::Ast::rebind_import`] supplies
    /// it once that file's parse is final.
    pub(crate) fn import(&mut self, raw: (u32, usize), name: &str, span: Span) -> AnchorId {
        let id = self.push(raw, name, NodeId(0), span, 0, Source::Imported);
        self.imported.insert(name.into(), id);
        id
    }

    fn push(
        &mut self,
        raw: (u32, usize),
        name: &str,
        node: NodeId,
        span: Span,
        document: u32,
        source: Source,
    ) -> AnchorId {
        let id = AnchorId(u32::try_from(self.defs.len()).expect("anchor table overflow"));
        let shadows = self.latest.get(name).copied();
        self.defs.push(AnchorDef { id, name: name.into(), node, span, document, shadows, source });
        self.latest.insert(name.into(), id);
        self.by_raw.insert(raw, id);
        id
    }

    /// Point an imported definition at the node it names. Returns whether it
    /// was an imported definition to begin with.
    pub(crate) fn rebind(&mut self, id: AnchorId, node: NodeId) -> bool {
        let Some(def) = self.defs.get_mut(id.index()).filter(|d| d.is_imported()) else {
            return false;
        };
        def.source = Source::Bound(node);
        true
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

    /// The definition a bare name denotes **at this point of this document**:
    /// the most recent state of `name` since the document began (D2.1, D5.1),
    /// or the binding an import installed at its start (D6.7).
    ///
    /// This is the table's own answer, and it is the one an alias is resolved
    /// against. The parser's answer, [`AnchorTable::by_raw`], is a different
    /// question: its anchor ids are namespaced by the *stream*, so a name
    /// redefined in a later document keeps answering with the earlier
    /// document's node — which is exactly how `E0130` recognises an alias that
    /// crossed a document boundary, and exactly why it cannot be what an alias
    /// binds through.
    ///
    /// An unnamed definition is unaddressable rather than addressable as the
    /// empty name: `E0120` already reports one, and letting it answer here
    /// would bind an alias whose own name could not be read to whichever node
    /// most recently failed the same recovery.
    #[must_use]
    pub fn in_document(&self, name: &str) -> Option<AnchorId> {
        if name.is_empty() {
            return None;
        }
        self.latest.get(name).copied()
    }
}
