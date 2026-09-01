// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The import prelude: what it binds, and what it must leave untouched.
//!
//! The front end is handed a file's imported names and synthesises a document
//! declaring them, because `saphyr-parser` will not otherwise let an alias to
//! them past the scanner. Everything here is about the second half of that
//! sentence — the prelude has to disappear again completely.

mod common;

use yamlfy_syntax::{
    parse, parse_with_imports, Ast, Code, FileId, Import, NodeId, ParseOptions, Pos, SourceMap,
    Span,
};

const IMPORTING: &str = concat!(
    "--- !yamlfy/header\nimports: [net.yfy]\n",
    "--- !node &Api\nextends: *Service\nport: 8443\n"
);

/// A source map holding the exporting file and the importing one, plus the
/// import record the exporting file's `&Service` produces.
fn two_files(importing: &str) -> (SourceMap, FileId, Vec<Import>) {
    let mut sources = SourceMap::new();
    let net = sources.add("net.yfy", "--- !type &Service\nport: !!int\n");
    let exported = parse(&sources, net, &ParseOptions::default());
    let def = exported
        .ast
        .anchors()
        .defs()
        .iter()
        .find(|d| &*d.name == "Service")
        .expect("net.yfy defines `&Service`");
    let import = Import { name: def.name.clone(), span: def.span };
    let app = sources.add("app.yfy", importing);
    (sources, app, vec![import])
}

/// The node `net.yfy`'s `&Service` names. Its arena is rebuilt per call, which
/// is exactly why the front end refuses to guess it during a parse.
const SERVICE: NodeId = NodeId(4);

/// Do what `yamlfy_core::bind::rebind` does: point every imported definition at
/// the node it names, once the exporting file will not be parsed again.
fn rebind(ast: &mut Ast, node: NodeId) {
    let ids: Vec<_> =
        ast.anchors().defs().iter().filter(|d| d.is_imported()).map(|d| d.id).collect();
    for id in ids {
        assert!(ast.rebind_import(id, node));
    }
}

#[test]
fn an_imported_name_binds_an_alias_that_would_otherwise_be_unknown() {
    let (sources, app, imports) = two_files(IMPORTING);
    let without = parse(&sources, app, &ParseOptions::default());
    assert_eq!(
        common::count(&without.diagnostics, Code::SyntaxError),
        1,
        "without the import `*Service` is an unknown anchor:\n{}",
        without.diagnostics.render(&sources)
    );

    let mut with = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);
    assert!(
        with.diagnostics.is_empty(),
        "with it the file is clean:\n{}",
        with.diagnostics.render(&sources)
    );
    let alias = common::value_of(&with.ast, common::root(&with.ast, 1), "extends");
    assert_eq!(with.ast.alias(alias).map(|a| &*a.name), Some("Service"));
    assert_eq!(
        with.ast.alias_binding(alias),
        None,
        "the node is not knowable while the exporting file may still be reparsed"
    );

    rebind(&mut with.ast, SERVICE);
    assert_eq!(
        with.ast.alias_binding(alias),
        Some((imports[0].span.file, SERVICE)),
        "once rebound the binding names a node of the exporting file"
    );
    assert_eq!(with.ast.alias_target(alias), None, "and no node of this one");
}

#[test]
fn the_prelude_moves_no_position_and_adds_no_document() {
    // The strongest statement available: parsing with a prelude and parsing
    // without one differ in the anchor table and in nothing else at all.
    let clean = "--- !yamlfy/header\nimports: [net.yfy]\n--- !node &Api\nport: 8443\n";
    let (sources, app, imports) = two_files(clean);
    let without = parse(&sources, app, &ParseOptions::default());
    let with = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);

    let positions = |ast: &Ast| -> Vec<(yamlfy_syntax::NodeKind, Span)> {
        ast.nodes().iter().map(|n| (n.kind, n.span)).collect()
    };
    assert_eq!(
        positions(&with.ast),
        positions(&without.ast),
        "every node keeps its kind and its position"
    );
    assert_eq!(with.ast.documents().len(), without.ast.documents().len());
    assert_eq!(with.ast.documents().len(), 2, "the synthetic document is not one of them");
    for (a, b) in with.ast.documents().iter().zip(without.ast.documents()) {
        assert_eq!(a.span, b.span);
        assert_eq!(a.root, b.root);
    }
}

#[test]
fn an_imported_definition_carries_the_exporting_file_s_span() {
    let (sources, app, imports) = two_files(IMPORTING);
    let parsed = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);
    let def = parsed.ast.anchors().defs().iter().find(|d| d.is_imported()).expect("one import");

    assert_eq!(def.span.file, imports[0].span.file, "`net.yfy`, not `app.yfy`");
    assert_eq!(def.span, imports[0].span);
    assert_eq!(
        sources.location(def.span),
        "net.yfy:1:11",
        "the `&Service` token, at its own line and column"
    );
    assert!(!parsed.ast.file().eq(&def.span.file), "the arena is still one file's");
}

#[test]
fn an_import_does_not_make_an_alias_cross_a_document() {
    // D2.6 unchanged: `&Local` belongs to the second document, so the third
    // document's alias to it is still `E0130`, in a file that imports.
    let text = concat!(
        "--- !yamlfy/header\nimports: [net.yfy]\n--- &Local\nk: 1\n",
        "--- \nhere: *Service\nthere: *Local\n"
    );
    let (sources, app, imports) = two_files(text);
    let parsed = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);
    let rendered = parsed.diagnostics.render(&sources);

    assert_eq!(common::count(&parsed.diagnostics, Code::SyntaxError), 0, "{rendered}");
    assert_eq!(
        common::count(&parsed.diagnostics, Code::CrossDocumentAlias),
        1,
        "only `*Local` crosses:\n{rendered}"
    );
    assert!(rendered.contains("app.yfy:7:8: alias `*Local`"), "{rendered}");

    let third = common::root(&parsed.ast, 2);
    let here = common::value_of(&parsed.ast, third, "here");
    let bound = parsed.ast.alias(here).and_then(|a| parsed.ast.anchors().get(a.anchor));
    assert!(
        bound.is_some_and(|def| def.is_imported() && def.span.file != app),
        "while `*Service` is a definition of every document of this file"
    );
}

#[test]
fn imports_survive_a_parser_restart_after_a_syntax_error() {
    // Recovery throws the parser away and starts a new one on the tail of the
    // file. The prelude goes in front of that one too, so the second half of
    // the file still sees the imports.
    let text = concat!(
        "--- !yamlfy/header\nimports: [net.yfy]\n--- \nbroken: \"unterminated\n",
        "--- \nlate: *Service\n"
    );
    let (sources, app, imports) = two_files(text);
    let mut parsed = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);
    let rendered = parsed.diagnostics.render(&sources);

    assert_eq!(common::count(&parsed.diagnostics, Code::SyntaxError), 1, "{rendered}");
    rebind(&mut parsed.ast, SERVICE);
    let last = parsed.ast.documents().last().expect("the tail parsed");
    let late = common::value_of(&parsed.ast, last.root, "late");
    assert_eq!(
        parsed.ast.alias_binding(late),
        Some((imports[0].span.file, SERVICE)),
        "the restarted segment reinstalled the same import:\n{rendered}"
    );
    assert_eq!(
        common::count(&parsed.diagnostics, Code::AnchorShadowed),
        0,
        "and re-installing it is not a redefinition of it:\n{rendered}"
    );
}

#[test]
fn a_nameless_import_is_dropped_rather_than_shifting_every_later_one() {
    let (sources, app, mut imports) = two_files(IMPORTING);
    let nameless = Import {
        name: String::new().into_boxed_str(),
        span: Span::empty(imports[0].span.file, Pos::default()),
    };
    imports.insert(0, nameless);
    let parsed = parse_with_imports(&sources, app, &ParseOptions::default(), &imports);

    assert!(parsed.diagnostics.is_empty(), "{}", parsed.diagnostics.render(&sources));
    let alias = common::value_of(&parsed.ast, common::root(&parsed.ast, 1), "extends");
    assert_eq!(parsed.ast.alias(alias).map(|a| &*a.name), Some("Service"));
    assert_eq!(
        parsed.ast.anchors().defs().iter().filter(|d| d.is_imported()).count(),
        1,
        "the nameless one is not installed under the next one's name"
    );
}

#[test]
fn passing_no_imports_is_exactly_an_ordinary_parse() {
    let mut sources = SourceMap::new();
    let file = sources.add("t.yml", "--- &ring\nself: *ring\n");
    let a = parse(&sources, file, &ParseOptions::default());
    let b = parse_with_imports(&sources, file, &ParseOptions::default(), &[]);
    assert_eq!(a.ast.dump(), b.ast.dump());
    assert!(b.diagnostics.is_empty());
}
