// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Merge-key classification. Resolution belongs to the link pass; what the
//! parser owes it is an unambiguous answer to "which entries are merges".

mod common;

use common::{count, parse, parse_clean, root, value_of};
use yfi_syntax::{is_merge_key, Code, NodeKind, ParseOptions, SourceMap};

fn merge_entries(ast: &yfi_syntax::Ast, map: yfi_syntax::NodeId) -> usize {
    ast.entries(map).expect("mapping").iter().filter(|e| e.merge).count()
}

#[test]
fn a_plain_merge_key_is_recognised() {
    let (_, parsed) = parse_clean("merge/simple.yml");
    let ast = &parsed.ast;
    let web = value_of(ast, root(ast, 0), "web");

    assert_eq!(merge_entries(ast, web), 1);
    let merge = ast.entries(web).unwrap().iter().find(|e| e.merge).unwrap();
    assert!(matches!(ast.node(merge.value).kind, NodeKind::Alias(_)));
}

#[test]
fn a_quoted_merge_key_is_an_ordinary_string_key() {
    let (_, parsed) = parse_clean("merge/quoted-merge-key-is-literal.yml");
    let ast = &parsed.ast;
    let derived = value_of(ast, root(ast, 0), "derived");
    let entries = ast.entries(derived).expect("mapping");

    assert_eq!(entries.len(), 2, "both `<<` and `\"<<\"` are present");
    assert_eq!(merge_entries(ast, derived), 1);
    assert_eq!(
        count(&parsed.diagnostics, Code::DuplicateKey),
        0,
        "a merge key and a literal `<<` key are different keys"
    );
}

#[test]
fn two_merge_keys_in_one_mapping_are_rejected() {
    let (sources, parsed) = parse("merge/multiple-merge-keys.yml");
    assert_eq!(count(&parsed.diagnostics, Code::DuplicateMergeKey), 1);
    assert_eq!(count(&parsed.diagnostics, Code::DuplicateKey), 0);

    let diagnostic = parsed.diagnostics.with_code(Code::DuplicateMergeKey).next().unwrap();
    assert!(diagnostic.message.contains("<<: [*a, *b]"), "the fix is spelled out");
    assert!(diagnostic.notes.first().and_then(|n| n.span).is_some());
    assert!(sources.location(diagnostic.span.unwrap()).ends_with(":11:3"));
}

#[test]
fn merge_position_within_the_mapping_is_recorded_but_not_significant() {
    let (_, parsed) = parse_clean("merge/own-key-wins.yml");
    let ast = &parsed.ast;
    let derived = value_of(ast, root(ast, 0), "derived");
    let entries = ast.entries(derived).expect("mapping");

    assert_eq!(entries.len(), 3);
    assert!(entries[1].merge, "the merge key sits between the two own keys");
    assert!(!entries[0].merge && !entries[2].merge);
}

#[test]
fn a_sequence_of_sources_is_kept_in_order() {
    let (_, parsed) = parse_clean("merge/sequence-precedence.yml");
    let ast = &parsed.ast;
    let derived = value_of(ast, root(ast, 0), "derived");
    let merge = ast.entries(derived).unwrap().iter().find(|e| e.merge).unwrap();
    let items = ast.items(merge.value).expect("sequence of sources");

    assert_eq!(items.len(), 2);
    assert_eq!(ast.alias_target(items[0]), Some(value_of(ast, root(ast, 0), "p")));
    assert_eq!(ast.alias_target(items[1]), Some(value_of(ast, root(ast, 0), "q")));
}

#[test]
fn an_inline_mapping_is_a_legal_source() {
    let (_, parsed) = parse_clean("merge/inline-mapping-source.yml");
    let ast = &parsed.ast;
    let derived = value_of(ast, root(ast, 0), "derived");
    let merge = ast.entries(derived).unwrap().iter().find(|e| e.merge).unwrap();

    assert!(ast.entries(merge.value).is_some(), "the source is a mapping, not an alias");
}

#[test]
fn merge_chains_and_diamonds_parse_cleanly() {
    for fixture in
        ["merge/transitive.yml", "cycles/merge-acyclic-chain.yml", "cycles/merge-diamond.yml"]
    {
        let (_, parsed) = parse_clean(fixture);
        assert!(parsed.ast.documents().len() == 1, "{fixture}");
    }
}

#[test]
fn explicit_merge_tag_is_honoured_and_a_string_tag_is_not() {
    let mut sources = SourceMap::new();
    let file = sources.add("tagged.yml", "a: 1\n!!merge <<: {b: 2}\n!!str \"<<\": 3\n");
    let parsed = yfi_syntax::parse(&sources, file, &ParseOptions::default());
    let ast = &parsed.ast;
    let entries = ast.entries(root(ast, 0)).expect("mapping");

    assert_eq!(entries.len(), 3);
    assert!(!entries[0].merge);
    assert!(entries[1].merge, "`!!merge` makes a merge key");
    assert!(!entries[2].merge, "`!!str` does not");
    assert!(is_merge_key(ast, entries[1].key));
}

#[test]
fn a_merge_tag_on_a_non_scalar_key_is_reported_and_merges_nothing() {
    // D1.1 says *scalar*, so `!!merge [k]: 1` is correctly not a merge key. It
    // then became an ordinary complex key and the tag classified nothing,
    // printed nothing and was consumed by nothing — a reservation with no
    // diagnostic behind it, which D7.4 names as the failure reserving a
    // spelling exists to prevent. `E0111` says it out loud.
    let (_, parsed) = parse("merge/merge-tag-on-complex-key.yml");
    let ast = &parsed.ast;
    let holder = value_of(ast, root(ast, 0), "holder");

    assert_eq!(merge_entries(ast, holder), 0, "the tag makes no merge key of a collection");
    assert_eq!(count(&parsed.diagnostics, Code::MergeTagOnComplexKey), 2, "one per tagged key");
    assert_eq!(
        count(&parsed.diagnostics, Code::DuplicateMergeKey),
        0,
        "and it is not a merge key, so it is not two of them either"
    );
}

#[test]
fn two_complex_keys_that_are_alike_are_still_not_compared() {
    // The deliberate half of the same fixture, and it is permanent: key
    // identity in this language is a key's **scalar text**, so a complex key
    // has no identity to compare and neither pair below is reported. The rule
    // is bounded — a non-merge key is absorbed as data, so this can only ever
    // be silence and never a wrong graph.
    let (_, parsed) = parse("merge/merge-tag-on-complex-key.yml");
    assert_eq!(count(&parsed.diagnostics, Code::DuplicateKey), 0);
}
