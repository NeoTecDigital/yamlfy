// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `E0219` — a `!ref` binding that shadows a definition of its own file.
//!
//! D4.12 owns the argument: a binding outranks the file's own definitions, so
//! adding one `!ref` line silently retargets every bare name already written,
//! and reversing the precedence only trades that for a quieter hole — so the
//! ambiguity itself is refused. The comparison is **file-wide**, because a
//! definition is a file's ([`super::table::Table::in_file`]) while a binding is
//! a document's. Imported names are not definitions of this file and do not
//! collide; `W0300` already governs those.

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
