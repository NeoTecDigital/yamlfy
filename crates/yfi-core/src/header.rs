// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `!yfi/header` document.
//!
//! A file's header is the **first** document of its stream, tagged
//! `!yfi/header`. It is optional: a file without one declares nothing and
//! inherits everything from its directory scope.
//!
//! Only a Yamlfication source file has a header. In a base YAML data file a
//! `!yfi/header` document is an ordinary tagged mapping and nothing else, so
//! [`read`] is never called on one — see `discover::FileClass`.
//!
//! The header carries the file boundary. `imports:` names other files of the
//! project; importing brings their definitions or objects into *this* document,
//! which is what lets an ordinary alias reach them. That is why crossing a file
//! needs no new operator and why D2.6 is untouched: by the time `*Service` is
//! written, `Service` is already defined in this document.
//!
//! Unknown keys are ignored rather than rejected. `fixtures/valid/header-document.yfy`
//! already carries a `schema:` key that no pass reads yet, and the header is the
//! natural place for forward-compatible metadata, so rejecting unknown keys
//! would break the corpus and freeze the format.

use tracing::debug;
use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, NodeId, Span};

use crate::scope::{Mutability, Visibility};
use crate::tags::{classify, TagKind};

/// A parsed header. Every field is optional: stating nothing is how a file says
/// "inherit".
#[derive(Debug)]
pub struct Header {
    /// The header document's root mapping.
    pub node: NodeId,
    /// The header document's span.
    pub span: Span,
    /// The declared language version.
    pub version: Option<u32>,
    /// The declared namespace and where it was written.
    pub namespace: Option<(Box<str>, Span)>,
    /// The declared visibility and where it was written.
    pub visibility: Option<(Visibility, Span)>,
    /// The declared mutability and where it was written.
    pub mutability: Option<(Mutability, Span)>,
    /// Files this one imports, in written order, before resolution.
    pub imports: Vec<(Box<str>, Span)>,
}

impl Header {
    fn empty(node: NodeId, span: Span) -> Self {
        Header {
            node,
            span,
            version: None,
            namespace: None,
            visibility: None,
            mutability: None,
            imports: Vec::new(),
        }
    }
}

/// Read `ast`'s header document, if it has one. Malformed values are reported
/// as `E0231` and skipped; the rest of the header is still read, because
/// diagnostics accumulate.
pub fn read(ast: &Ast, diagnostics: &mut Diagnostics) -> Option<Header> {
    let document = ast.documents().first()?;
    let root = document.root;
    if ast.tag(root).map(classify) != Some(TagKind::Header) {
        return None;
    }
    let Some(entries) = ast.entries(root) else {
        // `--- !yfi/header` with no body is an empty scalar, not a mapping.
        // It declares nothing, which is exactly what omitting the header does,
        // so it is legal rather than an error. Any other shape is not.
        if ast.scalar(root).is_some_and(|s| s.value.is_empty()) {
            return Some(Header::empty(root, document.span));
        }
        diagnostics.push(Diagnostic::new(
            Code::BadHeaderValue,
            ast.node(root).span,
            "a `!yfi/header` document must be a mapping",
        ));
        return None;
    };
    let mut header = Header::empty(root, document.span);
    for entry in entries.iter().filter(|e| !e.merge) {
        let Some(key) = ast.scalar(entry.key) else { continue };
        field(&mut header, ast, &key.value, entry.value, diagnostics);
    }
    Some(header)
}

fn field(
    header: &mut Header,
    ast: &Ast,
    key: &str,
    value: NodeId,
    diagnostics: &mut Diagnostics,
) {
    let span = ast.node(value).span;
    let text = ast.scalar(value).map(|s| s.value.trim().to_owned());
    match key {
        "version" => header.version = version(text.as_deref(), span, diagnostics),
        "namespace" => header.namespace = namespace(text.as_deref(), span, diagnostics),
        "visibility" => {
            header.visibility = axis(text.as_deref(), span, diagnostics, "visibility")
                .map(|value: Visibility| (value, span));
        }
        "mutability" => {
            header.mutability = axis(text.as_deref(), span, diagnostics, "mutability")
                .map(|value: Mutability| (value, span));
        }
        "imports" => header.imports = imports(ast, value, diagnostics),
        other => debug!(key = other, "ignoring unrecognised header key"),
    }
}

/// One of the two axes. Both parse the same way and fail the same way, so the
/// two spellings share one function rather than one each.
trait Axis: Sized {
    fn parse(text: &str) -> Option<Self>;
    fn choices() -> &'static str;
}

impl Axis for Visibility {
    fn parse(text: &str) -> Option<Self> {
        Visibility::parse(text)
    }
    fn choices() -> &'static str {
        Visibility::choices()
    }
}

impl Axis for Mutability {
    fn parse(text: &str) -> Option<Self> {
        Mutability::parse(text)
    }
    fn choices() -> &'static str {
        Mutability::choices()
    }
}

fn axis<A: Axis>(
    text: Option<&str>,
    span: Span,
    diagnostics: &mut Diagnostics,
    key: &str,
) -> Option<A> {
    let Some(text) = text else {
        diagnostics.push(bad(span, format!("`{key}` must be a scalar, one of {}", A::choices())));
        return None;
    };
    match A::parse(text) {
        Some(value) => Some(value),
        None => {
            diagnostics.push(bad(
                span,
                format!("`{text}` is not a valid `{key}`; expected one of {}", A::choices()),
            ));
            None
        }
    }
}

fn version(text: Option<&str>, span: Span, diagnostics: &mut Diagnostics) -> Option<u32> {
    let parsed = text.and_then(|t| t.parse::<u32>().ok());
    if parsed.is_none() {
        let written = text.unwrap_or("<not a scalar>");
        diagnostics
            .push(bad(span, format!("`version` must be a non-negative integer, not `{written}`")));
    }
    parsed
}

fn namespace(
    text: Option<&str>,
    span: Span,
    diagnostics: &mut Diagnostics,
) -> Option<(Box<str>, Span)> {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        diagnostics.push(bad(span, "`namespace` must be a non-empty scalar"));
        return None;
    };
    if let Some(reason) = namespace_fault(text) {
        diagnostics.push(bad(span, format!("`{text}` is not a valid namespace: {reason}")));
        return None;
    }
    Some((text.into(), span))
}

/// Why `text` is not a namespace, or `None` when it is one. A namespace is one
/// or more `::`-separated components of `[A-Za-z0-9_-]`.
fn namespace_fault(text: &str) -> Option<&'static str> {
    if text.starts_with("::") || text.ends_with("::") {
        return Some("it has an empty leading or trailing component");
    }
    for component in text.split("::") {
        if component.is_empty() {
            return Some("it has an empty component");
        }
        if !component.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Some("components may only contain letters, digits, `_` and `-`");
        }
    }
    None
}

/// Read `imports:`. A single scalar is accepted as well as a sequence, because
/// one import is the common case and `imports: core.yfy` is what an author
/// writes. Anything else is `E0231`, once per offending item.
fn imports(ast: &Ast, value: NodeId, diagnostics: &mut Diagnostics) -> Vec<(Box<str>, Span)> {
    let items: Vec<NodeId> = match ast.items(value) {
        Some(items) => items.to_vec(),
        None => vec![value],
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let span = ast.node(item).span;
        match ast.scalar(item).map(|s| s.value.trim()).filter(|t| !t.is_empty()) {
            Some(text) => out.push((text.into(), span)),
            None => diagnostics
                .push(bad(span, "each entry of `imports` must be a non-empty path scalar")),
        }
    }
    out
}

fn bad(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(Code::BadHeaderValue, span, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_namespace_needs_non_empty_components() {
        assert_eq!(namespace_fault("acme::billing"), None);
        assert_eq!(namespace_fault("acme"), None);
        assert_eq!(namespace_fault("a-b_1::c"), None);
        assert!(namespace_fault("::acme").is_some());
        assert!(namespace_fault("acme::").is_some());
        assert!(namespace_fault("acme::::billing").is_some());
        assert!(namespace_fault("acme::bil ling").is_some());
    }
}
