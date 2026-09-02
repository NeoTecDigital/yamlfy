// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The unified, stratified inheritance graph.
//!
//! All three operations are edges of **one** graph, and the cycle rule runs
//! over the union (D4.10). Take `A << B` with `B extends: A`: neither mechanism
//! contains a cycle by itself and two independent checks both pass, but
//! resolution does not run on two graphs — `R(A)` needs `R(B)` needs `R(A)`,
//! and the value oscillates. Separate rules would admit a construct whose
//! meaning depends on the compiler's visit order.
//!
//! # Two vertices per node
//!
//! * `own(N)` — N's literal keys with clauses removed. **No outgoing edges.**
//! * `R(N)` — N's resolved view.
//!
//! | from | to | source |
//! |---|---|---|
//! | `R(A)` | `R(B)` | `A << B` |
//! | `R(A)` | `R(B)` | `A extends: B` |
//! | `R(A)` | `R(B)` | `A extends: !ref B` — A is a type of B |
//! | `R(B)` | `own(A)` | `A extends: !ref B` — **B depends on A** |
//! | `R(B)` | `own(A)` | any other `!ref` in A — the same declaration, no keys |
//!
//! Because `own` vertices are sinks, a reverse edge can never lie on a cycle,
//! so SCC over this graph accepts every legal extended reference and still
//! finds every genuine cycle. That is not a convenience of the encoding: it is
//! D4.5's `own(A)`-not-`R(A)` rule, which had to hold anyway, showing up as the
//! property that makes the analysis decidable.
//!
//! # Why every edge records its kind
//!
//! In `A extends: !ref B` plus `B << A` the cycle closes through the two
//! **forward** edges; the reverse edge cannot participate. `E0212` must name
//! the forward edges and their kinds, or it blames the innocent half while the
//! author is certain the `!ref` is at fault.
//!
//! # Representation
//!
//! Edges are stored once, in insertion order, and indexed by source vertex as
//! CSR — a `Vec<EdgeId>` grouped by source with an offsets table. Tarjan wants
//! `out_edges(v)` as a contiguous slice and nothing else; a `Vec<Vec<_>>` would
//! allocate once per vertex, and an adjacency map would lose the deterministic
//! edge order that makes D1.8's back-edge recovery reproducible.

use std::collections::HashMap;

use yfi_syntax::{FileId, NodeId, Span};

use super::clause::{Clause, ClauseKind, Operand, OperandForm};
use super::refs::{RefRole, Reference};

/// Which of a node's two vertices this is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stratum {
    /// `own(N)` — the node's literal keys, clauses removed. A **sink**.
    Own,
    /// `R(N)` — the node's resolved view.
    Resolved,
}

/// Handle to a vertex.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct VertexId(pub u32);

impl VertexId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Handle to an edge.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EdgeId(pub u32);

impl EdgeId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One vertex: a node and which of its two strata this is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vertex {
    /// The file the node belongs to.
    pub file: FileId,
    /// The node.
    pub node: NodeId,
    /// Which stratum.
    pub stratum: Stratum,
}

/// Which operation contributed an edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    /// `<<` — inclusion, whether the operand was an alias or a `!ref`.
    Inclusion,
    /// `extends:` with an alias or an inline mapping.
    Extension,
    /// `extends: !ref` — the operation that installs `own(A)` on its operand.
    ExtendedReference,
    /// `!ref` written anywhere else. It contributes no keys, but it declares
    /// the same dependency direction — the target depends on this context — so
    /// it contributes the same reverse edge, into the same sink.
    Capability,
}

impl EdgeKind {
    /// The wording `E0212`'s notes use: `via <<`, `via extends`,
    /// `via extends !ref`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Inclusion => "<<",
            EdgeKind::Extension => "extends",
            EdgeKind::ExtendedReference => "extends !ref",
            EdgeKind::Capability => "!ref",
        }
    }
}

/// Which way an edge runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Child → base: resolving the source requires resolving the target.
    Forward,
    /// Base → `own` of the extending node. Terminates at a sink and can never
    /// lie on a cycle, so it must never be named as a cycle's cause.
    Reverse,
}

/// One inheritance edge.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    /// Where it starts.
    pub from: VertexId,
    /// Where it ends.
    pub to: VertexId,
    /// Which operation contributed it.
    pub kind: EdgeKind,
    /// Forward or reverse.
    pub direction: Direction,
    /// The operand that names the target.
    pub span: Span,
    /// The clause key that wrote the operator.
    pub site: Span,
}

/// The stratified inheritance graph.
#[derive(Default)]
pub struct Graph {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    /// Node to the index of its `own` vertex; `R` is the next one along, so
    /// the pair is always adjacent and one lookup answers both.
    index: HashMap<(FileId, NodeId), u32>,
    offsets: Vec<u32>,
    outgoing: Vec<EdgeId>,
}

impl Graph {
    /// Every vertex.
    #[must_use]
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Look up a vertex.
    #[must_use]
    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(id.index())
    }

    /// Every edge, in insertion order.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Look up an edge.
    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id.index())
    }

    /// The vertex for one stratum of a node, if the node is in the graph.
    #[must_use]
    pub fn vertex_of(&self, file: FileId, node: NodeId, stratum: Stratum) -> Option<VertexId> {
        let own = *self.index.get(&(file, node))?;
        Some(match stratum {
            Stratum::Own => VertexId(own),
            Stratum::Resolved => VertexId(own + 1),
        })
    }

    /// The edges leaving `vertex`, in insertion order. Empty for every `own`
    /// vertex, which is the whole point of the stratification.
    #[must_use]
    pub fn out_edges(&self, vertex: VertexId) -> &[EdgeId] {
        let at = vertex.index();
        let Some(end) = at.checked_add(1).and_then(|next| self.offsets.get(next)) else {
            return &[];
        };
        &self.outgoing[self.offsets[at] as usize..*end as usize]
    }

    /// Both vertices of a node, creating them if the node is new.
    fn pair(&mut self, file: FileId, node: NodeId) -> (VertexId, VertexId) {
        let own = *self.index.entry((file, node)).or_insert_with(|| {
            let at = u32::try_from(self.vertices.len()).expect("graph overflow");
            self.vertices.push(Vertex { file, node, stratum: Stratum::Own });
            self.vertices.push(Vertex { file, node, stratum: Stratum::Resolved });
            at
        });
        (VertexId(own), VertexId(own + 1))
    }

    fn push_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    /// Group the edges by source vertex into CSR.
    fn finish(&mut self) {
        let count = self.vertices.len();
        let mut offsets = vec![0u32; count + 1];
        for edge in &self.edges {
            offsets[edge.from.index() + 1] += 1;
        }
        for at in 1..=count {
            offsets[at] += offsets[at - 1];
        }
        let mut cursor = offsets.clone();
        let mut outgoing = vec![EdgeId(0); self.edges.len()];
        for (at, edge) in self.edges.iter().enumerate() {
            let slot = &mut cursor[edge.from.index()];
            outgoing[*slot as usize] = EdgeId(u32::try_from(at).expect("graph overflow"));
            *slot += 1;
        }
        self.offsets = offsets;
        self.outgoing = outgoing;
    }
}

/// Build the graph from every validated clause, and from every `!ref` written
/// outside one.
///
/// A `!ref` in a data position installs no keys, so it is not a clause and
/// contributes no forward edge — but it declares that its target depends on the
/// context that wrote it, which is the same direction an extended reference
/// declares. It therefore contributes the same reverse edge into the same
/// `own` sink. Reverse edges cannot lie on a cycle, so recording them cannot
/// make `E0212` fire on a construct that is not cyclic; leaving them out would
/// make the graph disagree with the language about what depends on what.
pub(crate) fn build(clauses: &[Clause], references: &[Reference]) -> Graph {
    let mut graph = Graph::default();
    for clause in clauses {
        for operand in &clause.operands {
            add(&mut graph, clause, operand);
        }
    }
    for reference in references.iter().filter(|held| held.capability) {
        capability(&mut graph, reference);
    }
    graph.finish();
    graph
}

/// The reverse edge a `!ref` outside an `extends:` clause contributes.
fn capability(graph: &mut Graph, reference: &Reference) {
    if reference.role == RefRole::Extension {
        return;
    }
    let Some(target) = reference.target else { return };
    let Some(owner) = reference.owner else { return };
    let (own, _) = graph.pair(reference.file, owner);
    let (_, base) = graph.pair(target.0, target.1);
    graph.push_edge(Edge {
        from: base,
        to: own,
        kind: EdgeKind::Capability,
        direction: Direction::Reverse,
        span: reference.span,
        site: reference.span,
    });
}

fn add(graph: &mut Graph, clause: &Clause, operand: &Operand) {
    let kind = match (clause.kind, operand.form) {
        (ClauseKind::Inclusion, _) => EdgeKind::Inclusion,
        (ClauseKind::Extension, OperandForm::Ref) => EdgeKind::ExtendedReference,
        (ClauseKind::Extension, _) => EdgeKind::Extension,
    };
    let (own, resolved) = graph.pair(clause.file, clause.owner);
    let (_, base) = graph.pair(operand.target.0, operand.target.1);
    let edge = Edge {
        from: resolved,
        to: base,
        kind,
        direction: Direction::Forward,
        span: operand.span,
        site: clause.site,
    };
    graph.push_edge(edge);
    if kind == EdgeKind::ExtendedReference {
        graph.push_edge(Edge { from: base, to: own, direction: Direction::Reverse, ..edge });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yfi_syntax::Pos;

    fn span() -> Span {
        Span::empty(FileId(0), Pos { byte: 0, line: 1, col: 1 })
    }

    fn clause(owner: u32, target: u32, kind: ClauseKind, form: OperandForm) -> Clause {
        Clause {
            file: FileId(0),
            owner: NodeId(owner),
            kind,
            site: span(),
            operands: vec![Operand {
                node: NodeId(target),
                form,
                target: (FileId(0), NodeId(target)),
                span: span(),
            }],
        }
    }

    #[test]
    fn a_node_gets_both_of_its_vertices() {
        let graph = build(&[clause(1, 2, ClauseKind::Inclusion, OperandForm::Alias)], &[]);
        for node in [1u32, 2] {
            let own = graph.vertex_of(FileId(0), NodeId(node), Stratum::Own).expect("own");
            let resolved = graph.vertex_of(FileId(0), NodeId(node), Stratum::Resolved).expect("R");
            assert_eq!(graph.vertex(own).expect("vertex").stratum, Stratum::Own);
            assert_eq!(graph.vertex(resolved).expect("vertex").stratum, Stratum::Resolved);
        }
        assert_eq!(graph.vertices().len(), 4);
    }

    #[test]
    fn an_extended_reference_adds_a_forward_edge_and_a_reverse_edge_to_a_sink() {
        let graph = build(&[clause(1, 2, ClauseKind::Extension, OperandForm::Ref)], &[]);
        assert_eq!(graph.edges().len(), 2);
        let own = graph.vertex_of(FileId(0), NodeId(1), Stratum::Own).expect("own");
        let reverse = graph
            .edges()
            .iter()
            .find(|edge| edge.direction == Direction::Reverse)
            .expect("a reverse edge");
        assert_eq!(reverse.to, own, "a reverse edge terminates at an `own` vertex");
        assert!(graph.out_edges(own).is_empty(), "`own` is a sink");
    }

    #[test]
    fn csr_groups_every_edge_under_the_vertex_it_leaves() {
        let graph = build(
            &[
                clause(1, 2, ClauseKind::Inclusion, OperandForm::Alias),
                clause(1, 3, ClauseKind::Extension, OperandForm::Alias),
            ],
            &[],
        );
        let from = graph.vertex_of(FileId(0), NodeId(1), Stratum::Resolved).expect("R");
        let kinds: Vec<EdgeKind> = graph
            .out_edges(from)
            .iter()
            .filter_map(|id| graph.edge(*id))
            .map(|edge| edge.kind)
            .collect();
        assert_eq!(kinds, [EdgeKind::Inclusion, EdgeKind::Extension]);
        assert_eq!(graph.out_edges(VertexId(u32::MAX)).len(), 0);
    }
}
