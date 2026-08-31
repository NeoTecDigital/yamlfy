// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Anchor shadowing is positional: an alias binds to the most recent
//! *preceding* definition of its name.

mod common;

use common::{count, parse, parse_clean, root, value_of};
use yamlfy_syntax::{Code, ParseOptions, Severity};

#[test]
fn two_definitions_bind_two_different_nodes() {
    let (_, parsed) = parse("shadowing/basic-shadow.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    let first_def = value_of(ast, doc, "first-def");
    let second_def = value_of(ast, doc, "second-def");
    let first_use = value_of(ast, doc, "first-use");
    let second_use = value_of(ast, doc, "second-use");

    assert_eq!(ast.alias_target(first_use), Some(first_def));
    assert_eq!(ast.alias_target(second_use), Some(second_def));
    assert_ne!(first_def, second_def);
    assert_eq!(count(&parsed.diagnostics, Code::AnchorShadowed), 1);
}

#[test]
fn shadowing_leaks_out_of_the_block_it_is_written_in() {
    let (_, parsed) = parse("shadowing/shadow-across-nesting.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);
    let outer_def = value_of(ast, doc, "outer-def");
    let inner = value_of(ast, doc, "inner");
    let inner_def = value_of(ast, inner, "inner-def");

    assert_eq!(ast.alias_target(value_of(ast, inner, "use-before")), Some(outer_def));
    assert_eq!(ast.alias_target(value_of(ast, inner, "use-after")), Some(inner_def));
    assert_eq!(
        ast.alias_target(value_of(ast, doc, "after-block")),
        Some(inner_def),
        "positional, not lexical: the inner definition governs after the block ends"
    );
}

#[test]
fn three_definitions_bind_three_nodes() {
    let (_, parsed) = parse("shadowing/shadow-three-times.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    let targets: Vec<_> = ["u1", "u2", "u3"]
        .iter()
        .map(|k| ast.alias_target(value_of(ast, doc, k)).expect("bound"))
        .collect();
    let defs: Vec<_> = ["d1", "d2", "d3"].iter().map(|k| value_of(ast, doc, k)).collect();

    assert_eq!(targets, defs);
    assert_eq!(count(&parsed.diagnostics, Code::AnchorShadowed), 2);
}

#[test]
fn shadowed_collections_bind_by_position_too() {
    let (_, parsed) = parse("shadowing/shadow-collection.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);

    assert_eq!(ast.alias_target(value_of(ast, doc, "p")), Some(value_of(ast, doc, "a")));
    assert_eq!(ast.alias_target(value_of(ast, doc, "q")), Some(value_of(ast, doc, "b")));
    assert!(ast.items(value_of(ast, doc, "a")).is_some());
    assert!(ast.entries(value_of(ast, doc, "b")).is_some());
}

#[test]
fn merge_sources_are_chosen_positionally() {
    let (_, parsed) = parse("shadowing/shadow-in-merge.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);
    let user = value_of(ast, doc, "user");
    let merge = ast.entries(user).expect("mapping").iter().find(|e| e.merge).expect("merge key");

    assert_eq!(
        ast.alias_target(merge.value),
        Some(value_of(ast, doc, "second")),
        "`<<: *defaults` must merge the second definition"
    );
}

#[test]
fn distinct_names_do_not_warn() {
    let (_, parsed) = parse_clean("shadowing/anchor-reused-not-shadowed.yml");
    assert_eq!(count(&parsed.diagnostics, Code::AnchorShadowed), 0);
}

#[test]
fn anchors_do_not_cross_a_document_boundary() {
    let (sources, parsed) = parse("shadowing/no-shadow-across-documents.yml");
    assert_eq!(
        count(&parsed.diagnostics, Code::CrossDocumentAlias),
        1,
        "{}",
        parsed.diagnostics.render(&sources)
    );
    assert_eq!(
        count(&parsed.diagnostics, Code::AnchorShadowed),
        0,
        "the same name in two documents is not shadowing"
    );
    let alias = parsed.ast.nodes().iter().enumerate().find_map(|(i, _)| {
        let id = yamlfy_syntax::NodeId(u32::try_from(i).unwrap());
        parsed.ast.alias(id).map(|a| a.cross_document)
    });
    assert_eq!(alias, Some(true));
}

#[test]
fn shadow_warning_severity_is_configurable() {
    let mut options = ParseOptions::default();
    options.severities.insert(Code::AnchorShadowed, Severity::Allow);
    let (_, parsed) = common::parse_with("shadowing/basic-shadow.yml", &options);
    assert!(parsed.diagnostics.is_empty());

    let mut options = ParseOptions::default();
    options.severities.insert(Code::AnchorShadowed, Severity::Error);
    let (_, parsed) = common::parse_with("shadowing/basic-shadow.yml", &options);
    assert!(parsed.diagnostics.has_errors());
}

#[test]
fn shadow_diagnostic_points_at_both_definitions() {
    let (sources, parsed) = parse("shadowing/basic-shadow.yml");
    let diagnostic = parsed.diagnostics.with_code(Code::AnchorShadowed).next().expect("warning");
    let primary = sources.location(diagnostic.span.expect("span"));
    let note = diagnostic.notes.first().expect("note");
    let earlier = sources.location(note.span.expect("span"));

    assert!(primary.ends_with("basic-shadow.yml:6:13"), "{primary}");
    assert!(earlier.ends_with("basic-shadow.yml:4:12"), "{earlier}");
}
