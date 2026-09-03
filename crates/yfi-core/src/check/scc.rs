// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tarjan's algorithm and back-edge removal over the stratified graph.
//!
//! Both walks are **iterative**. An inheritance chain is as deep as an author
//! writes it and a recursive walk would turn a deep chain into a stack
//! overflow, which is a crash where the pass owes a diagnostic.
//!
//! Both walks also start their roots in the project's **textual** total order,
//! `(file rank, document index, source position)`. That is not cosmetic: which
//! back edge the recovery drops decides which resolved views the later checks
//! see, and therefore which *other* diagnostics get reported. Ordered by
//! anything the filesystem chooses, the same tree would report a different set
//! on two machines.
//!
//! Neither function reports anything. `E0212`'s wording, its primary span and
//! its notes are [`super::cycles`]'s, so the algorithm can be tested for the
//! shapes it finds without a project behind it.

use std::collections::HashSet;

use crate::link::{EdgeId, Graph, VertexId};

/// One strongly connected component, in discovery order of its members.
pub(crate) struct Component {
    /// Its vertices.
    pub(crate) vertices: Vec<VertexId>,
    /// Whether it is a cycle: more than one member, or a lone vertex with an
    /// edge to itself. A one-cycle is an error under D1.8's uniform rule even
    /// when it is a no-op, because a rule that fires only on *observable*
    /// cycles makes legality depend on which values happened to collide.
    pub(crate) cyclic: bool,
}

/// One frame of an explicit depth-first walk.
struct Frame {
    vertex: VertexId,
    next: usize,
}

/// Tarjan's algorithm over `graph`, visiting roots in `order`.
pub(crate) fn components(graph: &Graph, order: &[VertexId]) -> Vec<Component> {
    let mut run = Tarjan {
        graph,
        index: vec![u32::MAX; graph.vertices().len()],
        low: vec![u32::MAX; graph.vertices().len()],
        on_stack: vec![false; graph.vertices().len()],
        counter: 0,
        stack: Vec::new(),
        frames: Vec::new(),
        out: Vec::new(),
    };
    for root in order {
        run.from(*root);
    }
    run.out
}

struct Tarjan<'a> {
    graph: &'a Graph,
    index: Vec<u32>,
    low: Vec<u32>,
    on_stack: Vec<bool>,
    counter: u32,
    stack: Vec<VertexId>,
    frames: Vec<Frame>,
    out: Vec<Component>,
}

impl Tarjan<'_> {
    fn from(&mut self, root: VertexId) {
        if self.index[root.index()] != u32::MAX {
            return;
        }
        self.open(root);
        while let Some(frame) = self.frames.last() {
            let (vertex, at) = (frame.vertex, frame.next);
            match self.graph.out_edges(vertex).get(at) {
                Some(edge) => self.descend(vertex, *edge),
                None => self.close(vertex),
            }
        }
    }

    fn open(&mut self, vertex: VertexId) {
        self.index[vertex.index()] = self.counter;
        self.low[vertex.index()] = self.counter;
        self.counter += 1;
        self.on_stack[vertex.index()] = true;
        self.stack.push(vertex);
        self.frames.push(Frame { vertex, next: 0 });
    }

    fn descend(&mut self, vertex: VertexId, edge: EdgeId) {
        if let Some(frame) = self.frames.last_mut() {
            frame.next += 1;
        }
        let Some(target) = self.graph.edge(edge).map(|held| held.to) else { return };
        if self.index[target.index()] == u32::MAX {
            self.open(target);
            return;
        }
        if self.on_stack[target.index()] {
            let seen = self.index[target.index()];
            self.low[vertex.index()] = self.low[vertex.index()].min(seen);
        }
    }

    fn close(&mut self, vertex: VertexId) {
        self.frames.pop();
        if let Some(parent) = self.frames.last() {
            let at = parent.vertex.index();
            self.low[at] = self.low[at].min(self.low[vertex.index()]);
        }
        if self.low[vertex.index()] != self.index[vertex.index()] {
            return;
        }
        self.emit(vertex);
    }

    /// Pop one component off the stack, down to and including `root`.
    fn emit(&mut self, root: VertexId) {
        let mut vertices = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack[member.index()] = false;
            vertices.push(member);
            if member == root {
                break;
            }
        }
        let cyclic = vertices.len() > 1 || self.has_self_loop(root);
        self.out.push(Component { vertices, cyclic });
    }

    fn has_self_loop(&self, vertex: VertexId) -> bool {
        self.graph
            .out_edges(vertex)
            .iter()
            .filter_map(|edge| self.graph.edge(*edge))
            .any(|edge| edge.to == vertex)
    }
}

/// Every edge a depth-first walk in `order` finds pointing back at a vertex
/// still on its own stack.
///
/// Dropping exactly these makes the graph acyclic (D1.8's recovery), so every
/// node has a defined resolved view and the later checks can report their own
/// findings instead of cascading.
pub(crate) fn back_edges(graph: &Graph, order: &[VertexId]) -> HashSet<EdgeId> {
    let mut walk = Walk {
        graph,
        state: vec![State::Unseen; graph.vertices().len()],
        frames: Vec::new(),
        dropped: HashSet::new(),
    };
    for root in order {
        walk.from(*root);
    }
    walk.dropped
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unseen,
    Open,
    Done,
}

struct Walk<'a> {
    graph: &'a Graph,
    state: Vec<State>,
    frames: Vec<Frame>,
    dropped: HashSet<EdgeId>,
}

impl Walk<'_> {
    fn from(&mut self, root: VertexId) {
        if self.state[root.index()] != State::Unseen {
            return;
        }
        self.state[root.index()] = State::Open;
        self.frames.push(Frame { vertex: root, next: 0 });
        while let Some(frame) = self.frames.last() {
            let (vertex, at) = (frame.vertex, frame.next);
            match self.graph.out_edges(vertex).get(at).copied() {
                Some(edge) => self.step(edge),
                None => {
                    self.state[vertex.index()] = State::Done;
                    self.frames.pop();
                }
            }
        }
    }

    fn step(&mut self, edge: EdgeId) {
        if let Some(frame) = self.frames.last_mut() {
            frame.next += 1;
        }
        let Some(target) = self.graph.edge(edge).map(|held| held.to) else { return };
        match self.state[target.index()] {
            State::Open => {
                self.dropped.insert(edge);
            }
            State::Unseen => {
                self.state[target.index()] = State::Open;
                self.frames.push(Frame { vertex: target, next: 0 });
            }
            State::Done => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Stratum;
    use yfi_syntax::{FileId, NodeId};

    /// Every vertex, lowest first — enough of a total order for a unit test.
    fn order(graph: &Graph) -> Vec<VertexId> {
        (0..graph.vertices().len()).map(|at| VertexId(u32::try_from(at).expect("index"))).collect()
    }

    fn resolved(graph: &Graph, node: u32) -> VertexId {
        graph.vertex_of(FileId(0), NodeId(node), Stratum::Resolved).expect("R")
    }

    fn graph_of(edges: &[(u32, u32, bool)]) -> Graph {
        crate::check::testing::graph(edges)
    }

    #[test]
    fn a_two_cycle_of_forward_edges_is_one_component() {
        let graph = graph_of(&[(1, 2, false), (2, 1, false)]);
        let found: Vec<usize> = components(&graph, &order(&graph))
            .iter()
            .filter(|held| held.cyclic)
            .map(|held| held.vertices.len())
            .collect();
        assert_eq!(found, [2]);
    }

    #[test]
    fn an_extended_reference_is_not_a_cycle_because_its_reverse_edge_ends_at_a_sink() {
        // Built the obvious way — one vertex per node — this is a two-cycle and
        // the checker hallucinates one on every use of the feature.
        let graph = graph_of(&[(1, 2, true)]);
        assert!(components(&graph, &order(&graph)).iter().all(|held| !held.cyclic));
        assert!(back_edges(&graph, &order(&graph)).is_empty());
    }

    #[test]
    fn a_self_loop_is_a_cycle_even_though_it_is_a_no_op() {
        let graph = graph_of(&[(1, 1, false)]);
        let cyclic = components(&graph, &order(&graph)).iter().filter(|held| held.cyclic).count();
        assert_eq!(cyclic, 1);
    }

    #[test]
    fn recovery_drops_one_edge_per_cycle_and_leaves_a_dag() {
        let graph = graph_of(&[(1, 2, false), (2, 3, false), (3, 1, false)]);
        let dropped = back_edges(&graph, &order(&graph));
        assert_eq!(dropped.len(), 1);
        let survivor = graph
            .edges()
            .iter()
            .enumerate()
            .filter(|(at, _)| !dropped.contains(&EdgeId(u32::try_from(*at).expect("index"))))
            .count();
        assert_eq!(survivor, 2);
        assert_ne!(resolved(&graph, 1), resolved(&graph, 2));
    }
}
