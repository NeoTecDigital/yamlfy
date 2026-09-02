// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 6 — the image.
//!
//! What is asserted here is the *shape* pass 6 produces, and each assertion is
//! one of the design's load-bearing claims read back out of it: `!node` is
//! emitted and nothing else is, the `is_a` axis survives flattening, inbound
//! traversal is indexed rather than scanned, privacy crosses one inheritance
//! step and not a chain, a data cycle is a legal shape, and a path means the
//! same thing to a query as it does to the compiler.

mod common;

use std::collections::HashSet;

use common::pipeline::{open, Compiled};
use yfi_core::emit::emit;
use yfi_core::image::{EdgeKind, Image, ModelId, Named};
use yfi_core::ScopeId;

/// A project taken all the way through pass 6.
fn image<'a>(fixture: &'a Compiled) -> Image<'a> {
    emit(&fixture.project, &fixture.interned, &fixture.linked, &fixture.checked)
}

/// The node an anchor names, whatever its kind.
fn by_name<'a>(image: &'a Image<'a>, name: &str) -> ModelId {
    image
        .nodes()
        .find(|held| held.name() == Some(name))
        .unwrap_or_else(|| panic!("no node called `{name}`"))
        .id()
}

/// The anchor names of a run of ids, in order.
fn names<'a>(image: &'a Image<'a>, ids: &[ModelId]) -> Vec<String> {
    ids.iter()
        .filter_map(|id| image.model(*id))
        .filter_map(|held| held.name())
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_fixture_compiles_clean() {
    let fixture = open("emit-graph");
    assert!(fixture.linked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
}

#[test]
fn a_model_is_emitted_for_node_and_for_nothing_else() {
    // D7.1. `!type` is abstract and never emitted; an **untagged** node is
    // abstract too, which is what stops the ordinary act of factoring shared
    // keys into a named mapping from polluting the graph.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let emitted: Vec<&str> = image.models().filter_map(|held| held.name()).collect();
    assert_eq!(emitted, ["Leaf", "Alpha", "Gamma", "Beta"], "every `!node`, in source order");
    for abstracted in ["Base", "Mid", "Mixin"] {
        let held = image.model(by_name(&image, abstracted)).expect("a node");
        assert!(!held.is_concrete(), "`{abstracted}` is abstract and is not output");
    }
    assert_eq!(image.len(), 4);
    assert!(!image.is_empty());
}

#[test]
fn an_untagged_mixin_project_emits_nothing_at_all() {
    // The corpus is the argument for abstract-by-default: this file is four
    // untagged anchored mappings that exist to be merged into each other. If
    // untagged defaulted to concrete it would emit four junk models.
    let fixture = common::pipeline::open_at("fixtures/cycles/merge-diamond.yml");
    let image = image(&fixture);
    assert_eq!(image.models().count(), 0, "a mixin is not a model");
    assert!(image.nodes().count() > 0, "and is still held, for its edges");
}

#[test]
fn a_base_yaml_file_emits_no_models_of_its_own() {
    // `!node` is not interpreted in base YAML (D6.6), so no node in a `.yaml`
    // is ever concrete. Nothing is added for that; it is D7.1's default
    // arriving at the right answer from the other direction.
    let fixture = open("imports-data");
    let image = image(&fixture);
    let data = fixture.file("services.yaml");
    assert!(image.nodes().any(|held| held.file() == data), "its nodes are held");
    assert!(!image.models().any(|held| held.file() == data), "and none is emitted");
}

#[test]
fn the_is_a_axis_survives_a_cross_file_diamond() {
    // `check-diamond` is four files: Leaf extends Left and Right, and both
    // extend Base. Flattening alone would leave the leaf with the right keys
    // and no way to answer what it is.
    let fixture = open("check-diamond");
    let image = image(&fixture);
    let leaf = by_name(&image, "Leaf");
    assert_eq!(names(&image, image.ancestors(leaf)), ["Left", "Right", "Base"]);
    assert_eq!(
        image.ancestors(leaf).iter().collect::<HashSet<_>>().len(),
        3,
        "Base is reached twice and appears once"
    );
    let base = by_name(&image, "Base");
    assert!(image.ancestors(base).is_empty());
}

#[test]
fn an_inclusion_is_composition_and_contributes_no_ancestry() {
    // `A << B` says A *has* a B. It creates no is-a relationship, so no query
    // over the `is_a` axis will ever return A for B (D4.1) — even though the
    // mixin's keys are right there in the resolved view.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let leaf = image.model(by_name(&image, "Leaf")).expect("Leaf");
    assert_eq!(names(&image, leaf.ancestors()), ["Mid", "Base"], "Mixin is not among them");
    let kinds: Vec<&str> = leaf.out().iter().map(|edge| edge.kind.as_str()).collect();
    assert_eq!(kinds, ["extends", "<<"], "the inclusion is an edge, just not an ancestry one");
    assert!(leaf.view().expect("a view").holds(fixture.symbol("shared")), "and it did compose");
}

#[test]
fn every_edge_is_indexed_forward_and_in_reverse() {
    // Inbound traversal has to be O(degree). The reverse index is materialised
    // rather than derived, so `inc` is a slice rather than a scan of every
    // edge in the project.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let forward: usize = image.nodes().map(|held| held.out().len()).sum();
    let reverse: usize = image.nodes().map(|held| held.inc().len()).sum();
    assert_eq!(forward, reverse, "one entry per edge in each direction");
    let base = image.model(by_name(&image, "Base")).expect("Base");
    assert!(base.out().is_empty(), "Base extends nothing");
    let arriving: Vec<(&str, Option<&str>)> = base
        .inc()
        .iter()
        .map(|edge| (edge.kind.as_str(), image.model(edge.from).and_then(|m| m.name())))
        .collect();
    assert_eq!(arriving, [("extends", Some("Mid"))], "found without scanning the graph");
}

#[test]
fn a_private_member_survives_one_extends_step_and_is_gone_at_two() {
    // Privacy crosses **one** inheritance step and then stops at the first
    // scope boundary. `secret` is Base's, becomes Mid's own across one step,
    // and does not reach Leaf — which sits in another scope and cannot read it
    // where Mid holds it. Without the bound it would be republished by
    // instalments, re-gated one directory further out at every step.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let secret = fixture.symbol("secret");
    let mid = image.model(by_name(&image, "Mid")).expect("Mid");
    let held = mid.field(secret).expect("`secret` crossed one step");
    assert_eq!(held.gate().visibility, yfi_core::Visibility::Private, "not laundered");
    assert_eq!(held.gate().scope, mid.scope(), "re-gated onto the inheritor");
    let leaf = image.model(by_name(&image, "Leaf")).expect("Leaf");
    assert!(leaf.field(secret).is_none(), "one step, not a chain");
    assert!(leaf.view().expect("a view").holds(fixture.symbol("kind")), "a public one descends");
}

#[test]
fn an_unreadable_member_is_absent_by_shape_rather_than_present_as_a_hole() {
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let mid = image.model(by_name(&image, "Mid")).expect("Mid");
    let outside = common::scope_by(&fixture.project, "emit-graph/leaf");
    let surface: Vec<&str> = mid.fields_readable_from(outside).map(|held| held.name()).collect();
    assert_eq!(surface, ["tier", "kind", "name"], "the public surface, and nothing under it");
    assert!(mid.fields().count() > surface.len(), "`secret` is resolved, just not readable");
    assert!(mid.field(fixture.symbol("secret")).expect("held").is_readable_from(mid.scope()));
}

#[test]
fn a_data_cycle_is_a_legal_shape_and_is_traversed_once() {
    // Only cycles through *inheritance* fail. `Alpha.peer` names Beta and
    // `Beta.peer` names Alpha, which is the point of the system — so a
    // traversal carries a visited set and terminates rather than recursing.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let start = by_name(&image, "Alpha");
    let mut seen: HashSet<ModelId> = HashSet::from([start]);
    let mut frontier = vec![start];
    let mut steps = 0;
    while let Some(held) = frontier.pop() {
        steps += 1;
        assert!(steps < 16, "a data cycle must terminate, not spin");
        for edge in image.out(held).iter().filter(|edge| edge.kind == EdgeKind::Data) {
            if seen.insert(edge.to) {
                frontier.push(edge.to);
            }
        }
    }
    assert_eq!(names(&image, &seen.into_iter().collect::<Vec<_>>()).len(), 2);
    assert_eq!(steps, 2, "each node visited exactly once");
    let beta = image.model(by_name(&image, "Beta")).expect("Beta");
    assert_eq!(beta.inc().len(), 1, "and the cycle is indexed in reverse too");
}

#[test]
fn the_name_index_resolves_every_form_of_the_path_syntax() {
    // The grammar and the walk are pass 4's, so a path cannot mean one thing to
    // the compiler and another to a query (D4.12).
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let origin = fixture.file("net/alpha.yfy");
    let named = |text: &str| match image.resolve(origin, text) {
        Some(Named::Model(id)) => image.model(id).and_then(|held| held.name()).map(str::to_owned),
        Some(Named::Field(id, name)) => Some(format!(
            "{}.{}",
            image.model(id).and_then(|held| held.name()).unwrap_or_default(),
            fixture.interned.symbols().resolve(name).unwrap_or_default()
        )),
        None => None,
    };
    assert_eq!(named("Alpha").as_deref(), Some("Alpha"), "a bare name is this file");
    assert_eq!(named("./Beta").as_deref(), Some("Beta"), "`./` is this directory");
    assert_eq!(named("beta/Beta").as_deref(), Some("Beta"), "a segment names a peer file");
    assert_eq!(named("../base/Base").as_deref(), Some("Base"), "`..` walks up a scope");
    assert_eq!(named("Alpha.peer").as_deref(), Some("Alpha.peer"), "`.` addresses a member");
    assert_eq!(named("Beta"), None, "a bare name does not silently reach a sibling file");
    assert_eq!(named("../../../nope"), None, "and `..` past the root names nothing");
    assert_eq!(named("http://host/thing"), None, "what is not a path stays data");
}

#[test]
fn a_member_path_chains_and_the_last_step_is_the_field() {
    // `Service.tls.port` is the `port` of the `tls` of `Service`. Every step but
    // the last addresses a node; the last names a member of it, which is what
    // makes a query answerable without inventing a node for a scalar.
    let fixture = open("link-ref-binding");
    let image = image(&fixture);
    let origin = fixture.file("core/service.yfy");
    let Some(Named::Field(holder, name)) = image.resolve(origin, "Service.tls.enabled") else {
        panic!("`Service.tls.enabled` names a member");
    };
    assert_eq!(fixture.interned.symbols().resolve(name), Some("enabled"));
    let tls = image.model(holder).expect("the `tls` mapping");
    assert!(tls.field(name).is_some(), "and the holder is the node that writes it");
    assert_eq!(image.resolve(origin, "Service.tls.absent"), None, "a member it does not hold");
}

#[test]
fn a_model_carries_its_scope_path_so_access_needs_no_tree_walk() {
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let leaf = image.model(by_name(&image, "Leaf")).expect("Leaf");
    let root = fixture.project.scopes().root().expect("a root");
    assert_eq!(leaf.scope_path().first(), Some(&root), "root first");
    assert_eq!(leaf.scope_path().last(), Some(&leaf.scope()), "the holder last");
    assert_eq!(leaf.scope_path(), fixture.project.scopes().path(leaf.scope()));
    assert!(leaf.is_visible_from(root), "a public scope is visible from the root");
    assert!(!image.is_visible_from(leaf.id(), ScopeId(u32::MAX)), "and an unknown scope sees none");
}

#[test]
fn emission_is_refused_when_pass_five_found_an_inheritance_cycle() {
    // The views pass 5 composed over a graph made acyclic by dropping back
    // edges are a **recovery**, not a meaning. Emitting them would put a value
    // in the output that no source text means.
    let fixture = common::pipeline::through(common::open("check-cycle"));
    assert!(fixture.checked.is_cyclic(), "the fixture is the cycle");
    let image = image(&fixture);
    assert!(image.is_refused());
    assert!(image.is_empty());
    assert_eq!(image.nodes().count(), 0, "nothing at all, not a partial graph");
}

#[test]
fn ids_are_assigned_in_the_order_the_project_is_written() {
    // `NodeOrder`'s node component is the arena index, which is post-order and
    // not textual order. Anything user-visible is ordered by where it is
    // written instead.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let orders: Vec<_> = image.nodes().map(|held| held.order()).collect();
    assert!(orders.windows(2).all(|pair| pair[0] <= pair[1]), "{orders:?}");
    let ids: Vec<u32> = image.models().map(|held| held.id().0).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn an_alias_is_a_data_edge_and_so_is_a_path() {
    // The two reaches differ — an alias is document-local (D2.6) and a path
    // walks the project — and the edge they write is the same one. Recording
    // only the path form would leave every YAML-native reference out of the
    // graph the engine exists to build.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let gamma = image.model(by_name(&image, "Gamma")).expect("Gamma");
    let out: Vec<(&str, Option<&str>, Option<&str>)> = gamma
        .out()
        .iter()
        .map(|edge| {
            (
                edge.kind.as_str(),
                image.model(edge.to).and_then(|held| held.name()),
                edge.key.and_then(|key| fixture.interned.symbols().resolve(key)),
            )
        })
        .collect();
    assert_eq!(out, [("data", Some("Alpha"), Some("mate"))], "the member carries the edge");
    let alpha = image.model(by_name(&image, "Alpha")).expect("Alpha");
    assert_eq!(alpha.inc().len(), 2, "one alias edge and one path edge arrive");
}

#[test]
fn a_member_finds_the_node_its_value_names() {
    let fixture = open("emit-graph");
    let image = image(&fixture);
    let alpha = image.model(by_name(&image, "Alpha")).expect("Alpha");
    let peer = alpha.field(fixture.symbol("peer")).expect("`peer`");
    assert_eq!(peer.target().and_then(|held| held.name()), Some("Beta"));
    assert_eq!(peer.text(), Some("./Beta"), "and its value as written");
    let label = alpha.field(fixture.symbol("label")).expect("`label`");
    assert_eq!(label.target().map(|held| held.id()), None, "an unanchored scalar is a string");
}

#[test]
fn a_ref_marks_the_edge_it_is_written_on_rather_than_becoming_one() {
    // `!ref` is legal wherever a path is and qualifies the operation rather
    // than replacing it: `service: !ref ../core/Service` is a data edge that
    // also declares this context intends to modify the target (D4.12).
    let fixture = open("link-ref-binding");
    let image = image(&fixture);
    let app = fixture.file("app/app.yfy");
    let holder = image
        .nodes()
        .find(|held| held.file() == app && held.out().iter().any(|edge| edge.capability))
        .expect("the binding is written somewhere");
    let declared: Vec<(&str, bool)> = holder
        .out()
        .iter()
        .filter(|edge| edge.capability)
        .map(|edge| (edge.kind.as_str(), edge.capability))
        .collect();
    assert_eq!(declared, [("data", true)], "a data edge, and a declaration on it");
}

#[test]
fn an_extended_reference_is_a_kind_and_still_carries_the_declaration() {
    // The third operation has a blast radius of its own, so it is a kind. It is
    // still the `!ref` that declares the intent, and the flag says so on the
    // same edge — otherwise "which lines demand write access" would have to be
    // answered by matching a kind *and* a flag, in two places.
    let fixture = open("check-private-inherit");
    let image = image(&fixture);
    let audit =
        image.nodes().find(|held| held.name() == Some("Audit")).expect("the extended reference");
    let written: Vec<(&str, bool)> =
        audit.out().iter().map(|edge| (edge.kind.as_str(), edge.capability)).collect();
    assert_eq!(written, [("extends !ref", true)]);
    let api = image.nodes().find(|held| held.name() == Some("Api")).expect("the extension");
    let plain: Vec<(&str, bool)> =
        api.out().iter().map(|edge| (edge.kind.as_str(), edge.capability)).collect();
    assert_eq!(plain, [("extends", false)], "a plain extension declares nothing");
}

#[test]
fn a_header_document_is_not_a_node_of_the_graph() {
    // A header declares the **file's** axes and its imports (D6.4, D6.7). It is
    // a mapping, so it resolves like any other, and it is not a thing the graph
    // holds.
    let fixture = open("emit-graph");
    let image = image(&fixture);
    for file in fixture.project.files() {
        let Some(header) = file.header.as_ref() else { continue };
        let document = fixture.interned.document_of(file.id, header.node);
        assert!(
            !image.nodes().any(|held| held.file() == file.id
                && fixture.interned.document_of(file.id, held.node()) == document),
            "the header of {} is emitted",
            file.relative.display()
        );
    }
}
