// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! The cycle-torture corpus.
//!
//! Every assertion here exists to prove one thing: an alias is a reference, not
//! a copy, so a cyclic alias graph parses in bounded time and bounded memory.

mod common;

use common::{parse_clean, root, value_of};
use yamlfy_syntax::NodeKind;

#[test]
fn self_alias_closes_a_one_cycle() {
    let (_, parsed) = parse_clean("cycles/self-alias.yml");
    let ast = &parsed.ast;
    let doc = root(ast, 0);
    let alias = value_of(ast, doc, "me");

    assert!(matches!(ast.node(alias).kind, NodeKind::Alias(_)));
    assert_eq!(ast.alias_target(alias), Some(doc), "`*self` must point back at its own mapping");
    assert!(ast.is_cyclic_from(doc));
}

#[test]
fn a_cycle_does_not_duplicate_nodes() {
    let (_, parsed) = parse_clean("cycles/self-alias.yml");
    // 2 keys, 1 scalar value, 1 alias, 1 mapping. If the alias were expanded by
    // copying, this count would grow without bound.
    assert_eq!(parsed.ast.nodes().len(), 5);
}

#[test]
fn mutual_alias_is_reachable_in_both_directions() {
    let (_, parsed) = parse_clean("cycles/mutual-alias.yml");
    let ast = &parsed.ast;
    let a = root(ast, 0);
    let b = value_of(ast, a, "child");

    let from_b = value_of(ast, b, "parent");
    assert_eq!(ast.alias_target(from_b), Some(a));
    let back = value_of(ast, a, "back");
    assert_eq!(ast.alias_target(back), Some(b));

    assert!(ast.is_cyclic_from(a));
    assert!(ast.is_cyclic_from(b));
}

#[test]
fn deep_cycle_terminates_and_visits_every_node_once() {
    let (_, parsed) = parse_clean("cycles/deep-cycle.yml");
    let ast = &parsed.ast;
    let n1 = root(ast, 0);

    let reachable = ast.reachable_from(n1);
    let unique: std::collections::HashSet<_> = reachable.iter().copied().collect();
    assert_eq!(reachable.len(), unique.len(), "reachable_from must not repeat a node");
    assert_eq!(reachable.len(), ast.nodes().len());
    assert!(ast.is_cyclic_from(n1));
}

#[test]
fn sequence_can_contain_itself() {
    let (_, parsed) = parse_clean("cycles/cycle-in-sequence.yml");
    let ast = &parsed.ast;
    let ring = root(ast, 0);
    let items = ast.items(ring).expect("root is a sequence");

    assert_eq!(items.len(), 3);
    assert_eq!(ast.alias_target(items[1]), Some(ring));
    assert!(ast.is_cyclic_from(ring));
}

#[test]
fn identical_anchor_names_in_two_documents_are_two_nodes() {
    let (_, parsed) = parse_clean("cycles/cycle-shared-across-documents.yml");
    let ast = &parsed.ast;
    assert_eq!(ast.documents().len(), 2);

    let first = root(ast, 0);
    let second = root(ast, 1);
    assert_ne!(first, second);
    assert_eq!(ast.alias_target(value_of(ast, first, "self")), Some(first));
    assert_eq!(ast.alias_target(value_of(ast, second, "self")), Some(second));
}

#[test]
fn merge_cycles_parse_without_error() {
    // Cyclic merge is a link-pass error (E0212). The parser's job is to hand the
    // link pass a complete, finite graph to reject.
    for fixture in [
        "cycles/merge-self-cycle.yml",
        "cycles/merge-mutual-cycle.yml",
        "cycles/merge-deep-cycle.yml",
        "cycles/merge-oscillating.yml",
    ] {
        let (_, parsed) = parse_clean(fixture);
        let merges = parsed
            .ast
            .nodes()
            .iter()
            .enumerate()
            .filter_map(|(i, _)| parsed.ast.entries(yamlfy_syntax::NodeId(i as u32)))
            .flatten()
            .filter(|e| e.merge)
            .count();
        assert!(merges > 0, "{fixture} should contain merge keys");
    }
}

#[test]
fn data_cycle_with_an_acyclic_merge_graph_is_clean() {
    let (_, parsed) = parse_clean("cycles/alias-cycle-with-merge-dag.yml");
    let ast = &parsed.ast;
    let ring = value_of(ast, root(ast, 0), "ring");
    assert!(ast.is_cyclic_from(ring), "the data edge `next: *ring` is a legal cycle");

    let merge = ast.entries(ring).expect("mapping").iter().filter(|e| e.merge).count();
    assert_eq!(merge, 1);
}
