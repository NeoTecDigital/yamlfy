// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 3: symbols, tag classification, and the two maps the front end omits.

mod common;

use yfi_core::link::{source_order, SourceOrder};
use yfi_core::{intern, TagKind};
use yfi_syntax::{NodeId, NodeKind};

#[test]
fn one_name_written_in_two_files_interns_to_one_symbol() {
    let project = common::open_clean("nested-namespaces");
    let interned = intern(&project);
    let port = interned.symbols().get("port").expect("`port` is a key in every file");
    let mut seen = 0usize;
    for file in project.files() {
        let keys: Vec<_> = (0..file.ast.nodes().len())
            .filter_map(|i| interned.key_of(file.id, NodeId(i as u32)))
            .collect();
        if keys.contains(&port) {
            seen += 1;
        }
    }
    assert!(seen >= 2, "the same key text in two files must share one symbol");
    assert_eq!(interned.symbols().resolve(port), Some("port"));
}

#[test]
fn namespace_components_are_interned_separately() {
    let project = common::open_clean("nested-namespaces");
    let interned = intern(&project);
    let billing = common::scope_by(&project, "nested-namespaces/billing");
    let components = interned.namespace_of(billing);
    let text: Vec<&str> =
        components.iter().filter_map(|s| interned.symbols().resolve(*s)).collect();
    assert_eq!(text, ["acme", "billing"]);

    let net = common::scope_by(&project, "nested-namespaces/net");
    assert_eq!(interned.namespace_of(net).first(), components.first(), "`acme` is shared");
}

#[test]
fn tags_are_classified_by_suffix() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file = project.files().iter().find(|f| f.relative.ends_with("tags.yfy")).expect("tags.yfy");
    let kinds: Vec<TagKind> = (0..file.ast.nodes().len())
        .filter_map(|i| interned.tag_kind(file.id, NodeId(i as u32)))
        .collect();

    for expected in [TagKind::Header, TagKind::Type, TagKind::Node, TagKind::Ref, TagKind::Edge] {
        assert!(kinds.contains(&expected), "{} missing from {kinds:?}", expected.as_str());
    }
    assert_eq!(
        kinds.iter().filter(|k| **k == TagKind::Node).count(),
        2,
        "`!node` and the verbatim `!<node>` are the same tag"
    );
    assert!(kinds.contains(&TagKind::Other), "`!!int` is a core-schema tag, never ours");
}

#[test]
fn a_tag_directive_rewrites_the_handle_and_classification_survives_it() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file =
        project.files().iter().find(|f| f.relative.ends_with("remapped.yfy")).expect("remapped");
    let kinds: Vec<TagKind> = (0..file.ast.nodes().len())
        .filter_map(|i| interned.tag_kind(file.id, NodeId(i as u32)))
        .collect();
    assert!(kinds.contains(&TagKind::Header), "the header tag is remapped, not lost: {kinds:?}");
    assert!(kinds.contains(&TagKind::Node), "so is `!node`: {kinds:?}");
    assert!(file.header.is_some(), "and the header is still read");
}

#[test]
fn every_node_knows_its_document() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file = project.files().iter().find(|f| f.relative.ends_with("tags.yfy")).expect("tags.yfy");
    for (index, document) in file.ast.documents().iter().enumerate() {
        assert_eq!(
            interned.document_of(file.id, document.root),
            Some(u32::try_from(index).expect("index")),
            "document {index}'s root"
        );
    }
    let documents = file.ast.documents().len();
    assert_eq!(documents, 3, "header, `!type`, `!node`");
    for index in 0..file.ast.nodes().len() {
        let found = interned.document_of(file.id, NodeId(index as u32));
        assert!(found.is_some_and(|d| (d as usize) < documents), "node {index} has no document");
    }
}

#[test]
fn a_node_orphaned_by_error_recovery_belongs_to_no_document() {
    let project = common::open_at("fixtures/malformed/multi-error-multidoc.yml");
    let interned = intern(&project);
    let file = &project.files()[0];
    assert!(project.diagnostics().has_errors(), "the fixture is malformed on purpose");
    let orphans = (0..file.ast.nodes().len())
        .filter(|i| interned.document_of(file.id, NodeId(*i as u32)).is_none())
        .count();
    assert!(
        orphans > 0,
        "recovery discarded two documents, so their nodes must belong to none; \
         without orphans this test proves nothing"
    );
    for index in 0..file.ast.nodes().len() {
        let node = NodeId(index as u32);
        let Some(document) = interned.document_of(file.id, node) else { continue };
        let span = file.ast.documents()[document as usize].span;
        let start = file.ast.node(node).span.start.byte;
        assert!(
            start >= span.start.byte && start <= span.end.byte,
            "node {index} was attributed to a document it does not sit inside"
        );
    }
}

#[test]
fn every_node_knows_its_parent_and_every_root_has_none() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file = project.files().iter().find(|f| f.relative.ends_with("tags.yfy")).expect("tags.yfy");

    for document in file.ast.documents() {
        assert_eq!(interned.parent_of(file.id, document.root), None, "a root has no parent");
    }
    for index in 0..file.ast.nodes().len() {
        let node = NodeId(index as u32);
        let Some(parent) = interned.parent_of(file.id, node) else { continue };
        assert!(parent.index() > node.index(), "the arena is post-order");
        assert!(
            file.ast.children(parent).contains(&node),
            "node {index} is not a child of the parent it was given"
        );
    }
}

#[test]
fn a_mapping_key_and_its_value_share_the_parent_mapping() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file = project.files().iter().find(|f| f.relative.ends_with("tags.yfy")).expect("tags.yfy");
    let mut checked = 0usize;
    for index in 0..file.ast.nodes().len() {
        let map = NodeId(index as u32);
        let Some(entries) = file.ast.entries(map) else { continue };
        for entry in entries {
            assert_eq!(interned.parent_of(file.id, entry.key), Some(map));
            assert_eq!(interned.parent_of(file.id, entry.value), Some(map));
            assert!(interned.key_of(file.id, entry.key).is_some(), "scalar keys are interned");
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn an_alias_is_a_leaf_with_a_parent_but_no_interned_key() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let file = project.files().iter().find(|f| f.relative.ends_with("tags.yfy")).expect("tags.yfy");
    let alias = (0..file.ast.nodes().len())
        .map(|i| NodeId(i as u32))
        .find(|id| matches!(file.ast.node(*id).kind, NodeKind::Alias(_)))
        .expect("`*Api` is an alias");
    assert!(interned.parent_of(file.id, alias).is_some());
    assert_eq!(interned.key_of(file.id, alias), None);
}

#[test]
fn every_node_records_the_scope_path_its_axes_compose_over() {
    let project = common::open_clean("nested-namespaces");
    let interned = intern(&project);
    let file =
        project.files().iter().find(|f| f.relative.ends_with("invoices.yfy")).expect("invoices");
    let root = file.ast.documents()[0].root;

    assert_eq!(interned.scope_of(file.id, root), Some(file.scope));
    let path = interned.scope_path_of(file.id, root).expect("a scope path");
    assert_eq!(path, project.scopes().path(file.scope));
    assert_eq!(path.len(), 2, "root then `billing`");
    assert_eq!(path.last(), Some(&file.scope));
}

#[test]
fn the_total_order_is_file_rank_then_document_then_written_position() {
    // The arena is post-order, so scanning it is *not* written order. That is
    // why the project orders by `source_order` and not by the node index.
    let project = common::open_clean("nested-namespaces");
    let interned = intern(&project);
    let mut scanned: Vec<SourceOrder> = Vec::new();
    for file in project.files() {
        for index in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(index).expect("arena index"));
            let Some(order) = source_order(&project, &interned, file.id, node) else { continue };
            assert_eq!(order.file, file.rank, "the file component is the file's rank");
            scanned.push(order);
        }
    }
    assert!(!scanned.is_empty(), "the project has nodes");
    let mut written = scanned.clone();
    written.sort();
    assert_ne!(scanned, written, "arena order is post-order, not written order");
}

#[test]
fn an_unknown_file_answers_none_rather_than_panicking() {
    let project = common::open_clean("tagged");
    let interned = intern(&project);
    let missing = yfi_syntax::FileId(9_999);
    assert!(interned.index(missing).is_none());
    assert_eq!(interned.document_of(missing, NodeId(0)), None);
    assert_eq!(interned.parent_of(missing, NodeId(0)), None);
    assert_eq!(interned.scope_of(missing, NodeId(0)), None);
    assert_eq!(interned.tag_kind(missing, NodeId(0)), None);
}
