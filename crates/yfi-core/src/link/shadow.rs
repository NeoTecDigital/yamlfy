// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0219` — a `!ref` binding that shadows a definition of its own file.
//!
//! D4.12 justifies the bare path form by saying that *"making the bare form
//! file-local means a name never silently starts resolving somewhere else when
//! a sibling file is added"*. A `!ref` binding outranks the file's own
//! definitions, so the guarantee held against a new *file* and not against a
//! new *line*:
//!
//! ```text
//! --- !type &Widget
//! near: !!int 1
//! --- !node &Use
//! Widget: !ref other/Widget    # <- add this one line
//! child: !node
//!   extends: Widget            # <- unchanged; now names another directory
//! ```
//!
//! With matching keys the retarget produces no diagnostic, no shape change and
//! no value change worth noticing — the strongest form of the fault D1.8
//! refuses, because nothing about the program says it happened.
//!
//! # Why the collision is the error rather than the precedence
//!
//! The alternative was to make local definitions outrank bindings. That closes
//! the retarget and opens a quieter hole: the binding would still be written,
//! still carry the capability, and still be the thing `Widget.member` addresses
//! through in an author's head — while every bare `Widget` silently meant
//! something else. One spelling would denote two things depending on whether a
//! `.` followed it, which is the same fault wearing the opposite sign.
//!
//! So the precedence is left exactly where it was, documented and unchanged,
//! and **the ambiguity itself is refused**. There is no resolution order to
//! learn, no line whose meaning depends on a line elsewhere, and the fix is a
//! rename the author can make in one place.
//!
//! # Scope of the comparison
//!
//! A binding is a document's; a definition is a file's, which is what a bare
//! path resolves against ([`super::table::Table::in_file`]). So the comparison
//! is file-wide: a binding in the second document of a file still makes one
//! spelling mean two things in one file, and that is what the rule is about.
//! Imported names are not definitions of this file and do not collide — they
//! are already governed by `W0300`.

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, Span};

use super::refs::Reference;
use super::table::Table;
use super::Ctx;

/// Report every `!ref` binding whose name its own file also defines.
pub(crate) fn check(
    ctx: &Ctx,
    table: &Table,
    references: &[Reference],
    diagnostics: &mut Diagnostics,
) {
    for reference in references {
        let Some(name) = binding_name(reference) else { continue };
        if table.in_file(reference.file, name).is_none() {
            continue;
        }
        let Some(defined) = definition_span(ctx, reference.file, name) else { continue };
        diagnostics.push(shadowed(reference, name, defined));
    }
}

/// The name a reference binds, when it binds one at all. Only a `!ref` does:
/// a plain path is a data edge and establishes no name (D4.3).
fn binding_name(reference: &Reference) -> Option<&str> {
    reference.capability.then_some(reference.binds.as_deref()).flatten()
}

/// Where `name`'s `&name` token is written in `file`.
///
/// The **last** definition, because a name repeated within one file is a state
/// sequence whose last state is what the name denotes (D5.1, D5.2) — the same
/// fold [`super::table`] applies before anything is indexed.
fn definition_span(ctx: &Ctx, file: FileId, name: &str) -> Option<Span> {
    let ast = ctx.ast(file)?;
    ast.anchors()
        .defs()
        .iter()
        .filter(|def| !def.is_imported() && &*def.name == name)
        .next_back()
        .map(|def| def.span)
}

fn shadowed(reference: &Reference, name: &str, defined: Span) -> Diagnostic {
    Diagnostic::new(
        Code::BindingShadowsDefinition,
        reference.span,
        format!(
            "`{name}` is bound here by a `!ref` and is also defined in this file; a bare path \
             is file-local, so every `{name}` already written here would start naming \
             `{}` instead",
            reference.text
        ),
    )
    .with_note(format!("`&{name}` is defined in this file"), Some(defined))
    .with_note(
        format!(
            "rename the bound key, or name the definition with a path -- `./{name}` is this \
             directory's and is never captured by a binding"
        ),
        None,
    )
}
