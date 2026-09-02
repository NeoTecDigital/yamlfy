// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether two written values are the same value.
//!
//! `E0214` fires on two extended references contributing one key with
//! *different* values, so the same value contributed twice must be recognised
//! as idempotent and stay legal — `fixtures/cycles/merge-diamond.yml`'s reason,
//! applied to contributions.
//!
//! The comparison is **structural over what is written**, never over a resolved
//! view: nothing is resolved in pass 4. An alias is followed to the node it
//! binds to, so `*water` and the mapping it names compare equal, and two
//! mappings compare by key text rather than by entry order, so re-typing a
//! contribution with its keys in another order is not a conflict.
//!
//! The walk is an explicit stack with a visited set of *pairs*, because the
//! data graph is legally cyclic: `next: *ring` compared against itself must
//! terminate rather than recurse forever. A pair already being compared is
//! assumed equal, which is the standard coinductive reading and the only one
//! that terminates.
//!
//! Two things are deliberately **not** compared. Scalar *style* is not: `'a'`
//! and `"a"` are one value written twice. Anchors are not: a contribution does
//! not become a different value by being given a name.

use std::collections::HashSet;

use yfi_syntax::{Ast, FileId, NodeId};

use super::Ctx;

type Place = (FileId, NodeId);

/// Whether the two nodes write the same value.
pub(crate) fn equal(ctx: &Ctx, left: Place, right: Place) -> bool {
    let mut stack = vec![(left, right)];
    let mut seen: HashSet<(Place, Place)> = HashSet::new();
    while let Some((left, right)) = stack.pop() {
        let pair = (follow(ctx, left), follow(ctx, right));
        if pair.0 == pair.1 || !seen.insert(pair) {
            continue;
        }
        if !shallow(ctx, pair.0, pair.1, &mut stack) {
            return false;
        }
    }
    true
}

/// An alias is a reference to a value, not a value of its own, so it is
/// followed before anything is compared.
fn follow(ctx: &Ctx, place: Place) -> Place {
    let Some(ast) = ctx.ast(place.0) else { return place };
    if ast.alias(place.1).is_none() {
        return place;
    }
    ast.alias_binding(place.1).unwrap_or(place)
}

fn shallow(ctx: &Ctx, left: Place, right: Place, stack: &mut Vec<(Place, Place)>) -> bool {
    let (Some(here), Some(there)) = (ctx.ast(left.0), ctx.ast(right.0)) else { return false };
    if here.tag(left.1) != there.tag(right.1) {
        return false;
    }
    if let (Some(a), Some(b)) = (here.scalar(left.1), there.scalar(right.1)) {
        return a.value == b.value;
    }
    if let (Some(a), Some(b)) = (here.items(left.1), there.items(right.1)) {
        return sequence(a, b, left.0, right.0, stack);
    }
    match (here.entries(left.1), there.entries(right.1)) {
        (Some(_), Some(_)) => mapping(here, there, left, right, stack),
        _ => false,
    }
}

fn sequence(
    here: &[NodeId],
    there: &[NodeId],
    left: FileId,
    right: FileId,
    stack: &mut Vec<(Place, Place)>,
) -> bool {
    if here.len() != there.len() {
        return false;
    }
    for (a, b) in here.iter().zip(there) {
        stack.push(((left, *a), (right, *b)));
    }
    true
}

fn mapping(
    here: &Ast,
    there: &Ast,
    left: Place,
    right: Place,
    stack: &mut Vec<(Place, Place)>,
) -> bool {
    let (Some(ours), Some(theirs)) = (here.entries(left.1), there.entries(right.1)) else {
        return false;
    };
    if ours.len() != theirs.len() {
        return false;
    }
    for entry in ours {
        // A non-scalar key cannot be compared without resolved values, so a
        // mapping holding one is reported as different rather than guessed at.
        let Some(key) = here.scalar(entry.key) else { return false };
        let found = theirs
            .iter()
            .find(|other| there.scalar(other.key).is_some_and(|s| s.value == key.value));
        let Some(other) = found else { return false };
        stack.push(((left.0, entry.value), (right.0, other.value)));
    }
    true
}
