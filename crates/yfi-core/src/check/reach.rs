// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0217` — what a `!ref` may change.
//!
//! The two reserved keyword pairs answer the two halves of "allowed" and are
//! asked in two different passes; which pass each belongs to is a semantic
//! decision rather than a scheduling one (D4.12). **`E0216` — not visible** —
//! is pass 4's, asked inside path resolution ([`crate::link::path`]) in front
//! of the lookup, so an invisible landing resolves to nothing and no reference
//! reaches this pass for `E0217` to be asked about. **`E0217` — not writable** —
//! is asked here, of a target that already resolved whose path carried `!ref`.
//!
//! The predicate is [`ScopeTree::not_writable_by`], the composed analogue of
//! the one the visibility gate uses, so the two axes can never disagree about
//! who blocked what. It is not `E0241`, which `discover` raises about a header
//! line and answers with a header's fix. A path naming a definition in its own
//! file needs neither check. The fix is a different keyword — `brute`, below —
//! or the plain path, which asks for nothing.

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use crate::link::{Ctx, Linked, Reference};
use crate::member;
use crate::scope::{ScopeId, ScopeTree};

use super::names::span_of;

/// Check every resolved path reference of the project.
pub(crate) fn reach(ctx: &Ctx, linked: &Linked, diagnostics: &mut Diagnostics) {
    for reference in linked.references() {
        let Some(target) = reference.target else { continue };
        if target.0 == reference.file || !reference.capability {
            continue;
        }
        check_one(ctx, reference, target, diagnostics);
    }
}

fn check_one(
    ctx: &Ctx,
    reference: &Reference,
    target: (FileId, NodeId),
    diagnostics: &mut Diagnostics,
) {
    let Some(observer) = ctx.interned.scope_of(reference.file, reference.node) else { return };
    let Some(scope) = ctx.interned.scope_of(target.0, target.1) else { return };
    let scopes = ctx.project.scopes();
    let Some(blocker) = scopes.not_writable_by(scope, observer) else { return };
    if forces(reference) {
        diagnostics.push(forced(ctx, reference, target, (blocker, observer)));
        return;
    }
    diagnostics.push(unwritable(ctx, reference, target, (blocker, observer)));
}

/// Whether the member this reference binds declared `brute`.
///
/// The flag is read from the key the `!ref` sits under, which is the one
/// position [`member::split`] is defined over. A `!ref` that binds no key —
/// a sequence element, or a clause operand — forces nothing, because there is
/// no member on which the word could have been written.
fn forces(reference: &Reference) -> bool {
    reference.binds.as_ref().is_some_and(|key| member::split(key).0.is_brute())
}

fn forced(
    ctx: &Ctx,
    reference: &Reference,
    target: (FileId, NodeId),
    blocked: (ScopeId, ScopeId),
) -> Diagnostic {
    let scopes = ctx.project.scopes();
    let at: Option<Span> = scopes.get(blocked.0).and_then(|scope| scope.mutability_span);
    Diagnostic::new(
        Code::ForcedWrite,
        reference.span,
        format!(
            "`brute` forces this write: `{}` names a target that may not be written from \
             here, and the write is performed anyway",
            reference.text
        ),
    )
    .with_note(composed(scopes, blocked, "immutable"), at)
    .with_note("the definition is here", Some(span_of(ctx, target)))
}

fn unwritable(
    ctx: &Ctx,
    reference: &Reference,
    target: (FileId, NodeId),
    blocked: (ScopeId, ScopeId),
) -> Diagnostic {
    let scopes = ctx.project.scopes();
    let at: Option<Span> = scopes.get(blocked.0).and_then(|scope| scope.mutability_span);
    Diagnostic::new(
        Code::RefNotWritable,
        reference.span,
        format!(
            "`!ref {}` declares that this context intends to modify the target, and the target \
             may not be written from here",
            reference.text
        ),
    )
    .with_note(composed(scopes, blocked, "immutable"), at)
    .with_note(
        format!(
            "drop the `!ref` if `{}` is meant to be read rather than changed; a plain path asks \
             for nothing",
            reference.text
        ),
        None,
    )
    .with_note("the definition is here", Some(span_of(ctx, target)))
}

/// The note the code carries: which scope shut the observer out, and that the
/// answer was composed over the whole path rather than read off the target.
///
/// The target is already visible by the time this is reached, so naming its
/// scope and pointing at the line that closed it discloses nothing.
fn composed(scopes: &ScopeTree, blocked: (ScopeId, ScopeId), keyword: &str) -> String {
    format!(
        "`{}` is `{keyword}` and `{}` is outside it; both axes compose over the whole path from \
         the root",
        scopes.qualified(blocked.0),
        scopes.qualified(blocked.1)
    )
}
