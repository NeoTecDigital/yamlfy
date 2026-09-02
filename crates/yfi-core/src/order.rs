// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The project's total order over nodes.
//!
//! Discovery order is **normative**, and it is not cosmetic. Later passes make
//! a cyclic graph acyclic by dropping back edges in this order; without a total
//! order `readdir` would choose which back edge is dropped, which changes
//! *which other diagnostics get reported*, so the same tree would print a
//! different error set on two machines and CI snapshots would flake.
//!
//! The key is the path **relative to the project root**, compared
//! component-wise. It is deliberately *not* the canonicalized path:
//! canonicalization resolves symlinks, so a tree that pulls files in through
//! links would be ranked by wherever the targets happen to live on that
//! machine — exactly the nondeterminism the rule exists to prevent.
//! Canonicalization is used only to recognise that two entries are the same
//! real file.
//!
//! "Document order" is then the triple `(file rank, document index, node
//! index)`. The node index is the arena index, which is *post-order*: a
//! collection's index exceeds every one of its children's. That is a total
//! order, which is all this needs to be, but it is bottom-up rather than
//! source order, so a pass that wants the textually first node of a document
//! must compare spans instead.

/// A position in the project, total across every file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeOrder {
    /// Rank of the file in relative-path order.
    pub file: u32,
    /// Zero-based document index within that file.
    pub document: u32,
    /// Arena index of the node within that file.
    pub node: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triple_orders_by_file_then_document_then_node() {
        let a = NodeOrder { file: 0, document: 9, node: 9 };
        let b = NodeOrder { file: 1, document: 0, node: 0 };
        let c = NodeOrder { file: 1, document: 0, node: 1 };
        assert!(a < b);
        assert!(b < c);
    }
}
