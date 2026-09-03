// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What `!edge` is, in one place (D4.13).
//!
//! **An edge is a node.** It has identity, it is addressable by the path
//! syntax, it is extended by the same three operators, and it is validated by
//! the same checks. There is one set of rules and this module adds no second
//! construct — it names the two members the language owns on such a node and
//! nothing else:
//!
//! * **`connections`** — a sequence of the nodes the edge relates. It is what
//!   makes the node an edge, so an `!edge` that holds none is `E0223`. The
//!   sequence is **n-ary**: a three-way edge is one edge, never three binary
//!   ones, and nothing anywhere assumes two endpoints.
//! * **`definition`** — optional, a mapping of **handles**: a name for a
//!   position in `connections`, so an endpoint can be addressed as `source`
//!   rather than as `0`. A handle that names no position is `E0225`.
//!
//! Both are read from the node's **resolved** view, not from its own keys, so
//! an edge that inherits its connections from a base has them. That is not a
//! rule about edges; it is what extension already means.
//!
//! # Everything else on an edge is an ordinary member
//!
//! "Nothing but `connections[]`" says what makes a node an edge, not what a
//! node may hold. An edge sitting between two nodes and carrying its own
//! members is **middleware**, and it is reachable here precisely because
//! nothing precludes it: the members are members, the validation is D7.3's, and
//! no feature had to be added to allow it.
//!
//! # The two names are the language's, not the family's
//!
//! `connections` and `definition` on an `!edge` are written by the language,
//! so they are exempt from `W0301`. Without the exemption an edge extending any
//! abstract family that does not itself declare them would be warned about for
//! writing the two members the tag requires it to write.
//!
//! # `connections` is not a reserved word
//!
//! It is a reach position on the nodes an `!edge` **reads it from**, and an
//! ordinary member name everywhere else. Which those nodes are is not a
//! property of the holder's tag — see [`endpoint_holders`].

use std::collections::{HashMap, HashSet};

use yfi_syntax::{FileId, NodeId};

use crate::intern::Interned;
use crate::link::{Clause, ClauseKind, OperandForm};
use crate::tags::TagKind;

/// A node of the project.
type Place = (FileId, NodeId);

/// The member that carries an edge's endpoints.
pub const CONNECTIONS: &str = "connections";

/// The member that names positions in [`CONNECTIONS`].
pub const DEFINITION: &str = "definition";

/// Whether `name` is one of the two members the language owns on an edge.
#[must_use]
pub fn is_reserved_member(name: &str) -> bool {
    name == CONNECTIONS || name == DEFINITION
}

/// Whether the node at `(file, node)` is an `!edge`.
///
/// In base YAML the tag vocabulary is not interpreted (D6.6), so pass 3 has
/// already classified every tag there as [`TagKind::Other`] and this is false
/// for every node of a `.yaml`.
#[must_use]
pub fn is_edge(interned: &Interned, file: FileId, node: NodeId) -> bool {
    interned.tag_kind(file, node) == Some(TagKind::Edge)
}

/// Every node some `!edge` reads a `connections` member **from**.
///
/// # Why this is not a question about the holder's tag
///
/// A `connections` item is a reach, so pass 4 has to decide — while it is
/// looking at the item — whether the scalar beside it names a node or is a
/// string. Asking the holder's tag cannot answer that, in either direction:
///
/// * an edge inherits `connections` from an **untagged mixin** or a `!node`
///   base, which is D7.1's ordinary form and carries no tag saying so. Reading
///   those items as data leaves every one of them resolved to nothing, and the
///   edge relates nobody with no diagnostic to say why;
/// * widening the test to `!type` makes `connections` a **reserved word on
///   every `!type` in the language**, with no escape — quoting does not help,
///   because there is no prefix in that position for a quote to escape. A
///   router type listing `["eth0", "eth1"]` is then two unresolved paths.
///
/// Reach-ness belongs to the **consumer**: a `connections` member is an edge's
/// endpoints exactly when an `!edge` ends up holding it. That is a question
/// about the inheritance relation, so it is answered from the relation — every
/// node reachable from an `!edge` by walking the contribution edges of D4.7
/// backwards, which is what "this key can arrive in an edge's resolved view"
/// means. A node no `!edge` inherits from keeps `connections` as an ordinary
/// member name, and nothing about it is reserved.
///
/// Both directions of the tier-5 edge are followed: `X extends: !ref A`
/// contributes `own(X)` **to** A, so an edge A reads X's keys as well as its
/// own bases'.
#[must_use]
pub(crate) fn endpoint_holders(interned: &Interned, clauses: &[Clause]) -> HashSet<Place> {
    let mut sources: HashMap<Place, Vec<Place>> = HashMap::new();
    let mut stack: Vec<Place> = Vec::new();
    for clause in clauses {
        contributions(clause, &mut sources);
        seed(interned, clause, &mut stack);
    }
    let mut out: HashSet<Place> = HashSet::new();
    while let Some(at) = stack.pop() {
        let Some(from) = sources.get(&at) else { continue };
        stack.extend(from.iter().copied().filter(|source| out.insert(*source)));
    }
    out
}

/// Record which nodes contribute members to which, for one clause.
fn contributions(clause: &Clause, sources: &mut HashMap<Place, Vec<Place>>) {
    let owner = (clause.file, clause.owner);
    for operand in &clause.operands {
        sources.entry(owner).or_default().push(operand.target);
        if clause.kind == ClauseKind::Extension && operand.form == OperandForm::Ref {
            sources.entry(operand.target).or_default().push(owner);
        }
    }
}

/// Every `!edge` this clause mentions, as a starting point for the walk.
fn seed(interned: &Interned, clause: &Clause, stack: &mut Vec<Place>) {
    let owner = (clause.file, clause.owner);
    if is_edge(interned, owner.0, owner.1) {
        stack.push(owner);
    }
    for operand in &clause.operands {
        if is_edge(interned, operand.target.0, operand.target.1) {
            stack.push(operand.target);
        }
    }
}
