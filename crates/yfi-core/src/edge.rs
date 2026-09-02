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
//! an edge that inherits its connections from an `!type`d edge family has them.
//! That is not a rule about edges; it is what extension already means.
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

use yfi_syntax::{FileId, NodeId};

use crate::intern::Interned;
use crate::tags::TagKind;

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

/// Whether the node at `(file, node)` may declare `connections` — that is,
/// whether it is an edge or the **abstract** form of one.
///
/// There is no second tag for an abstract edge: `!edge` is concrete exactly as
/// `!node` is, so a family that fixes its endpoints once is a `!type` that
/// declares `connections`, and a concrete `!edge` extending it supplies none of
/// its own. Asking only [`is_edge`] here would leave that family's items
/// resolved to nothing — the member is inherited, its endpoints are not, and
/// the edge silently relates nobody.
#[must_use]
pub fn declares_connections(interned: &Interned, file: FileId, node: NodeId) -> bool {
    matches!(interned.tag_kind(file, node), Some(TagKind::Edge | TagKind::Type))
}
