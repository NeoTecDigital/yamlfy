// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an `!edge` node relates, as a record (D4.13).
//!
//! Read by pass 6; built by [`super::collect`]. Re-exported from
//! [`crate::check::edges`], so this file is a split and not a new address.

use std::collections::HashMap;

use yfi_syntax::Span;

use crate::symbol::Symbol;

use super::super::view::Place;

/// One endpoint of one edge, in written order.
#[derive(Clone, Copy, Debug)]
pub struct Connection {
    /// Its position in `connections`, which is what a handle names and what
    /// never renumbers: filtering an endpoint an observer cannot see removes it
    /// from a result, and an endpoint that resolved to nothing leaves a gap,
    /// and neither moves the ones beside it.
    pub index: u32,
    /// The item node that named it, in the file that wrote the sequence.
    pub item: Place,
    /// The node it names.
    pub target: Place,
    /// Whether the item was written `!ref` — the same declaration of intent it
    /// is anywhere else (D4.3), checked as `E0217` and recorded here so the
    /// image can answer *which endpoints does this edge intend to modify*.
    pub capability: bool,
    /// The handle `definition` gives this position, if it gives it one.
    pub handle: Option<Symbol>,
    /// Where the item is written.
    pub span: Span,
}

/// One `!edge` node, and what it relates.
#[derive(Debug)]
pub struct EdgeNode {
    /// The node carrying the tag.
    pub place: Place,
    /// Its endpoints, in written order. Possibly empty: an edge that writes
    /// `connections: []` relates nothing *yet*, which is a shape and not a
    /// mistake, while an edge with no `connections` member at all is `E0223`.
    pub connections: Vec<Connection>,
    /// Every handle `definition` declares, as a name and the position it names,
    /// in written order.
    ///
    /// This is a list and not a field on [`Connection`] because the mapping is
    /// **many-to-one**: two handles may name one position, and that is not a
    /// degenerate case but the ordinary way a self-loop is written — `from: 0`
    /// and `to: 0` over a single endpoint. Recording the handle on the position
    /// it names lets only the last one survive, which silently loses `from`.
    pub handles: Vec<(Symbol, u32)>,
    /// Where the resolved view sites `connections`, whatever shape it turned
    /// out to have, and `definition` likewise. Emission reads them to keep the
    /// language's own members out of the **data** edges: a handle's value is a
    /// position rather than a reference, and a `connections` of the wrong shape
    /// has been reported once already and must not be reported again as a
    /// member that names a node.
    pub connections_value: Option<Place>,
    /// Where the resolved view sites `definition`. See
    /// [`EdgeNode::connections_value`].
    pub definition_value: Option<Place>,
}

impl EdgeNode {
    /// The endpoint a handle names, if it names one.
    ///
    /// Matched by **written position**, which is what the handle was checked
    /// against. A handle naming a position whose item resolved to nothing binds
    /// to no endpoint and answers `None`, and the endpoint after it keeps the
    /// name it was given.
    #[must_use]
    pub fn connection(&self, handle: Symbol) -> Option<&Connection> {
        let (_, index) = self.handles.iter().find(|(name, _)| *name == handle)?;
        self.connections.iter().find(|held| held.index == *index)
    }
}

/// Every `!edge` node of the project, indexed by the node carrying the tag.
#[derive(Default)]
pub struct Edges {
    items: Vec<EdgeNode>,
    index: HashMap<Place, usize>,
}

impl Edges {
    /// Record one edge node, keyed by the node carrying the tag.
    pub(super) fn push(&mut self, held: EdgeNode) {
        self.index.insert(held.place, self.items.len());
        self.items.push(held);
    }

    /// Every edge node, in the order pass 5 resolved them.
    #[must_use]
    pub fn items(&self) -> &[EdgeNode] {
        &self.items
    }

    /// The edge written at `place`, if one is.
    #[must_use]
    pub fn get(&self, place: Place) -> Option<&EdgeNode> {
        self.index.get(&place).map(|at| &self.items[*at])
    }

    /// How many edge nodes the project writes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the project writes no edge at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
