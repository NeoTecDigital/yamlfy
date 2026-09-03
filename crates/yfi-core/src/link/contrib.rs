// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extended-reference contributions: `E0214` and `W0303` (D4.11).
//!
//! Nothing is *installed* here — pass 5 is what obeys D4.5's additivity — and
//! only the two findings decidable without resolving anything are reported. So
//! "the base already defines" means `own(base)`, the keys the base writes
//! directly; a key it holds only through its own `<<` or `extends` chain is
//! `check`'s half of `W0303`, over disjoint input (§4).

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId};

use super::clause::{Clause, ClauseKind, OperandForm};
use super::keys::{own_keys, OwnKey};
use super::table::Table;
use super::{source_order, value, Ctx};
use crate::symbol::Symbol;

/// One key an extended reference contributes to its base.
pub struct ContributedKey {
    /// The interned key text.
    pub name: Symbol,
    /// The key node in the contributing file.
    pub key: NodeId,
    /// The value node in the contributing file.
    pub value: NodeId,
    /// Whether the base already defines the key, in which case the
    /// contribution loses and does nothing (`W0303`).
    pub inert: bool,
}

/// One `extends: !ref B` and everything it installs on B.
pub struct Contribution {
    /// The base, which every node that is a B now carries these keys through.
    pub base: (FileId, NodeId),
    /// The contributing file.
    pub file: FileId,
    /// The contributing node, `A`.
    pub node: NodeId,
    /// The `!ref` operand that made the contribution.
    pub operand: NodeId,
    /// `own(A)`, never `R(A)`: if the base absorbed `R(A)` then `R(B)` would
    /// depend on `R(A)`, which depends on `R(B)` because A is a type of B, and
    /// every extended reference would be a cycle.
    pub keys: Vec<ContributedKey>,
}

/// Collect every contribution, in the order they are written, reporting
/// `W0303` and `E0214`.
pub(crate) fn collect(
    ctx: &Ctx,
    table: &Table,
    clauses: &[Clause],
    diagnostics: &mut Diagnostics,
) -> Vec<Contribution> {
    let mut out = gather(ctx, clauses);
    out.sort_by_key(|held| source_order(ctx.project, ctx.interned, held.file, held.node));
    for contribution in &mut out {
        mark_inert(ctx, table, contribution, diagnostics);
    }
    report_conflicts(ctx, table, &out, diagnostics);
    out
}

fn gather(ctx: &Ctx, clauses: &[Clause]) -> Vec<Contribution> {
    let mut out = Vec::new();
    for clause in clauses.iter().filter(|c| c.kind == ClauseKind::Extension) {
        let operands = clause.operands.iter().filter(|o| o.form == OperandForm::Ref);
        for operand in operands {
            let keys = own_keys(ctx, clause.file, clause.owner)
                .into_iter()
                .map(|key| ContributedKey {
                    name: key.name,
                    key: key.key,
                    value: key.value,
                    inert: false,
                })
                .collect();
            out.push(Contribution {
                base: operand.target,
                file: clause.file,
                node: clause.owner,
                operand: operand.node,
                keys,
            });
        }
    }
    out
}

/// Flag every contributed key the base already writes, and warn about it.
fn mark_inert(
    ctx: &Ctx,
    table: &Table,
    contribution: &mut Contribution,
    diagnostics: &mut Diagnostics,
) {
    let base = own_keys(ctx, contribution.base.0, contribution.base.1);
    let path = table.path_of(contribution.base.0, contribution.base.1).unwrap_or("the base");
    for key in &mut contribution.keys {
        key.inert = base.iter().any(|held| held.name == key.name);
    }
    for key in contribution.keys.iter().filter(|key| key.inert) {
        let Some(existing) = base.iter().find(|held| held.name == key.name) else { continue };
        diagnostics.push(inert(ctx, contribution, key, existing, path));
    }
}

fn inert(
    ctx: &Ctx,
    contribution: &Contribution,
    key: &ContributedKey,
    existing: &OwnKey,
    path: &str,
) -> Diagnostic {
    let name = ctx.interned.symbols().resolve(key.name).unwrap_or_default();
    Diagnostic::new(
        Code::InertContribution,
        ctx.span_of(contribution.file, key.key),
        format!(
            "`{name}` is contributed to `{path}`, which already defines it; an extended \
             reference may add a key to a base but never change one, so this does nothing"
        ),
    )
    .with_note("the base defines it here", Some(ctx.span_of(contribution.base.0, existing.key)))
    .with_note(
        "contributed through this extended reference",
        Some(ctx.span_of(contribution.file, contribution.operand)),
    )
}

/// Report two contributions of one key to one base with different values.
///
/// Each key is compared against the **first** contribution of it, so `n`
/// disagreeing files produce `n - 1` diagnostics all pointing back at the same
/// place, rather than every pair reported twice.
fn report_conflicts(
    ctx: &Ctx,
    table: &Table,
    contributions: &[Contribution],
    diagnostics: &mut Diagnostics,
) {
    let mut first: Claimed = HashMap::new();
    for contribution in contributions {
        // A key the base itself defines is inert from every contributor, so
        // the base's own value decides it and there is nothing to rank.
        for key in contribution.keys.iter().filter(|key| !key.inert) {
            compare(ctx, table, &mut first, (contribution, key), diagnostics);
        }
    }
}

/// The first contribution of each `(base, key)`, which every later one is
/// compared against.
type Claimed<'a> = HashMap<((FileId, NodeId), Symbol), (&'a Contribution, &'a ContributedKey)>;

fn compare<'a>(
    ctx: &Ctx,
    table: &Table,
    first: &mut Claimed<'a>,
    later: (&'a Contribution, &'a ContributedKey),
    diagnostics: &mut Diagnostics,
) {
    match first.entry((later.0.base, later.1.name)) {
        Entry::Vacant(slot) => {
            slot.insert(later);
        }
        Entry::Occupied(slot) => {
            let held = *slot.get();
            let ours = (later.0.file, later.1.value);
            let theirs = (held.0.file, held.1.value);
            if !value::equal(ctx, ours, theirs) {
                diagnostics.push(conflict(ctx, table, later, held));
            }
        }
    }
}

fn conflict(
    ctx: &Ctx,
    table: &Table,
    later: (&Contribution, &ContributedKey),
    first: (&Contribution, &ContributedKey),
) -> Diagnostic {
    let name = ctx.interned.symbols().resolve(later.1.name).unwrap_or_default();
    let path = table.path_of(later.0.base.0, later.0.base.1).unwrap_or("the base");
    let base = table.get(path).map(|definition| definition.span);
    Diagnostic::new(
        Code::ConflictingExtension,
        ctx.span_of(later.0.file, later.1.key),
        format!(
            "two extended references contribute `{name}` to `{path}` with different values; \
             nothing ranks two files' claims on a base except their filenames, so this \
             cannot be ordered"
        ),
    )
    .with_note("the other contribution is here", Some(ctx.span_of(first.0.file, first.1.key)))
    .with_note("both extend this definition", base)
}
