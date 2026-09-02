// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `W0303` over a **resolved** base.
//!
//! D4.5's additivity rule is that a contribution ranks below everything the base
//! already has — "below B's own keys, **its inclusions and its extensions**".
//! Pass 4 can only test the first of those three, because the other two are
//! resolution and resolution is this pass's. So the check is split rather than
//! duplicated:
//!
//! * **pass 4** reports a contributed key the base writes directly. That is
//!   decidable with nothing resolved, it is the common case, and it is the one
//!   worth reporting as early as possible.
//! * **here** reports a contributed key the base holds only through its own
//!   `<<` or `extends` chain. Pass 4 marked those keys not-inert, so the two
//!   sets are disjoint and no key is warned about twice.
//!
//! Both are the same finding and carry the same code, because the author's
//! mistake is the same one: the contribution loses, so it does nothing, and by
//! D4.5's identity result their own node resolves identically whether they
//! wrote `extends: *Base` or `extends: !ref ns/Base`. `W0303` is the only local
//! signal that someone wrote `!ref` where they meant an extension.

use yamlfy_syntax::{Code, Diagnostic, Diagnostics};

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
