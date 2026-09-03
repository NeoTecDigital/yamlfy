// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0220`, `E0221` and `W0301` — validating concrete nodes.
//!
//! Each concrete node is validated against **every abstract ancestor's declared
//! view**, never against its own flattened one, and the effective value it is
//! compared against is read from its resolved view (D7.3). Flattening first can
//! only confirm that the winner agrees with itself: by D4.7 a local `<<` mixin
//! outranks the ancestor, so the very operation under test overwrites the
//! violation.
//!
//! `W0301` fires only on a node holding at least one abstract ancestor — a
//! concrete node with no ancestry declares its own shape — and is asked of the
//! keys that node **writes**, not of every key its resolved view ends up
//! holding: a key absorbed from a shared mixin would otherwise be reported once
//! per including node, at the mixin's own span, naming a line whose author did
//! nothing wrong. The input set is project-wide, with the consequence D4.11
//! states — a contributed key silences `W0301` for that key on every descendant
//! of the base.

use std::collections::HashSet;

use yfi_syntax::{Code, Diagnostic, Diagnostics};

use crate::edge;
use crate::link::{Ctx, EdgeId, Graph, Linked};
use crate::symbol::Symbol;

use super::ancestry::{ancestors, is_abstract, is_concrete};
use super::declare::{compare, spelling, state_of, Mismatch, State};
use super::names::{display, key_text, span_of};
use super::resolve::Views;
use super::view::{Field, Place, View};

/// One concrete node and the abstract ancestors it answers to.
struct Subject<'a> {
    place: Place,
    resolved: &'a View,
    own: &'a View,
    ancestors: Vec<(Place, &'a View)>,
}

/// Validate every concrete node of the project.
pub(crate) fn validate(
    ctx: &Ctx,
    linked: &Linked,
    views: &Views,
    dropped: &HashSet<EdgeId>,
    order: &[Place],
    diagnostics: &mut Diagnostics,
) {
    for place in order.iter().filter(|held| is_concrete(ctx.interned, **held)) {
        let Some(subject) = subject(ctx, views, linked.graph(), dropped, *place) else {
            continue;
        };
        required(ctx, linked, &subject, diagnostics);
        tags(ctx, linked, &subject, diagnostics);
        undeclared(ctx, linked, &subject, diagnostics);
    }
}

/// Gather a node's views and its abstract ancestors, or nothing when it has no
/// declared view to be measured against.
fn subject<'a>(
    ctx: &Ctx,
    views: &'a Views,
    graph: &Graph,
    dropped: &HashSet<EdgeId>,
    place: Place,
) -> Option<Subject<'a>> {
    let held: Vec<(Place, &View)> = ancestors(graph, dropped, place)
        .into_iter()
        .filter(|base| is_abstract(ctx.interned, *base))
        .filter_map(|base| views.declared(base).map(|view| (base, view)))
        .collect();
    if held.is_empty() {
        return None;
    }
    Some(Subject {
        place,
        resolved: views.resolved(place)?,
        own: views.own(place)?,
        ancestors: held,
    })
}

/// `E0220` — a required field left unsatisfied, once per key however many
/// ancestors declare it.
fn required(ctx: &Ctx, linked: &Linked, subject: &Subject, diagnostics: &mut Diagnostics) {
    let mut reported: HashSet<Symbol> = HashSet::new();
    for (base, declared) in &subject.ancestors {
        for field in declared.fields().iter().filter(|f| is_required(ctx, f)) {
            if supplied(ctx, subject.resolved, field.name) || !reported.insert(field.name) {
                continue;
            }
            diagnostics.push(unsatisfied(ctx, linked, subject, (*base, field)));
        }
    }
}

fn is_required(ctx: &Ctx, field: &Field) -> bool {
    ctx.ast(field.value.0).is_some_and(|ast| state_of(ast, field.value.1) == State::Required)
}

/// Whether the node's effective entry for a key carries a value. The
/// declaration itself is inherited into the resolved view, so "present" is not
/// the question — "no longer empty" is.
fn supplied(ctx: &Ctx, resolved: &View, name: Symbol) -> bool {
    let Some(field) = resolved.get(name) else { return false };
    ctx.ast(field.value.0).is_some_and(|ast| !super::declare::is_empty(ast, field.value.1))
}

fn unsatisfied(
    ctx: &Ctx,
    linked: &Linked,
    subject: &Subject,
    declaration: (Place, &Field),
) -> Diagnostic {
    let name = key_text(ctx, declaration.1.name);
    let base = display(ctx, linked, declaration.0);
    Diagnostic::new(
        Code::RequiredFieldUnsatisfied,
        span_of(ctx, subject.place),
        format!(
            "`{name}` is required by `{base}` and this node supplies no value; a tagged, empty \
             declaration means a descendant must supply one"
        ),
    )
    .with_note("declared here, with a tag and no value", Some(span_of(ctx, declaration.1.key)))
}

/// `E0221` — the effective value contradicts a declared tag.
///
/// # Why the subject is named in a note
///
/// The primary span is the **effective value**, because that is the token whose
/// text has to change. That token is frequently not the failing node's: a
/// shared `<<` mixin supplying a bad value is one node written once and
/// resolved into many, so every node that includes it fails at the same
/// position. Without a note naming the subject, two failures print byte for
/// byte identically and neither says which node to fix — and a reader who
/// edits the mixin to satisfy one may break the others.
///
/// So the subject is stated first, with its own span, before the declaration
/// that was violated and before the origin note that explains why the flattened
/// node nevertheless looks consistent.
fn tags(ctx: &Ctx, linked: &Linked, subject: &Subject, diagnostics: &mut Diagnostics) {
    for (base, declared) in &subject.ancestors {
        for field in declared.fields() {
            let Some(found) = mismatch(ctx, subject, field) else { continue };
            diagnostics.push(contradicts(ctx, linked, subject, (*base, field), &found));
        }
    }
}

/// The reason a node's effective value for `field` fails the declared tag.
fn mismatch(ctx: &Ctx, subject: &Subject, field: &Field) -> Option<String> {
    let declaring = ctx.ast(field.value.0)?;
    let declared = declaring.tag(field.value.1)?;
    let effective = subject.resolved.get(field.name)?;
    // The declaration reaching the node unchanged is `E0220`'s business when it
    // is required and nobody's when it is a default.
    if effective.value == field.value {
        return None;
    }
    let supplied = ctx.ast(effective.value.0)?;
    match compare(declared, supplied, effective.value.1)? {
        Mismatch::Tagged(found) => Some(found),
        Mismatch::Kind(what) => Some(what.to_owned()),
    }
}

fn contradicts(
    ctx: &Ctx,
    linked: &Linked,
    subject: &Subject,
    declaration: (Place, &Field),
    found: &str,
) -> Diagnostic {
    let name = key_text(ctx, declaration.1.name);
    let base = display(ctx, linked, declaration.0);
    let want = ctx
        .ast(declaration.1.value.0)
        .and_then(|ast| ast.tag(declaration.1.value.1))
        .map_or_else(|| "the declared tag".to_owned(), spelling);
    let effective = subject.resolved.get(declaration.1.name).expect("a compared field");
    let subject_name = display(ctx, linked, subject.place);
    let mut diagnostic = Diagnostic::new(
        Code::DeclaredTagMismatch,
        span_of(ctx, effective.value),
        format!("`{name}` is declared `{want}` by `{base}`, and this value is {found}"),
    )
    .with_note(
        format!("`{subject_name}` is the node this is reported against"),
        Some(span_of(ctx, subject.place)),
    )
    .with_note("declared here", Some(span_of(ctx, declaration.1.key)));
    if effective.origin != subject.place {
        let origin = display(ctx, linked, effective.origin);
        diagnostic = diagnostic.with_note(
            format!(
                "`{origin}` supplies the value this node resolves to, and outranks the \
                     declaration, so the flattened node looks consistent"
            ),
            Some(span_of(ctx, effective.origin)),
        );
    }
    diagnostic
}

/// `W0301` — a field no abstract ancestor declares.
///
/// `connections` and `definition` on an `!edge` are excepted, because they are
/// the **language's** members and not the family's (D4.13).
fn undeclared(ctx: &Ctx, linked: &Linked, subject: &Subject, diagnostics: &mut Diagnostics) {
    let is_edge = edge::is_edge(ctx.interned, subject.place.0, subject.place.1);
    for field in subject.own.fields() {
        if subject.ancestors.iter().any(|(_, declared)| declared.holds(field.name)) {
            continue;
        }
        if is_edge && edge::is_reserved_member(&key_text(ctx, field.name)) {
            continue;
        }
        diagnostics.push(unknown(ctx, linked, subject, field));
    }
}

fn unknown(ctx: &Ctx, linked: &Linked, subject: &Subject, field: &Field) -> Diagnostic {
    let name = key_text(ctx, field.name);
    let base = display(ctx, linked, subject.ancestors[0].0);
    Diagnostic::new(
        Code::UndeclaredField,
        span_of(ctx, field.key),
        format!(
            "`{name}` is declared by no ancestor of this node; a misspelled field name adds a \
             junk key and silently keeps the inherited value"
        ),
    )
    .with_note(
        format!("`{base}` is one of the shapes this node claims to be"),
        Some(span_of(ctx, subject.ancestors[0].0)),
    )
}
