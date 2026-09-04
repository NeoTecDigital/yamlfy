// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The edge index, built once and read in both directions.
//!
//! Pass 6 records every relationship the project **writes** — the three
//! operators, the bare capability declaration, the `connections` of every
//! `!edge` node (D4.13) and the data edges a path or an alias writes — as one
//! flat list, gives each end an id, and groups it by source and by target.
//!
//! Read from the **clauses, the references and the edge nodes**, which are what
//! an author wrote, rather than from pass 4's stratified graph, which is the
//! encoding SCC needs. That encoding gives an extended reference two vertices
//! and two edges so that a reverse dependency can never lie on a cycle; here
//! the two directions are the two indexes, so decoding it back would only be a
//! way of losing the operand's written form on the way through.
//!
//! # One relationship is one record
//!
//! Nothing written once is recorded twice, and the two sets that keep it that
//! way are [`owned_values`] and the clause-operand set in [`alias_edges`]. Both
//! are needed because a data edge leaves the *collection* a value sits in,
//! which for a sequence element is the sequence and not the node that wrote the
//! member — so a duplicate is invisible to any check made on the edge node's
//! own run.

use std::collections::{HashMap, HashSet};

use yfi_syntax::{FileId, NodeId, Span};

use crate::check::Checked;
use crate::image::{Edge, EdgeKind, Flat, ModelId};
use crate::link::{ClauseKind, Ctx, Linked, OperandForm, RefRole, SourceOrder};
use crate::symbol::Symbol;

use super::{order_of, Place};

/// An edge before its endpoints have ids.
pub(super) struct RawEdge {
    pub(super) from: Place,
    pub(super) to: Place,
    kind: EdgeKind,
    capability: bool,
    overrides: bool,
    key: Option<Symbol>,
    span: Span,
    order: SourceOrder,
}

/// Every relationship the project writes, in the order it is written.
pub(super) fn raw_edges(ctx: &Ctx, linked: &Linked, checked: &Checked) -> Vec<RawEdge> {
    let mut out = Vec::new();
    let owned = owned_values(ctx, checked);
    clause_edges(linked, &mut out);
    connection_edges(checked, &mut out);
    reference_edges(ctx, linked, &owned, &mut out);
    alias_edges(ctx, linked, &owned, &mut out);
    for edge in &mut out {
        edge.order = order_of(ctx, edge.from);
    }
    out.sort_by_key(|edge| (edge.order, edge.span.start.byte));
    out
}

/// The two inheritance operators, and the third operation the `!ref` operand
/// makes of the second (D4.1).
fn clause_edges(linked: &Linked, out: &mut Vec<RawEdge>) {
    for clause in linked.clauses() {
        for operand in &clause.operands {
            let kind = match (clause.kind, operand.form) {
                (ClauseKind::Inclusion, _) => EdgeKind::Inclusion,
                (ClauseKind::Extension, OperandForm::Ref) => EdgeKind::ExtendedReference,
                (ClauseKind::Extension, _) => EdgeKind::Extension,
            };
            out.push(RawEdge {
                from: (clause.file, clause.owner),
                to: operand.target,
                kind,
                capability: operand.form == OperandForm::Ref,
                overrides: operand.overrides,
                key: None,
                span: operand.span,
                order: SourceOrder { file: 0, document: 0, byte: 0 },
            });
        }
    }
}

/// The endpoints of every `!edge` node (D4.13).
///
/// One record per endpoint, all leaving the edge node — the incidence encoding
/// an n-ary relation forces, and the one that keeps a `!edge` node addressable
/// as a node while being traversable as a relation. The `key` carries the
/// handle `definition` named that position with, so a handle is answered out of
/// the same index rather than out of a second table, and `capability` carries
/// the `!ref` the item was written with, so an endpoint the edge declared it
/// intends to modify is marked exactly as any other `!ref` operand is.
///
/// Read from [`Checked::edges`], whose `connections` came from the **resolved**
/// view: an edge extending an `!type`d edge family therefore has the family's
/// endpoints, by nothing but what extension already means.
fn connection_edges(checked: &Checked, out: &mut Vec<RawEdge>) {
    for held in checked.edges().items() {
        for connection in &held.connections {
            out.push(RawEdge {
                from: held.place,
                to: connection.target,
                kind: EdgeKind::Connection,
                capability: connection.capability,
                overrides: false,
                key: connection.handle,
                span: connection.span,
                order: SourceOrder { file: 0, document: 0, byte: 0 },
            });
        }
    }
}

/// Every node the two members the language owns on an edge are written from.
///
/// Nothing in it is a **data** edge, and each part of it is there for its own
/// reason:
///
/// * a `connections` **item** is an endpoint, already recorded once as an
///   [`EdgeKind::Connection`]. An alias standing there would otherwise be
///   recorded a second time as data, because it leaves the *sequence* rather
///   than the edge and so is invisible to a check on the edge's own run;
/// * the `connections` **value** itself. An alias standing there is the
///   sequence of endpoints — dereferenced for the value exactly as for an item
///   — so recording it as data would say a second time, under a second kind,
///   what the connection records already say. And when the value is the wrong
///   shape it has earned `E0224` and relates nothing, so reading it as a member
///   naming a node would put a relationship in the image that the compiler had
///   just refused;
/// * a `definition` **handle's value**, which is a position in a sequence.
///   `owner: *Team` names no position and is `E0225`; it is not also a member
///   of the edge that points at `Team`.
fn owned_values(ctx: &Ctx, checked: &Checked) -> HashSet<Place> {
    let mut out: HashSet<Place> = HashSet::new();
    for held in checked.edges().items() {
        out.extend(held.connections.iter().map(|connection| connection.item));
        out.extend(held.connections_value);
        out.extend(handle_values(ctx, held.definition_value));
    }
    out
}

/// The value node of every entry of an edge's `definition` mapping.
fn handle_values(ctx: &Ctx, at: Option<Place>) -> Vec<Place> {
    let Some(at) = at else { return Vec::new() };
    let Some(ast) = ctx.ast(at.0) else { return Vec::new() };
    let Some(entries) = ast.entries(at.1) else { return vec![at] };
    entries.iter().map(|entry| (at.0, entry.value)).collect()
}

/// The data edges a **path** writes: `key: ../peer/Thing`, and the same
/// carrying `!ref`. A path under `<<` or `extends` is a clause and is already
/// an edge, so only [`RefRole::Data`] is read here.
///
/// `key: !ref P` stays a **data** edge and is marked a capability. It is one
/// edge and two claims — the member names P, and this context declares it
/// intends to modify P — and splitting it into two edges would double every
/// `!ref` in the graph while saying nothing the flag does not.
fn reference_edges(ctx: &Ctx, linked: &Linked, owned: &HashSet<Place>, out: &mut Vec<RawEdge>) {
    for held in linked.references().iter().filter(|held| held.role == RefRole::Data) {
        if owned.contains(&(held.file, held.node)) {
            continue;
        }
        let (Some(target), Some(from)) = (held.target, holder_of(ctx, held.file, held.node)) else {
            continue;
        };
        out.push(RawEdge {
            from,
            to: target,
            kind: EdgeKind::Data,
            capability: held.capability,
            overrides: held.overrides,
            key: member_key(ctx, held.file, held.node),
            span: held.span,
            order: SourceOrder { file: 0, document: 0, byte: 0 },
        });
    }
}

/// The data edges an **alias** writes. An alias standing as a clause operand is
/// that clause's edge already, so it is skipped here rather than recorded twice
/// under two kinds.
fn alias_edges(ctx: &Ctx, linked: &Linked, owned: &HashSet<Place>, out: &mut Vec<RawEdge>) {
    let operands: HashSet<Place> = linked
        .clauses()
        .iter()
        .flat_map(|clause| clause.operands.iter().map(|held| (clause.file, held.node)))
        .collect();
    for file in ctx.project.files() {
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            let held = (file.id, node);
            if operands.contains(&held) || owned.contains(&held) {
                continue;
            }
            if file.ast.alias(node).is_none() {
                continue;
            }
            let (Some(target), Some(from)) =
                (file.ast.alias_binding(node), holder_of(ctx, file.id, node))
            else {
                continue;
            };
            out.push(RawEdge {
                from,
                to: target,
                kind: EdgeKind::Data,
                capability: false,
                overrides: false,
                key: member_key(ctx, file.id, node),
                span: file.ast.node(node).span,
                order: SourceOrder { file: 0, document: 0, byte: 0 },
            });
        }
    }
}

/// The collection a value is written inside — the node the edge leaves.
fn holder_of(ctx: &Ctx, file: FileId, node: NodeId) -> Option<Place> {
    let mut at = ctx.interned.parent_of(file, node)?;
    let ast = ctx.ast(file)?;
    while ast.entries(at).is_none() && ast.items(at).is_none() {
        at = ctx.interned.parent_of(file, at)?;
    }
    Some((file, at))
}

/// The member whose value is `node`, with its flag prefix already taken off.
///
/// One level of sequence is stepped through, because `key: [*a, *b]` writes two
/// edges and both belong to `key`. A value that is not a mapping entry's — a
/// bare document root, say — belongs to no member and yields nothing.
fn member_key(ctx: &Ctx, file: FileId, node: NodeId) -> Option<Symbol> {
    let ast = ctx.ast(file)?;
    let parent = ctx.interned.parent_of(file, node)?;
    let (holder, value) = match ast.items(parent) {
        Some(_) => (ctx.interned.parent_of(file, parent)?, parent),
        None => (parent, node),
    };
    let entry = ast.entries(holder)?.iter().find(|entry| entry.value == value)?;
    ctx.interned.key_of(file, entry.key)
}

/// Give every raw edge its ids, dropping any whose endpoints the image does not
/// hold — which is only ever an edge into a file that failed to parse.
pub(super) fn resolved_edges(index: &HashMap<Place, u32>, raw: Vec<RawEdge>) -> Vec<Edge> {
    raw.into_iter()
        .filter_map(|edge| {
            let (from, to) = (index.get(&edge.from)?, index.get(&edge.to)?);
            Some(Edge {
                from: ModelId(*from),
                to: ModelId(*to),
                kind: edge.kind,
                capability: edge.capability,
                overrides: edge.overrides,
                key: edge.key,
                span: edge.span,
            })
        })
        .collect()
}

/// Group every edge by its source and by its target, so both directions are one
/// contiguous slice per node. The reverse index is materialised rather than
/// derived: inbound traversal has to be O(degree), and scanning every edge to
/// answer it would make `inc` O(V+E) per call.
pub(super) fn csr(edges: &[Edge], count: usize) -> (Flat<Edge>, Flat<Edge>) {
    let mut out: Vec<Vec<Edge>> = (0..count).map(|_| Vec::new()).collect();
    let mut inc: Vec<Vec<Edge>> = (0..count).map(|_| Vec::new()).collect();
    for edge in edges {
        if let Some(run) = out.get_mut(edge.from.index()) {
            run.push(*edge);
        }
        if let Some(run) = inc.get_mut(edge.to.index()) {
            run.push(*edge);
        }
    }
    (Flat::build(out), Flat::build(inc))
}
