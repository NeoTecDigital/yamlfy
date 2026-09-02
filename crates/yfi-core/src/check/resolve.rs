// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolving inheritance into a view per node.
//!
//! # Precedence (D4.7)
//!
//! Highest first, and every tier is left-biased against the ones below it:
//!
//! 1. every key written **directly** in A,
//! 2. A's inclusions (`<<`), in written order,
//! 3. A's extensions (`extends:` with an alias or an inline operand),
//! 4. the bases of A's extended references (`extends: !ref`),
//! 5. `own(X)` for every X holding an extended reference **to** A.
//!
//! Tier 5 is deliberately last: an extended reference adds to a base and never
//! overrides it. Extensions rank below inclusions because an inclusion is a
//! deliberate node-local statement about *this* node while a base is a general
//! statement about a family — the specific must beat the general, or adding a
//! `<<` to a node could be silently ignored.
//!
//! # Three views, not one
//!
//! * `own(N)` — N's literal keys with its clauses removed (D4.9).
//! * `base(N)` — tiers 1 to 4. What N holds *without* anything installed on it
//!   by an extended reference, which is what D4.5's additivity is measured
//!   against.
//! * `declared(N)` — tiers 1 and 5. The keys and tags N **declares**, including
//!   what extended references installed on it, and excluding what it merely
//!   *includes*: an inclusion is compositional, not definitional (D4.1), so a
//!   key absorbed from a mixin is not a declaration of N's family.
//! * `resolved(N)` — all five tiers.
//!
//! Merge is **shallow** (D1.5) and **transitive over resolved views** (D1.3): a
//! source contributes its own resolved view, and its clause has already been
//! discharged in producing it (D4.9). Nothing re-applies a source's clause at
//! the including node's level, which is what keeps
//! `fixtures/cycles/merge-diamond.yml` idempotent when `a` is reached twice.
//!
//! # Evaluation order
//!
//! The graph is a DAG once [`super::scc::back_edges`] has been dropped, so
//! views are composed in one explicit-stack post-order walk. It is explicit
//! rather than recursive for the reason [`super::scc`] gives: an inheritance
//! chain is as deep as an author writes it.

use std::collections::{HashMap, HashSet};

use crate::link::graph::EdgeKind;
use crate::link::keys::own_keys;
use crate::link::{Ctx, Direction, EdgeId, Graph, Linked, SourceOrder, Stratum};
use crate::scope::{Mutability, ScopeId, ScopeTree, Visibility};

use super::view::{Acquisition, Field, FieldGate, Place, Relation, View};

/// The tiers of D4.7 that come from A's own clauses, in precedence order.
const OUTBOUND: [EdgeKind; 3] =
    [EdgeKind::Inclusion, EdgeKind::Extension, EdgeKind::ExtendedReference];

/// Every node's views.
pub(crate) struct Views {
    own: HashMap<Place, View>,
    base: HashMap<Place, View>,
    declared: HashMap<Place, View>,
    resolved: HashMap<Place, View>,
}

impl Views {
    /// N's literal keys, clauses removed.
    pub(crate) fn own(&self, place: Place) -> Option<&View> {
        self.own.get(&place)
    }

    /// Tiers 1 to 4 — what N holds before any extended reference installs
    /// anything on it.
    pub(crate) fn base(&self, place: Place) -> Option<&View> {
        self.base.get(&place)
    }

    /// What N declares: its own keys plus everything installed on it.
    pub(crate) fn declared(&self, place: Place) -> Option<&View> {
        self.declared.get(&place)
    }

    /// N's resolved view, all five tiers.
    pub(crate) fn resolved(&self, place: Place) -> Option<&View> {
        self.resolved.get(&place)
    }

    /// How many nodes have a resolved view.
    pub(crate) fn len(&self) -> usize {
        self.resolved.len()
    }
}

/// Compose every view of the project, following no dropped edge.
pub(crate) fn resolve(
    ctx: &Ctx,
    linked: &Linked,
    dropped: &HashSet<EdgeId>,
    order: &[Place],
) -> Views {
    let mut run = Run {
        ctx,
        graph: linked.graph(),
        dropped,
        root: ctx.project.scopes().root(),
        views: Views {
            own: HashMap::new(),
            base: HashMap::new(),
            declared: HashMap::new(),
            resolved: HashMap::new(),
        },
    };
    for place in order {
        run.ensure(*place);
    }
    run.views
}

/// Every node of the project that holds members, textually first to last.
///
/// That is every mapping, and every sequence written as a member list. Views
/// are wanted for nodes the graph never saw — a concrete node with no clause at
/// all still has to be validated — so the walk is over the arenas rather than
/// over the vertices.
pub(crate) fn every_holder(ctx: &Ctx) -> Vec<Place> {
    let mut out: Vec<(SourceOrder, Place)> = Vec::new();
    for file in ctx.project.files() {
        for position in 0..file.ast.nodes().len() {
            let node = yfi_syntax::NodeId(u32::try_from(position).expect("arena overflow"));
            if file.ast.entries(node).is_none() && !holds_members(ctx, file.id, node) {
                continue;
            }
            let place = (file.id, node);
            let order = crate::link::source_order(ctx.project, ctx.interned, place.0, place.1)
                .unwrap_or(SourceOrder { file: u32::MAX, document: u32::MAX, byte: u32::MAX });
            out.push((order, place));
        }
    }
    out.sort_by_key(|held| held.0);
    out.into_iter().map(|held| held.1).collect()
}

/// Whether a sequence is a member list: whether any item names a member.
///
/// **Membership is the file class**, not a spelling (D4.12). Every scalar
/// nested in a Yamlfication source file names a member of the collection
/// holding it, however it is quoted or tagged; a base YAML sequence is data and
/// names nothing. So this asks pass 3 rather than re-reading the items, and the
/// only sequences without a view are those in a `.yaml` and those holding
/// nothing but collections, which have no names to hold.
fn holds_members(ctx: &Ctx, file: yfi_syntax::FileId, node: yfi_syntax::NodeId) -> bool {
    let Some(ast) = ctx.ast(file) else { return false };
    ast.items(node)
        .is_some_and(|items| items.iter().any(|item| ctx.interned.key_of(file, *item).is_some()))
}

struct Run<'a> {
    ctx: &'a Ctx<'a>,
    graph: &'a Graph,
    dropped: &'a HashSet<EdgeId>,
    root: Option<ScopeId>,
    views: Views,
}

impl Run<'_> {
    /// Compose `place` and everything it depends on, deepest first.
    fn ensure(&mut self, place: Place) {
        let mut queued: HashSet<Place> = HashSet::from([place]);
        let mut stack = vec![(place, false)];
        while let Some((held, expanded)) = stack.pop() {
            if self.views.resolved.contains_key(&held) {
                continue;
            }
            if expanded {
                self.compose(held);
                continue;
            }
            stack.push((held, true));
            for dep in self.dependencies(held) {
                if self.views.resolved.contains_key(&dep) || !queued.insert(dep) {
                    continue;
                }
                stack.push((dep, false));
            }
        }
    }

    /// The nodes whose resolved views `place` needs: its surviving forward
    /// edges only. A reverse edge lands on an `own` view, which needs nothing.
    fn dependencies(&self, place: Place) -> Vec<Place> {
        self.edges(place, Direction::Forward)
            .into_iter()
            .map(|edge| edge.1)
            .filter(|target| *target != place)
            .collect()
    }

    /// The surviving edges of one direction out of `R(place)`, in written
    /// order, each as `(kind, the node it lands on)`.
    ///
    /// The dropped set is consulted here even though the memoised walk below
    /// reaches the same views without it: the walk breaks a cycle wherever its
    /// post-order happens to close, and that it closes at the same edge
    /// [`super::scc::back_edges`] chose is a property of the two walks running
    /// in the same order, not a rule. Stating the recovery is what keeps it a
    /// rule.
    fn edges(&self, place: Place, direction: Direction) -> Vec<(EdgeKind, Place)> {
        let Some(from) = self.graph.vertex_of(place.0, place.1, Stratum::Resolved) else {
            return Vec::new();
        };
        self.graph
            .out_edges(from)
            .iter()
            .filter(|id| !self.dropped.contains(id))
            .filter_map(|id| self.graph.edge(*id))
            .filter(|edge| edge.direction == direction)
            .filter_map(|edge| {
                self.graph.vertex(edge.to).map(|held| (edge.kind, (held.file, held.node)))
            })
            .collect()
    }

    fn compose(&mut self, place: Place) {
        let own = self.own_view(place);
        let scope = self.scope_of(place);
        let mut base = own.clone();
        for kind in OUTBOUND {
            self.absorb_tier(&mut base, place, kind, scope);
        }
        let mut declared = own.clone();
        let mut resolved = base.clone();
        for source in self.installed_on(place) {
            let view = self.own_view(source);
            declared.absorb(&view, Relation::Installation, scope, self.scopes());
            resolved.absorb(&view, Relation::Installation, scope, self.scopes());
        }
        self.views.own.insert(place, own);
        self.views.base.insert(place, base);
        self.views.declared.insert(place, declared);
        self.views.resolved.insert(place, resolved);
    }

    fn absorb_tier(&self, into: &mut View, place: Place, kind: EdgeKind, scope: ScopeId) {
        let relation = match kind {
            EdgeKind::Inclusion => Relation::Inclusion,
            _ => Relation::Extension,
        };
        for (_, target) in self.edges(place, Direction::Forward).iter().filter(|e| e.0 == kind) {
            let Some(view) = self.views.resolved.get(target) else { continue };
            into.absorb(view, relation, scope, self.scopes());
        }
    }

    /// Every node holding an extended reference to `place`, in document order,
    /// so the resulting table is deterministic. Any *observable* disagreement
    /// between two of them is `E0214`, so this order is never load-bearing for
    /// meaning.
    fn installed_on(&self, place: Place) -> Vec<Place> {
        let mut out: Vec<Place> = self
            .edges(place, Direction::Reverse)
            .into_iter()
            .map(|edge| edge.1)
            .filter(|source| *source != place)
            .collect();
        out.sort_by_key(|held| {
            crate::link::source_order(self.ctx.project, self.ctx.interned, held.0, held.1)
        });
        out
    }

    /// `own(N)`, with each member's gate decided by what it declared and where
    /// it is written.
    fn own_view(&self, place: Place) -> View {
        if let Some(held) = self.views.own.get(&place) {
            return held.clone();
        }
        let scope = self.scope_of(place);
        let mut view = View::default();
        for key in own_keys(self.ctx, place.0, place.1) {
            view.push(Field {
                name: key.name,
                key: (place.0, key.key),
                value: (place.0, key.value),
                origin: place,
                acquired: Acquisition::Own,
                reach: self.gate_of(place.0, key.key, scope),
            });
        }
        view
    }

    /// One member's gate: its own declaration, composed with its scope's.
    ///
    /// **A member that says nothing grants nothing** (D6.4, one level down), so
    /// an unflagged member of a `.yfy` is `private` and `immutable`. Composition
    /// with the scope is what stops a `pub` member from granting more than the
    /// directory holding it does — the `public` marking means "visible to
    /// everyone who can get here", and getting here is the enclosing scope's
    /// business (D6.5).
    ///
    /// **Base YAML declares nothing** (D6.6), so there is no flag to read and
    /// the gate is the scope's alone. That is not a special case for data; it is
    /// what "the flags are not interpreted here" means, and it leaves an
    /// imported `.yaml`'s members exactly as readable as its directory is.
    fn gate_of(&self, file: yfi_syntax::FileId, key: yfi_syntax::NodeId, scope: ScopeId)
        -> FieldGate {
        let declared = match self.ctx.is_source(file) {
            true => self.ctx.interned.member_of(file, key).map(|held| held.flags),
            false => None,
        };
        let visibility = declared.map_or(Visibility::Public, |flags| flags.visibility());
        let mutability = declared.map_or(Mutability::Mutable, |flags| flags.mutability());
        FieldGate {
            visibility: self.composed(visibility, scope, ScopeTree::visible),
            mutability: self.composed(mutability, scope, ScopeTree::writable),
            scope,
        }
    }

    /// A declared axis value, closed unless the scope is open to the project at
    /// large on that same axis.
    ///
    /// The root may be `private` without cutting the project off from itself,
    /// because every scope is inside the root — so the root is the observer that
    /// answers "would anyone outside this subtree see it".
    fn composed<T: Axis>(&self, declared: T, scope: ScopeId, open: Opens) -> T {
        let Some(root) = self.root else { return T::OPEN };
        match declared.is_open() && open(self.ctx.project.scopes(), scope, root) {
            true => T::OPEN,
            false => T::CLOSED,
        }
    }

    fn scopes(&self) -> &ScopeTree {
        self.ctx.project.scopes()
    }

    fn scope_of(&self, place: Place) -> ScopeId {
        self.ctx.interned.scope_of(place.0, place.1).or(self.root).unwrap_or(ScopeId(0))
    }
}

/// One of [`ScopeTree`]'s two composition predicates.
type Opens = fn(&ScopeTree, ScopeId, ScopeId) -> bool;

/// One of the two orthogonal axes, so the composition is written once.
trait Axis: Copy {
    /// The permissive value.
    const OPEN: Self;
    /// The closed value, which is every default (D6.4).
    const CLOSED: Self;
    /// Whether this value admits an observer from outside.
    fn is_open(self) -> bool;
}

impl Axis for Visibility {
    const OPEN: Self = Visibility::Public;
    const CLOSED: Self = Visibility::Private;
    fn is_open(self) -> bool {
        Visibility::is_open(self)
    }
}

impl Axis for Mutability {
    const OPEN: Self = Mutability::Mutable;
    const CLOSED: Self = Mutability::Immutable;
    fn is_open(self) -> bool {
        Mutability::is_open(self)
    }
}
