// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 4 — the link pass, over the project corpus.
//!
//! Five codes are owed here and each has a project fixture that fires it:
//! `E0230` (duplicate definition), `E0213` (unresolved `!ref`), `E0211`
//! (illegal source), `E0214` (conflicting extended references) and `W0303`
//! (inert contribution). The rest of the file is about the shape of the graph
//! pass 5 inherits, because a graph built the plausible wrong way passes every
//! diagnostic test in this file and then hallucinates a cycle on every extended
//! reference.

mod common;

use std::path::Path;

use yamlfy_core::intern::{intern, Interned};
use yamlfy_core::link::{
    link, link_with, source_order, Direction, EdgeKind, Linked, RefRole, Stratum,
};
use yamlfy_core::Project;
use yamlfy_syntax::{Code, FileId, NodeId, Severity, SeverityMap};

/// A project taken all the way through pass 4.
struct Linked3 {
    project: Project,
    interned: Interned,
    linked: Linked,
}

impl Linked3 {
    fn rendered(&self) -> String {
        self.linked.diagnostics().render(self.project.sources())
    }

    fn count(&self, code: Code) -> usize {
        common::count(self.linked.diagnostics(), code)
    }

    fn file(&self, relative: &str) -> FileId {
        self.project
            .files()
            .iter()
            .find(|file| file.relative == Path::new(relative))
            .unwrap_or_else(|| panic!("no file `{relative}`"))
            .id
    }
}

/// Discover, intern and link a project fixture, asserting the passes before
/// this one found nothing — so every diagnostic in a test below is pass 4's.
fn open(name: &str) -> Linked3 {
    let project = common::open_clean(name);
    let interned = intern(&project);
    let linked = link(&project, &interned);
    Linked3 { project, interned, linked }
}

/// The same for a single-file fixture, which is a project of one file.
fn open_at(relative: &str) -> Linked3 {
    let project = common::open_at(relative);
    let interned = intern(&project);
    let linked = link(&project, &interned);
    Linked3 { project, interned, linked }
}

// ---------------------------------------------------------------- E0230

#[test]
fn e0230_fires_when_two_files_define_one_canonical_path() {
    let fixture = open("link-duplicate-definition");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::DuplicateNamespace), 1, "{rendered}");
    assert!(
        rendered.contains("b.yfy:6:11: `dup/Defaults` is already the canonical path"),
        "the primary span is the later definition's `&name`:\n{rendered}"
    );
    assert!(
        rendered.contains("note: ") && rendered.contains("a.yfy:6:11 first defined here"),
        "and the note is the earlier one, in the file that wrote it:\n{rendered}"
    );
}

#[test]
fn a_canonical_path_is_namespace_qualified_so_two_namespaces_do_not_collide() {
    // `other/c.yfy` defines `&Defaults` too. It is a different namespace and
    // therefore a different path, which is the whole reason an ordinary local
    // mixin named `&defaults` is safe to write in every file of a project.
    let fixture = open("link-duplicate-definition");
    assert!(fixture.linked.definition("dup/Defaults").is_some());
    assert!(fixture.linked.definition("dup::other/Defaults").is_some());
    assert_eq!(fixture.count(Code::DuplicateNamespace), 1, "{}", fixture.rendered());
}

#[test]
fn an_anchored_scalar_is_a_value_and_carries_no_canonical_path() {
    // `&limit 30` is written in both files. A scalar is a value, not a type,
    // so it is not addressable and the two do not collide.
    let fixture = open("link-duplicate-definition");
    let paths: Vec<&str> =
        fixture.linked.definitions().iter().map(|held| &*held.path).collect();
    assert!(paths.contains(&"dup/Local"), "an anchored mapping is addressable: {paths:?}");
    assert!(!paths.iter().any(|path| path.ends_with("/limit")), "but a scalar is not: {paths:?}");
}

// ---------------------------------------------------------------- E0211

#[test]
fn e0211_rejects_every_operand_shape_d1_6_excludes() {
    let fixture = open("link-illegal-source");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::IllegalMergeSource), 4, "{rendered}");
    for (at, found) in [
        ("app.yfy:17:7", "this is an alias to a scalar"),
        ("app.yfy:19:8", "this is a sequence, and a merge sequence must be flat"),
        ("app.yfy:21:7", "this is a path to a sequence"),
        ("app.yfy:23:12", "this is a scalar"),
    ] {
        assert!(
            rendered.lines().any(|line| line.contains(at) && line.contains(found)),
            "expected `{at}` to report `{found}`:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("app.yfy:23:12: an `extends` operand must be"),
        "an illegal `extends` operand is reported, never reinterpreted as a field:\n{rendered}"
    );
}

#[test]
fn the_merge_corpus_fixtures_that_owed_a_pass_raise_it_here() {
    for relative in
        ["fixtures/merge/merge-non-mapping.yml", "fixtures/merge/nested-sequence-source.yml"]
    {
        let fixture = open_at(relative);
        assert_eq!(
            fixture.count(Code::IllegalMergeSource),
            1,
            "{relative}:\n{}",
            fixture.rendered()
        );
    }
    let legal = open_at("fixtures/merge/inline-mapping-source.yml");
    assert!(legal.linked.diagnostics().is_empty(), "an inline mapping is a legal source");
}

#[test]
fn a_legal_clause_keeps_its_operands_in_written_order() {
    let fixture = open("link-illegal-source");
    let app = fixture.file("app.yfy");
    let owner = common::entry_at(&fixture.project, app, 1, &["legal"]);
    let clauses: Vec<_> =
        fixture.linked.clauses().iter().filter(|held| held.owner == owner).collect();
    assert_eq!(clauses.len(), 2, "one `<<` and one `extends` on the same mapping");
    assert_eq!(clauses[0].operands.len(), 2, "`<<: [*Base, {{inline: 1}}]` is two operands");
    assert_eq!(clauses[1].operands.len(), 1);
}

// ---------------------------------------------------------------- E0214

#[test]
fn e0214_fires_on_contradictory_contributions_and_not_on_identical_ones() {
    // `one.yfy` and `three.yfy` both contribute `reagent: sunroot`, which is
    // idempotent and legal. `two.yfy` contributes `moonleaf`, and nothing but
    // a filename ranks it against the other two.
    let fixture = open("link-conflicting-extends");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::ConflictingExtension), 1, "{rendered}");
    assert!(
        rendered.contains(
            "two.yfy:9:1: two extended references contribute `reagent` to `guild/BasePotion`"
        ),
        "the primary span is the contradicting key:\n{rendered}"
    );
    assert!(
        rendered.contains("one.yfy:9:1 the other contribution is here")
            && rendered.contains("base.yfy:6:11 both extend this definition"),
        "with the first contribution and the base in notes:\n{rendered}"
    );
}

// ---------------------------------------------------------------- W0303

#[test]
fn w0303_reports_the_inert_half_of_a_contribution() {
    // The apprentice's `label:` is contributed to a base that already declares
    // `label: !!str`, so it loses and does nothing. `reagent:` is new and takes
    // effect. One warning, on the inert key only.
    let fixture = open("link-inert-contribution");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::InertContribution), 1, "{rendered}");
    assert!(
        rendered.contains("apprentice.yfy:9:1: `label` is contributed to `guild::stock/BasePotion`"),
        "the primary span is the contributed key:\n{rendered}"
    );
    assert!(
        rendered.contains("guild.yfy:9:1 the base defines it here")
            && rendered.contains("apprentice.yfy:8:15 contributed through this extended reference"),
        "with the base's own declaration and the `!ref` that carried it:\n{rendered}"
    );
    let contribution = &fixture.linked.contributions()[0];
    let inert: Vec<bool> = contribution.keys.iter().map(|key| key.inert).collect();
    assert_eq!(inert, [true, false], "`label` is inert, `reagent` is not");
}

#[test]
fn w0303_is_a_warning_and_a_project_may_deny_it() {
    let project = common::open_clean("link-inert-contribution");
    let interned = intern(&project);
    assert!(
        !link(&project, &interned).diagnostics().has_errors(),
        "a partly inert contribution is legitimate, so the default is a warning"
    );
    let mut severities = SeverityMap::new();
    severities.insert(Code::InertContribution, Severity::Error);
    let denied = link_with(&project, &interned, severities);
    assert_eq!(denied.diagnostics().error_count(), 1, "`--deny W0303` is available");
}

// ---------------------------------------------------------------- the graph

#[test]
fn every_node_of_the_graph_carries_both_of_its_strata() {
    let fixture = open("link-graph-shapes");
    let graph = fixture.linked.graph();
    assert!(!graph.vertices().is_empty());
    for vertex in graph.vertices() {
        for stratum in [Stratum::Own, Stratum::Resolved] {
            let id = graph
                .vertex_of(vertex.file, vertex.node, stratum)
                .unwrap_or_else(|| panic!("{vertex:?} is missing its {stratum:?} vertex"));
            assert_eq!(graph.vertex(id).expect("vertex").stratum, stratum);
        }
    }
    assert_eq!(graph.vertices().len() % 2, 0, "vertices come in pairs");
}

#[test]
fn own_vertices_are_sinks() {
    // This is the property that makes SCC over the graph exact: a reverse edge
    // ends at an `own` vertex, and a vertex with no outgoing edges cannot lie
    // on a cycle.
    let fixture = open("link-graph-shapes");
    let graph = fixture.linked.graph();
    for (index, vertex) in graph.vertices().iter().enumerate() {
        if vertex.stratum != Stratum::Own {
            continue;
        }
        let id = graph.vertex_of(vertex.file, vertex.node, Stratum::Own).expect("own");
        assert_eq!(id.index(), index);
        assert!(graph.out_edges(id).is_empty(), "{vertex:?} has an outgoing edge");
    }
    for edge in graph.edges() {
        let ends_at = graph.vertex(edge.to).expect("target").stratum;
        let reverse = edge.direction == Direction::Reverse;
        assert_eq!(reverse, ends_at == Stratum::Own, "{edge:?} points the wrong way");
    }
}

#[test]
fn an_extended_reference_facing_an_inclusion_produces_the_three_edges_pass_five_needs() {
    // `A extends: !ref B` with `B << A`. The cycle closes through the two
    // *forward* edges; the reverse edge cannot participate, so `E0212` must
    // name the forward pair or it blames the `!ref` the author is already sure
    // is at fault.
    let fixture = open("link-graph-shapes");
    let graph = fixture.linked.graph();
    let file = fixture.file("cycle.yfy");
    let a = common::declaration(&fixture.project, file, "A");
    let b = common::declaration(&fixture.project, file, "B");
    let at = |node: NodeId, stratum| graph.vertex_of(file, node, stratum).expect("vertex");

    let found: Vec<(EdgeKind, Direction)> = graph
        .edges()
        .iter()
        .filter(|edge| {
            let ends = [edge.from, edge.to];
            ends.iter().all(|end| {
                let vertex = graph.vertex(*end).expect("vertex");
                vertex.file == file && (vertex.node == a || vertex.node == b)
            })
        })
        .map(|edge| (edge.kind, edge.direction))
        .collect();
    assert_eq!(found.len(), 3, "{found:?}");

    let edge = |from, to| {
        graph
            .edges()
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("no edge"))
    };
    let forward = edge(at(a, Stratum::Resolved), at(b, Stratum::Resolved));
    assert_eq!((forward.kind, forward.direction), (EdgeKind::ExtendedReference, Direction::Forward));
    let reverse = edge(at(b, Stratum::Resolved), at(a, Stratum::Own));
    assert_eq!((reverse.kind, reverse.direction), (EdgeKind::ExtendedReference, Direction::Reverse));
    let inclusion = edge(at(b, Stratum::Resolved), at(a, Stratum::Resolved));
    assert_eq!((inclusion.kind, inclusion.direction), (EdgeKind::Inclusion, Direction::Forward));
}

#[test]
fn every_edge_records_the_operator_that_wrote_it() {
    let fixture = open("link-graph-shapes");
    let graph = fixture.linked.graph();
    let kinds = |wanted: EdgeKind| graph.edges().iter().filter(|e| e.kind == wanted).count();
    assert_eq!(kinds(EdgeKind::Inclusion), 2, "`<<: *A` and the cross-file `<<: ../cycle/B`");
    assert_eq!(kinds(EdgeKind::Extension), 1, "`extends: *Base`");
    assert_eq!(kinds(EdgeKind::ExtendedReference), 2, "one forward, one reverse");
    assert_eq!(
        [
            EdgeKind::Inclusion.as_str(),
            EdgeKind::Extension.as_str(),
            EdgeKind::ExtendedReference.as_str(),
            EdgeKind::Capability.as_str(),
        ],
        ["<<", "extends", "extends !ref", "!ref"],
        "the wording `E0212`'s notes use"
    );
}

#[test]
fn a_reference_carries_the_role_of_the_operator_it_is_an_operand_of() {
    // A path has no single meaning: under `<<` it is cross-file inclusion,
    // under `extends` it is extension, and in a value position it is a data
    // edge. `!ref` is orthogonal to all three — it is what the reference
    // *intends*, not where it sits.
    let fixture = open("link-graph-shapes");
    let roles = |file: FileId| -> Vec<(RefRole, bool)> {
        fixture
            .linked
            .references()
            .iter()
            .filter(|held| held.file == file)
            .map(|held| (held.role, held.capability))
            .collect()
    };
    assert_eq!(roles(fixture.file("cycle.yfy")), [(RefRole::Extension, true)]);
    assert_eq!(
        roles(fixture.file("mix/mix.yfy")),
        [(RefRole::Inclusion, false)],
        "`<<: ../cycle/B` is a plain path: cross-file inclusion asks for nothing"
    );

    let data = open("link-unresolved-ref");
    let app = data.file("app.yfy");
    assert!(
        data.linked
            .references()
            .iter()
            .filter(|held| held.file == app)
            .all(|held| held.role == RefRole::Data && held.capability),
        "every `!ref` there is written in a value position"
    );
}

#[test]
fn cyclic_data_stays_legal_and_contributes_no_inheritance_edge() {
    // `ring` merges `base` and points at itself. The merge graph is a DAG and
    // the data graph is cyclic; only the first is an inheritance edge.
    let fixture = open_at("fixtures/cycles/alias-cycle-with-merge-dag.yml");
    assert!(fixture.linked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert_eq!(fixture.linked.graph().edges().len(), 1, "the `next: *ring` edge is data");
    assert_eq!(fixture.linked.graph().edges()[0].kind, EdgeKind::Inclusion);
}

#[test]
fn linking_builds_the_graph_and_leaves_the_cycle_to_pass_five() {
    // `link-graph-shapes` contains a genuine inheritance cycle. Pass 4 records
    // it and says nothing: detecting it is `E0212`, and `E0212` is pass 5's.
    let fixture = open("link-graph-shapes");
    assert!(
        fixture.linked.diagnostics().is_empty(),
        "pass 4 reports no cycle:\n{}",
        fixture.rendered()
    );
}

// ---------------------------------------------------------------- ordering

#[test]
fn source_order_is_textual_where_the_arena_is_post_order() {
    // The arena is post-order, so a collection's index exceeds every child's.
    // Anything user-visible — which member of a cycle is named first — must be
    // ordered by where it is *written* instead.
    let fixture = open("link-graph-shapes");
    let file = fixture.file("cycle.yfy");
    let root = fixture.project.file(file).expect("file").ast.documents()[1].root;
    let a = common::declaration(&fixture.project, file, "A");
    let order = |node| {
        source_order(&fixture.project, &fixture.interned, file, node).expect("an order")
    };
    assert!(a.index() < root.index(), "the child has the lower arena index");
    assert!(order(root) < order(a), "but the document root is written first");
}
