// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `is_a` axis, and which nodes are abstract.
//!
//! **Extensions and extended references create ancestry; inclusions do not**
//! (D4.1). `A << B` says *A has a B in it* and creates no is-a relationship, so
//! a mixin's keys are not declarations of A's family and must not be validated
//! against.
//!
//! `!type` is abstract, `!node` concrete, and an untagged node abstract (D7.1).
//! Inheritance across the boundary is unrestricted (D7.2); the only structural
//! restriction is D4.10's acyclicity, whatever the tags on the nodes.

use std::collections::HashSet;

use crate::intern::Interned;
use crate::link::{Direction, EdgeId, EdgeKind, Graph, Stratum};
use crate::tags::TagKind;

use super::view::Place;

/// Whether a node is emitted as a model, and therefore validated.
///
/// `!node` and `!edge` in Yamlfication source, and nothing else (D7.1, D4.13).
/// A tag in a base YAML file classifies as [`TagKind::Other`] (D6.6), so a
/// `.yaml` emits no models of its own by the default arriving at the right
/// answer rather than by a rule of its own.
///
/// Published because pass 6 asks the same question, and a second spelling of it
/// would be a second rule.
#[must_use]
pub fn is_concrete(interned: &Interned, place: Place) -> bool {
    matches!(interned.tag_kind(place.0, place.1), Some(TagKind::Node | TagKind::Edge))
}

/// Whether a node is inheritable-and-never-emitted, which is what makes its
/// keys declarations rather than data.
pub(crate) fn is_abstract(interned: &Interned, place: Place) -> bool {
    !is_concrete(interned, place)
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
