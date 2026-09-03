// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! D7.3's three-state declaration rule, and what a tag comparison means.
//!
//! Tags are compared by `(is core schema, suffix)`, never by handle: a `%TAG`
//! directive rewrites the handle and leaves the suffix alone, so comparing
//! handles would make a declaration silently stop matching in any file carrying
//! a directive — and `!node` must not compare equal to the core schema's
//! `tag:yaml.org,2002:node`, which is a different tag.
//!
//! An **untagged scalar** is not resolved against the core schema and is
//! therefore never `E0221`: YAML's own resolution would call plain `8443` an
//! `!!int`, and a diagnostic there would be this compiler inventing a rule D7.3
//! does not state. What *is* compared without a tag is the **kind** — a mapping
//! or a sequence supplied where a core scalar tag is declared is a mismatch no
//! schema resolution can explain away.

use yfi_syntax::{Ast, NodeId, Tag};

/// The core schema's handle. Anything carrying it is a YAML built-in.
const CORE_HANDLE: &str = "tag:yaml.org,2002:";

/// The core-schema tags that admit only a scalar.
const SCALAR_TAGS: [&str; 5] = ["str", "int", "float", "bool", "null"];

/// What a declaring entry says about a field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    /// Tagged and empty: a descendant must supply a value.
    Required,
    /// Carries a value, and a tag that constrains what may replace it.
    OptionalTagged,
    /// Carries a value and no tag, or no tag and no value: the key exists and
    /// nothing constrains it.
    Unconstrained,
}

/// Which of the three states an entry's value node is in.
pub(crate) fn state_of(ast: &Ast, value: NodeId) -> State {
    let tagged = ast.tag(value).is_some();
    match (tagged, is_empty(ast, value)) {
        (true, true) => State::Required,
        (true, false) => State::OptionalTagged,
        (false, _) => State::Unconstrained,
    }
}

/// Whether a value node is the empty scalar a bare tag leaves behind.
pub(crate) fn is_empty(ast: &Ast, value: NodeId) -> bool {
    ast.scalar(value).is_some_and(|scalar| scalar.value.is_empty())
}

/// A tag's comparable identity: whether it is the core schema's, and its
/// suffix.
fn identity(tag: &Tag) -> (bool, &str) {
    (&*tag.handle == CORE_HANDLE, &tag.suffix)
}

/// How a tag is spelled back to the author.
pub(crate) fn spelling(tag: &Tag) -> String {
    if identity(tag).0 {
        return format!("!!{}", tag.suffix);
    }
    format!("!{}", tag.suffix)
}

/// Why a supplied value does not satisfy a declared tag, or `None` when it
/// does.
pub(crate) enum Mismatch {
    /// The value carries a tag, and it is a different tag.
    Tagged(String),
    /// The value carries no tag, and its kind cannot be what was declared.
    Kind(&'static str),
}

/// Compare a supplied value against a declared tag.
pub(crate) fn compare(declared: &Tag, supplied: &Ast, value: NodeId) -> Option<Mismatch> {
    if let Some(tag) = supplied.tag(value) {
        if identity(tag) == identity(declared) {
            return None;
        }
        return Some(Mismatch::Tagged(spelling(tag)));
    }
    if !identity(declared).0 || !SCALAR_TAGS.contains(&identity(declared).1) {
        return None;
    }
    if supplied.entries(value).is_some() {
        return Some(Mismatch::Kind("a mapping"));
    }
    if supplied.items(value).is_some() {
        return Some(Mismatch::Kind("a sequence"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use yfi_syntax::{parse, ParseOptions, SourceMap};

    fn parsed(text: &str) -> Ast {
        let mut sources = SourceMap::new();
        let file = sources.add("t.yfy", text);
        parse(&sources, file, &ParseOptions::default()).ast
    }

    fn value_at(ast: &Ast, key: &str) -> NodeId {
        let root = ast.documents()[0].root;
        ast.entries(root)
            .expect("mapping")
            .iter()
            .find(|entry| ast.scalar(entry.key).is_some_and(|s| &*s.value == key))
            .expect("key")
            .value
    }

    #[test]
    fn the_three_states_are_told_apart_by_tag_and_emptiness() {
        let ast = parsed("required: !!int\noptional: !!int 3\nopen:\nplain: 7\n");
        assert_eq!(state_of(&ast, value_at(&ast, "required")), State::Required);
        assert_eq!(state_of(&ast, value_at(&ast, "optional")), State::OptionalTagged);
        assert_eq!(state_of(&ast, value_at(&ast, "open")), State::Unconstrained);
        assert_eq!(state_of(&ast, value_at(&ast, "plain")), State::Unconstrained);
    }

    #[test]
    fn a_tag_is_compared_by_suffix_and_schema_never_by_handle() {
        let ast = parsed("a: !!int 1\nb: !!str '1'\nc: !node {}\n");
        let declared = ast.tag(value_at(&ast, "a")).expect("tag");
        assert!(compare(declared, &ast, value_at(&ast, "a")).is_none());
        assert!(matches!(compare(declared, &ast, value_at(&ast, "b")), Some(Mismatch::Tagged(_))));
        assert_eq!(spelling(declared), "!!int");
        let ours = ast.tag(value_at(&ast, "c")).expect("tag");
        assert_eq!(spelling(ours), "!node");
    }

    #[test]
    fn an_untagged_scalar_is_never_a_mismatch_but_a_collection_is() {
        let ast = parsed("a: !!int 1\nplain: hello\nmap: {x: 1}\nseq: [1]\n");
        let declared = ast.tag(value_at(&ast, "a")).expect("tag");
        assert!(
            compare(declared, &ast, value_at(&ast, "plain")).is_none(),
            "untagged scalars are not schema-resolved"
        );
        assert!(matches!(
            compare(declared, &ast, value_at(&ast, "map")),
            Some(Mismatch::Kind("a mapping"))
        ));
        assert!(matches!(
            compare(declared, &ast, value_at(&ast, "seq")),
            Some(Mismatch::Kind("a sequence"))
        ));
    }
}
