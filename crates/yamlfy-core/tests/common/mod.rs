// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for the project corpus.
//!
//! `fixtures/` holds single-file fixtures and is driven by
//! `yamlfy-syntax/tests/common::all_fixtures`, which walks exactly one
//! directory level and treats every file as an independent parse. Cross-file
//! behaviour cannot be expressed there, so project fixtures live in a **sibling
//! tree**, `projects/<name>/`, where one directory is one compilable project.
//! Keeping them apart is what leaves `fixtures/` and the three corpus-wide
//! invariants that walk it untouched.
//!
//! Both corpora hold both file classes. `.yfy` is Yamlfication source and
//! `.yml`/`.yaml` is base YAML; the front end parses them identically, and only
//! `discover` tells them apart.

#![allow(dead_code)]

use std::path::PathBuf;

use yamlfy_core::{discover, DiscoverOptions, Project, ScopeId};
use yamlfy_syntax::{Code, Diagnostics, FileId, NodeId};

/// Repository root.
pub fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Root of the project corpus.
pub fn projects() -> PathBuf {
    repository().join("projects")
}

/// Discover any path in the repository as a project. Used to point a project
/// at a single-file fixture, which is legal: a file is a project of one file.
pub fn open_at(relative: &str) -> Project {
    discover(repository().join(relative), &DiscoverOptions::default())
}

/// Discover a project fixture by name.
pub fn open(name: &str) -> Project {
    open_with(name, &DiscoverOptions::default())
}

/// Discover a project fixture with explicit options.
pub fn open_with(name: &str, options: &DiscoverOptions) -> Project {
    discover(projects().join(name), options)
}

/// Discover a project fixture, asserting it raised nothing at all.
pub fn open_clean(name: &str) -> Project {
    let project = open(name);
    assert!(
        project.diagnostics().is_empty(),
        "{name} was expected to be clean:\n{}",
        project.diagnostics().render(project.sources())
    );
    project
}

/// Number of diagnostics carrying `code`.
pub fn count(diagnostics: &Diagnostics, code: Code) -> usize {
    diagnostics.with_code(code).count()
}

/// The relative paths of every discovered file, in rank order.
pub fn relative_paths(project: &Project) -> Vec<String> {
    project.files().iter().map(|f| f.relative.display().to_string()).collect()
}

/// The scope whose `root/dir/sub` name is `qualified`.
pub fn scope_by(project: &Project, qualified: &str) -> ScopeId {
    project
        .scopes()
        .scopes()
        .iter()
        .find(|s| project.scopes().qualified(s.id) == qualified)
        .unwrap_or_else(|| panic!("no scope `{qualified}`"))
        .id
}

/// The node reached from `document`'s root by following `path` key by key.
pub fn entry_at(project: &Project, file: FileId, document: usize, path: &[&str]) -> NodeId {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    let mut at = ast.documents()[document].root;
    for key in path {
        at = ast
            .entries(at)
            .unwrap_or_else(|| panic!("{at:?} of {file:?} is not a mapping"))
            .iter()
            .find(|e| ast.scalar(e.key).is_some_and(|s| s.value.as_ref() == *key))
            .unwrap_or_else(|| panic!("no key `{key}`"))
            .value;
    }
    at
}

/// The node the last definition of `name` written in `file` names.
pub fn declaration(project: &Project, file: FileId, name: &str) -> NodeId {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    ast.anchors()
        .defs()
        .iter()
        .filter(|d| !d.is_imported() && &*d.name == name)
        .next_back()
        .unwrap_or_else(|| panic!("`&{name}` is not defined in {file:?}"))
        .node
}

/// The names a header import installed into `file`, in authored order, each
/// listed once however many parser segments re-installed it.
pub fn imported_names(project: &Project, file: FileId) -> Vec<String> {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    let mut out: Vec<String> = Vec::new();
    for def in ast.anchors().defs().iter().filter(|d| d.is_imported()) {
        let name = def.name.to_string();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}
