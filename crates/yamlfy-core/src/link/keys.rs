// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a node writes for itself, and what counts as an inheritance key.
//!
//! `own(A)` is A's literal keys **with its clauses removed** (D4.9): a clause is
//! resolved in the mapping that writes it and then ceases to exist. It appears
//! in no resolved view and is never re-applied through a further clause, so the
//! keys an extended reference contributes never include the `extends` or `<<`
//! entry that carried it.
//!
//! # Two spellings of a member list
//!
//! A node's members may be written as a **mapping**, where each key names a
//! member and its value declares it (D7.3), or as a **sequence**, where each
//! plain scalar item names a member and declares nothing about it — D7.3's
//! third state, spelled without the colon. Both are read here, and a member is
//! a member either way; only the mapping form can state a declaration, which is
//! why the sequence form is the shorter one rather than a second model.

use std::collections::HashMap;

use yamlfy_syntax::{Ast, Code, Diagnostic, Diagnostics, NodeId, ScalarStyle};

use super::Ctx;
use crate::symbol::Symbol;
use yamlfy_syntax::FileId;

/// The plain scalar that introduces an inheritance clause.
pub(crate) const EXTENDS_KEY: &str = "extends";

/// One key a node writes directly.
pub(crate) struct OwnKey {
    /// The interned key text.
    pub(crate) name: Symbol,
    /// The key node, which is what a diagnostic about the key points at.
    pub(crate) key: NodeId,
    /// The value node.
    pub(crate) value: NodeId,
}

/// Whether `key` is an `extends` key.
///
/// D4.2 is D1.1's rule with a different spelling: an **untagged plain scalar**
/// whose content is exactly `extends`. A quoted `"extends"` or one carrying any
/// explicit tag is an ordinary string key.
///
/// The rule must not consult the operand. Recognising `extends` only when its
/// value is a legal operand would let a mistake in the *value* silently decide
/// whether the *key* is an operation at all, and the failure that produces is a
/// node that quietly stopped inheriting with no diagnostic.
pub(crate) fn is_extends_key(ast: &Ast, key: NodeId) -> bool {
    let Some(scalar) = ast.scalar(key) else { return false };
    if ast.tag(key).is_some() {
        return false;
    }
    scalar.style == ScalarStyle::Plain && &*scalar.value == EXTENDS_KEY
}

/// `own(A)`: every member `node` writes directly, clauses removed, first
/// occurrence of a repeated name kept — a repeat is already `E0110`.
pub(crate) fn own_keys(ctx: &Ctx, file: FileId, node: NodeId) -> Vec<OwnKey> {
    let Some(ast) = ctx.ast(file) else { return Vec::new() };
    if let Some(items) = ast.items(node) {
        return sequence_members(ctx, file, items);
    }
    let Some(entries) = ast.entries(node) else { return Vec::new() };
    let source = ctx.is_source(file);
    let mut out: Vec<OwnKey> = Vec::new();
    for entry in entries {
        if entry.merge || (source && is_extends_key(ast, entry.key)) {
            continue;
        }
        let Some(name) = ctx.interned.key_of(file, entry.key) else { continue };
        if out.iter().any(|held| held.name == name) {
            continue;
        }
        out.push(OwnKey { name, key: entry.key, value: entry.value });
    }
    out
}

/// The members of a sequence written as a member list. An item that names a
/// member is its own key *and* its own value: it declares that the member
/// exists and constrains nothing (D7.3's third state).
fn sequence_members(ctx: &Ctx, file: FileId, items: &[NodeId]) -> Vec<OwnKey> {
    let mut out: Vec<OwnKey> = Vec::new();
    for item in items {
        let Some(name) = ctx.interned.key_of(file, *item) else { continue };
        if out.iter().any(|held| held.name == name) {
            continue;
        }
        out.push(OwnKey { name, key: *item, value: *item });
    }
    out
}

/// `E0110` — two keys of one mapping naming the same member.
///
/// The parser compares keys by their **text** (§4), which is the right rule
/// there and cannot see past a flag prefix: `pub port` and `port` are two texts
/// and one member. Left-biased absorption would then keep the first and drop
/// the second in silence, so the collision is reported here, where the member
/// names are known.
pub(crate) fn check_member_names(ctx: &Ctx, diagnostics: &mut Diagnostics) {
    for file in ctx.project.files() {
        if !ctx.is_source(file.id) {
            continue;
        }
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            let Some(entries) = file.ast.entries(node) else { continue };
            let mut seen: HashMap<Symbol, NodeId> = HashMap::new();
            for entry in entries.iter().filter(|entry| !entry.merge) {
                let Some(name) = ctx.interned.key_of(file.id, entry.key) else { continue };
                let Some(first) = seen.insert(name, entry.key) else { continue };
                if first == entry.key || text_of(&file.ast, first) == text_of(&file.ast, entry.key)
                {
                    continue;
                }
                diagnostics.push(collision(ctx, file.id, (first, entry.key), name));
            }
        }
    }
}

fn text_of(ast: &Ast, node: NodeId) -> &str {
    ast.scalar(node).map_or("", |scalar| &scalar.value)
}

fn collision(ctx: &Ctx, file: FileId, keys: (NodeId, NodeId), name: Symbol) -> Diagnostic {
    let ast = ctx.ast(file).expect("the file is being walked");
    let spelled = ctx.interned.symbols().resolve(name).unwrap_or_default().to_owned();
    Diagnostic::new(
        Code::DuplicateKey,
        ast.node(keys.1).span,
        format!(
            "`{}` names the member `{spelled}`, which this mapping already declares",
            text_of(ast, keys.1)
        ),
    )
    .with_note("first declared here", Some(ast.node(keys.0).span))
}
