// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The span model: byte offsets that actually index the file, one-based
//! line and column, on every node.

mod common;

use common::{parse_clean, root, value_of};
use yfi_syntax::{ParseOptions, SourceMap};

#[test]
fn byte_offsets_index_the_source_through_multi_byte_scalars() {
    let (sources, parsed) = parse_clean("spans/utf8.yml");
    let ast = &parsed.ast;
    let file = sources.file(ast.file());
    let doc = root(ast, 0);

    for key in ["ascii", "accents", "cjk", "emoji"] {
        let value = value_of(ast, doc, key);
        let span = ast.node(value).span;
        let text = &file.text()[span.start.byte as usize..span.end.byte as usize];
        let scalar = ast.scalar(value).expect("scalar");
        let expected = match scalar.style {
            yfi_syntax::ScalarStyle::DoubleQuoted => format!("\"{}\"", scalar.value),
            _ => scalar.value.to_string(),
        };
        assert_eq!(text, expected, "span of `{key}` must slice its own text");
    }
}

#[test]
fn columns_are_one_based_and_count_characters() {
    let (_, parsed) = parse_clean("spans/utf8.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    // `accents: héllo wörld` — the key starts at column 1, the value after
    // "accents: ", which is 9 characters.
    let entries = ast.entries(doc).expect("mapping");
    let accents = entries[1];
    assert_eq!(ast.node(accents.key).span.start.col, 1);
    assert_eq!(ast.node(accents.value).span.start.col, 10);
}

#[test]
fn a_bom_shifts_byte_offsets_but_not_lines() {
    let (sources, parsed) = parse_clean("spans/bom.yml");
    let ast = &parsed.ast;
    let file = sources.file(ast.file());
    let doc = root(ast, 0);
    let key = ast.entries(doc).expect("mapping")[0].key;
    let span = ast.node(key).span;

    assert_eq!(span.start.line, 3);
    assert_eq!(span.start.col, 1);
    let raw = std::fs::read(file.path()).expect("fixture readable");
    let sliced = &raw[span.start.byte as usize..span.end.byte as usize];
    assert_eq!(sliced, b"key", "byte offsets are relative to the file, BOM included");
}

#[test]
fn crlf_line_endings_do_not_drift() {
    let (_, parsed) = parse_clean("spans/crlf.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    assert_eq!(ast.node(value_of(ast, doc, "first")).span.start.line, 3);
    assert_eq!(ast.node(value_of(ast, doc, "second")).span.start.line, 4);
    let third = value_of(ast, doc, "third");
    let items = ast.items(third).expect("sequence");
    assert_eq!(ast.node(items[0]).span.start.line, 6);
    assert_eq!(ast.node(items[1]).span.start.line, 7);
}

#[test]
fn a_collection_span_covers_all_of_its_children() {
    let (_, parsed) = parse_clean("spans/nested.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    for id in ast.reachable_from(doc) {
        let parent = ast.node(id).span;
        for child in ast.children(id) {
            let child = ast.node(child).span;
            assert!(
                parent.start.byte <= child.start.byte && child.end.byte <= parent.end.byte,
                "child span {child:?} escapes parent {parent:?}"
            );
        }
    }
}

#[test]
fn every_node_in_the_corpus_carries_a_usable_position() {
    for relative in common::all_fixtures() {
        let (_, parsed) = common::parse(&relative);
        for node in parsed.ast.nodes() {
            assert!(node.span.start.line >= 1, "{relative}: line must be one-based");
            assert!(node.span.start.col >= 1, "{relative}: column must be one-based");
            assert!(node.span.start.byte <= node.span.end.byte, "{relative}: inverted span");
        }
    }
}

#[test]
fn documents_are_separated_and_spanned() {
    let (_, parsed) = parse_clean("spans/multidoc.yml");
    let ast = &parsed.ast;
    assert_eq!(ast.documents().len(), 3);

    let lines: Vec<u32> = ast.documents().iter().map(|d| d.span.start.line).collect();
    assert_eq!(lines, vec![3, 6, 8]);
    assert!(ast.documents().iter().all(|d| d.explicit));
}

#[test]
fn spans_survive_a_parser_restart_after_a_syntax_error() {
    let mut sources = SourceMap::new();
    let text = "--- \na: [1,\n--- \nb: 2\n";
    let file = sources.add("restart.yml", text);
    let parsed = yfi_syntax::parse(&sources, file, &ParseOptions::default());
    let ast = &parsed.ast;

    let doc = root(ast, 0);
    let value = value_of(ast, doc, "b");
    assert_eq!(ast.node(value).span.start.line, 4, "rebased onto the original file");
    assert_eq!(ast.node(value).span.start.col, 4);
}
