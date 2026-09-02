// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `.yfy` front end: three constructs YAML rejects, and the promise that
//! adding them moves nothing.
//!
//! The pre-pass exists because `//`, `<?-- … --!>` and `<?-- … -->` are scanner
//! errors in every YAML implementation. It is a **character-for-character
//! substitution**, and that is the whole design: the span model survived BOMs,
//! CRLF, multi-byte UTF-8 and parser restarts, and a front end that shifted a
//! single offset would undo all of it. So the tests here are mostly about what
//! *did not* move.

mod common;

use yfi_syntax::{
    parse, Ast, BlockKind, Code, Dialect, NodeKind, ParseOptions, ScalarStyle, SourceMap, Span,
};

/// Parse `text` as one dialect or the other.
fn read(name: &str, text: &str, dialect: Dialect) -> (SourceMap, yfi_syntax::Parsed) {
    let mut sources = SourceMap::new();
    let file = sources.add_as(name, text, dialect);
    let parsed = parse(&sources, file, &ParseOptions::default());
    (sources, parsed)
}

/// Every node's kind and span, in arena order.
fn shape(ast: &Ast) -> Vec<(NodeKind, Span)> {
    ast.nodes().iter().map(|node| (node.kind, node.span)).collect()
}

/// The same file written twice: once with the constructs, once with the plain
/// YAML an author would otherwise have written in the same space.
const WITH: &str = "--- !node &Api\n\
                    name: service      // the name\n\
                    port: 8443\n\
                    doc: <?-- why 8443 --!>\n\
                    list:\n\
                      - one            // first\n\
                      - two\n";

const WITHOUT: &str = "--- !node &Api\n\
                       name: service      #  the name\n\
                       port: 8443\n\
                       doc:                   \n\
                       list:\n\
                         - one            #  first\n\
                         - two\n";

#[test]
fn the_constructs_move_no_node_at_all() {
    // The two files are the same length and hold the same nodes; one is written
    // in `.yfy` and the other is ordinary YAML. Nothing about the arena may
    // depend on which was written.
    let (_, with) = read("a.yfy", WITH, Dialect::Yamlfication);
    let (_, without) = read("a.yml", WITHOUT, Dialect::BaseYaml);
    assert!(with.diagnostics.is_empty() && without.diagnostics.is_empty());
    assert_eq!(shape(&with.ast), shape(&without.ast));
}

#[test]
fn reading_the_whole_corpus_as_yamlfication_source_changes_nothing() {
    // Every fixture is already free of the three constructs, so the pre-pass
    // must be the identity over all 47 of them — nodes, kinds, spans and
    // diagnostics alike. This is the guard that catches a rewrite rule that is
    // too eager: a `//` inside a URL, a `#` inside a block scalar, a `<` in a
    // flow sequence.
    for relative in common::all_fixtures() {
        // One fixture is deliberately not UTF-8; there is no text to compare.
        let Ok(text) = std::fs::read_to_string(common::fixtures().join(&relative)) else {
            continue;
        };
        let (_, base) = read(&relative, &text, Dialect::BaseYaml);
        let (_, source) = read(&relative, &text, Dialect::Yamlfication);
        assert_eq!(shape(&base.ast), shape(&source.ast), "{relative}: the pre-pass moved a node");
        assert_eq!(
            base.diagnostics.items().len(),
            source.diagnostics.items().len(),
            "{relative}: the pre-pass changed what was found"
        );
    }
}

#[test]
fn a_line_comment_is_a_comment_and_a_url_is_not() {
    let (_, parsed) = read(
        "c.yfy",
        "// leading\nurl: http://host/thing\nkept: \"a // b\"\n",
        Dialect::Yamlfication,
    );
    assert!(parsed.diagnostics.is_empty());
    let doc = parsed.ast.documents()[0].root;
    let url = common::value_of(&parsed.ast, doc, "url");
    assert_eq!(&*parsed.ast.scalar(url).expect("scalar").value, "http://host/thing");
    let kept = common::value_of(&parsed.ast, doc, "kept");
    assert_eq!(&*parsed.ast.scalar(kept).expect("scalar").value, "a // b");
}

#[test]
fn a_code_block_reaches_the_arena_as_a_scalar_carrying_its_text_verbatim() {
    let text = "handler: <?-- fn(x) { return x: 1 } -->\nport: 80\n";
    let (_, parsed) = read("h.yfy", text, Dialect::Yamlfication);
    assert!(parsed.diagnostics.is_empty(), "the contents are never parsed");
    let doc = parsed.ast.documents()[0].root;
    let handler = common::value_of(&parsed.ast, doc, "handler");
    let scalar = parsed.ast.scalar(handler).expect("a code block is a scalar");
    assert_eq!(scalar.style, ScalarStyle::Code, "carrying the execute flag");
    assert_eq!(&*scalar.value, " fn(x) { return x: 1 } ", "verbatim, delimiters removed");

    let span = parsed.ast.node(handler).span;
    assert_eq!(span.start.col, 10, "the span is the region the author wrote");
    assert_eq!(span.end.col, 40, "and ends one past its last character");
}

#[test]
fn a_code_block_may_hold_anything_and_is_never_parsed() {
    // Every one of these is a syntax error as YAML. None of them is one here.
    for body in ["[unbalanced", "{a: [1,", "\"", "--- x", "\ta\tb", "*unknown"] {
        let text = format!("a: <?-- {body} -->\n");
        let (_, parsed) = read("k.yfy", &text, Dialect::Yamlfication);
        assert!(parsed.diagnostics.is_empty(), "`{body}` was read as YAML");
        let doc = parsed.ast.documents()[0].root;
        let value = common::value_of(&parsed.ast, doc, "a");
        assert_eq!(&*parsed.ast.scalar(value).expect("scalar").value, &format!(" {body} "));
    }
}

#[test]
fn a_documentation_block_emits_no_node_and_is_captured_for_the_generator() {
    let text = "--- !type &Port\n// what a port is\nport: <?-- the listening port --!>\n";
    let (sources, parsed) = read("d.yfy", text, Dialect::Yamlfication);
    assert!(parsed.diagnostics.is_empty());
    let file = sources.file(parsed.ast.file());
    let blocks = file.blocks();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, BlockKind::Documentation);
    assert_eq!(&*blocks[0].text, " the listening port ");
    assert_eq!(blocks[0].span.start.line, 3);
    assert_eq!(blocks[0].span.start.col, 7);

    let doc = parsed.ast.documents()[0].root;
    let value = common::value_of(&parsed.ast, doc, "port");
    assert_eq!(
        &*parsed.ast.scalar(value).expect("scalar").value,
        "~",
        "documentation is not a value: the key is left null"
    );
}

#[test]
fn a_multi_line_code_block_keeps_the_lines_below_it_where_they_were() {
    let text = "a: <?--\n  anything at all\n  -->\nb: 2\nc: 3\n";
    let (_, parsed) = read("m.yfy", text, Dialect::Yamlfication);
    assert!(parsed.diagnostics.is_empty());
    let doc = parsed.ast.documents()[0].root;
    for (key, line) in [("b", 4), ("c", 5)] {
        let value = common::value_of(&parsed.ast, doc, key);
        assert_eq!(parsed.ast.node(value).span.start.line, line, "`{key}` moved");
    }
    let a = common::value_of(&parsed.ast, doc, "a");
    assert_eq!(&*parsed.ast.scalar(a).expect("scalar").value, "\n  anything at all\n  ");
}

#[test]
fn byte_offsets_still_index_the_file_around_multi_byte_text_in_a_block() {
    let text = "a: héllo\nb: <?-- ü --!>\nc: wörld\n";
    let (sources, parsed) = read("u.yfy", text, Dialect::Yamlfication);
    let file = sources.file(parsed.ast.file());
    let doc = parsed.ast.documents()[0].root;
    for key in ["a", "c"] {
        let value = common::value_of(&parsed.ast, doc, key);
        let span = parsed.ast.node(value).span;
        let sliced = &file.text()[span.start.byte as usize..span.end.byte as usize];
        assert_eq!(
            sliced,
            &*parsed.ast.scalar(value).expect("scalar").value,
            "the span of `{key}` must slice its own text out of the file as written"
        );
    }
}

#[test]
fn an_unterminated_block_is_one_diagnostic_and_costs_one_line() {
    let text = "a: <?-- oops\nb: 2\n";
    let (sources, parsed) = read("e.yfy", text, Dialect::Yamlfication);
    assert_eq!(common::count(&parsed.diagnostics, Code::UnterminatedBlock), 1);
    assert!(parsed.diagnostics.render(&sources).contains("e.yfy:1:4"));
    let doc = parsed.ast.documents()[0].root;
    let b = common::value_of(&parsed.ast, doc, "b");
    assert_eq!(&*parsed.ast.scalar(b).expect("scalar").value, "2", "the rest of the file survives");
}

#[test]
fn none_of_it_happens_in_base_yaml() {
    let text = "a: 1 // not a comment\nb: <?-- not a block -->\n";
    let (_, parsed) = read("p.yml", text, Dialect::BaseYaml);
    let doc = parsed.ast.documents()[0].root;
    assert_eq!(
        &*parsed.ast.scalar(common::value_of(&parsed.ast, doc, "a")).expect("s").value,
        "1 // not a comment"
    );
    let b = common::value_of(&parsed.ast, doc, "b");
    assert_eq!(parsed.ast.scalar(b).expect("s").style, ScalarStyle::Plain);
    assert_eq!(&*parsed.ast.scalar(b).expect("s").value, "<?-- not a block -->");
}
