// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `definition` — the handles an edge gives its positions, and `E0225`.
//!
//! A handle is a **name for a position** in `connections`, so an endpoint can
//! be addressed as `source` rather than as `0`. That is the whole of it, and
//! every rule below follows from it: the value is an index and never a node, a
//! position has one spelling, the mapping is many-to-one on purpose, and the
//! two names the language owns on an edge are not available to take.
//!
//! # What survives a malformed `connections`
//!
//! The two members are read **independently** (D4.13), which has to hold when
//! one of them is broken. `definition`'s own shape, a handle taking an owned
//! name and a value that is not a position at all are wrong whatever the
//! sequence above them holds, so all three are reported. Only the **bound** is
//! lost with the sequence, so only *past the end* is withheld: raised against
//! zero it would print one fault once per handle, and every one of them would
//! disappear when the member above was fixed.

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, NodeId};

use crate::edge::{self, DEFINITION};
use crate::link::{Ctx, Linked};
use crate::symbol::Symbol;

use super::super::names::{display, span_of};
use super::super::view::{Field, Place};
use super::shape;

/// The handles `definition` declares, each checked against the number of
/// positions `connections` writes — or against nothing, when `connections` was
/// never read.
///
/// The two members are read **independently**, and that has to survive one of
/// them being malformed: `definition`'s own shape, a handle taking a name the
/// language owns and a value that is not a position at all are all wrong
/// whatever the sequence above them holds. Only the **bound** is lost with the
/// sequence, so only `OutOfRange` is withheld — reporting it against zero would
/// print one fault as one per handle, and every one of them would go away when
/// the member above was fixed.
pub(super) fn handles(
    ctx: &Ctx,
    linked: &Linked,
    subject: Place,
    field: Option<&Field>,
    bound: Option<usize>,
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
        let held = Unbound { subject, at, name: text, count: bound.unwrap_or_default(), origin };
        match handle(ast, text, entry.value, bound) {
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
///
/// `bound` is `None` when `connections` yielded no sequence. The first two
/// rejections still apply — neither asks how long the sequence is — and the
/// third is not raised, because there is no length to be past the end of.
fn handle(ast: &Ast, name: &str, value: NodeId, bound: Option<usize>) -> Result<u32, Rejection> {
    if edge::is_reserved_member(name) {
        return Err(Rejection::Reserved);
    }
    let text = ast.scalar(value).ok_or(Rejection::NotAnIndex)?;
    let index = index_of(&text.value).ok_or(Rejection::NotAnIndex)?;
    match bound {
        Some(count) if (index as usize) >= count => Err(Rejection::OutOfRange(index)),
        _ => Ok(index),
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
