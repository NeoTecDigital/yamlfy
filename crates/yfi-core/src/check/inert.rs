// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `W0303` over a **resolved** base.
//!
//! D4.5 ranks a contribution below the base's own keys, its inclusions *and*
//! its extensions; pass 4 can only test the first, because the other two are
//! resolution. So the check is split rather than duplicated, over disjoint
//! inputs and under one code (§4): pass 4 reports a contributed key the base
//! writes directly, and this pass reports one the base holds only through its
//! own `<<` or `extends` chain, which pass 4 marked not-inert.

use yfi_syntax::{Code, Diagnostic, Diagnostics};

use crate::link::{Ctx, Linked};

use super::names::{display, key_text, span_of};
use super::resolve::Views;

/// Report every contributed key the base already holds through inheritance.
pub(crate) fn inert(ctx: &Ctx, linked: &Linked, views: &Views, diagnostics: &mut Diagnostics) {
    for contribution in linked.contributions() {
        let Some(base) = views.base(contribution.base) else { continue };
        // `inert` is pass 4's verdict against `own(base)`; those are already
        // reported and must not be reported again.
        for key in contribution.keys.iter().filter(|key| !key.inert) {
            let Some(held) = base.get(key.name) else { continue };
            let name = key_text(ctx, key.name);
            let path = display(ctx, linked, contribution.base);
            diagnostics.push(
                Diagnostic::new(
                    Code::InertContribution,
                    span_of(ctx, (contribution.file, key.key)),
                    format!(
                        "`{name}` is contributed to `{path}`, which already holds it through \
                         its own inheritance; an extended reference may add a key to a base \
                         but never change one, so this does nothing"
                    ),
                )
                .with_note("the base already inherits it from here", Some(span_of(ctx, held.key)))
                .with_note(
                    "contributed through this extended reference",
                    Some(span_of(ctx, (contribution.file, contribution.operand))),
                ),
            );
        }
    }
}
