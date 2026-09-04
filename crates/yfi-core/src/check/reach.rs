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
//!
//! **`override` is not asked this question at all** (D4.14). It declares
//! priority among the nodes that claim a target, not intent to modify one, so
//! it sets no capability flag and no reference reaches here on its account:
//! `extends: override P` into an immutable scope is legal, and the refusal
//! below is always about the tag. That is why the message names `!ref` however
//! the operand was spelled, and why the fix it offers is truthful — dropping
//! the tag from `extends: !ref override P` leaves the claim standing and asks
//! the axis for nothing.
//!
//! # How far `brute` reaches
//!
//! `brute` is **required** for `!ref override` into a target that is not
//! mutable (D6.4b), so it has to be reachable from a clause operand. An
//! operator is not a member and binds no key, so the member that forces one is
//! the member whose *value* writes the clause — one level up, and one only:
//!
//! ```text
//! brute k: !ref P                 // the member's own value
//! brute Amend: !node              // the member's value writes the clause
//!   extends: !ref override P
//! brute outer:
//!   inner:
//!     extends: !ref override P    // not forced: `inner` is a member of the
//! ```                             // value, and members carry their own word
//!
//! The rule is one sentence because clauses are not members: nothing that is
//! itself a member of the value is reached, so a refusal keeps standing until
//! its own key says otherwise. Widening it to the whole subtree was rejected —
//! the author who writes `brute` reads the block it governs, and a `brute` that
//! reaches past that silences a refusal nobody asked it to silence.

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use crate::link::{Ctx, Linked, RefRole, Reference};
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
    if forces(ctx, reference) {
        diagnostics.push(forced(ctx, reference, target, (blocker, observer)));
        return;
    }
    diagnostics.push(unwritable(ctx, reference, target, (blocker, observer)));
}

/// Whether the member this reference is written under declared `brute`.
fn forces(ctx: &Ctx, reference: &Reference) -> bool {
    let Some(key) = declaring_member(ctx, reference) else { return false };
    ctx.interned.member_of(reference.file, key).is_some_and(|held| held.flags.is_brute())
}

/// The key of the member a reference is written under, which is the one
/// position `brute` can be spelled in.
///
/// Two shapes, one step apart and no further: a data `!ref` is a member's
/// **value**, and a clause operand is written by the mapping that **is** a
/// member's value. A reference under neither — a sequence element, a clause
/// written at a document root — has no member on which the word could have been
/// written, and forces nothing.
///
/// The flags are read from [`crate::intern`] rather than re-lexed off the key
/// text, so there is one reader of a member's declaration instead of two. That
/// is also what makes D4.2's escape work here: `"brute k": !ref P` is a member
/// genuinely called `brute k`, and the one word where forcing is decided is the
/// last place the escape should stop.
fn declaring_member(ctx: &Ctx, reference: &Reference) -> Option<NodeId> {
    let owner = reference.owner?;
    match reference.role {
        RefRole::Inclusion | RefRole::Extension => {
            let above = ctx.interned.parent_of(reference.file, owner)?;
            key_of(ctx, reference.file, above, owner)
        }
        _ => key_of(ctx, reference.file, owner, reference.node),
    }
}

/// The key `mapping` holds `value` under, if it holds it as an entry value.
fn key_of(ctx: &Ctx, file: FileId, mapping: NodeId, value: NodeId) -> Option<NodeId> {
    let entries = ctx.ast(file)?.entries(mapping)?;
    entries.iter().find(|entry| entry.value == value).map(|entry| entry.key)
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
    // The tag is named however the operand was spelled, because the tag is what
    // was refused. `override` beside it declares a priority among claimants and
    // is not a write, so naming it would send an author to drop the one word
    // that changes nothing about this answer.
    let mut held = Diagnostic::new(
        Code::RefNotWritable,
        reference.span,
        format!(
            "`!ref {}` declares that this context intends to modify the target, and the \
             target may not be written from here",
            reference.text
        ),
    )
    .with_note(composed(scopes, blocked, "immutable"), at)
    .with_note(
        format!(
            "drop the `!ref` if `{}` is meant to be read rather than changed; a plain path \
             asks for nothing, and `brute` on the member that writes this performs the write \
             anyway",
            reference.text
        ),
        None,
    );
    if reference.overrides {
        held = held.with_note(
            "the priority claim `override` makes is not what was refused and survives dropping \
             the tag: it ranks this node against the target's other claimants rather than \
             modifying it",
            None,
        );
    }
    held.with_note("the definition is here", Some(span_of(ctx, target)))
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
