// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 6 — emission: the first pass whose output is a shape rather than a
//! verdict.
//!
//! It builds one [`Image`] — a node per resolved holder, marked concrete or
//! abstract (D7.1, D6.6); each node's ancestor chain, because flattening
//! destroys the `is_a` axis; a CSR edge index in both directions over the three
//! operators, the bare capability declaration, data edges and every `!edge`'s
//! `connections` (D4.13); each node's scope path; and the name index a path
//! query resolves against.
//!
//! Emission is refused outright when [`Checked::is_cyclic`] holds, because a
//! recovered view is not a language semantic (D1.8). A refused image holds
//! nothing.
//!
//! Because emission is refused there, the graph pass 6 walks is a **DAG on the
//! inheritance edges**, and the dropped-edge set is empty by construction. That
//! is why [`Checked::ancestors`] has nothing to drop when pass 6 asks it.
//!
//! Data cycles stay legal, so every traversal over the edge index carries a
//! visited set (§0); the index itself is built by one pass with no traversal.
//!
//! The arena is post-order — a collection's `NodeId` exceeds every child's — so
//! the node set is gathered by one forward scan per file. Ordering for anything
//! user-visible is `(file rank, document index, source position)` via
//! [`crate::link::source_order`], never the arena index.
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

mod relations;

use std::collections::{HashMap, HashSet};

use tracing::debug;
use yfi_syntax::{FileId, NodeId};

use crate::check::{self, Checked};
use crate::discover::Project;
use crate::edge;
use crate::image::{Flat, Image, Model, ModelId, ModelKind};
use crate::intern::Interned;
use crate::link::{source_order, Ctx, Linked, SourceOrder};
use crate::scope::ScopeId;

use relations::{csr, raw_edges, resolved_edges, RawEdge};

/// A node of the project, before it has an id.
pub(crate) type Place = (FileId, NodeId);

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
    if checked.is_cyclic() {
        debug!("emission refused: the inheritance graph held a cycle");
        return refused(project, interned, linked, checked);
    }
    let ctx = Ctx { project, interned };
    let raw = raw_edges(&ctx, linked, checked);
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
        refused: false,
    }
}

/// An image holding nothing, which is what a cyclic project emits.
fn refused<'a>(
    project: &'a Project,
    interned: &'a Interned,
    linked: &'a Linked,
    checked: &'a Checked,
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
        refused: true,
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
    source_order(ctx.project, ctx.interned, place.0, place.1).unwrap_or(SourceOrder {
        file: u32::MAX,
        document: u32::MAX,
        byte: u32::MAX,
    })
}

/// One record per node, in id order.
fn models(ctx: &Ctx, places: &[Place]) -> Vec<Model> {
    places
        .iter()
        .map(|place| Model {
            place: *place,
            kind: kind_of(ctx, *place),
            scope: ctx
                .interned
                .scope_of(place.0, place.1)
                .or_else(|| ctx.project.scopes().root())
                .unwrap_or(ScopeId(0)),
            span: ctx.span_of(place.0, place.1),
            order: order_of(ctx, *place),
        })
        .collect()
}

/// What a node is: an edge, some other concrete node, or abstract. The
/// concrete/abstract question is [`check::is_concrete`]'s and is not restated;
/// only the edge case is added, and it is a *refinement* of concrete rather
/// than a third answer to the same question (D4.13, D7.1).
fn kind_of(ctx: &Ctx, place: Place) -> ModelKind {
    if edge::is_edge(ctx.interned, place.0, place.1) {
        return ModelKind::Edge;
    }
    match check::is_concrete(ctx.interned, place) {
        true => ModelKind::Concrete,
        false => ModelKind::Abstract,
    }
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
