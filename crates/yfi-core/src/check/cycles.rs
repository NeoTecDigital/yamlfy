// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0212` — cyclic inheritance.
//!
//! A cycle in the inheritance graph is an error, not a fixed point (D1.8,
//! D4.10). On key sets alone a least fixed point would exist; on **values** it
//! does not, because left-biased union *chooses* between competing values and
//! choice is not monotone. `fixtures/cycles/merge-oscillating.yml` oscillates
//! with period two under simultaneous iteration and converges to whichever
//! answer the visit order produces under sequential iteration. An
//! order-dependent answer is not a meaning, so the construct is rejected rather
//! than resolved — uniformly, including a one-cycle, because a rule that fired
//! only when the cycle was *observable* would make a document's legality depend
//! on which values happened to collide.
//!
//! # Two things this gets right that are easy to get wrong
//!
//! **The notes name the *forward* edges.** In `A extends: !ref B` with
//! `B << A` the cycle closes through `R(A) → R(B)` and `R(B) → R(A)`; the
//! reverse edge `R(B) → own(A)` ends at a sink and cannot lie on a cycle.
//! Naming it would blame the innocent half while the author is already certain
//! the `!ref` is at fault.
//!
//! **The primary span is the textually first member.** The arena is
//! post-order, so the lowest-indexed member of a component is its
//! deepest-leftmost *leaf* rather than the node written first. Members are
//! ordered by `(file rank, document index, source position)` instead.
//!
//! # What is not an error
//!
//! Cycles in the **data** graph are legal and are the point of the system.
//! Only cycles through inheritance edges are rejected;
//! `fixtures/cycles/alias-cycle-with-merge-dag.yml` is the fixtured case.

use yfi_syntax::{Code, Diagnostic, Diagnostics, Span};

use crate::link::{Ctx, Direction, Edge, Graph, Linked, SourceOrder, VertexId};

use super::names::{display, short, span_of};
use super::scc::Component;
use super::view::Place;

/// The order a node with no recoverable position sorts at: last, so a cycle is
/// never reported without one of its members.
const LAST: SourceOrder = SourceOrder { file: u32::MAX, document: u32::MAX, byte: u32::MAX };

/// One node taking part in a cycle, with the order that decides which is first.
struct Member {
    place: Place,
    order: SourceOrder,
}

/// Report one `E0212` for every cyclic component, in the order the components'
/// first members are written.
pub(crate) fn report(
    ctx: &Ctx,
    linked: &Linked,
    components: &[Component],
    diagnostics: &mut Diagnostics,
) {
    let mut cyclic: Vec<Vec<Member>> = components
        .iter()
        .filter(|held| held.cyclic)
        .map(|held| members(ctx, linked.graph(), &held.vertices))
        .filter(|held| !held.is_empty())
        .collect();
    cyclic.sort_by_key(|held| held[0].order);
    for component in &cyclic {
        diagnostics.push(one(ctx, linked, component));
    }
}

/// Whether any component is cyclic. Compilation fails whenever it is, however
/// far the recovered views carry the later checks.
pub(crate) fn any_cyclic(components: &[Component]) -> bool {
    components.iter().any(|held| held.cyclic)
}

/// Vertices in the project's textual order, which is the order both graph walks
/// start their roots in.
pub(crate) fn walk_order(ctx: &Ctx, graph: &Graph) -> Vec<VertexId> {
    let mut out: Vec<VertexId> = (0..graph.vertices().len())
        .map(|at| VertexId(u32::try_from(at).expect("graph overflow")))
        .collect();
    out.sort_by_key(|id| match graph.vertex(*id) {
        Some(vertex) => (order_of(ctx, (vertex.file, vertex.node)), vertex.stratum as u8),
        None => (LAST, 0),
    });
    out
}

/// The distinct nodes of a component, textually first at the front.
fn members(ctx: &Ctx, graph: &Graph, vertices: &[VertexId]) -> Vec<Member> {
    let mut out: Vec<Member> = Vec::new();
    for vertex in vertices.iter().filter_map(|id| graph.vertex(*id)) {
        let place = (vertex.file, vertex.node);
        if out.iter().any(|held| held.place == place) {
            continue;
        }
        out.push(Member { place, order: order_of(ctx, place) });
    }
    out.sort_by_key(|held| held.order);
    out
}

/// Where a node is written.
fn order_of(ctx: &Ctx, place: Place) -> SourceOrder {
    crate::link::source_order(ctx.project, ctx.interned, place.0, place.1).unwrap_or(LAST)
}

fn one(ctx: &Ctx, linked: &Linked, members: &[Member]) -> Diagnostic {
    let first = display(ctx, linked, members[0].place);
    let mut diagnostic = Diagnostic::new(
        Code::CyclicInheritance,
        span_of(ctx, members[0].place),
        format!(
            "cyclic inheritance: the resolved view of `{first}` depends on itself through {} \
             node(s), and a cycle in the inheritance graph has no fixed point — only an answer \
             chosen by the compiler's visit order",
            members.len()
        ),
    );
    for member in members.iter().skip(1) {
        let name = display(ctx, linked, member.place);
        diagnostic = diagnostic
            .with_note(format!("`{name}` is in the same cycle"), Some(span_of(ctx, member.place)));
    }
    for (note, span) in closing(ctx, linked.graph(), members) {
        diagnostic = diagnostic.with_note(note, Some(span));
    }
    diagnostic
}

/// Every **forward** edge whose ends are both in the component, worded for a
/// note and ordered by where it is written.
fn closing(ctx: &Ctx, graph: &Graph, members: &[Member]) -> Vec<(String, Span)> {
    let inside = |vertex: VertexId| -> bool {
        graph
            .vertex(vertex)
            .is_some_and(|held| members.iter().any(|m| m.place == (held.file, held.node)))
    };
    let mut out: Vec<(String, Span)> = graph
        .edges()
        .iter()
        .filter(|edge| edge.direction == Direction::Forward && inside(edge.from) && inside(edge.to))
        .map(|edge| (wording(ctx, graph, edge), edge.span))
        .collect();
    out.sort_by_key(|held| (held.1.file.0, held.1.start.line, held.1.start.col));
    out
}

/// `closed via <<`, and the file the edge lands in when it leaves this one. The
/// participating spans alone do not say which operator closed the cycle, and a
/// cross-file edge does not say where it goes.
fn wording(ctx: &Ctx, graph: &Graph, edge: &Edge) -> String {
    let kind = edge.kind.as_str();
    match graph.vertex(edge.to).map(|held| held.file) {
        Some(file) if file != edge.span.file => {
            format!("closed via `{kind}`, into `{}`", short(ctx, file))
        }
        _ => format!("closed via `{kind}`"),
    }
}
