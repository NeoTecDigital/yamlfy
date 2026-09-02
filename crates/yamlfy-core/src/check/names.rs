// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Naming a node in a diagnostic, and finding its span.
//!
//! `E0212` is a **whole-program** diagnostic: a file that is acyclic alone can
//! be the second half of a cycle completed by a file it has never heard of. So
//! a note that says only "here" is not enough — the reader may be looking at a
//! file they did not open. Every node named below is therefore named the way
//! the project addresses it, preferring the canonical path over the local
//! anchor, and every cross-file edge names the file it lands in.

use yamlfy_syntax::{FileId, Pos, Span};

use crate::link::{Ctx, Linked};

use super::view::Place;

/// A node's span. The fallback cannot be reached — every place named here came
/// from an arena this pass walked — and is an empty span in the right file
/// rather than a panic, because a diagnostic that cannot be placed must still
/// be reported.
pub(crate) fn span_of(ctx: &Ctx, place: Place) -> Span {
    ctx.ast(place.0)
        .and_then(|ast| ast.nodes().get(place.1.index()).map(|held| held.span))
        .unwrap_or_else(|| Span::empty(place.0, Pos::default()))
}

/// How a node is named in a diagnostic: its canonical path when it has one, its
/// anchor name when it does not, and its position when it has neither.
pub(crate) fn display(ctx: &Ctx, linked: &Linked, place: Place) -> String {
    if let Some(path) = linked.path_of(place.0, place.1) {
        return path.to_owned();
    }
    if let Some(name) = anchor_name(ctx, place) {
        return format!("&{name}");
    }
    format!("the mapping at {}", short(ctx, place.0))
}

/// The anchor a node carries, if it carries one.
fn anchor_name(ctx: &Ctx, place: Place) -> Option<String> {
    let ast = ctx.ast(place.0)?;
    let id = ast.nodes().get(place.1.index())?.anchor?;
    ast.anchors().get(id).map(|def| def.name.to_string())
}

/// A file's path relative to the project root.
pub(crate) fn short(ctx: &Ctx, file: FileId) -> String {
    ctx.project
        .file(file)
        .map_or_else(|| "an unknown file".to_owned(), |held| held.relative.display().to_string())
}

/// The interned text of a key, for a message.
pub(crate) fn key_text(ctx: &Ctx, name: crate::symbol::Symbol) -> String {
    ctx.interned.symbols().resolve(name).unwrap_or("<unknown>").to_owned()
}
