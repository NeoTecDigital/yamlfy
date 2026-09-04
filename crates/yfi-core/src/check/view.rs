// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! A resolved or declared view: an ordered, left-biased key table.
//!
//! Absorption is **left-biased** and **shallow** (D1.5): the first entry to
//! claim a key keeps it, and a lower-precedence entry for the same key is
//! discarded whole, never merged into it key by key. So every tier of D4.7 is
//! one call to [`View::absorb`] in precedence order, and precedence is
//! expressed by call order rather than by a rank stored per entry.
//!
//! **Access is a relation, not a flag.** Whether a member can be read depends
//! on the relationship that brought it into the node holding it, and the three
//! operators are three different relationships (D4.12). So a field records how
//! it arrived ([`Acquisition`]) and what gates it ([`FieldGate`]), and reading
//! is answered by [`Field::is_readable_from`] as a question about an observer
//! *and* a field — never about a node.
//!
//! The epistemic gate is decided before any of this, where the reach is
//! written: `E0241` for an import, `E0216` for a path ([`super::reach`]).
//!
//! What pass 6 owns is *applying* this predicate while it walks — filtering
//! members as it emits and as it traverses, so scoping never leaks through
//! result shape. Pass 5 owes it the per-member data to do that with, and that
//! is what this type is.

use std::collections::HashMap;

use yfi_syntax::{FileId, NodeId};

use crate::scope::{Mutability, ScopeId, ScopeTree, Visibility};
use crate::symbol::Symbol;

/// A node in a file.
pub type Place = (FileId, NodeId);

/// How a member arrived in the view that holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Acquisition {
    /// Written directly by the node.
    Own,
    /// Absorbed through `<<`. The member is still the source's, addressed
    /// through this node, and keeps the source's gate.
    Included,
    /// Absorbed through one `extends` step from the node that *wrote* it. The
    /// member is now this node's own, and its privacy came with it.
    Extended,
    /// Reached through a further inheritance step. Only public members arrive
    /// this way; a private one does not propagate down a chain.
    Descended,
    /// Installed onto a base by an extended reference (D4.5). The member is the
    /// base's, additively, and ranks below everything the base already has.
    Installed,
}

impl Acquisition {
    /// Whether a member acquired this way belongs to the node holding it, and
    /// therefore carries its privacy across one further `extends`.
    fn is_the_holders_own(self) -> bool {
        matches!(self, Acquisition::Own | Acquisition::Installed)
    }
}

/// Which operator is absorbing one view into another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Relation {
    /// `<<` — containment.
    Inclusion,
    /// `extends` — instantiation, with or without a `!ref` operand.
    Extension,
    /// The reversed edge of an extended reference: `own(X)` onto its base.
    Installation,
}

/// What gates a member, on both axes.
///
/// Each axis is the member's own declaration (`pub`, `mut`) **composed with**
/// its scope's, because a member cannot grant what the scope holding it does
/// not — which is D6.5's composition read one level down, not a second
/// predicate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldGate {
    /// `Public` when the member declared itself `pub` **and** the project at
    /// large can reach the scope holding it. `Private` otherwise, which is the
    /// default: a member that says nothing grants nothing (D6.4).
    pub visibility: Visibility,
    /// `Mutable` on the same terms, from `mut`. Nothing writes at compile time
    /// except an extended reference, which is gated on the *scope* (`E0217`),
    /// so this is carried for `emit` and the runtime rather than enforced here.
    pub mutability: Mutability,
    /// The scope the member is gated *to*. For a member the node wrote, or took
    /// across one `extends`, this is the **holder's** scope; for one absorbed by
    /// `<<` it stays the source's, because containment does not republish.
    pub scope: ScopeId,
}

/// One key of a view.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// The interned key text.
    pub name: Symbol,
    /// The key node, at the site that wrote it.
    pub key: Place,
    /// The value node, at the site that wrote it.
    pub value: Place,
    /// The mapping that wrote the entry.
    pub origin: Place,
    /// How the member arrived in the view holding it.
    pub acquired: Acquisition,
    /// What gates it, on both axes.
    pub reach: FieldGate,
}

impl Field {
    /// Whether an observer in `scope` may read this member.
    ///
    /// A public member is readable from anywhere. A private one is readable
    /// only from inside the scope that gates it — which is the holder's scope
    /// when the member was written there or taken across one `extends`, and the
    /// source's when it was merely included.
    #[must_use]
    pub fn is_readable_from(&self, scopes: &ScopeTree, observer: ScopeId) -> bool {
        self.reach.visibility.is_open() || scopes.encloses(self.reach.scope, observer)
    }

    /// Whether an observer in `scope` may change this member.
    ///
    /// The exact analogue of [`Field::is_readable_from`], and deliberately the
    /// same shape: the two axes are orthogonal and answer unrelated questions,
    /// but they compose identically, so neither can give an account of who was
    /// shut out that the other contradicts (D6.4, D6.5).
    #[must_use]
    pub fn is_writable_from(&self, scopes: &ScopeTree, observer: ScopeId) -> bool {
        self.reach.mutability.is_open() || scopes.encloses(self.reach.scope, observer)
    }
}

/// An ordered, left-biased key table.
#[derive(Default, Clone)]
pub struct View {
    fields: Vec<Field>,
    index: HashMap<Symbol, usize>,
}

impl View {
    /// Every field, highest precedence first.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The winning entry for a key.
    #[must_use]
    pub fn get(&self, name: Symbol) -> Option<&Field> {
        self.index.get(&name).map(|at| &self.fields[*at])
    }

    /// Whether the view holds a key.
    #[must_use]
    pub fn holds(&self, name: Symbol) -> bool {
        self.index.contains_key(&name)
    }

    /// The fields an observer in `scope` may read, in precedence order. This is
    /// the surface a `!ref` yields: a public node's private members are not in
    /// it, because referencing a node is not reaching into it.
    pub fn readable_from<'a>(
        &'a self,
        scopes: &'a ScopeTree,
        observer: ScopeId,
    ) -> impl Iterator<Item = &'a Field> {
        self.fields.iter().filter(move |field| field.is_readable_from(scopes, observer))
    }

    /// How many keys the view holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the view holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Add one field, keeping the entry already present for that key.
    pub(crate) fn push(&mut self, field: Field) {
        if self.index.contains_key(&field.name) {
            return;
        }
        self.index.insert(field.name, self.fields.len());
        self.fields.push(field);
    }

    /// Take a lower-precedence view's fields **as they stand**, gates and
    /// acquisitions untouched.
    ///
    /// Not [`View::absorb`]: nothing crosses a relationship here. The one
    /// caller is D4.14's overriding installation, where a base's own composed
    /// view is folded in *underneath* keys already claimed — the fields are
    /// already the holder's and already gated for it, and re-carrying them
    /// would re-gate a member onto the scope it is already gated to and
    /// demote its acquisition a second time.
    pub(crate) fn adopt(&mut self, lower: &View) {
        for field in &lower.fields {
            self.push(*field);
        }
    }

    /// Absorb a lower-precedence view under one relationship.
    pub(crate) fn absorb(
        &mut self,
        lower: &View,
        relation: Relation,
        holder: ScopeId,
        scopes: &ScopeTree,
    ) {
        for field in &lower.fields {
            if let Some(carried) = carry(field, relation, holder, scopes) {
                self.push(carried);
            }
        }
    }
}

/// One member crossing one relationship, or `None` when it does not cross.
fn carry(field: &Field, relation: Relation, holder: ScopeId, scopes: &ScopeTree) -> Option<Field> {
    if relation == Relation::Inclusion {
        // Containment: the member stays the source's, gate and all — including
        // the flags it was declared with, which are part of that gate.
        return Some(Field { acquired: Acquisition::Included, ..*field });
    }
    let regated = FieldGate { scope: holder, ..field.reach };
    if field.acquired.is_the_holders_own() {
        let acquired = match relation {
            Relation::Installation => Acquisition::Installed,
            _ => Acquisition::Extended,
        };
        return Some(Field { acquired, reach: regated, ..*field });
    }
    // Descent. A member already one step from its author arrives only where the
    // inheritor can **read** it, which is the same question
    // [`Field::is_readable_from`] answers for any other observer — so a public
    // member descends anywhere, and a private one descends only inside the scope
    // that gates it. Asking "is it public" instead would drop an ancestor's
    // ordinary member on a chain written entirely inside one directory, where no
    // boundary is being crossed and nothing is being republished.
    let descended = Field { acquired: Acquisition::Descended, reach: regated, ..*field };
    field.is_readable_from(scopes, holder).then_some(descended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yfi_syntax::{FileId, NodeId};

    fn field(name: u32, scope: u32, visibility: Visibility, acquired: Acquisition) -> Field {
        let place = (FileId(0), NodeId(name));
        Field {
            name: Symbol(name),
            key: place,
            value: place,
            origin: place,
            acquired,
            reach: FieldGate {
                visibility,
                mutability: Mutability::Immutable,
                scope: ScopeId(scope),
            },
        }
    }

    /// Absorb one field, from a project whose scopes are unrelated to each
    /// other — so descent across them is descent across a boundary.
    fn absorbed(field: Field, relation: Relation) -> Option<Field> {
        let mut lower = View::default();
        lower.push(field);
        let mut view = View::default();
        view.absorb(&lower, relation, ScopeId(7), &ScopeTree::new());
        view.get(field.name).copied()
    }

    #[test]
    fn the_first_claim_on_a_key_wins_and_the_later_one_is_dropped_whole() {
        let mut view = View::default();
        view.push(field(1, 1, Visibility::Public, Acquisition::Own));
        view.push(field(1, 2, Visibility::Public, Acquisition::Own));
        assert_eq!(view.len(), 1);
        assert_eq!(view.get(Symbol(1)).expect("field").reach.scope, ScopeId(1));
    }

    #[test]
    fn inclusion_carries_a_private_member_in_without_republishing_it() {
        let held =
            absorbed(field(1, 3, Visibility::Private, Acquisition::Own), Relation::Inclusion)
                .expect("containment brings it in");
        assert_eq!(held.acquired, Acquisition::Included);
        assert_eq!(held.reach.scope, ScopeId(3), "still gated by the source's context");
    }

    #[test]
    fn one_extends_step_absorbs_a_private_member_as_the_inheritors_own() {
        let held =
            absorbed(field(1, 3, Visibility::Private, Acquisition::Own), Relation::Extension)
                .expect("privacy crosses one step");
        assert_eq!(held.acquired, Acquisition::Extended);
        assert_eq!(held.reach.visibility, Visibility::Private, "not laundered");
        assert_eq!(held.reach.scope, ScopeId(7), "re-gated onto the inheritor");
    }

    #[test]
    fn a_private_member_does_not_propagate_down_a_chain() {
        let already = field(1, 3, Visibility::Private, Acquisition::Extended);
        assert!(absorbed(already, Relation::Extension).is_none(), "one step, not a chain");
        let public = field(2, 3, Visibility::Public, Acquisition::Extended);
        let held = absorbed(public, Relation::Extension).expect("a public member descends");
        assert_eq!(held.acquired, Acquisition::Descended);
    }

    #[test]
    fn an_installed_member_is_the_bases_own_and_crosses_one_further_step() {
        let held =
            absorbed(field(1, 3, Visibility::Private, Acquisition::Own), Relation::Installation)
                .expect("installed onto the base");
        assert_eq!(held.acquired, Acquisition::Installed);
        assert_eq!(held.reach.scope, ScopeId(7));
        assert_eq!(
            absorbed(held, Relation::Extension).expect("a descendant of the base").acquired,
            Acquisition::Extended
        );
    }

    #[test]
    fn an_absorbed_key_never_displaces_one_already_held() {
        let mut lower = View::default();
        lower.push(field(1, 1, Visibility::Public, Acquisition::Own));
        let mut view = View::default();
        view.push(field(1, 9, Visibility::Public, Acquisition::Own));
        view.absorb(&lower, Relation::Extension, ScopeId(0), &ScopeTree::new());
        assert_eq!(view.get(Symbol(1)).expect("field").reach.scope, ScopeId(9));
        assert!(!view.is_empty());
        assert!(!view.holds(Symbol(2)));
    }
}
