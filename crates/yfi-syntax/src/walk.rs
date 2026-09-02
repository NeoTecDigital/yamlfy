// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cycle-safe traversal and a flat rendering of the arena.
//!
//! The node graph may contain cycles, so every traversal here carries a visited
//! set and every renderer prints the node *table*, never a recursive expansion.

use std::collections::HashSet;

use crate::ast::{Ast, NodeId, NodeKind};

impl Ast {
    /// Direct children of `id`: sequence items, and both halves of every
    /// mapping entry. An alias is a leaf here — following it is
    /// [`Ast::alias_target`], and is what makes the graph cyclic.
    #[must_use]
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        match self.node(id).kind {
            NodeKind::Sequence(_) => self.items(id).unwrap_or_default().to_vec(),
            NodeKind::Mapping(_) => self
                .entries(id)
                .unwrap_or_default()
                .iter()
                .flat_map(|e| [e.key, e.value])
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Every node reachable from `start`, following aliases through their
    /// anchors. Terminates on any graph, cyclic or not.
    #[must_use]
    pub fn reachable_from(&self, start: NodeId) -> Vec<NodeId> {
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        let mut order = Vec::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            order.push(id);
            stack.extend(self.children(id));
            stack.extend(self.alias_target(id));
        }
        order
    }

    /// Whether following aliases from `start` can return to `start`.
    #[must_use]
    pub fn is_cyclic_from(&self, start: NodeId) -> bool {
        let mut seen = HashSet::new();
        let mut stack: Vec<NodeId> = self.children(start);
        stack.extend(self.alias_target(start));
        while let Some(id) = stack.pop() {
            if id == start {
                return true;
            }
            if !seen.insert(id) {
                continue;
            }
            stack.extend(self.children(id));
            stack.extend(self.alias_target(id));
        }
        false
    }

    /// A flat, line-per-node rendering of the arena. Deterministic and safe on
    /// cyclic graphs because it never follows an edge.
    #[must_use]
    pub fn dump(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let id = NodeId(u32::try_from(index).unwrap_or(u32::MAX));
            let _ = writeln!(
                out,
                "{index:>4} {:<10} {}:{}{}{}",
                self.describe(id),
                node.span.start.line,
                node.span.start.col,
                self.tag(id).map_or(String::new(), |t| format!(" tag={}{}", t.handle, t.suffix)),
                node.anchor.map_or(String::new(), |a| {
                    let name = self.anchors.get(a).map_or("", |d| &d.name);
                    format!(" anchor=&{name}#{}", a.0)
                }),
            );
        }
        out
    }

    fn describe(&self, id: NodeId) -> String {
        match self.node(id).kind {
            NodeKind::Scalar(_) => "scalar".to_owned(),
            NodeKind::Sequence(_) => {
                format!("seq[{}]", self.items(id).map_or(0, <[NodeId]>::len))
            }
            NodeKind::Mapping(_) => {
                format!("map[{}]", self.entries(id).map_or(0, <[crate::ast::Entry]>::len))
            }
            NodeKind::Alias(_) => {
                let alias = self.alias(id).map_or("", |a| &a.name);
                format!("alias(*{alias})")
            }
        }
    }
}
