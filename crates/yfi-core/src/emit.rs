// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 6 — emission.
//!
//! Pass 5 answered *what does each node mean, and is that allowed*. Pass 6
//! answers *what does the graph look like when you have to walk it*, and it is
//! the first pass whose output is a shape rather than a verdict.
//!
//! It builds one [`Image`]:
//!
//! * a node per resolved holder, marked **concrete** (`!node`) or **abstract**
//!   (`!type`, untagged, or anything in base YAML — D7.1, D6.6);
//! * each node's **ancestor chain**, because flattening destroys the `is_a`
//!   axis and retaining it is the whole reason `extends` is not `<<`;
//! * a **CSR edge index in both directions**, over the three operators, the
//!   bare capability declaration and **data edges**;
//! * each node's **scope path**, so an access question needs no tree walk;
//! * the **name index** a path query resolves against.
//!
//! # What it refuses
//!
//! Emission is refused outright when [`Checked::is_cyclic`] holds. Pass 5 still
//! composed a view for every node, over a graph made acyclic by dropping back
//! edges — but that is a recovery and not a language semantic, and shipping it
//! would put a value in the output that no source text means. A refused image
//! holds nothing.
//!
//! Because emission is refused there, the graph pass 6 walks is a **DAG on the
//! inheritance edges**, and the dropped-edge set is empty by construction. That
//! is why [`Checked::ancestors`] has nothing to drop when pass 6 asks it.
//!
//! # Data edges are the ones that may cycle
//!
//! Only *inheritance* cycles are illegal. `a: ./B` in one file and `b: ./A` in
//! the next is a legal, intended shape, so every traversal over the edge index
//! carries a visited set (spec §0) and the index itself is built by a single
//! pass with no traversal at all.
//!
//! # One forward scan, no recursion
//!
//! The arena is post-order — a collection's `NodeId` exceeds every child's — so
//! the node set is gathered by one forward scan per file. Ordering for anything
//! user-visible is `(file rank, document index, source position)` via
//! [`crate::link::source_order`]; `NodeOrder`'s node component is the arena
//! index and is *not* textual order.
//!
//! # Example
//!
//! ```no_run
//! use yfi_core::{check, discover, emit, intern, link, DiscoverOptions};
//!
//! let project = discover("projects/check-diamond", &DiscoverOptions::default());
//! let interned = intern::intern(&project);
//! let linked = link::link(&project, &interned);
//! let checked = check::check(&project, &interned, &linked);
//! let image = emit::emit(&project, &interned, &linked, &checked);
//! assert!(!image.is_refused());
//! ```

use std::collections::{HashMap, HashSet};

use tracing::debug;
use yfi_syntax::{FileId, NodeId, Pos, Span};

use crate::check::{self, Checked};
use crate::discover::Project;
use crate::image::{Edge, EdgeKind, Flat, Image, Model, ModelId, ModelKind};
use crate::intern::Interned;
use crate::link::path::Space;
use crate::link::{source_order, ClauseKind, Ctx, Linked, OperandForm, RefRole, SourceOrder};
use crate::scope::ScopeId;
use crate::symbol::Symbol;

/// A node of the project, before it has an id.
type Place = (FileId, NodeId);

/// Build the image for a checked project.
///
/// Returns a **refused** image when pass 5 found an inheritance cycle: the
/// recovered views are not a meaning and must not reach output.
#[must_use]
pub fn emit<'a>(
    project: &'a Project,
    interned: &'a Interned,
    linked: &'a Linked,
    checked: &'a Checked,
) -> Image<'a> {
    let ctx = Ctx { project, interned };
    if checked.is_cyclic() {
        debug!("emission refused: the inheritance graph held a cycle");
        return empty(project, interned, linked, checked, &ctx, true);
    }
    let raw = raw_edges(&ctx, linked);
    let places = places(&ctx, checked, &raw);
    let index: HashMap<Place, u32> = places
        .iter()
        .enumerate()
        .map(|(at, place)| (*place, u32::try_from(at).expect("image overflow")))
        .collect();
    let models = models(&ctx, &places);
    let concrete = models
        .iter()
        .enumerate()
        .filter(|(_, model)| model.kind.is_concrete())
        .map(|(at, _)| ModelId(u32::try_from(at).expect("image overflow")))
        .collect();
    let edges = resolved_edges(&index, raw);
    let (out, inc) = csr(&edges, places.len());
    let ancestry = ancestry(checked, linked, &index, &places);
    let paths = paths(&ctx, &places);
    debug!(nodes = places.len(), edges = edges.len(), "emitted image");
    Image {
        project,
        interned,
        linked,
        checked,
        models,
        index,
        concrete,
        out,
        inc,
        ancestry,
        paths,
        space: Space::build(&ctx),
        refused: false,
    }
}

/// An image holding nothing, which is what a cyclic project emits.
fn empty<'a>(
    project: &'a Project,
    interned: &'a Interned,
    linked: &'a Linked,
    checked: &'a Checked,
    ctx: &Ctx,
    refused: bool,
) -> Image<'a> {
    Image {
        project,
        interned,
        linked,
        checked,
        models: Vec::new(),
        index: HashMap::new(),
        concrete: Vec::new(),
        out: Flat::empty(0),
        inc: Flat::empty(0),
        ancestry: Flat::empty(0),
        paths: Flat::empty(0),
        space: Space::build(ctx),
        refused,
    }
}

/// Every node the image holds, in the project's source order.
///
/// A node qualifies by holding a resolved view — every mapping, and every
/// sequence written as a member list — or by being named by an edge, which is
/// how a `!ref` to a scalar member keeps somewhere to land. A header document
/// declares the *file's* axes rather than a node of the graph (D6.4), so it is
/// excluded.
fn places(ctx: &Ctx, checked: &Checked, raw: &[RawEdge]) -> Vec<Place> {
    let mut held: HashSet<Place> = HashSet::new();
    for file in ctx.project.files() {
        let header = header_document(ctx, file.id);
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            if header.is_some() && ctx.interned.document_of(file.id, node) == header {
                continue;
            }
            if checked.resolved(file.id, node).is_some() {
                held.insert((file.id, node));
            }
        }
    }
    for edge in raw {
        held.insert(edge.from);
        held.insert(edge.to);
    }
    let mut out: Vec<Place> = held.into_iter().collect();
    out.sort_by_key(|place| order_of(ctx, *place));
    out
}

/// The document index of a file's header, if it declares one.
fn header_document(ctx: &Ctx, file: FileId) -> Option<u32> {
    let held = ctx.project.file(file)?.header.as_ref()?;
    ctx.interned.document_of(file, held.node)
}

/// Where a node sits in the project's user-visible order. A node the order
/// cannot be read for sorts last rather than first, so recovery debris never
/// displaces written source.
fn order_of(ctx: &Ctx, place: Place) -> SourceOrder {
    source_order(ctx.project, ctx.interned, place.0, place.1)
        .unwrap_or(SourceOrder { file: u32::MAX, document: u32::MAX, byte: u32::MAX })
}

/// One record per node, in id order.
fn models(ctx: &Ctx, places: &[Place]) -> Vec<Model> {
    places
        .iter()
        .map(|place| Model {
            place: *place,
            kind: match check::is_concrete(ctx.interned, *place) {
                true => ModelKind::Concrete,
                false => ModelKind::Abstract,
            },
            scope: ctx
                .interned
                .scope_of(place.0, place.1)
                .or_else(|| ctx.project.scopes().root())
                .unwrap_or(ScopeId(0)),
            span: span_of(ctx, *place),
            order: order_of(ctx, *place),
        })
        .collect()
}

fn span_of(ctx: &Ctx, place: Place) -> Span {
    ctx.ast(place.0)
        .and_then(|ast| ast.nodes().get(place.1.index()).map(|node| node.span))
        .unwrap_or_else(|| Span::empty(place.0, Pos { byte: 0, line: 1, col: 1 }))
}

/// An edge before its endpoints have ids.
struct RawEdge {
    from: Place,
    to: Place,
    kind: EdgeKind,
    capability: bool,
    key: Option<Symbol>,
    span: Span,
    order: SourceOrder,
}

/// Every relationship the project writes, in the order it is written.
///
/// Read from the **clauses and references**, which are what an author wrote,
/// rather than from pass 4's stratified graph, which is the encoding SCC needs.
/// That encoding gives an extended reference two vertices and two edges so that
/// a reverse dependency can never lie on a cycle; here the two directions are
/// the two indexes, so decoding it back would only be a way of losing the
/// operand's written form on the way through.
fn raw_edges(ctx: &Ctx, linked: &Linked) -> Vec<RawEdge> {
    let mut out = Vec::new();
    clause_edges(linked, &mut out);
    reference_edges(ctx, linked, &mut out);
    alias_edges(ctx, linked, &mut out);
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
                key: None,
                span: operand.span,
                order: SourceOrder { file: 0, document: 0, byte: 0 },
            });
        }
    }
}

/// The data edges a **path** writes: `key: ../peer/Thing`, and the same
/// carrying `!ref`. A path under `<<` or `extends` is a clause and is already
/// an edge, so only [`RefRole::Data`] is read here.
///
/// `key: !ref P` stays a **data** edge and is marked a capability. It is one
/// edge and two claims — the member names P, and this context declares it
/// intends to modify P — and splitting it into two edges would double every
/// `!ref` in the graph while saying nothing the flag does not.
fn reference_edges(ctx: &Ctx, linked: &Linked, out: &mut Vec<RawEdge>) {
    for held in linked.references().iter().filter(|held| held.role == RefRole::Data) {
        let (Some(target), Some(from)) = (held.target, holder_of(ctx, held.file, held.node))
        else {
            continue;
        };
        out.push(RawEdge {
            from,
            to: target,
            kind: EdgeKind::Data,
            capability: held.capability,
            key: member_key(ctx, held.file, held.node),
            span: held.span,
            order: SourceOrder { file: 0, document: 0, byte: 0 },
        });
    }
}

/// The data edges an **alias** writes. An alias standing as a clause operand is
/// that clause's edge already, so it is skipped here rather than recorded twice
/// under two kinds.
fn alias_edges(ctx: &Ctx, linked: &Linked, out: &mut Vec<RawEdge>) {
    let operands: HashSet<Place> = linked
        .clauses()
        .iter()
        .flat_map(|clause| clause.operands.iter().map(|held| (clause.file, held.node)))
        .collect();
    for file in ctx.project.files() {
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            if operands.contains(&(file.id, node)) || file.ast.alias(node).is_none() {
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
fn resolved_edges(index: &HashMap<Place, u32>, raw: Vec<RawEdge>) -> Vec<Edge> {
    raw.into_iter()
        .filter_map(|edge| {
            let (from, to) = (index.get(&edge.from)?, index.get(&edge.to)?);
            Some(Edge {
                from: ModelId(*from),
                to: ModelId(*to),
                kind: edge.kind,
                capability: edge.capability,
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
fn csr(edges: &[Edge], count: usize) -> (Flat<Edge>, Flat<Edge>) {
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

/// Each node's `is_a` axis, nearest first.
///
/// The walk is pass 5's ([`Checked::ancestors`]) so that a query and a
/// validation can never disagree about what a node is a type of. The dropped
/// set is empty because emission was refused if anything had to be dropped.
fn ancestry(
    checked: &Checked,
    linked: &Linked,
    index: &HashMap<Place, u32>,
    places: &[Place],
) -> Flat<ModelId> {
    let runs = places
        .iter()
        .map(|place| {
            checked
                .ancestors(linked, place.0, place.1)
                .into_iter()
                .filter_map(|base| index.get(&base).map(|id| ModelId(*id)))
                .collect()
        })
        .collect();
    Flat::build(runs)
}

/// Each node's stored `root → scope` path, copied into the image so an access
/// question composes over a slice rather than a tree walk.
fn paths(ctx: &Ctx, places: &[Place]) -> Flat<ScopeId> {
    let runs = places
        .iter()
        .map(|place| ctx.interned.scope_path_of(place.0, place.1).unwrap_or_default().to_vec())
        .collect();
    Flat::build(runs)
}
