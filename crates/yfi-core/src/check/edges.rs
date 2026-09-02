// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0223`, `E0224` and `E0225` — reading what an `!edge` node relates (D4.13).
//!
//! An edge is a node, so this pass adds no construct: it reads two members off
//! a node's **resolved** view and records what they name. Reading the resolved
//! view rather than `own(A)` is the whole of the answer to *what does extending
//! an edge mean* — nothing new. `connections` is one member, absorbed by the
//! ordinary left-biased, shallow rule (D1.5), so a child either restates the
//! sequence whole or inherits it whole. Extension never **appends** endpoints,
//! and the operators are untouched.
//!
//! # Why this runs in pass 5 and not pass 4
//!
//! Because `connections` may be inherited. Pass 4 has resolved every path and
//! knows what each item names, but only pass 5 knows which `connections` a node
//! ends up holding, and reporting `E0223` against `own(A)` would fire on every
//! concrete edge of a family that declares its endpoints once in the base.

use std::collections::HashMap;

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use crate::edge::{is_edge, CONNECTIONS, DEFINITION};
use crate::link::{Ctx, Linked, RefRole};
use crate::symbol::Symbol;

use super::names::span_of;
use super::resolve::Views;
use super::view::Place;

/// One endpoint of one edge, in written order.
#[derive(Clone, Copy, Debug)]
pub struct Connection {
    /// Its position in `connections`, which is what a handle names and what
    /// never renumbers: filtering an endpoint an observer cannot see removes it
    /// from a result, and does not move the ones beside it.
    pub index: u32,
    /// The item node that named it, in the file that wrote the sequence.
    pub item: Place,
    /// The node it names.
    pub target: Place,
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
}

impl EdgeNode {
    /// The endpoint a handle names, if it names one.
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

/// Collect every `!edge` node's endpoints, reporting what is malformed.
pub(crate) fn collect(
    ctx: &Ctx,
    linked: &Linked,
    views: &Views,
    order: &[Place],
    diagnostics: &mut Diagnostics,
) -> Edges {
    let targets = resolved_items(linked);
    let mut edges = Edges::default();
    for place in order.iter().filter(|held| is_edge(ctx.interned, held.0, held.1)) {
        let held = one(ctx, views, &targets, *place, diagnostics);
        edges.index.insert(*place, edges.items.len());
        edges.items.push(held);
    }
    edges
}

/// What each `connections` item resolved to, from pass 4's reference table.
/// An item that resolved to nothing is absent, and `E0213` has already named
/// it — reporting it again here would give one fault two codes.
fn resolved_items(linked: &Linked) -> HashMap<Place, Place> {
    linked
        .references()
        .iter()
        .filter(|held| held.role == RefRole::Connection)
        .filter_map(|held| held.target.map(|target| ((held.file, held.node), target)))
        .collect()
}

/// Read one edge node.
fn one(
    ctx: &Ctx,
    views: &Views,
    targets: &HashMap<Place, Place>,
    place: Place,
    diagnostics: &mut Diagnostics,
) -> EdgeNode {
    let Some(items) = connection_items(ctx, views, place, diagnostics) else {
        return EdgeNode { place, connections: Vec::new(), handles: Vec::new() };
    };
    let mut connections = endpoints(ctx, targets, items);
    let handles = handles(ctx, views, place, connections.len(), diagnostics);
    // The **first** handle naming a position labels the edge record for it, so
    // the index carries a stable name. The rest are not lost: every handle is
    // kept on the edge, because the mapping is many-to-one and a self-loop
    // names one position twice on purpose.
    for (name, index) in &handles {
        if let Some(held) = connections.get_mut(*index as usize) {
            if held.handle.is_none() {
                held.handle = Some(*name);
            }
        }
    }
    EdgeNode { place, connections, handles }
}

/// The items of an edge's `connections`, or `None` when it has none to read.
///
/// `E0223` when the resolved view holds no such member — the tag then relates
/// nothing, and a tag that means nothing is the failure this decision exists to
/// remove. `E0224` when it holds one that is not a sequence.
fn connection_items(
    ctx: &Ctx,
    views: &Views,
    place: Place,
    diagnostics: &mut Diagnostics,
) -> Option<Vec<(FileId, NodeId)>> {
    let name = ctx.interned.symbols().get(CONNECTIONS);
    let field = name.and_then(|name| views.resolved(place)?.get(name));
    let Some(field) = field else {
        diagnostics.push(missing(ctx, place));
        return None;
    };
    let ast = ctx.ast(field.value.0)?;
    let Some(items) = ast.items(field.value.1) else {
        diagnostics.push(shape(ctx, CONNECTIONS, "a sequence", field.value));
        return None;
    };
    Some(items.iter().map(|item| (field.value.0, *item)).collect())
}

/// Pair each item with the node it names, keeping written order and the
/// position each item holds. An item that named nothing keeps its position:
/// renumbering the endpoints after it would silently move every handle.
fn endpoints(
    ctx: &Ctx,
    targets: &HashMap<Place, Place>,
    items: Vec<(FileId, NodeId)>,
) -> Vec<Connection> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(at, item)| {
            let target = named(ctx, targets, item)?;
            Some(Connection {
                index: u32::try_from(at).ok()?,
                item,
                target,
                handle: None,
                span: ctx.ast(item.0)?.node(item.1).span,
            })
        })
        .collect()
}

/// The node one item names: an alias binding, a resolved path, or the item
/// itself when it is written inline. An inline endpoint is a node like any
/// other; it simply has no name to be addressed by.
fn named(ctx: &Ctx, targets: &HashMap<Place, Place>, item: Place) -> Option<Place> {
    let ast = ctx.ast(item.0)?;
    if ast.alias(item.1).is_some() {
        return ast.alias_binding(item.1);
    }
    if ast.scalar(item.1).is_some() {
        return targets.get(&item).copied();
    }
    Some(item)
}

/// The handles `definition` declares, each checked against the number of
/// endpoints there are to name.
fn handles(
    ctx: &Ctx,
    views: &Views,
    place: Place,
    count: usize,
    diagnostics: &mut Diagnostics,
) -> Vec<(Symbol, u32)> {
    let name = ctx.interned.symbols().get(DEFINITION);
    let Some(field) = name.and_then(|name| views.resolved(place)?.get(name)) else {
        return Vec::new();
    };
    let Some(ast) = ctx.ast(field.value.0) else { return Vec::new() };
    let Some(entries) = ast.entries(field.value.1) else {
        diagnostics.push(shape(ctx, DEFINITION, "a mapping", field.value));
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries {
        let Some(name) = ctx.interned.key_of(field.value.0, entry.key) else { continue };
        match position(ast, entry.value, count) {
            Some(index) => out.push((name, index)),
            None => diagnostics.push(unbound(ctx, (field.value.0, entry.value), count)),
        }
    }
    out
}

/// A handle's value read as a position in `connections`.
fn position(ast: &Ast, node: NodeId, count: usize) -> Option<u32> {
    let index: u32 = ast.scalar(node)?.value.trim().parse().ok()?;
    (index as usize).lt(&count).then_some(index)
}

/// `E0223` — an `!edge` that relates nothing.
fn missing(ctx: &Ctx, place: Place) -> Diagnostic {
    Diagnostic::new(
        Code::EdgeWithoutConnections,
        span_of(ctx, place),
        "an `!edge` holds no `connections`, so the tag relates nothing",
    )
    .with_note(
        "`connections` is a sequence of the nodes the edge connects, and is what makes the node \
         an edge; an edge that relates nothing yet writes `connections: []`",
        None,
    )
}

/// `E0224` — one of the two members the language owns has the wrong shape.
fn shape(ctx: &Ctx, member: &str, wanted: &str, at: Place) -> Diagnostic {
    Diagnostic::new(
        Code::EdgeMemberShape,
        span_of(ctx, at),
        format!("an edge's `{member}` must be {wanted}"),
    )
    .with_note(
        "`connections` is a sequence of endpoints and `definition` is a mapping of handle names \
         to positions in it",
        None,
    )
}

/// `E0225` — a handle naming no endpoint.
fn unbound(ctx: &Ctx, at: Place, count: usize) -> Diagnostic {
    Diagnostic::new(
        Code::UnboundHandle,
        span_of(ctx, at),
        format!(
            "a `definition` handle names no connection; this edge has {count} of them, numbered \
             from 0"
        ),
    )
    .with_note(
        "a handle is a name for a position in `connections`, so its value is an index into that \
         sequence",
        None,
    )
}
