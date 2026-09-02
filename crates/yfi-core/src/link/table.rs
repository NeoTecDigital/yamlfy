// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The canonical-path table, and `E0230`'s duplicate-definition condition.
//!
//! # What is addressable
//!
//! An anchored node that can be a parent scope — a **collection** — is a member
//! of its file and referenceable as a type. An anchored **scalar** is a value,
//! not a type, and carries no canonical path at all. That distinction is what
//! stops `E0230` from firing against two files that each write `&limit 30`,
//! which are two values and not two definitions of one thing.
//!
//! Only a Yamlfication source file whose scope claims a namespace contributes.
//! A base YAML file has no header, therefore no namespace, therefore no
//! canonical path, and holds no addressable definition at all (D6.6).
//!
//! # What `E0230` compares here
//!
//! **One namespace, one name, two files.** Several files contributing to one
//! namespace is the ordinary arrangement and must stay silent, and a name
//! repeated *within* one file is a state sequence with a defined last state
//! (D5.1, D5.2) — so repetition is folded to the last state per file first, and
//! only then compared across files. Across two files there is no authored order
//! at all: the winner would be decided by D6.2's path ranking, which is to say
//! by a filename, and a graph whose values depend on a filename is what D1.8
//! refuses.
//!
//! This is the third condition behind `E0230`. The two `discover` already
//! raises — headers in one directory disagreeing about an axis, and one
//! namespace claimed by two directories — are declaration conflicts and stay.

use std::collections::HashMap;

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use super::Ctx;
use crate::scope::ScopeId;

/// One addressable node.
pub struct Definition {
    /// The canonical path, `namespace/name`.
    pub path: Box<str>,
    /// The namespace half.
    pub namespace: Box<str>,
    /// The anchor name half.
    pub name: Box<str>,
    /// The file that wrote it.
    pub file: FileId,
    /// The anchored collection itself.
    pub node: NodeId,
    /// The `&name` token, which is what a diagnostic about the definition
    /// points at.
    pub span: Span,
}

/// Every addressable node of the project, indexed four ways.
///
/// `by_path` is the **canonical** index, `namespace/name`, and it is what
/// `E0230`'s duplicate-definition condition compares. The other two are what a
/// *path* walks (D4.12): a path addresses a file or a directory, not a
/// namespace, so a `.yfy` in a directory whose headers claim no namespace is
/// still reachable as `sibling/Name` while carrying no canonical path at all.
/// Addressability and canonical identity are two questions and this is where
/// they part.
pub(crate) struct Table {
    definitions: Vec<Definition>,
    by_path: HashMap<Box<str>, usize>,
    by_node: HashMap<(FileId, NodeId), usize>,
    by_file: HashMap<(FileId, Box<str>), (FileId, NodeId)>,
    by_scope: HashMap<(ScopeId, Box<str>), (FileId, NodeId)>,
}

impl Table {
    /// Every definition, in discovery order.
    pub(crate) fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    /// The definition a canonical path names.
    pub(crate) fn get(&self, path: &str) -> Option<&Definition> {
        self.by_path.get(path).map(|index| &self.definitions[*index])
    }

    /// The canonical path of an addressable node.
    pub(crate) fn path_of(&self, file: FileId, node: NodeId) -> Option<&str> {
        self.by_node.get(&(file, node)).map(|index| &*self.definitions[*index].path)
    }

    /// The definition `name` in `file`, which is what a bare path names.
    pub(crate) fn in_file(&self, file: FileId, name: &str) -> Option<(FileId, NodeId)> {
        self.by_file.get(&(file, name.into())).copied()
    }

    /// The definition `name` anywhere in `scope`'s own directory, which is what
    /// a path ending on a directory names. Several files contributing to one
    /// directory is the ordinary arrangement (D6.1) and two of them defining
    /// one name is already `E0230`, so the first wins and the duplicate is
    /// reported rather than silently ranked.
    pub(crate) fn in_scope(&self, scope: ScopeId, name: &str) -> Option<(FileId, NodeId)> {
        self.by_scope.get(&(scope, name.into())).copied()
    }
}

/// Build the table, reporting one `E0230` per duplicated canonical path.
pub(crate) fn build(ctx: &Ctx, diagnostics: &mut Diagnostics) -> Table {
    let mut table = Table {
        definitions: Vec::new(),
        by_path: HashMap::new(),
        by_node: HashMap::new(),
        by_file: HashMap::new(),
        by_scope: HashMap::new(),
    };
    for file in ctx.project.files() {
        if !ctx.is_source(file.id) {
            continue;
        }
        let namespace = ctx.namespace_of(file.id);
        for definition in in_one_file(&file.ast, namespace.unwrap_or_default()) {
            table.by_file.insert((file.id, definition.name.clone()), (file.id, definition.node));
            table
                .by_scope
                .entry((file.scope, definition.name.clone()))
                .or_insert((file.id, definition.node));
            if namespace.is_some() {
                insert(&mut table, definition, diagnostics);
            }
        }
    }
    table
}

fn insert(table: &mut Table, definition: Definition, diagnostics: &mut Diagnostics) {
    if let Some(first) = table.get(&definition.path) {
        diagnostics.push(duplicate(&definition, first));
        return;
    }
    let index = table.definitions.len();
    table.by_path.insert(definition.path.clone(), index);
    table.by_node.insert((definition.file, definition.node), index);
    table.definitions.push(definition);
}

fn duplicate(definition: &Definition, first: &Definition) -> Diagnostic {
    Diagnostic::new(
        Code::DuplicateNamespace,
        definition.span,
        format!(
            "`{}` is already the canonical path of a definition in another file; one \
             namespace, one name, one definition, because nothing ranks two files' claims \
             on a name except their filenames",
            definition.path
        ),
    )
    .with_note("first defined here", Some(first.span))
}

/// One file's addressable definitions, repetition already folded to the last
/// state of each name.
fn in_one_file(ast: &Ast, namespace: &str) -> Vec<Definition> {
    let mut out: Vec<Definition> = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for def in ast.anchors().defs() {
        if def.is_imported() || !is_addressable(ast, def.node) || def.name.is_empty() {
            continue;
        }
        let definition = Definition {
            path: format!("{namespace}/{}", def.name).into(),
            namespace: namespace.into(),
            name: def.name.clone(),
            file: ast.file(),
            node: def.node,
            span: def.span,
        };
        match seen.get(&*def.name) {
            Some(index) => out[*index] = definition,
            None => {
                seen.insert(&def.name, out.len());
                out.push(definition);
            }
        }
    }
    out
}

/// Whether an anchored node carries a canonical path: a collection does, a
/// scalar does not.
fn is_addressable(ast: &Ast, node: NodeId) -> bool {
    if node.index() >= ast.nodes().len() {
        return false;
    }
    ast.entries(node).is_some() || ast.items(node).is_some()
}
