// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0223`, `E0224` and `E0225` — reading what an `!edge` node relates (D4.13).
//!
//! An edge is a node, so this pass adds no construct: it reads two members off
//! a node's **resolved** view and records what they name. Reading the resolved
//! view rather than `own(A)` is the whole of the answer to *what does extending
//! an edge mean* — nothing new. `connections` is one member, absorbed by the
//! ordinary left-biased, shallow rule (D1.5), so a child either restates the
//! sequence whole or inherits it whole. Extension never **appends** endpoints,
//! and the operators are untouched.
//!
//! # Why this runs in pass 5 and not pass 4
//!
//! Because `connections` may be inherited. Pass 4 has resolved every path and
//! knows what each item names, but only pass 5 knows which `connections` a node
//! ends up holding, and reporting `E0223` against `own(A)` would fire on every
//! concrete edge of a family that declares its endpoints once in the base.
//!
//! # A position is what is written, not what survived
//!
//! An item that named nothing is `E0213` and holds no endpoint, and the items
//! beside it **do not renumber**: `connections` writes three positions whether
//! or not all three resolve. A handle is checked against the count the sequence
//! writes and is matched to an endpoint by that written position, so one bad
//! item costs exactly one diagnostic and moves no handle.

use std::collections::HashMap;

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, NodeId, ScalarStyle, Span};

use crate::edge::{self, is_edge, CONNECTIONS, DEFINITION};
use crate::link::{Ctx, Linked, RefRole};
use crate::symbol::Symbol;
use crate::tags::TagKind;

use super::names::{display, span_of};
use super::resolve::Views;
use super::view::{Field, Place};

/// One endpoint of one edge, in written order.
#[derive(Clone, Copy, Debug)]
pub struct Connection {
    /// Its position in `connections`, which is what a handle names and what
    /// never renumbers: filtering an endpoint an observer cannot see removes it
    /// from a result, and an endpoint that resolved to nothing leaves a gap,
    /// and neither moves the ones beside it.
    pub index: u32,
    /// The item node that named it, in the file that wrote the sequence.
    pub item: Place,
    /// The node it names.
    pub target: Place,
    /// Whether the item was written `!ref` — the same declaration of intent it
    /// is anywhere else (D4.3), checked as `E0217` and recorded here so the
    /// image can answer *which endpoints does this edge intend to modify*.
    pub capability: bool,
    /// The handle `definition` gives this position, if it gives it one.
    pub handle: Option<Symbol>,
    /// Where the item is written.
    pub span: Span,
}

/// One `!edge` node, and what it relates.
#[derive(Debug)]
pub struct EdgeNode {
    /// The node carrying the tag.
    pub place: Place,
    /// Its endpoints, in written order. Possibly empty: an edge that writes
    /// `connections: []` relates nothing *yet*, which is a shape and not a
    /// mistake, while an edge with no `connections` member at all is `E0223`.
    pub connections: Vec<Connection>,
    /// Every handle `definition` declares, as a name and the position it names,
    /// in written order.
    ///
    /// This is a list and not a field on [`Connection`] because the mapping is
    /// **many-to-one**: two handles may name one position, and that is not a
    /// degenerate case but the ordinary way a self-loop is written — `from: 0`
    /// and `to: 0` over a single endpoint. Recording the handle on the position
    /// it names lets only the last one survive, which silently loses `from`.
    pub handles: Vec<(Symbol, u32)>,
    /// Where the resolved view sites `connections`, whatever shape it turned
    /// out to have, and `definition` likewise. Emission reads them to keep the
    /// language's own members out of the **data** edges: a handle's value is a
    /// position rather than a reference, and a `connections` of the wrong shape
    /// has been reported once already and must not be reported again as a
    /// member that names a node.
    pub connections_value: Option<Place>,
    /// Where the resolved view sites `definition`. See
    /// [`EdgeNode::connections_value`].
    pub definition_value: Option<Place>,
}

impl EdgeNode {
    /// The endpoint a handle names, if it names one.
    ///
    /// Matched by **written position**, which is what the handle was checked
    /// against. A handle naming a position whose item resolved to nothing binds
    /// to no endpoint and answers `None`, and the endpoint after it keeps the
    /// name it was given.
    #[must_use]
    pub fn connection(&self, handle: Symbol) -> Option<&Connection> {
        let (_, index) = self.handles.iter().find(|(name, _)| *name == handle)?;
        self.connections.iter().find(|held| held.index == *index)
    }
}

/// Every `!edge` node of the project, indexed by the node carrying the tag.
#[derive(Default)]
pub struct Edges {
    items: Vec<EdgeNode>,
    index: HashMap<Place, usize>,
}

impl Edges {
    /// Every edge node, in the order pass 5 resolved them.
    #[must_use]
    pub fn items(&self) -> &[EdgeNode] {
        &self.items
    }

    /// The edge written at `place`, if one is.
    #[must_use]
    pub fn get(&self, place: Place) -> Option<&EdgeNode> {
        self.index.get(&place).map(|at| &self.items[*at])
    }

    /// How many edge nodes the project writes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the project writes no edge at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Collect every `!edge` node's endpoints, reporting what is malformed.
pub(crate) fn collect(
    ctx: &Ctx,
    linked: &Linked,
    views: &Views,
    order: &[Place],
    diagnostics: &mut Diagnostics,
) -> Edges {
    let targets = resolved_items(linked);
    let mut edges = Edges::default();
    for place in order.iter().filter(|held| is_edge(ctx.interned, held.0, held.1)) {
        let held = one(ctx, linked, views, &targets, *place, diagnostics);
        edges.index.insert(*place, edges.items.len());
        edges.items.push(held);
    }
    edges
}

/// What each `connections` item resolved to, from pass 4's reference table.
/// An item that resolved to nothing is absent, and `E0213` has already named
/// it — reporting it again here would give one fault two codes.
fn resolved_items(linked: &Linked) -> HashMap<Place, Place> {
    linked
        .references()
        .iter()
        .filter(|held| held.role == RefRole::Connection)
        .filter_map(|held| held.target.map(|target| ((held.file, held.node), target)))
        .collect()
}

/// Read one edge node.
fn one(
    ctx: &Ctx,
    linked: &Linked,
    views: &Views,
    targets: &HashMap<Place, Place>,
    place: Place,
    diagnostics: &mut Diagnostics,
) -> EdgeNode {
    let sequence = member(ctx, views, place, CONNECTIONS);
    let definition = member(ctx, views, place, DEFINITION);
    let mut held = EdgeNode {
        place,
        connections: Vec::new(),
        handles: Vec::new(),
        connections_value: sequence.map(|field| field.value),
        definition_value: definition.map(|field| field.value),
    };
    let Some(items) = connection_items(ctx, sequence, place, diagnostics) else { return held };
    // The bound a handle is checked against is what `connections` **writes**,
    // not what resolved: an item naming nothing costs its own `E0213` and must
    // not also make the position after it unnameable.
    let written = items.len();
    held.connections = endpoints(ctx, targets, items);
    held.handles = handles(ctx, linked, place, definition, written, diagnostics);
    for (name, index) in &held.handles {
        label(&mut held.connections, *name, *index);
    }
    held
}

/// One of the two members the language owns, off the node's resolved view.
fn member<'a>(ctx: &Ctx, views: &'a Views, place: Place, name: &str) -> Option<&'a Field> {
    let name = ctx.interned.symbols().get(name)?;
    views.resolved(place)?.get(name)
}

/// The **first** handle naming a position labels the edge record for it, so the
/// index carries a stable name. The rest are not lost: every handle is kept on
/// the edge, because the mapping is many-to-one and a self-loop names one
/// position twice on purpose.
fn label(connections: &mut [Connection], name: Symbol, index: u32) {
    let Some(held) = connections.iter_mut().find(|held| held.index == index) else { return };
    if held.handle.is_none() {
        held.handle = Some(name);
    }
}

/// The items of an edge's `connections`, or `None` when it has none to read.
///
/// `E0223` when the resolved view holds no such member, and when it holds one
/// that is an **unsatisfied declaration** — `pub connections:` in a base, never
/// supplied. Both relate nothing, which is the failure this decision exists to
/// remove, and both have the same fix: write the endpoints. `E0224` is for a
/// member that holds a value of the wrong *shape*, which is a different fix.
fn connection_items(
    ctx: &Ctx,
    field: Option<&Field>,
    place: Place,
    diagnostics: &mut Diagnostics,
) -> Option<Vec<Place>> {
    let Some(field) = field else {
        diagnostics.push(missing(ctx, place, None));
        return None;
    };
    let ast = ctx.ast(field.value.0)?;
    if is_unsupplied(ast, field.value.1) {
        diagnostics.push(missing(ctx, place, Some(field.key)));
        return None;
    }
    let Some(items) = ast.items(field.value.1) else {
        diagnostics.push(shape(ctx, CONNECTIONS, "a sequence", field.value));
        return None;
    };
    Some(items.iter().map(|item| (field.value.0, *item)).collect())
}

/// Whether a member's value is the **empty node** a declaration leaves behind:
/// `pub connections:` in a base, with nothing after it.
///
/// Two spellings reach here for one thing — a tagged empty declaration parses
/// to an empty scalar, a bare one to the plain null YAML resolves it to — and
/// every plain spelling of null is the same statement. None of them is a
/// sequence, and none of them is the *shape* fault `E0224` reports: the member
/// was declared and never supplied, which is what `E0223` is for.
fn is_unsupplied(ast: &Ast, value: NodeId) -> bool {
    ast.scalar(value).is_some_and(|held| {
        held.style == ScalarStyle::Plain
            && matches!(&*held.value, "" | "~" | "null" | "Null" | "NULL")
    })
}

/// Pair each item with the node it names, keeping written order and the
/// position each item holds. An item that named nothing keeps its position:
/// renumbering the endpoints after it would silently move every handle.
fn endpoints(ctx: &Ctx, targets: &HashMap<Place, Place>, items: Vec<Place>) -> Vec<Connection> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(at, item)| {
            let target = named(ctx, targets, item)?;
            Some(Connection {
                index: u32::try_from(at).ok()?,
                item,
                target,
                capability: ctx.interned.tag_kind(item.0, item.1) == Some(TagKind::Ref),
                handle: None,
                span: ctx.ast(item.0)?.node(item.1).span,
            })
        })
        .collect()
}

/// The node one item names: an alias binding, a resolved path, or the item
/// itself when it is written inline. An inline endpoint is a node like any
/// other; it simply has no name to be addressed by.
fn named(ctx: &Ctx, targets: &HashMap<Place, Place>, item: Place) -> Option<Place> {
    let ast = ctx.ast(item.0)?;
    if ast.alias(item.1).is_some() {
        return ast.alias_binding(item.1);
    }
    if ast.scalar(item.1).is_some() {
        return targets.get(&item).copied();
    }
    Some(item)
}

/// The handles `definition` declares, each checked against the number of
/// positions `connections` writes.
fn handles(
    ctx: &Ctx,
    linked: &Linked,
    subject: Place,
    field: Option<&Field>,
    count: usize,
    diagnostics: &mut Diagnostics,
) -> Vec<(Symbol, u32)> {
    let Some(field) = field else { return Vec::new() };
    let Some(ast) = ctx.ast(field.value.0) else { return Vec::new() };
    let Some(entries) = ast.entries(field.value.1) else {
        diagnostics.push(shape(ctx, DEFINITION, "a mapping", field.value));
        return Vec::new();
    };
    let origin = (field.origin != subject).then_some(field.origin);
    let mut out = Vec::new();
    for entry in entries {
        let Some(name) = ctx.interned.key_of(field.value.0, entry.key) else { continue };
        let text = ctx.interned.symbols().resolve(name).unwrap_or_default();
        let at = (field.value.0, entry.value);
        let held = Unbound { subject, at, name: text, count, origin };
        match handle(ast, text, entry.value, count) {
            Ok(index) => out.push((name, index)),
            Err(why) => diagnostics.push(unbound(ctx, linked, &held, why)),
        }
    }
    out
}

/// Why one handle names no position.
#[derive(Clone, Copy)]
enum Rejection {
    /// It takes one of the two names the language owns on an edge.
    Reserved,
    /// Its value is not a position at all.
    NotAnIndex,
    /// Its value is a position the sequence does not write.
    OutOfRange(u32),
}

/// One handle's value read as a position in `connections`, or why it is not
/// one.
fn handle(ast: &Ast, name: &str, value: NodeId, count: usize) -> Result<u32, Rejection> {
    if edge::is_reserved_member(name) {
        return Err(Rejection::Reserved);
    }
    let text = ast.scalar(value).ok_or(Rejection::NotAnIndex)?;
    let index = index_of(&text.value).ok_or(Rejection::NotAnIndex)?;
    match (index as usize) < count {
        true => Ok(index),
        false => Err(Rejection::OutOfRange(index)),
    }
}

/// A position, written the one way a position is written: decimal digits, no
/// sign, no padding and no surrounding space.
///
/// `" 0 "`, `"+0"` and `"00"` are rejected. Accepting them would give one
/// position several spellings and buy nothing: a handle's value is written by
/// the author beside the sequence it indexes, and there is no format to be
/// lenient about.
fn index_of(text: &str) -> Option<u32> {
    let canonical = !text.is_empty()
        && text.bytes().all(|held| held.is_ascii_digit())
        && (text.len() == 1 || !text.starts_with('0'));
    if !canonical {
        return None;
    }
    text.parse().ok()
}

/// `E0223` — an `!edge` that relates nothing.
fn missing(ctx: &Ctx, place: Place, declared: Option<Place>) -> Diagnostic {
    let message = match declared {
        Some(_) => {
            "an `!edge`'s `connections` is a declaration nothing supplied, so the tag \
                    relates nothing"
        }
        None => "an `!edge` holds no `connections`, so the tag relates nothing",
    };
    let held = Diagnostic::new(Code::EdgeWithoutConnections, span_of(ctx, place), message)
        .with_note(
            "`connections` is a sequence of the nodes the edge connects, and is what makes the \
             node an edge; an edge that relates nothing yet writes `connections: []`",
            None,
        );
    match declared {
        Some(key) => held.with_note("declared here, and left empty", Some(span_of(ctx, key))),
        None => held,
    }
}

/// `E0224` — one of the two members the language owns has the wrong shape.
fn shape(ctx: &Ctx, member: &str, wanted: &str, at: Place) -> Diagnostic {
    Diagnostic::new(
        Code::EdgeMemberShape,
        span_of(ctx, at),
        format!("an edge's `{member}` must be {wanted}"),
    )
    .with_note(
        "`connections` is a sequence of endpoints and `definition` is a mapping of handle names \
         to positions in it",
        None,
    )
}

/// One rejected handle, and everything its diagnostic has to name.
struct Unbound<'a> {
    subject: Place,
    at: Place,
    name: &'a str,
    count: usize,
    origin: Option<Place>,
}

/// `E0225` — a handle naming no endpoint.
///
/// The message names its **subject**, and an inherited `definition` earns a
/// note naming where it came from, exactly as `E0221` does. Without both, a
/// family that declares two handles and three edges that each narrow
/// `connections` produce three byte-identical errors on the base's line — a
/// line that is correct for every node that reads it whole.
fn unbound(ctx: &Ctx, linked: &Linked, held: &Unbound, why: Rejection) -> Diagnostic {
    let subject = display(ctx, linked, held.subject);
    let out = Diagnostic::new(
        Code::UnboundHandle,
        span_of(ctx, held.at),
        format!("`{}` names no connection of `{subject}`: {}", held.name, reason(held, why)),
    )
    .with_note(
        "a handle is a name for a position in `connections`, so its value is an index into that \
         sequence, counted from 0",
        None,
    );
    let Some(origin) = held.origin else { return out };
    out.with_note(
        format!(
            "`{subject}` inherits this `definition` from `{}` and resolves to {} connection(s); \
             the declaration is correct for a node that reads the sequence whole",
            display(ctx, linked, origin),
            held.count
        ),
        Some(span_of(ctx, held.subject)),
    )
}

/// The half of `E0225`'s message that says which of the three conditions fired.
fn reason(held: &Unbound, why: Rejection) -> String {
    match why {
        Rejection::Reserved => format!(
            "`{}` is one of the two member names the language owns on an edge, and a handle may \
             not take it",
            held.name
        ),
        Rejection::NotAnIndex => {
            "its value is not a position, which is a whole number written plainly".to_owned()
        }
        Rejection::OutOfRange(index) => {
            format!("it names position {index}, and this edge writes {}", held.count)
        }
    }
}
