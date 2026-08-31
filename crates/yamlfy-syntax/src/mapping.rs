// Written by Richard Christopher, Copyright 2026 Richard Christopher

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
    if let Some(tag) = ast.tag(key) {
        return tag.is_core("merge");
    }
    matches!(ast.scalar(key), Some(s) if s.style == ScalarStyle::Plain && &*s.value == MERGE_KEY)
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

/// Key identity for uniqueness. Only scalar keys participate: comparing complex
/// keys requires resolved values, which the parser does not have.
#[derive(PartialEq, Eq, Hash)]
struct KeyId<'a> {
    merge: bool,
    text: &'a str,
}

/// Report every repeated key in one mapping.
pub(crate) fn check_keys(ast: &Ast, entries: &[Entry], diags: &mut Diagnostics) {
    let mut seen: HashMap<KeyId<'_>, Span> = HashMap::with_capacity(entries.len());
    for entry in entries {
        let Some(scalar) = ast.scalar(entry.key) else { continue };
        let id = KeyId { merge: entry.merge, text: &scalar.value };
        let span = ast.node(entry.key).span;
        if let Some(first) = seen.insert(id, span) {
            diags.push(duplicate(entry.merge, &scalar.value, span, first));
        }
    }
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
