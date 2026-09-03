// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What `!edge` is, in one place (D4.13).
//!
//! **An edge is a node** — same identity, same path syntax, same three
//! operators, same checks. This module names the two members the language owns
//! on such a node, `connections` and `definition`, and adds no second
//! construct. Both are read from the node's **resolved** view rather than from
//! its own keys, which is not a rule about edges but what extension already
//! means.
//!
//! `connections` is not a reserved word. It is a reach position on the nodes an
//! `!edge` **reads it from**, and which those are is not a property of the
//! holder's tag — see [`endpoint_holders`]. What pass 4 is finally handed is
//! one step past that: the **sequences** those members name, because the value
//! may be an alias and the items are written wherever the alias points — see
//! [`endpoint_sequences`].

use std::collections::{HashMap, HashSet};

use yfi_syntax::{FileId, NodeId};

use crate::intern::Interned;
use crate::link::{Clause, ClauseKind, Ctx, OperandForm};
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

/// Every sequence whose items an `!edge` reads as its endpoints.
///
/// This, and not [`endpoint_holders`], is what pass 4 asks: the reach position
/// is the **sequence**, not the key, so the items an edge relates may be
/// written in a file that holds no edge and names no `connections` at all.
///
/// A holder in a **base YAML** file contributes nothing: nothing there is a
/// reach (D6.6), and an edge that ends up with such a member is `E0223`.
pub(crate) fn endpoint_sequences(ctx: &Ctx, clauses: &[Clause]) -> HashSet<Place> {
    let inherited = endpoint_holders(ctx.interned, clauses);
    tagged(ctx)
        .chain(inherited.into_iter().filter(|held| ctx.is_source(held.0)))
        .filter_map(|holder| sequence_of(ctx, holder))
        .collect()
}

/// Every `!edge` node the project writes.
///
/// Walked rather than derived from the clauses, because the commonest edge of
/// all inherits nothing and therefore appears in no clause.
fn tagged<'a>(ctx: &'a Ctx<'a>) -> impl Iterator<Item = Place> + 'a {
    ctx.project
        .files()
        .iter()
        .filter(|file| ctx.is_source(file.id))
        .flat_map(|file| {
            (0..file.ast.nodes().len())
                .map(|at| (file.id, NodeId(u32::try_from(at).expect("arena overflow"))))
        })
        .filter(|held| is_edge(ctx.interned, held.0, held.1))
}

/// The sequence one holder's `connections` names, or `None` when it names none.
fn sequence_of(ctx: &Ctx, holder: Place) -> Option<Place> {
    let ast = ctx.ast(holder.0)?;
    let entries = ast.entries(holder.1)?;
    let entry = entries.iter().find(|entry| names_connections(ctx, holder.0, entry.key))?;
    let at = dereference(ctx, (holder.0, entry.value));
    ctx.ast(at.0)?.items(at.1).map(|_| at)
}

/// Whether a key names the member that carries an edge's endpoints. Read from
/// the interned member name, so a `pub connections:` is the same member as a
/// bare one.
fn names_connections(ctx: &Ctx, file: FileId, key: NodeId) -> bool {
    ctx.interned.key_of(file, key).and_then(|name| ctx.interned.symbols().resolve(name))
        == Some(CONNECTIONS)
}

/// Follow an alias to the node it binds; answer any other node unchanged.
///
/// One hop is the whole chain: YAML does not let an alias node carry an anchor,
/// so nothing an alias binds to is itself an alias.
///
/// **Total**, and that is the point rather than a convenience. An alias binding
/// nothing answers the alias node itself, so a caller reading the answer's
/// shape sees the node the author wrote and says what is wrong with it; an
/// `Option` here would let a dereference that failed become a member nobody
/// mentions, which is the silence every rule in this module exists to remove.
pub(crate) fn dereference(ctx: &Ctx, at: Place) -> Place {
    let Some(ast) = ctx.ast(at.0) else { return at };
    if ast.alias(at.1).is_none() {
        return at;
    }
    ast.alias_binding(at.1).unwrap_or(at)
}

/// Every node some `!edge` reads a `connections` member **from**.
///
/// Reach-ness belongs to the **consumer**, not to the holder's tag (D4.13), so
/// this is the reverse closure of D4.7's contribution edges beginning at every
/// `!edge`. Tier 5 is followed in both directions: `X extends: !ref A`
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
