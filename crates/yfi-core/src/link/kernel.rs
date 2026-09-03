// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What passes 4, 5 and 6 all read: the project borrow, and source order.
//!
//! Re-exported from [`crate::link`], so this file is a split and not a new
//! address.

use yfi_syntax::{Ast, FileId, NodeId, Pos, Span};

use crate::discover::{FileClass, Project};
use crate::intern::Interned;

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

    /// A node's span, for a diagnostic or a model record.
    ///
    /// The fallback needs a node absent from the arena it was taken from, which
    /// no caller can produce. It is an empty span at the start of the right
    /// file rather than a panic, because a diagnostic that cannot be placed
    /// must still be reported.
    pub(crate) fn span_of(&self, file: FileId, node: NodeId) -> Span {
        self.ast(file)
            .and_then(|ast| ast.nodes().get(node.index()).map(|held| held.span))
            .unwrap_or_else(|| Span::empty(file, Pos::default()))
    }
}

/// A position in the project ordered by where it is **written**.
///
/// The arena index is *post-order*: the lowest-indexed member of a set is its
/// deepest-leftmost leaf, not the one written first. Every user-visible choice
/// — which member of a cycle `E0212` points at, which of two contributions is
/// "the first" — must be the textually first one, so it is ordered by this
/// instead. Provided here rather than left to pass 5, which would otherwise
/// have to guess.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triple_orders_by_file_then_document_then_position() {
        let a = SourceOrder { file: 0, document: 9, byte: 9 };
        let b = SourceOrder { file: 1, document: 0, byte: 0 };
        let c = SourceOrder { file: 1, document: 0, byte: 1 };
        assert!(a < b);
        assert!(b < c);
    }
}
