// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `is_a` axis, and which nodes are abstract.
//!
//! **Extensions and extended references create ancestry; inclusions do not.**
//! `A << B` says *A has a B in it* — it creates no is-a relationship and no
//! query over the `is_a` axis will ever return A for B. A node that includes
//! `water` is not a water, so a mixin's keys are not declarations of A's family
//! and must not be validated against.
//!
//! # What is abstract
//!
//! `!type` is abstract, `!node` is concrete, and **an untagged node is
//! abstract**. The corpus supplies the argument for the third: `fixtures/merge/*`
//! and `fixtures/cycles/merge-diamond.yml` are built out of untagged anchored
//! mappings that exist solely to be merged into something else. If untagged
//! defaulted to concrete, every one of them would emit junk models and the
//! ordinary act of factoring shared keys into a named mapping would pollute the
//! graph.
//!
//! Inheritance across the boundary is **unrestricted** (D7.2): `!node` may
//! extend `!node` and `!type` may extend `!node`. This is a prototypal model,
//! not a class-based one, and the only structural restriction is D4.10's — the
//! graph must be acyclic, whatever the tags on its nodes.

use std::collections::HashSet;

use crate::link::{Ctx, Direction, EdgeId, EdgeKind, Graph, Stratum};
use crate::tags::TagKind;

use super::view::Place;

/// Whether a node is emitted as a model, and therefore validated.
pub(crate) fn is_concrete(ctx: &Ctx, place: Place) -> bool {
    ctx.interned.tag_kind(place.0, place.1) == Some(TagKind::Node)
}

/// Whether a node is inheritable-and-never-emitted, which is what makes its
/// keys declarations rather than data.
pub(crate) fn is_abstract(ctx: &Ctx, place: Place) -> bool {
    !is_concrete(ctx, place)
}

/// Every node on `place`'s `is_a` axis, nearest first, following no dropped
/// edge and visiting each ancestor once however many paths reach it.
pub(crate) fn ancestors(graph: &Graph, dropped: &HashSet<EdgeId>, place: Place) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut seen: HashSet<Place> = HashSet::from([place]);
    let mut frontier = vec![place];
    while let Some(held) = frontier.pop() {
        for base in bases(graph, dropped, held) {
            if !seen.insert(base) {
                continue;
            }
            out.push(base);
            frontier.push(base);
        }
    }
    out
}

/// The nodes one node directly claims to be a type of.
fn bases(graph: &Graph, dropped: &HashSet<EdgeId>, place: Place) -> Vec<Place> {
    let Some(from) = graph.vertex_of(place.0, place.1, Stratum::Resolved) else {
        return Vec::new();
    };
    graph
        .out_edges(from)
        .iter()
        .filter(|id| !dropped.contains(id))
        .filter_map(|id| graph.edge(*id))
        .filter(|edge| edge.direction == Direction::Forward)
        .filter(|edge| edge.kind != EdgeKind::Inclusion)
        .filter_map(|edge| graph.vertex(edge.to).map(|held| (held.file, held.node)))
        .collect()
}
