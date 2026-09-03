// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mapping assembly: merge-key classification and key uniqueness.
//!
//! Both checks are purely syntactic and therefore belong to the parser. Whether
//! a merge key's *value* is a legal merge source, and whether the merge graph is
//! acyclic, are questions for the link pass.

use std::collections::HashMap;

use crate::ast::{Ast, Entry, Node, NodeId, NodeKind, Scalar, ScalarStyle};
use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::span::Span;

/// The plain scalar that introduces a merge key.
pub const MERGE_KEY: &str = "<<";

/// Whether `key` is a YAML merge key.
///
/// A merge key is an untagged **plain** scalar `<<`, or any scalar explicitly
/// tagged `!!merge`. A quoted `"<<"` is an ordinary string key.
#[must_use]
pub fn is_merge_key(ast: &Ast, key: NodeId) -> bool {
    // D1.1 says *scalar*, so the kind is checked before the tag: a collection
    // carrying `!!merge` (`!!merge [k]: 1`) is not a merge key.
    let Some(scalar) = ast.scalar(key) else { return false };
    if let Some(tag) = ast.tag(key) {
        return tag.is_core("merge");
    }
    scalar.style == ScalarStyle::Plain && &*scalar.value == MERGE_KEY
}

/// Turn a flat `key, value, key, value, ...` child list into entries.
///
/// A trailing key with no value cannot be produced by a conforming event
/// stream; if one appears it is paired with a synthesised empty scalar so the
/// arena stays well formed.
pub(crate) fn pair_up(ast: &mut Ast, children: &[NodeId], span: Span) -> Vec<Entry> {
    let mut entries = Vec::with_capacity(children.len() / 2);
    let mut pairs = children.chunks_exact(2);
    for pair in &mut pairs {
        let (key, value) = (pair[0], pair[1]);
        entries.push(Entry { key, value, merge: is_merge_key(ast, key) });
    }
    if let [key] = pairs.remainder() {
        let value = synthesise_empty(ast, span);
        entries.push(Entry { key: *key, value, merge: is_merge_key(ast, *key) });
    }
    entries
}

fn synthesise_empty(ast: &mut Ast, span: Span) -> NodeId {
    ast.scalars.push(Scalar { value: "".into(), style: ScalarStyle::Plain });
    let scalar = u32::try_from(ast.scalars.len() - 1).expect("arena side table overflow");
    let id = NodeId(u32::try_from(ast.nodes.len()).expect("arena overflow"));
    let span = Span::empty(span.file, span.end);
    ast.nodes.push(Node { kind: NodeKind::Scalar(scalar), span, anchor: None, tag: None });
    id
}

/// Report every repeated key in one mapping.
///
/// Merge keys are counted separately from ordinary keys, because D1.7 bounds
/// them by *role*, not by text: `<<:` together with `!!merge zz:` is two merge
/// keys in one mapping even though the two key texts differ. Keying both on the
/// same table would let that pair through silently, and the link pass would then
/// receive a mapping with two merge sources and no defined precedence between
/// them — exactly the ambiguity D1.7 exists to forbid.
///
/// Holding them apart also preserves D1.1's coexistence rule for free: a literal
/// `"<<"` key never meets the real merge key in the same table, so the two are
/// still different keys.
///
/// Only scalar keys participate; comparing complex keys requires resolved
/// values, which the parser does not have. A complex key still costs `E0111`
/// when it carries the merge tag — see [`decorative_merge`], which also records
/// why the *comparison* stays silent on purpose.
pub(crate) fn check_keys(ast: &Ast, entries: &[Entry], diags: &mut Diagnostics) {
    let mut seen: HashMap<&str, Span> = HashMap::with_capacity(entries.len());
    let mut first_merge: Option<Span> = None;
    for entry in entries {
        let span = ast.node(entry.key).span;
        if entry.merge {
            match first_merge {
                Some(first) => diags.push(duplicate(true, MERGE_KEY, span, first)),
                None => first_merge = Some(span),
            }
            continue;
        }
        let Some(scalar) = ast.scalar(entry.key) else {
            decorative_merge(ast, entry.key, span, diags);
            continue;
        };
        if let Some(first) = seen.insert(&scalar.value, span) {
            diags.push(duplicate(false, &scalar.value, span, first));
        }
    }
}

/// `E0111` — `!!merge` on a **non-scalar** key.
///
/// D1.1 says a merge key is a *scalar* tagged `!!merge`, so `!!merge [k]: 1` is
/// correctly not one. What it becomes is an ordinary complex key, and the tag
/// then classifies nothing, resolves nothing and is consumed by nothing. A tag
/// that means nothing is worse than one that does not exist — the argument
/// D7.4 makes for `!oneof` and D4.13 makes for `E0223`, and it holds here for
/// the same reason: silence leaves the author believing they wrote a merge.
///
/// The **other** silence in this position is deliberate and stays. Two complex
/// keys alike in one mapping are not reported, because key identity in this
/// language is a key's scalar text and a complex key has none to compare;
/// deciding it needs resolved values, which the parser does not have. It is
/// bounded — a non-merge key is absorbed as data — so it can only ever be
/// silence and never a wrong graph.
fn decorative_merge(ast: &Ast, key: NodeId, span: Span, diags: &mut Diagnostics) {
    if !ast.tag(key).is_some_and(|tag| tag.is_core("merge")) {
        return;
    }
    diags.push(
        Diagnostic::new(
            Code::MergeTagOnComplexKey,
            span,
            "`!!merge` on a non-scalar key is not a merge key, and means nothing here",
        )
        .with_note(
            "a merge key is a scalar: a plain `<<`, or any scalar tagged `!!merge`; this entry \
             is an ordinary complex key and its value is data",
            None,
        ),
    );
}

fn duplicate(merge: bool, text: &str, span: Span, first: Span) -> Diagnostic {
    let (code, message) = if merge {
        (
            Code::DuplicateMergeKey,
            "a mapping may contain only one merge key; write `<<: [*a, *b]` to merge \
             several sources, where the earlier source wins"
                .to_owned(),
        )
    } else {
        (Code::DuplicateKey, format!("duplicate mapping key `{text}`"))
    };
    Diagnostic::new(code, span, message).with_note("first defined here", Some(first))
}
