// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `W0303` over a **resolved** base, and `W0305`, which is its mirror.
//!
//! D4.5 ranks a contribution below the base's own keys, its inclusions *and*
//! its extensions; pass 4 can only test the first, because the other two are
//! resolution. So the check is split rather than duplicated, over disjoint
//! inputs and under one code (§4): pass 4 reports a contributed key the base
//! writes directly, and this pass reports one the base holds only through its
//! own `<<` or `extends` chain, which pass 4 marked not-inert.
//!
//! **`override` inverts the condition and therefore does not split** (D4.14).
//! An overriding contribution outranks everything in the base, so it is never
//! inert and `W0303` has nothing to say about it; the fault worth reporting is
//! the opposite one — that it landed on no key of the base at all — and *that*
//! is decidable only here, because "the base already holds it" is exactly the
//! three things D4.5 names and only a resolved base answers all three. One
//! pass, one code, `W0305`.
//!
//! The two conditions are complements over one input — *does the base hold this
//! key* — which is what keeps "silent where `W0303` speaks, and speaking where
//! it is silent" one decision rather than two rules that can drift apart.

use yfi_syntax::{Code, Diagnostic, Diagnostics, Span};

use crate::link::{ContributedKey, Contribution, Ctx, Linked};

use super::names::{display, key_text, span_of};
use super::resolve::Views;

/// Report every contribution that does nothing: an additive key the base
/// already holds (`W0303`), and an overriding key it does not hold at all
/// (`W0305`).
pub(crate) fn inert(ctx: &Ctx, linked: &Linked, views: &Views, diagnostics: &mut Diagnostics) {
    for contribution in linked.contributions() {
        let Some(base) = views.base(contribution.base) else { continue };
        let path = display(ctx, linked, contribution.base);
        // `inert` is pass 4's verdict against `own(base)`; those are already
        // reported and must not be reported again.
        for key in contribution.keys.iter().filter(|key| !key.inert) {
            let held = (contribution, key);
            match (contribution.overrides, base.get(key.name)) {
                (false, Some(found)) => {
                    diagnostics.push(inherited(ctx, held, &path, span_of(ctx, found.key)));
                }
                (true, None) => diagnostics.push(vacuous(ctx, held, &path)),
                _ => (),
            }
        }
    }
}

/// One contribution and one of its keys, which every message below quotes.
type Claim<'a> = (&'a Contribution, &'a ContributedKey);

/// `W0303` — an additive contribution of a key the base already holds through
/// its own `<<` or `extends` chain.
fn inherited(ctx: &Ctx, held: Claim, path: &str, found: Span) -> Diagnostic {
    let name = key_text(ctx, held.1.name);
    Diagnostic::new(
        Code::InertContribution,
        span_of(ctx, (held.0.file, held.1.key)),
        format!(
            "`{name}` is contributed to `{path}`, which already holds it through its own \
             inheritance; an extended reference may add a key to a base but never change one, \
             so this does nothing"
        ),
    )
    .with_note("the base already inherits it from here", Some(found))
    .with_note("contributed through this extended reference", Some(operand(ctx, held.0)))
}

/// `W0305` — an overriding contribution that overrides nothing.
fn vacuous(ctx: &Ctx, held: Claim, path: &str) -> Diagnostic {
    let name = key_text(ctx, held.1.name);
    Diagnostic::new(
        Code::VacuousOverride,
        span_of(ctx, (held.0.file, held.1.key)),
        format!(
            "`{name}` is contributed to `{path}` with `override`, and `{path}` does not hold \
             `{name}`; there was nothing to replace"
        ),
    )
    .with_note(
        "an `override` that lands on no key is the additive contribution it would have been \
         unwritten, and D4.5's identity result makes that invisible from here; check the \
         spelling of the key, or drop the keyword",
        None,
    )
    .with_note("contributed through this extended reference", Some(operand(ctx, held.0)))
}

fn operand(ctx: &Ctx, contribution: &Contribution) -> Span {
    span_of(ctx, (contribution.file, contribution.operand))
}
