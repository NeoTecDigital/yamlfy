// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `!edge` — D4.13.
//!
//! Every assertion here is one of the design's load-bearing claims read back
//! out of a compiled project: an edge is a **node** and is emitted as one, its
//! `connections` are **incidence records** rather than a second notion of edge,
//! a relation is **n-ary** and is never decomposed into pairs, an edge may be
//! an endpoint of another edge, extending an edge **replaces** its endpoints
//! whole rather than appending to them, a handle names a **position** that
//! filtering never renumbers, and the two members the language owns on an edge
//! are the language's and not the family's.

mod common;

use common::edge::{by_name, endpoints, image, symbol};
use common::pipeline::open;
use yfi_core::edge::{CONNECTIONS, DEFINITION};
use yfi_core::image::{EdgeKind, ModelId, ModelKind, Named};
use yfi_syntax::Code;

// ------------------------------------------------------------ an edge is a node

#[test]
fn an_edge_is_a_node_addressable_and_emitted_like_any_other() {
    // Constraint 1: one set of rules, not two. An edge has identity, a scope, a
    // span, a resolved view and a place in the name index, and it is *emitted*
    // — a relation nothing holds is not a relation, so `!edge` is concrete.
    let fixture = open("edge-binary");
    let image = image(&fixture);
    let calls = image.model(by_name(&image, "Calls")).expect("a node");

    assert_eq!(calls.kind(), ModelKind::Edge);
    assert!(calls.is_edge());
    assert!(calls.is_concrete(), "an edge is emitted");
    assert!(
        image.models().any(|held| held.id() == calls.id()),
        "and is in the compiled output beside every other model"
    );
    assert_eq!(calls.canonical(), Some("edges::binary/Calls"), "it has an identity");
    assert!(calls.view().is_some(), "and a resolved view like any node");
    assert_eq!(
        image.resolve(calls.file(), "Calls"),
        Some(Named::Model(calls.id())),
        "and the path syntax addresses it"
    );
}

#[test]
fn an_edge_carries_its_own_members_which_is_what_makes_middleware_possible() {
    // Consequence 4. Nothing was added to allow this: the members are members.
    let fixture = open("edge-binary");
    let image = image(&fixture);
    let calls = image.model(by_name(&image, "Calls")).expect("a node");
    let names: Vec<&str> = calls.fields().map(|held| held.name()).collect();
    assert!(names.contains(&"protocol"), "an edge holds ordinary members too: {names:?}");
    assert!(names.contains(&CONNECTIONS), "beside the one that makes it an edge");
}

// ----------------------------------------------------- one notion of "edge"

#[test]
fn connections_are_incidence_records_and_not_data_references() {
    // The reconciliation. `image::Edge` stays the *record* type and gains one
    // kind; the `!edge` node is a **vertex** that contributes a run of them.
    // A data edge says "this member names that node"; a connection says "this
    // edge relates that node", and a query for what relates a node must not
    // collect everything that happens to point at it.
    let fixture = open("edge-binary");
    let image = image(&fixture);
    let calls = image.model(by_name(&image, "Calls")).expect("a node");
    let service = by_name(&image, "Service");

    let kinds: Vec<EdgeKind> = calls.out().iter().map(|held| held.kind).collect();
    assert_eq!(kinds, [EdgeKind::Connection, EdgeKind::Connection], "{kinds:?}");
    assert!(
        !calls.out().iter().any(|held| held.kind == EdgeKind::Data),
        "the endpoints are not *also* recorded as data edges; one written \
         relationship is one record"
    );
    assert!(
        !calls.out().iter().any(|held| held.kind.is_ancestry()),
        "and a connection puts nothing on the `is_a` axis"
    );

    let incident: Vec<ModelId> =
        image.model(service).expect("a node").incident_edges().map(|held| held.id()).collect();
    assert_eq!(incident, [calls.id()], "the reverse index answers it in O(degree)");
}

#[test]
fn the_edge_index_and_the_edge_nodes_are_the_same_index() {
    // There are not two graphs. Every connection an edge node declares is a
    // record of the CSR index, and every `Connection` record of the index
    // leaves an edge node.
    let fixture = open("edge-nary");
    let image = image(&fixture);
    let declared: usize =
        fixture.checked.edges().items().iter().map(|held| held.connections.len()).sum();
    let recorded = image
        .nodes()
        .flat_map(|held| held.out().iter())
        .filter(|held| held.kind == EdgeKind::Connection)
        .count();
    assert_eq!(declared, recorded, "one record per declared endpoint, and no others");
    for held in image.nodes() {
        for record in held.out().iter().filter(|held| held.kind == EdgeKind::Connection) {
            assert!(
                image.model(record.from).expect("a node").is_edge(),
                "a connection leaves an `!edge` node and nothing else"
            );
        }
    }
}

#[test]
fn an_alias_standing_as_the_connections_value_is_dereferenced_like_an_item() {
    // The asymmetry that was: an alias was dereferenced for an **item** and not
    // for the member's own value, so `connections: *Pair` was the wrong shape
    // though `*Pair` named a perfectly good sequence. One rule now answers both
    // spellings, and two edges share one endpoint list without either of them
    // extending the other.
    let fixture = open("edge-shared-sequence");
    assert_eq!(fixture.count(Code::EdgeMemberShape), 0, "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "Aliased"), ["Alpha", "Beta"]);
    assert_eq!(
        endpoints(&image, "AlsoAliased"),
        ["Alpha", "Beta"],
        "and the second reader of the sequence gets the same answer"
    );
    let aliased = image.model(by_name(&image, "Aliased")).expect("a node");
    assert_eq!(
        aliased.connection(symbol(&fixture, "to")).and_then(|held| held.name()),
        Some("Beta"),
        "the positions a handle indexes are the aliased sequence's, in written order"
    );
}

// ---------------------------------------------------------------- arity

#[test]
fn a_three_way_edge_is_one_edge_and_never_three_binary_ones() {
    // Consequence 2. Nothing anywhere assumes two endpoints.
    let fixture = open("edge-nary");
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "Route"), ["Ingress", "Filter", "Backend"]);

    let ingress = image.model(by_name(&image, "Ingress")).expect("a node");
    assert!(
        !ingress.out().iter().any(|held| held.kind == EdgeKind::Connection),
        "an endpoint of a relation does not itself acquire the relation's edges"
    );
    let relates: Vec<String> =
        ingress.incident_edges().filter_map(|held| held.name()).map(str::to_owned).collect();
    assert_eq!(relates, ["Route"], "one relation, reached in one hop, not two pair-edges");
}

#[test]
fn an_edge_may_be_an_endpoint_of_another_edge() {
    // Mechanically it follows from "an edge is a node"; it is also intended,
    // because a relation over relations is how relations compose.
    let fixture = open("edge-nary");
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "Supersedes"), ["Route", "Backend"]);
    let route = image.model(by_name(&image, "Route")).expect("a node");
    assert!(route.is_edge(), "the endpoint is itself an edge");
    let over: Vec<String> =
        route.incident_edges().filter_map(|held| held.name()).map(str::to_owned).collect();
    assert_eq!(over, ["Supersedes"]);
}

// --------------------------------------------------------------- handles

#[test]
fn a_handle_names_a_position_in_connections() {
    // Consequence 3: an endpoint addressed by name rather than by index.
    let fixture = open("edge-handles");
    let image = image(&fixture);
    let owns = image.model(by_name(&image, "Owns")).expect("a node");

    assert_eq!(
        owns.connection(symbol(&fixture, "owner")).and_then(|held| held.name()),
        Some("Team")
    );
    assert_eq!(
        owns.connection(symbol(&fixture, "owned")).and_then(|held| held.name()),
        Some("Service")
    );
    // `port` is written in this project and is an ordinary member of an
    // endpoint, not a handle — so it is the sharp case: a name that exists and
    // still names no position.
    assert!(owns.connection(symbol(&fixture, "port")).is_none(), "an unbound name names none");
    assert!(owns.fields().any(|held| held.name() == DEFINITION), "and `definition` is a member");
}

#[test]
fn two_handles_may_name_one_position_which_is_what_a_self_loop_is() {
    let fixture = open("edge-handles");
    let image = image(&fixture);
    let depends = image.model(by_name(&image, "DependsOn")).expect("a node");
    assert_eq!(depends.connections().count(), 1, "one endpoint");
    for handle in ["from", "to"] {
        assert_eq!(
            depends.connection(symbol(&fixture, handle)).and_then(|held| held.name()),
            Some("Service"),
            "`{handle}` names the one position there is"
        );
    }
}

// -------------------------------------------------------- the three operators

#[test]
fn extending_an_edge_replaces_its_connections_whole_and_never_appends() {
    // `connections` is one member, absorbed by D1.5's shallow, left-biased
    // rule. There is no edge-specific rule here and there must not be one.
    let fixture = open("edge-extends");
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "Owns"),
        ["Alpha", "Beta"],
        "the child's sequence replaces the base's, and does not extend it"
    );
    let owns = image.model(by_name(&image, "Owns")).expect("a node");
    let bases: Vec<String> = owns
        .ancestors()
        .iter()
        .filter_map(|id| image.model(*id))
        .filter_map(|held| held.name())
        .map(str::to_owned)
        .collect();
    assert_eq!(bases, ["Relation"], "and the `is_a` axis survives being an edge");
}

#[test]
fn a_concrete_edge_inherits_the_endpoints_its_family_declares() {
    // The abstract edge is a `!type` that declares `connections`. Nothing new
    // spells it, because `!edge` is concrete exactly as `!node` is.
    let fixture = open("edge-extends");
    let image = image(&fixture);
    let inherits = image.model(by_name(&image, "Inherits")).expect("a node");
    assert!(inherits.is_edge());
    assert_eq!(
        endpoints(&image, "Inherits"),
        ["Alpha", "Gamma"],
        "written nowhere in this node, and held by it"
    );
    let relation = image.model(by_name(&image, "Relation")).expect("a node");
    assert!(!relation.is_edge(), "a `!type` is not tagged `!edge` and is not one");
    assert!(!relation.is_concrete(), "and is never emitted");
    assert_eq!(
        relation.connections().count(),
        0,
        "an abstract family declares the member without relating anything"
    );
}

#[test]
fn the_two_members_the_language_owns_are_never_reported_as_undeclared() {
    // `Named` extends a family that declares neither. Without the exemption
    // `W0301` reports the compiler's own vocabulary as a misspelled field.
    let fixture = open("edge-extends");
    assert_eq!(fixture.count(Code::UndeclaredField), 0, "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "Named"), ["Alpha", "Beta"]);
}

// -------------------------------------------------------------- visibility

// ------------------------------------------------------------------- shapes

// --------------------------------------------------------------- the corpus

#[test]
fn the_migrated_tag_fixture_writes_an_edge_that_means_something() {
    // `projects/tagged` used to write `owner: !edge {to: *Api, kind: owns}`,
    // which classified as an edge and was consumed by nothing at all.
    let fixture = open("tagged");
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    let image = image(&fixture);
    let api = image.model(by_name(&image, "Api")).expect("a node");
    let owns = api
        .fields()
        .find(|held| held.name() == "owns")
        .and_then(|held| image.at(held.value().0, held.value().1))
        .expect("`owns` is a nested edge node");

    assert!(owns.is_edge());
    assert_eq!(owns.name(), None, "nested, anchorless, and a node all the same");
    assert_eq!(
        owns.connection(symbol(&fixture, "owner")).map(|held| held.id()),
        Some(api.id()),
        "a relation with a node at both ends is a self-loop, not a cycle to reject"
    );
    assert_eq!(
        owns.connection(symbol(&fixture, "owned")).and_then(|held| held.name()),
        Some("Service")
    );
    assert_eq!(api.incident_edges().count(), 1, "and `Api` knows what relates it");
}

// ------------------------------------- whose `connections` an edge reads

#[test]
fn an_edge_inherits_endpoints_from_a_base_that_carries_no_edge_tag() {
    // The mixin case, and the whole reason reach-ness is not a tag test. There
    // is no tag for "abstract edge" because `!edge` is concrete exactly as
    // `!node` is, so a base that fixes the endpoints once is whatever the
    // author wrote — most often nothing at all. Reading its items as data
    // leaves the edge relating nobody, silently.
    let fixture = open("edge-mixin");
    assert!(fixture.linked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "FromMixin"), ["Alpha"], "an untagged base");
    assert_eq!(endpoints(&image, "FromNode"), ["Alpha", "Beta"], "a `!node` base");
    assert_eq!(endpoints(&image, "Includes"), ["Beta"], "and an inclusion, which absorbs alike");
    assert_eq!(
        image
            .model(by_name(&image, "FromMixin"))
            .expect("a node")
            .connection(symbol(&fixture, "only"))
            .and_then(|held| held.name()),
        Some("Alpha"),
        "the inherited `definition` names the inherited position"
    );
}

#[test]
fn a_connections_member_no_edge_reads_is_an_ordinary_member() {
    // The other half, and the one a tag test cannot have: `connections` is not
    // a reserved word. If it were, a `!type` listing interface names would be
    // two unresolved paths with no way out — quoting escapes nothing in a
    // position the language declares to be a reach.
    let fixture = open("edge-not-a-reach");
    assert_eq!(fixture.count(Code::UnresolvedRef), 0, "{}", fixture.rendered());
    assert!(fixture.linked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());

    let image = image(&fixture);
    for name in ["Router", "Switch"] {
        let held = image.model(by_name(&image, name)).expect("a node");
        assert!(!held.is_edge(), "`{name}` is not an edge");
        assert_eq!(held.connections().count(), 0, "and relates nothing");
        assert!(
            held.fields().any(|field| field.name() == CONNECTIONS),
            "while holding a member of that name like any other"
        );
    }
}

// ------------------------------------------- one bad item costs one position

#[test]
fn an_endpoint_written_inline_is_an_endpoint() {
    let fixture = open("edge-positions");
    let image = image(&fixture);
    assert_eq!(endpoints(&image, "Inline"), ["Alpha", "<anonymous>"]);
    let inline = image.model(by_name(&image, "Inline")).expect("a node");
    let anonymous = inline.connection(symbol(&fixture, "anonymous")).expect("an endpoint");
    assert_eq!(anonymous.name(), None, "a node like any other, with no name to be addressed by");
    assert!(anonymous.fields().any(|held| held.name() == "host"));
}

// --------------------------------------------------------- what a handle is

// ------------------------------------------------- the two members are owned

#[test]
fn the_language_owns_definition_as_well_as_connections() {
    // Both names, not one: `Named` extends a family that declares neither, and
    // writes both.
    let fixture = open("edge-extends");
    assert_eq!(fixture.count(Code::UndeclaredField), 0, "{}", fixture.rendered());
    let image = image(&fixture);
    let named = image.model(by_name(&image, "Named")).expect("a node");
    assert!(named.fields().any(|held| held.name() == DEFINITION));
    assert_eq!(named.connection(symbol(&fixture, "to")).and_then(|held| held.name()), Some("Beta"));
}

#[test]
fn a_handle_value_is_a_position_and_never_a_data_edge() {
    // The two members the language owns are the language's, on both sides:
    // nothing written under them is read as a member naming a node.
    let fixture = open("edge-binary");
    let image = image(&fixture);
    assert!(
        image.nodes().flat_map(|held| held.out().iter()).all(|held| held.kind != EdgeKind::Data),
        "an alias standing in `connections` is a connection and is not also data"
    );
}

// ---------------------------------------------------- a handle labels a record

#[test]
fn the_index_carries_the_handle_and_the_first_one_wins() {
    // The rule that decides what an index record is called when a position has
    // two names, which is the ordinary self-loop.
    let fixture = open("edge-handles");
    let image = image(&fixture);
    let names = |model: &str| -> Vec<Option<&str>> {
        image
            .model(by_name(&image, model))
            .expect("a node")
            .connection_edges()
            .map(|held| held.key.and_then(|key| fixture.interned.symbols().resolve(key)))
            .collect()
    };
    assert_eq!(names("Owns"), [Some("owner"), Some("owned")], "each position carries its handle");
    assert_eq!(
        names("DependsOn"),
        [Some("from")],
        "and a position named twice carries the first, so the index does not silently \
         depend on which handle was written last"
    );
}

// ------------------------------------------------------- `!ref` on an endpoint

#[test]
fn a_ref_endpoint_is_checked_and_recorded() {
    // `!ref` is a declaration of intent wherever a path is legal, so it is one
    // here too — and the record keeps it, or the image would say two endpoints
    // are alike when the compiler had just reported that they are not.
    let fixture = common::pipeline::through(common::open("edge-capability"));
    assert_eq!(fixture.count(Code::RefNotWritable), 1, "{}", fixture.rendered());
    let image = image(&fixture);
    let governs = image.model(by_name(&image, "Governs")).expect("a node");
    let marked: Vec<bool> = governs.connection_edges().map(|held| held.capability).collect();
    assert_eq!(
        marked,
        [true, false],
        "the endpoint the edge declared it may modify, and the other"
    );
}

// ------------------------------------------------ what relates a node, gated

#[test]
fn what_relates_a_node_is_the_relations_and_not_everything_pointing_at_it() {
    // An endpoint that is also a base and also a data target. `incident_edges`
    // answers with relations, and an ancestry or data record arriving at the
    // same node is not one.
    let fixture = open("edge-positions");
    let image = image(&fixture);
    let alpha = image.model(by_name(&image, "Alpha")).expect("a node");
    assert!(
        alpha.inc().iter().any(|held| held.kind != EdgeKind::Connection),
        "the fixture writes inbound edges that are not connections"
    );
    assert!(
        alpha.incident_edges().all(|held| held.is_edge()),
        "and what relates `Alpha` is `!edge` nodes only"
    );
    assert_eq!(
        alpha.inc().iter().filter(|held| held.kind == EdgeKind::Data).count(),
        1,
        "one ordinary member names `Alpha`; a `connections` of the wrong shape and a handle \
         value are members the language owns, and neither is read as one"
    );
}

#[test]
fn a_sequence_an_edge_reads_holds_endpoints_whatever_wrote_it() {
    // F1. `endpoint` was decided *after* four other exits in `refs::site`, so a
    // sequence nested inside another sequence -- or standing as a `<<`/`extends`
    // operand list, or as a complex mapping key -- reached its own answer first.
    // Each left the edge relating nothing, silently, and the items reappeared as
    // data edges leaving the anonymous list. Aliasing a shared sequence is the
    // ordinary way to write one, so this was the flagship feature producing a
    // wrong graph on its most natural input.
    let fixture = open("edge-nested-sequence");
    let image = image(&fixture);

    assert_eq!(endpoints(&image, "Web"), ["Alpha", "Beta"]);
    assert_eq!(endpoints(&image, "Db"), ["Beta", "Gamma"]);

    // And the endpoints are *connections*, not data edges from the list.
    for name in ["Web", "Db"] {
        let held = image.model(by_name(&image, name)).expect("a node");
        assert!(
            held.out().iter().all(|edge| edge.kind == EdgeKind::Connection),
            "{name} emitted something other than connections: {:?}",
            held.out().iter().map(|edge| edge.kind).collect::<Vec<_>>()
        );
    }
    assert!(
        !image.nodes().any(|held| held.name().is_none()
            && held.out().iter().any(|edge| edge.kind == EdgeKind::Data)),
        "an anonymous list must not emit data edges to an edge's endpoints"
    );
}

#[test]
fn an_edge_reads_a_connections_installed_on_it_by_an_extended_reference() {
    // M5. Reach-ness follows D4.7's contribution edges backwards from every
    // `!edge`, and tier 5 runs *both* ways: `X extends: !ref Rel` installs
    // `own(X)` onto `Rel`, so `Rel` holds a `connections` it never wrote. That
    // reverse direction is documented and load-bearing, and had no test --
    // deleting it left `Rel` relating nothing, silently, which is the class of
    // defect the whole reach-ness rule exists to remove.
    let fixture = open("edge-tier-five");
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "Rel"),
        ["Alpha", "Beta"],
        "an edge reads what an extended reference installed on it"
    );
}
