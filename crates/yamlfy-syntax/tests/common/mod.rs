// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for the integration corpora.
//!
//! Included by every integration binary; each uses a different subset, so the
//! unused-function warning here is structural rather than a real finding.

#![allow(dead_code)]

use std::path::PathBuf;

use yamlfy_syntax::{parse_file, Ast, Code, Diagnostics, NodeId, ParseOptions, Parsed, SourceMap};

/// Root of the fixture corpus.
pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Parse a fixture by corpus-relative path.
pub fn parse(relative: &str) -> (SourceMap, Parsed) {
    parse_with(relative, &ParseOptions::default())
}

/// Parse a fixture with explicit options.
pub fn parse_with(relative: &str, options: &ParseOptions) -> (SourceMap, Parsed) {
    let mut sources = SourceMap::new();
    let parsed = parse_file(&mut sources, fixtures().join(relative), options);
    (sources, parsed)
}

/// Parse a fixture, asserting it produced no diagnostics at all.
pub fn parse_clean(relative: &str) -> (SourceMap, Parsed) {
    let (sources, parsed) = parse(relative);
    assert!(
        parsed.diagnostics.is_empty(),
        "{relative} was expected to be clean:\n{}",
        parsed.diagnostics.render(&sources)
    );
    (sources, parsed)
}

/// Every fixture file in the corpus, as corpus-relative paths.
pub fn all_fixtures() -> Vec<String> {
    let root = fixtures();
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = read_sorted(&root);
    dirs.retain(|p| p.is_dir());
    for dir in dirs {
        for file in read_sorted(&dir) {
            if let Ok(relative) = file.strip_prefix(&root) {
                out.push(relative.display().to_string());
            }
        }
    }
    out
}

fn read_sorted(dir: &PathBuf) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
}

/// Number of diagnostics carrying `code`.
pub fn count(diagnostics: &Diagnostics, code: Code) -> usize {
    diagnostics.with_code(code).count()
}

/// The value node of the first mapping entry under `map` whose key is the
/// scalar `key`.
pub fn value_of(ast: &Ast, map: NodeId, key: &str) -> NodeId {
    let entries = ast.entries(map).unwrap_or_else(|| panic!("node {map:?} is not a mapping"));
    entries
        .iter()
        .find(|e| ast.scalar(e.key).is_some_and(|s| &*s.value == key))
        .unwrap_or_else(|| panic!("no key `{key}`"))
        .value
}

/// The root of the `index`-th document.
pub fn root(ast: &Ast, index: usize) -> NodeId {
    ast.documents()[index].root
}
