// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the files of one directory jointly declare about its scope, and when
//! that becomes `E0230`.
//!
//! Every header in a directory declares *that* directory's scope (D6.1), so
//! several files contributing to one namespace is the ordinary arrangement and
//! is silent. `E0230` is raised for a **conflict**, never for repetition: two
//! claims conflict when, for one of the three declared properties —
//! `namespace`, `visibility`, `mutability` — both state a value and the two
//! values differ. Restating a value is therefore silent, and so is omission,
//! which means "inherit" and is the absence of a claim rather than a competing
//! one. `version` is excluded: it describes the file's own header format, not
//! the scope's identity.
//!
//! The first claim in discovery order wins and is the one the scope keeps, so
//! the resolved tree does not depend on which of the conflicting files is
//! fixed; the diagnostic points at the later declaration and notes the earlier.
//!
//! The second, separate conflict — one namespace naming two scopes — lives in
//! [`check_namespace_uniqueness`].

use std::collections::HashMap;

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, Span};

use crate::header::Header;
use crate::scope::{Declared, ScopeId, ScopeTree};

/// Which of a scope's declared properties a claim is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Property {
    Namespace,
    Visibility,
    Mutability,
}

impl Property {
    fn as_str(self) -> &'static str {
        match self {
            Property::Namespace => "namespace",
            Property::Visibility => "visibility",
            Property::Mutability => "mutability",
        }
    }

    fn slot(self) -> usize {
        match self {
            Property::Namespace => 0,
            Property::Visibility => 1,
            Property::Mutability => 2,
        }
    }
}

struct Claim {
    text: Box<str>,
    span: Span,
}

#[derive(Default)]
struct ScopeClaim {
    slots: [Option<Claim>; 3],
    declared: Declared,
    namespace: Option<(Box<str>, Span)>,
    declared_by: Vec<FileId>,
}

/// Every scope's accumulated declarations.
pub(crate) struct Claims {
    scopes: Vec<ScopeClaim>,
}

impl Claims {
    pub(crate) fn new(scope_count: usize) -> Self {
        let mut scopes = Vec::with_capacity(scope_count);
        scopes.resize_with(scope_count, ScopeClaim::default);
        Claims { scopes }
    }

    /// Record everything `header` declares about `scope`, reporting conflicts.
    pub(crate) fn declare(
        &mut self,
        scope: ScopeId,
        file: FileId,
        header: &Header,
        diagnostics: &mut Diagnostics,
    ) {
        let Some(entry) = self.scopes.get_mut(scope.index()) else { return };
        entry.declared_by.push(file);
        if let Some((namespace, span)) = &header.namespace {
            if entry.set(Property::Namespace, namespace, *span, diagnostics) {
                entry.namespace = Some((namespace.clone(), *span));
            }
        }
        if let Some((visibility, span)) = header.visibility {
            if entry.set(Property::Visibility, visibility.as_str(), span, diagnostics) {
                entry.declared.visibility = Some(visibility);
            }
        }
        if let Some((mutability, span)) = header.mutability {
            if entry.set(Property::Mutability, mutability.as_str(), span, diagnostics) {
                entry.declared.mutability = Some(mutability);
            }
        }
    }

    /// Write the surviving claims into the tree and resolve inheritance.
    pub(crate) fn apply(self, tree: &mut ScopeTree) {
        for (index, entry) in self.scopes.into_iter().enumerate() {
            let id = ScopeId(u32::try_from(index).expect("scope tree overflow"));
            let visibility_span = entry.span_of(Property::Visibility);
            let mutability_span = entry.span_of(Property::Mutability);
            if let Some(scope) = tree.get_mut(id) {
                scope.declared = entry.declared;
                scope.declared_by = entry.declared_by;
                scope.visibility_span = visibility_span;
                scope.mutability_span = mutability_span;
            }
            if let Some((namespace, span)) = entry.namespace {
                tree.claim_namespace(id, &namespace, span);
            }
        }
        tree.resolve();
    }
}

impl ScopeClaim {
    /// Where the claim that won `property` was written, if one was. `E0241`
    /// reports it, so an author told a scope is `private` is also told which
    /// header line said so.
    fn span_of(&self, property: Property) -> Option<Span> {
        self.slots[property.slot()].as_ref().map(|claim| claim.span)
    }

    /// Record a claim, returning whether it is the one that wins. A repeat of
    /// the same text wins nothing and reports nothing; a different text is
    /// `E0230`.
    fn set(
        &mut self,
        property: Property,
        text: &str,
        span: Span,
        diagnostics: &mut Diagnostics,
    ) -> bool {
        let slot = &mut self.slots[property.slot()];
        match slot {
            Some(existing) if &*existing.text == text => false,
            Some(existing) => {
                diagnostics.push(conflict(property, existing, text, span));
                false
            }
            None => {
                *slot = Some(Claim { text: text.into(), span });
                true
            }
        }
    }
}

fn conflict(property: Property, existing: &Claim, text: &str, span: Span) -> Diagnostic {
    let message = format!(
        "this scope is already declared `{}: {}`; every file in a directory declares the \
         same scope, so `{text}` conflicts",
        property.as_str(),
        existing.text
    );
    Diagnostic::new(Code::DuplicateNamespace, span, message)
        .with_note("first declared here", Some(existing.span))
}

/// Report a namespace that names more than one scope. Two directories claiming
/// one namespace is not repetition — nothing could resolve the name — so it is
/// `E0230` even when the two agree on every axis.
pub(crate) fn check_namespace_uniqueness(tree: &ScopeTree, diagnostics: &mut Diagnostics) {
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for scope in tree.scopes() {
        let (Some(namespace), Some(span)) = (&scope.namespace, scope.namespace_span) else {
            continue;
        };
        match seen.get(&**namespace) {
            Some(first) => diagnostics.push(
                Diagnostic::new(
                    Code::DuplicateNamespace,
                    span,
                    format!(
                        "namespace `{namespace}` is already claimed by another directory; \
                         a namespace must name exactly one scope"
                    ),
                )
                .with_note("first claimed here", Some(*first)),
            ),
            None => {
                seen.insert(namespace, span);
            }
        }
    }
}
