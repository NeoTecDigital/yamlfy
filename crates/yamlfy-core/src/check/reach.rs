// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0216` and `E0217` — what a path may reach, and what `!ref` may change.
//!
//! Pass 4 resolves a path against the project's directories and files and
//! reports `E0213` when it names nothing. Resolving is not the same as being
//! *allowed* to, and the two reserved keyword pairs — `private`/`public` and
//! `mutable`/`immutable` — answer the two halves of "allowed" between them.
//! Neither pair answers it alone, so both are consulted, in this order:
//!
//! * **`E0216` — not visible.** The target sits in a scope the referencing
//!   scope cannot see. Visibility composes over the whole `root → target` path
//!   (D6.5), so the scope that blocked the reach may be several directories
//!   above the target. There is no fix: you may not have this at all.
//! * **`E0217` — not writable.** The path carries `!ref`, which declares
//!   mutation intent, and the target sits in a scope this one may not write.
//!   The fix is a different keyword — or the plain path, which asks for
//!   nothing.
//!
//! # Why the order is fixed
//!
//! An extended reference into something you cannot see is not a mutability
//! failure. Reporting it as one sends the author to change `mutability:` on a
//! scope whose `visibility:` is what actually stopped them, and the change they
//! make will not help. Visibility is therefore decided first and `E0217` is
//! reached only for a target that is already in view — exactly the ordering
//! D4.12 fixed for the reach codes when there were two of them.
//!
//! # Why mutability is now a gate rather than a record
//!
//! An extended reference **is a write**, performed at compile time: it installs
//! `own(A)` on a base that every other B in the program then carries. Phase 1
//! shipped no runtime writer, and the axis was therefore recorded and
//! propagated but never consulted. `!ref` is what consults it. The predicate is
//! [`ScopeTree::not_writable_by`], the composed analogue of the one `E0216`
//! uses, so the two axes can never disagree about who blocked what.
//!
//! # Why they are their own codes and not `E0241`
//!
//! `E0241` is raised by `discover` about an `imports:` entry — a header line,
//! with a header's fix. These are raised by `check` about a path written in the
//! body of a document, and an author told "import target not visible" about a
//! line containing no import has been sent to the wrong place.
//!
//! A path naming a definition **in its own file** needs neither check: a file
//! can always see and always write what it wrote.

use yamlfy_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use crate::link::{Ctx, Linked, Reference};
use crate::scope::{ScopeId, ScopeTree};

use super::names::span_of;

/// Check every resolved path reference of the project.
pub(crate) fn reach(ctx: &Ctx, linked: &Linked, diagnostics: &mut Diagnostics) {
    for reference in linked.references() {
        let Some(target) = reference.target else { continue };
        if target.0 == reference.file {
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
    if let Some(blocker) = scopes.blocked_by(scope, observer) {
        diagnostics.push(invisible(ctx, reference, target, (blocker, observer)));
        return;
    }
    if !reference.capability {
        return;
    }
    if let Some(blocker) = scopes.not_writable_by(scope, observer) {
        diagnostics.push(unwritable(ctx, reference, target, (blocker, observer)));
    }
}

fn invisible(
    ctx: &Ctx,
    reference: &Reference,
    target: (FileId, NodeId),
    blocked: (ScopeId, ScopeId),
) -> Diagnostic {
    let scopes = ctx.project.scopes();
    let at: Option<Span> = scopes.get(blocked.0).and_then(|scope| scope.visibility_span);
    Diagnostic::new(
        Code::RefNotVisible,
        reference.span,
        format!(
            "`{}` names a definition this scope cannot see; the path grants the reach, and \
             `private` decides that you may not have it",
            reference.text
        ),
    )
    .with_note(composed(scopes, blocked, "private"), at)
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

/// The note both codes carry: which scope shut the observer out, and that the
/// answer was composed over the whole path rather than read off the target.
fn composed(scopes: &ScopeTree, blocked: (ScopeId, ScopeId), keyword: &str) -> String {
    format!(
        "`{}` is `{keyword}` and `{}` is outside it; both axes compose over the whole path from \
         the root",
        scopes.qualified(blocked.0),
        scopes.qualified(blocked.1)
    )
}
