// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The arena AST.
//!
//! Nodes live in one flat `Vec` and refer to each other by `u32` index. An
//! alias is stored as a *reference record* — an anchor handle plus the span of
//! the `*name` token — and is never expanded by copying. That is what lets the
//! node graph be cyclic while every Rust value in it stays a plain owned struct
//! with no `Rc`, no `RefCell` and no recursive `Drop`.

use crate::anchor::{AnchorId, AnchorTable};
use crate::span::{FileId, Span};

/// Handle to a node in an [`Ast`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// How a scalar was written. Style is retained because it is semantically
/// load-bearing: a plain `<<` is a merge key, a quoted `"<<"` is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarStyle {
    /// Unquoted.
    Plain,
    /// `'...'`.
    SingleQuoted,
    /// `"..."`.
    DoubleQuoted,
    /// `|` block.
    Literal,
    /// `>` block.
    Folded,
}

impl ScalarStyle {
    pub(crate) fn from_parser(style: saphyr_parser::ScalarStyle) -> Self {
        match style {
            saphyr_parser::ScalarStyle::Plain => ScalarStyle::Plain,
            saphyr_parser::ScalarStyle::SingleQuoted => ScalarStyle::SingleQuoted,
            saphyr_parser::ScalarStyle::DoubleQuoted => ScalarStyle::DoubleQuoted,
            saphyr_parser::ScalarStyle::Literal => ScalarStyle::Literal,
            saphyr_parser::ScalarStyle::Folded => ScalarStyle::Folded,
        }
    }
}

/// A resolved YAML tag.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tag {
    /// Handle, for example `!` or `tag:yaml.org,2002:`.
    pub handle: Box<str>,
    /// Suffix, for example `node` or `str`.
    pub suffix: Box<str>,
}

impl Tag {
    /// Whether this is the YAML core-schema tag `suffix`.
    #[must_use]
    pub fn is_core(&self, suffix: &str) -> bool {
        &*self.handle == "tag:yaml.org,2002:" && &*self.suffix == suffix
    }
}

/// A scalar's payload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scalar {
    /// The scalar's resolved text content.
    pub value: Box<str>,
    /// How it was written.
    pub style: ScalarStyle,
}

/// An alias occurrence: a reference, never a copy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AliasRef {
    /// The anchor definition this alias binds to, chosen positionally.
    pub anchor: AnchorId,
    /// The anchor name as written after `*`.
    pub name: Box<str>,
    /// Set when the bound definition lives in an earlier document, which YAML
    /// forbids. The binding is still recorded so later passes can report on it.
    pub cross_document: bool,
}

/// One key/value pair of a mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// The key node.
    pub key: NodeId,
    /// The value node.
    pub value: NodeId,
    /// Whether the key is a YAML merge key (`<<`), decided syntactically.
    pub merge: bool,
}

/// What a node is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// Index into the scalar table.
    Scalar(u32),
    /// Index into the sequence table.
    Sequence(u32),
    /// Index into the mapping table.
    Mapping(u32),
    /// Index into the alias table.
    Alias(u32),
}

/// A node in the arena.
#[derive(Clone, Debug)]
pub struct Node {
    /// The node's payload discriminant and side-table index.
    pub kind: NodeKind,
    /// Where the node's content begins and ends. Anchor and tag properties are
    /// *not* included; they have their own spans.
    pub span: Span,
    /// The anchor defined on this node, if any.
    pub anchor: Option<AnchorId>,
    /// Index into the tag table, if the node carries an explicit tag.
    pub tag: Option<u32>,
}

/// One document of a YAML stream.
#[derive(Clone, Debug)]
pub struct Document {
    /// The document's root node.
    pub root: NodeId,
    /// Span from the document start to its end.
    pub span: Span,
    /// Whether the document was introduced by an explicit `---`.
    pub explicit: bool,
}

/// Half-open `u32` range into a side table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Range32 {
    pub start: u32,
    pub end: u32,
}

/// The parsed form of one file.
pub struct Ast {
    pub(crate) file: FileId,
    pub(crate) nodes: Vec<Node>,
    pub(crate) scalars: Vec<Scalar>,
    pub(crate) tags: Vec<Tag>,
    pub(crate) aliases: Vec<AliasRef>,
    pub(crate) seqs: Vec<Range32>,
    pub(crate) seq_items: Vec<NodeId>,
    pub(crate) maps: Vec<Range32>,
    pub(crate) entries: Vec<Entry>,
    pub(crate) documents: Vec<Document>,
    pub(crate) anchors: AnchorTable,
}

impl Ast {
    pub(crate) fn new(file: FileId) -> Self {
        Ast {
            file,
            nodes: Vec::new(),
            scalars: Vec::new(),
            tags: Vec::new(),
            aliases: Vec::new(),
            seqs: Vec::new(),
            seq_items: Vec::new(),
            maps: Vec::new(),
            entries: Vec::new(),
            documents: Vec::new(),
            anchors: AnchorTable::new(),
        }
    }

    /// The file this AST was parsed from.
    #[must_use]
    pub fn file(&self) -> FileId {
        self.file
    }

    /// Every node, in creation order.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Look up a node.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// The documents of the stream, in source order.
    #[must_use]
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    /// The anchor definitions found while parsing.
    #[must_use]
    pub fn anchors(&self) -> &AnchorTable {
        &self.anchors
    }

    /// The scalar payload of `id`, or `None` if it is not a scalar.
    #[must_use]
    pub fn scalar(&self, id: NodeId) -> Option<&Scalar> {
        match self.node(id).kind {
            NodeKind::Scalar(i) => Some(&self.scalars[i as usize]),
            _ => None,
        }
    }

    /// The items of the sequence `id`, or `None` if it is not a sequence.
    #[must_use]
    pub fn items(&self, id: NodeId) -> Option<&[NodeId]> {
        match self.node(id).kind {
            NodeKind::Sequence(i) => {
                let r = self.seqs[i as usize];
                Some(&self.seq_items[r.start as usize..r.end as usize])
            }
            _ => None,
        }
    }

    /// The entries of the mapping `id`, or `None` if it is not a mapping.
    #[must_use]
    pub fn entries(&self, id: NodeId) -> Option<&[Entry]> {
        match self.node(id).kind {
            NodeKind::Mapping(i) => {
                let r = self.maps[i as usize];
                Some(&self.entries[r.start as usize..r.end as usize])
            }
            _ => None,
        }
    }

    /// The alias record of `id`, or `None` if it is not an alias.
    #[must_use]
    pub fn alias(&self, id: NodeId) -> Option<&AliasRef> {
        match self.node(id).kind {
            NodeKind::Alias(i) => Some(&self.aliases[i as usize]),
            _ => None,
        }
    }

    /// The explicit tag on `id`, if any.
    #[must_use]
    pub fn tag(&self, id: NodeId) -> Option<&Tag> {
        self.node(id).tag.map(|i| &self.tags[i as usize])
    }

    /// Follow an alias to the node its anchor names. Returns `None` for a
    /// non-alias node or an unbound anchor. This is a lookup, not a copy, so
    /// following it repeatedly can revisit nodes: callers must keep a visited
    /// set.
    #[must_use]
    pub fn alias_target(&self, id: NodeId) -> Option<NodeId> {
        let alias = self.alias(id)?;
        self.anchors.get(alias.anchor).map(|def| def.node)
    }
}
