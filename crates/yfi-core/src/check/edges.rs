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
//!
//! # Where the second member lives
//!
//! `connections` is read here; `definition` and its `E0225` are read in the
//! `handles` submodule, because the two members are read **independently** and the
//! handle rules — a position has one spelling, an owned name may not be taken,
//! the mapping is many-to-one — are a concern of their own that says nothing
//! about endpoints.

mod handles;

use std::collections::HashMap;

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, NodeId, ScalarStyle, Span};

use crate::edge::{self, is_edge, CONNECTIONS, DEFINITION};
use crate::link::{Ctx, Linked, RefRole};
use crate::symbol::Symbol;
use crate::tags::TagKind;

use self::handles::handles;

use super::names::span_of;
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
    let items = connection_items(ctx, sequence, place, diagnostics);
    // The bound a handle is checked against is what `connections` **writes**,
    // not what resolved: an item naming nothing costs its own `E0213` and must
    // not also make the position after it unnameable. A `connections` that was
    // never read leaves **no** bound, and `handles` reports what is knowable
    // without one — which is everything except a position past the end.
    let found = handles(ctx, linked, place, definition, items.as_ref().map(Vec::len), diagnostics);
    let Some(items) = items else { return held };
    held.connections = endpoints(ctx, targets, items, diagnostics);
    held.handles = found;
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
/// `E0223` for every way the member relates nothing: absent from the resolved
/// view, an **unsatisfied declaration** — `pub connections:` in a base, never
/// supplied — or written in a **base YAML** file, where no item is a reach. All
/// three relate nothing, which is the failure this decision exists to remove,
/// and all three have the same fix: write the endpoints, in a `.yfy`. `E0224` is
/// for a member that holds a value of the wrong *shape*, which is a different
/// fix.
///
/// An **alias** standing as the member's value is dereferenced, exactly as one
/// standing as an item is: `connections: *Pair` relates what `&Pair` names. The
/// two spellings asked one question and were given two answers, and the item
/// rule is the one that was right — a sequence reached through an alias is
/// still a sequence, and the resolved view can still see it. Everything below
/// is judged **after** the dereference, so an alias to a mapping is `E0224` and
/// an alias to a `.yaml` sequence is `E0223`, exactly as the written forms are.
fn connection_items(
    ctx: &Ctx,
    field: Option<&Field>,
    place: Place,
    diagnostics: &mut Diagnostics,
) -> Option<Vec<Place>> {
    let Some(field) = field else {
        diagnostics.push(missing(ctx, place, Unrelated::NoMember));
        return None;
    };
    let at = edge::dereference(ctx, field.value);
    if !ctx.is_source(at.0) {
        diagnostics.push(missing(ctx, place, Unrelated::BaseYaml(at)));
        return None;
    }
    let ast = ctx.ast(at.0)?;
    if is_unsupplied(ast, at.1) {
        let inherited = field.origin != place;
        diagnostics.push(missing(ctx, place, Unrelated::Empty { key: field.key, inherited }));
        return None;
    }
    let Some(items) = ast.items(at.1) else {
        diagnostics.push(shape(ctx, CONNECTIONS, "a sequence", field.value));
        return None;
    };
    Some(items.iter().map(|item| (at.0, *item)).collect())
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
fn endpoints(
    ctx: &Ctx,
    targets: &HashMap<Place, Place>,
    items: Vec<Place>,
    diagnostics: &mut Diagnostics,
) -> Vec<Connection> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(at, item)| {
            let target = named(ctx, targets, item, diagnostics)?;
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
fn named(
    ctx: &Ctx,
    targets: &HashMap<Place, Place>,
    item: Place,
    diagnostics: &mut Diagnostics,
) -> Option<Place> {
    let ast = ctx.ast(item.0)?;
    if ast.alias(item.1).is_some() {
        return a_node(ctx, item, ast.alias_binding(item.1)?, diagnostics);
    }
    if ast.scalar(item.1).is_some() {
        return targets.get(&item).copied();
    }
    Some(item)
}

/// An endpoint is a **node**, so an alias that names an anchored scalar names
/// no endpoint and is `E0213`.
///
/// This is the same answer the other two spellings give. A *path* naming the
/// same anchor is `E0213` already, because D6.1 makes only an anchored
/// collection addressable, and an inline scalar — `connections: [7]` — has
/// never been an endpoint either. Reported here rather than in pass 4 because
/// an alias is not a path and never became a reference: pass 4 never saw it.
fn a_node(ctx: &Ctx, item: Place, bound: Place, diagnostics: &mut Diagnostics) -> Option<Place> {
    if ctx.ast(bound.0)?.scalar(bound.1).is_none() {
        return Some(bound);
    }
    diagnostics.push(a_value(ctx, item, bound));
    None
}

/// `E0213` — an alias endpoint that names a value rather than a node.
fn a_value(ctx: &Ctx, item: Place, bound: Place) -> Diagnostic {
    let name = ctx
        .ast(item.0)
        .and_then(|ast| ast.anchors().get(ast.alias(item.1)?.anchor))
        .map_or_else(String::new, |def| def.name.to_string());
    Diagnostic::new(
        Code::UnresolvedRef,
        span_of(ctx, item),
        format!("`*{name}` names an anchored scalar, which an edge cannot relate"),
    )
    .with_note(
        "an endpoint is a node; only an anchored collection is one, and an anchored scalar is a \
         value, not a type",
        Some(span_of(ctx, bound)),
    )
}

/// Why an `!edge` relates nothing: the three conditions `E0223` covers.
#[derive(Clone, Copy)]
enum Unrelated {
    /// The resolved view holds no `connections` at all.
    NoMember,
    /// It holds one whose value is the empty node a declaration leaves behind.
    /// `inherited` separates the two situations that reach here: a base's
    /// `pub connections:` no concrete edge supplied, and a node that emptied
    /// the member it wrote itself. Only the first is a declaration, and telling
    /// the second it inherited one sends the author to a base there is none of.
    Empty { key: Place, inherited: bool },
    /// It holds one written in a **base YAML** file, where nothing is a reach
    /// (D6.6), so the sequence names nodes to a reader and nothing to the
    /// compiler.
    BaseYaml(Place),
}

/// `E0223` — an `!edge` that relates nothing.
fn missing(ctx: &Ctx, place: Place, why: Unrelated) -> Diagnostic {
    let message = match why {
        Unrelated::NoMember => "an `!edge` holds no `connections`, so the tag relates nothing",
        Unrelated::Empty { inherited: true, .. } => {
            "an `!edge`'s `connections` is a declaration nothing supplied, so the tag \
             relates nothing"
        }
        Unrelated::Empty { inherited: false, .. } => {
            "an `!edge`'s own `connections` is empty, so the tag relates nothing"
        }
        Unrelated::BaseYaml(_) => {
            "an `!edge`'s `connections` is written in a base YAML file, so the tag relates \
             nothing"
        }
    };
    let held = Diagnostic::new(Code::EdgeWithoutConnections, span_of(ctx, place), message)
        .with_note(
            "`connections` is a sequence of the nodes the edge connects, and is what makes the \
             node an edge; an edge that relates nothing yet writes `connections: []`",
            None,
        );
    match why {
        Unrelated::NoMember => held,
        Unrelated::Empty { key, inherited: true } => {
            held.with_note("declared here, and left empty", Some(span_of(ctx, key)))
        }
        Unrelated::Empty { key, inherited: false } => {
            held.with_note("written here, with no value after it", Some(span_of(ctx, key)))
        }
        Unrelated::BaseYaml(value) => held.with_note(
            "written here: base YAML is data and declares nothing, so no item of this sequence \
             is a reach; an edge's endpoints belong in a `.yfy`",
            Some(span_of(ctx, value)),
        ),
    }
}

/// `E0224` — one of the two members the language owns has the wrong shape.
pub(super) fn shape(ctx: &Ctx, member: &str, wanted: &str, at: Place) -> Diagnostic {
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
