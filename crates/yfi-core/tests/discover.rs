// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 1: project discovery, headers, and the scope tree.

mod common;

use std::collections::HashSet;

use yfi_core::{discover_in, DiscoverOptions, FileClass, Mutability, ScopeKind, Visibility};
use yfi_syntax::{Code, FileId, SourceMap};

#[test]
fn every_file_lands_in_one_source_map() {
    let project = common::open_clean("nested-namespaces");
    let ids: HashSet<_> = project.files().iter().map(|f| f.id).collect();
    assert_eq!(
        ids.len(),
        project.files().len(),
        "a per-file source map would collide on FileId(0)"
    );
    for (rank, file) in project.files().iter().enumerate() {
        assert_eq!(file.rank as usize, rank);
        assert_eq!(project.rank(file.id), Some(file.rank));
        assert!(project.file(file.id).is_some());
    }
}

/// `FileId` is an index into one [`SourceMap`], so two projects discovered into
/// two maps both start at `FileId(0)` and a span from either renders against
/// the other's text. [`discover_in`] exists to stop that, and this is where the
/// property is asserted: the CLI cannot show it, because it renders each
/// group's report before discovering the next and never holds two projects at
/// once — a fresh map per group produces byte-identical output and is caught by
/// nothing.
#[test]
fn a_second_project_discovered_into_one_map_keeps_the_first_one_s_file_ids() {
    let options = DiscoverOptions::default();
    let first = discover_in(SourceMap::new(), common::projects().join("imports-source"), &options);
    let first_ids: Vec<FileId> = first.files().iter().map(|f| f.id).collect();
    let first_paths: Vec<std::path::PathBuf> =
        first.files().iter().map(|f| f.path.clone()).collect();
    assert!(first_ids.len() > 1, "the first project must occupy more than `FileId(0)`");

    let second =
        discover_in(first.into_sources(), common::projects().join("import-alias"), &options);
    for id in second.files().iter().map(|f| f.id) {
        assert!(
            !first_ids.contains(&id),
            "{id:?} was already the first project's; a fresh map would restart at FileId(0)"
        );
    }
    for (id, path) in first_ids.iter().zip(&first_paths) {
        assert_eq!(
            second.sources().file(*id).path(),
            path,
            "and the map handed on still names the first project's files, so a span from \
             either project renders against its own text"
        );
    }
}

#[test]
fn discovery_order_is_lexicographic_by_path_relative_to_the_root() {
    let project = common::open_clean("nested-namespaces");
    assert_eq!(
        common::relative_paths(&project),
        [
            "billing/invoices.yfy",
            "billing/ledger.yfy",
            "net/edge.yaml",
            "net/service.yfy",
            "root.yfy",
        ]
    );
}

#[test]
fn the_same_tree_ranks_the_same_way_on_every_run() {
    let first = common::relative_paths(&common::open("scope-matrix"));
    let second = common::relative_paths(&common::open("scope-matrix"));
    assert_eq!(first, second);
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted, "rank order must be the relative-path order");
}

#[test]
fn the_extension_decides_the_class_and_an_unlisted_one_is_ignored() {
    let project = common::open_clean("nested-namespaces");
    for file in project.files() {
        let expected = if file.relative.extension().is_some_and(|e| e == "yfy") {
            FileClass::Source
        } else {
            FileClass::Data
        };
        assert_eq!(file.class, expected, "{}", file.relative.display());
    }
    assert!(
        !common::relative_paths(&project).iter().any(|p| p.ends_with(".txt")),
        "`.txt` is in neither list"
    );
}

#[test]
fn both_class_lists_are_configurable() {
    let options = DiscoverOptions {
        source_extensions: vec!["yaml".to_owned()],
        data_extensions: Vec::new(),
        ..DiscoverOptions::default()
    };
    let project = common::open_with("nested-namespaces", &options);
    assert_eq!(common::relative_paths(&project), ["net/edge.yaml"]);
    assert_eq!(project.files()[0].class, FileClass::Source);
}

#[test]
fn a_single_file_is_a_project_of_one_file() {
    let project = common::open("nested-namespaces/root.yfy");
    assert_eq!(common::relative_paths(&project), ["root.yfy"]);
    let root = project.scopes().root().expect("a one-file project still has a root scope");
    assert_eq!(project.files()[0].scope, root);
    assert_eq!(project.scopes().get(root).map(|s| s.kind), Some(ScopeKind::Root));
}

#[test]
fn a_header_supplies_its_directory_scope() {
    let project = common::open_clean("nested-namespaces");
    let billing = common::scope_by(&project, "nested-namespaces/billing");
    let scope = project.scopes().get(billing).expect("billing scope");
    assert_eq!(scope.namespace.as_deref(), Some("acme::billing"));
    assert_eq!(scope.visibility, Visibility::Public);
    assert_eq!(scope.mutability, Mutability::Immutable, "unstated axes inherit");
    assert_eq!(scope.declared_by.len(), 2, "both files declare this one scope");
}

#[test]
fn several_files_may_contribute_to_one_namespace() {
    let project = common::open_clean("nested-namespaces");
    let billing = common::scope_by(&project, "nested-namespaces/billing");
    let files: Vec<_> =
        project.files().iter().filter(|f| f.scope == billing).map(|f| &f.relative).collect();
    assert_eq!(files.len(), 2, "repetition of a namespace is the ordinary arrangement");
    assert_eq!(common::count(project.diagnostics(), Code::DuplicateNamespace), 0);
}

#[test]
fn a_file_without_a_header_inherits_its_directory_scope() {
    let project = common::open_clean("inherited-header");
    let child = project.files().iter().find(|f| f.relative.ends_with("child.yfy")).expect("child");
    assert!(child.header.is_none(), "the file states nothing");

    let scope = project.scopes().get(child.scope).expect("sub scope");
    assert!(scope.declared.visibility.is_none(), "nothing declared here");
    assert_eq!(scope.visibility, Visibility::Public, "inherited from the root scope");
    assert_eq!(scope.mutability, Mutability::Immutable, "inherited from the root scope");
}

#[test]
fn the_root_scope_states_both_axes() {
    let project = common::open_clean("inherited-header");
    let root = project.scopes().root().expect("root");
    let outside = project.scopes().get(root).expect("root scope");
    assert_eq!(outside.kind, ScopeKind::Root);
    assert!(outside.parent.is_none());
    assert_eq!(outside.path, vec![root], "the root's path is itself");
}

#[test]
fn an_undeclared_root_defaults_to_private_and_immutable() {
    let project = common::open("bad-axis");
    let root = project.scopes().root().expect("root");
    let scope = project.scopes().get(root).expect("root scope");
    // Both axes are opt-in. A scope that says nothing grants nothing, so
    // reaching into it and writing into it both have to be asked for.
    assert_eq!(scope.visibility, Visibility::Private);
    assert_eq!(scope.mutability, Mutability::Immutable);
}

#[test]
fn one_namespace_naming_two_directories_is_e0230() {
    let project = common::open("duplicate-namespace");
    let rendered = project.diagnostics().render(project.sources());
    assert_eq!(common::count(project.diagnostics(), Code::DuplicateNamespace), 1, "{rendered}");
    assert!(rendered.contains("first claimed here"), "{rendered}");
}

#[test]
fn disagreeing_headers_in_one_directory_are_e0230_but_agreeing_ones_are_not() {
    let project = common::open("conflicting-scope");
    let rendered = project.diagnostics().render(project.sources());
    assert_eq!(
        common::count(project.diagnostics(), Code::DuplicateNamespace),
        1,
        "three files, one namespace, one disagreement:\n{rendered}"
    );
    assert!(rendered.contains("visibility"), "{rendered}");
    assert!(rendered.contains("first declared here"), "{rendered}");

    let scope = common::scope_by(&project, "conflicting-scope/pkg");
    assert_eq!(
        project.scopes().get(scope).map(|s| s.visibility),
        Some(Visibility::Public),
        "the first declaration in discovery order wins"
    );
}

#[test]
fn bad_header_values_are_e0231_and_accumulate() {
    let project = common::open("bad-axis");
    let rendered = project.diagnostics().render(project.sources());
    assert_eq!(
        common::count(project.diagnostics(), Code::BadHeaderValue),
        4,
        "version, namespace, visibility and mutability are each wrong:\n{rendered}"
    );
    for expected in ["pubic", "read-only", "private, public", "immutable, mutable"] {
        assert!(rendered.contains(expected), "`{expected}` missing from:\n{rendered}");
    }
    let scope = common::scope_by(&project, "bad-axis");
    let scope = project.scopes().get(scope).expect("root scope");
    assert!(scope.namespace.is_none(), "a rejected value declares nothing");
    assert!(scope.declared.visibility.is_none());
}

#[test]
fn a_root_that_does_not_exist_is_a_diagnostic_not_a_panic() {
    let project = common::open("no-such-project");
    assert!(project.files().is_empty());
    assert_eq!(common::count(project.diagnostics(), Code::IoError), 1);
}

#[cfg(unix)]
#[test]
fn a_file_reached_twice_through_a_symlink_is_registered_once() {
    let tree = common::scratch::Tree::new("symlink-file");
    tree.write("zzz/real.yfy", "--- !node &n\nport: 1\n");
    tree.link("zzz/real.yfy", "aaa.yfy");

    let project = yfi_core::discover(tree.path(), &DiscoverOptions::default());
    assert_eq!(
        common::relative_paths(&project),
        ["aaa.yfy"],
        "one real file, and the lexicographically first path names it"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_cycle_terminates_and_ranks_by_relative_path() {
    let tree = common::scratch::Tree::new("symlink-cycle");
    tree.write("pkg/one.yfy", "--- !node &a\nport: 1\n");
    tree.link("", "pkg/loop");

    let project = yfi_core::discover(tree.path(), &DiscoverOptions::default());
    assert_eq!(common::relative_paths(&project), ["pkg/one.yfy"]);
}

#[cfg(unix)]
#[test]
fn rank_follows_the_relative_path_not_where_a_link_points() {
    let tree = common::scratch::Tree::new("symlink-order");
    tree.write("targets/zzz.yfy", "--- !node &z\nport: 1\n");
    tree.write("targets/aaa.yfy", "--- !node &a\nport: 2\n");
    tree.link("targets/zzz.yfy", "links/aaa.yfy");
    tree.link("targets/aaa.yfy", "links/zzz.yfy");

    let project = yfi_core::discover(tree.path(), &DiscoverOptions::default());
    assert_eq!(
        common::relative_paths(&project),
        ["links/aaa.yfy", "links/zzz.yfy"],
        "ranking by canonicalized path would order these by their targets instead"
    );
}
