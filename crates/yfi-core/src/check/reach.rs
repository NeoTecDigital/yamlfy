// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0217` — what a `!ref` may change.
//!
//! The two reserved keyword pairs — `private`/`public` and
//! `mutable`/`immutable` — answer the two halves of "allowed" between them, and
//! neither answers it alone. They are asked in two different passes, and which
//! pass each belongs to is a semantic decision rather than a scheduling one:
//!
//! * **`E0216` — not visible.** Asked in **pass 4**, inside path resolution
//!   ([`crate::link::path`]), before the landing is searched for a name and
//!   before any `.` member is addressed.
//! * **`E0217` — not writable.** Asked here, in pass 5, of a target that
//!   already resolved. The path carries `!ref`, which declares mutation intent,
//!   and the target sits in a scope this one may not write. The fix is a
//!   different keyword — or the plain path, which asks for nothing.
//!
//! # Why visibility is not asked here
//!
//! It used to be, and that was a leak. Resolution ran first, so
//! `vault/Secret.password`, `vault/Secret.nosuch` and `vault/NoSuchNode`
//! against one private scope earned three *distinguishable* answers — `E0216`
//! whose note printed the definition's file, line and column; `E0218`, "the
//! node it names does not hold `nosuch`"; and `E0213`, "no definition called
//! `NoSuchNode`". Between them an outsider enumerates a private scope's node
//! names and each node's member names, which is precisely the access D4.12 says
//! it has none of: *"If B is private and outside A's scope, A has no access to B
//! at all — not its members, not its public surface, not its name"*.
//!
//! A gate that only decorates the diagnostic is not a gate. So the question
//! moved to the only place that closes it — in front of the lookup — and an
//! invisible landing now resolves to **nothing**. That is also what makes the
//! ordering structural instead of conventional: a reference this pass sees at
//! all is one whose target is already in view, so `E0217` can no longer be
//! reported about a scope whose `visibility:` was the real obstacle.
//!
//! Nothing downstream sees an invisible target either — no `is_a` edge, no
//! ancestry, no `E0220` blaming a base the reader may not have.
//!
//! # Why mutability is a gate rather than a record
//!
//! An extended reference **is a write**, performed at compile time: it installs
//! `own(A)` on a base that every other B in the program then carries. Phase 1
//! shipped no runtime writer, and the axis was therefore recorded and
//! propagated but never consulted. `!ref` is what consults it. The predicate is
//! [`ScopeTree::not_writable_by`], the composed analogue of the one the
//! visibility gate uses, so the two axes can never disagree about who blocked
//! what.
//!
//! # Why it is its own code and not `E0241`
//!
//! `E0241` is raised by `discover` about an `imports:` entry — a header line,
//! with a header's fix. This is raised about a path written in the body of a
//! document, and an author told "import target not visible" about a line
//! containing no import has been sent to the wrong place.
//!
//! # `brute` forces the second gate and never the first
//!
//! A `!ref` written under a `brute` member (D6.6) performs its write even where
//! `E0217` would refuse it, and the refusal becomes `W0304` — the write stands
//! and the forcing is recorded. That asymmetry is the point: mutability is a
//! *policy* about what may be changed, and a policy is the kind of thing an
//! author can be entitled to override in the open. Visibility is not. `E0216`
//! says you may not have this at all, and a member cannot grant itself sight of
//! what it was never shown, so `brute` is never consulted for it. Since `E0216`
//! moved into resolution that is structural rather than ordered: an invisible
//! target never resolves, so this pass never sees it and `brute` has nothing to
//! force.
//!
//! A path naming a definition **in its own file** needs neither check: a file
//! can always see and always write what it wrote.

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
