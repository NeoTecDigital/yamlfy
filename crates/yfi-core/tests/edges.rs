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

use common::pipeline::{open, Compiled};
use yfi_core::edge::{CONNECTIONS, DEFINITION};
use yfi_core::emit::emit;
use yfi_core::image::{EdgeKind, Image, ModelId, ModelKind, Named};
use yfi_core::{ScopeId, Symbol};
use yfi_syntax::Code;

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

/// The anchor names of the nodes an edge connects, in written order.
fn endpoints<'a>(image: &'a Image<'a>, edge: &str) -> Vec<String> {
    image
        .model(by_name(image, edge))
        .expect("a node")
        .connections()
        .map(|held| held.name().unwrap_or("<anonymous>").to_owned())
        .collect()
}

/// An interned name, which is how a handle is addressed.
fn symbol(fixture: &Compiled, text: &str) -> Symbol {
    fixture.interned.symbols().get(text).unwrap_or_else(|| panic!("`{text}` is never written"))
}

/// Every edge fixture that is meant to compile without a word said about it.
/// Two corpus-wide sweeps read it, so a fixture added here is held to both.
const CLEAN: [&str; 7] = [
    "edge-binary",
    "edge-nary",
    "edge-handles",
    "edge-extends",
    "edge-visibility",
    "edge-mixin",
    "edge-not-a-reach",
];

/// The scope a project fixture's directory claims.
fn scope(fixture: &Compiled, qualified: &str) -> ScopeId {
    common::scope_by(&fixture.project, qualified)
}

// ------------------------------------------------------------ an edge is a node

#[test]
fn every_edge_fixture_that_should_be_clean_is_clean() {
    for name in CLEAN {
        let fixture = open(name);
        assert!(fixture.linked.diagnostics().is_empty(), "{name}: {}", fixture.rendered());
        assert!(fixture.checked.diagnostics().is_empty(), "{name}: {}", fixture.rendered());
    }
}

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

#[test]
fn an_edge_with_no_endpoints_is_a_shape_and_an_edge_with_no_member_is_a_fault() {
    // The degenerate/absent split, and it is D7.3's split one level over:
    // `connections: []` is written, and absence is not.
    let clean = open("edge-nary");
    let image = image(&clean);
    let planned = image.model(by_name(&image, "Planned")).expect("a node");
    assert_eq!(planned.connections().count(), 0);
    assert!(planned.is_edge(), "it is still an edge, and still emitted");
    assert_eq!(clean.count(Code::EdgeWithoutConnections), 0, "{}", clean.rendered());

    let broken = open("edge-errors");
    assert_eq!(broken.count(Code::EdgeWithoutConnections), 1, "{}", broken.rendered());
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

#[test]
fn a_handle_naming_no_position_is_reported_and_the_others_still_bind() {
    // Two conditions, one code: past the end, and not an index at all.
    // Diagnostics accumulate, so a bad handle does not cost the good ones.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::UnboundHandle), 2, "{}", fixture.rendered());
    let image = image(&fixture);
    let bad = image.model(by_name(&image, "BadHandles")).expect("a node");
    assert_eq!(
        bad.connection(symbol(&fixture, "source")).and_then(|held| held.name()),
        Some("Alpha"),
        "the handle that does name a position still names it"
    );
    assert!(bad.connection(symbol(&fixture, "target")).is_none());
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

#[test]
fn an_edges_endpoints_are_gated_by_the_connections_member() {
    // Reusing pass 5's predicate and writing no second one. A bare member is
    // private (D4.12), so an edge may be public and addressable while what it
    // relates is not disclosed.
    let fixture = open("edge-visibility");
    let image = image(&fixture);
    let inside = scope(&fixture, "edge-visibility/open");
    let outside = scope(&fixture, "edge-visibility/peer");

    let shown = image.model(by_name(&image, "Shown")).expect("a node");
    let hidden = image.model(by_name(&image, "Hidden")).expect("a node");

    assert_eq!(shown.connections_readable_from(outside).count(), 2, "`pub connections`");
    assert_eq!(hidden.connections_readable_from(outside).count(), 0, "a bare one discloses none");
    assert_eq!(hidden.connections_readable_from(inside).count(), 2, "and is ordinary inside");
    assert!(hidden.is_visible_from(outside), "the edge is still a visible, addressable node");
    assert_eq!(hidden.connections().count(), 2, "the unfiltered walk is unchanged");
}

#[test]
fn an_endpoint_the_observer_may_not_see_is_absent_by_shape() {
    // Never a hole, a null or a count, or the scoping leaks through the result.
    let fixture = open("edge-visibility");
    let image = image(&fixture);
    let outside = scope(&fixture, "edge-visibility/peer");
    let hidden = image.model(by_name(&image, "Hidden")).expect("a node");
    assert!(hidden.connections_readable_from(outside).next().is_none());
}

#[test]
fn a_connection_into_a_scope_the_edge_cannot_see_is_e0216_and_stays_undisclosed() {
    // The gate stands in front of a connections item exactly as it stands in
    // front of a clause operand, because the item *is* a reach. And a project
    // that raised `E0216` still emits, so the image must give the same answer
    // the compiler gave rather than becoming the way around it.
    let fixture = common::pipeline::through(common::open("edge-invisible-connection"));
    assert_eq!(fixture.count(Code::RefNotVisible), 1, "{}", fixture.rendered());
    let image = image(&fixture);
    let reaches = image.model(by_name(&image, "Reaches")).expect("a node");
    let observer = scope(&fixture, "edge-invisible-connection/open");
    let seen: Vec<&str> =
        reaches.connections_readable_from(observer).filter_map(|held| held.name()).collect();
    assert_eq!(seen, ["Alpha"], "the endpoint it may not see is absent, not present as a hole");
}

#[test]
fn no_clean_project_writes_an_edge_whose_endpoint_its_own_scope_cannot_see() {
    // The premise the member gate rests on, asserted rather than assumed: it is
    // `E0216` that makes the node gate unreachable behind the member gate, so
    // if `E0216` ever weakens this fails and says which gate needs rethinking.
    for name in CLEAN {
        let fixture = open(name);
        let image = image(&fixture);
        for held in image.edges() {
            for endpoint in held.connections() {
                assert!(
                    endpoint.is_visible_from(held.scope()),
                    "{name}: an edge names what its own scope cannot see"
                );
            }
        }
    }
}

// ------------------------------------------------------------------- shapes

#[test]
fn a_reserved_member_of_the_wrong_shape_is_one_code_over_two_conditions() {
    // `connections` that is not a sequence and `definition` that is not a
    // mapping are one fault with one fix, in two places.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::EdgeMemberShape), 2, "{}", fixture.rendered());
    let image = image(&fixture);
    for name in ["NotASequence", "NotAMapping"] {
        let held = image.model(by_name(&image, name)).expect("a node");
        assert!(held.is_edge(), "`{name}` is still an edge, and is still held");
    }
    assert_eq!(endpoints(&image, "NotASequence"), Vec::<String>::new());
}

#[test]
fn a_connections_item_naming_nothing_is_the_reach_code_and_not_a_new_one() {
    // A connections item is a reach, so it fails the way every other reach
    // fails. Inventing a code for it would give one fault two numbers.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::UnresolvedRef), 1, "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "Nowhere"),
        Vec::<String>::new(),
        "and the endpoint that named nothing is not invented as one"
    );
}

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
fn an_unresolvable_endpoint_costs_its_own_position_and_no_others() {
    // Three defects from one input, and they are one defect: the number of
    // positions is what `connections` **writes**, never what survived it.
    let fixture = open("edge-positions");
    let image = image(&fixture);
    let gapped = image.model(by_name(&image, "Gapped")).expect("a node");

    assert_eq!(
        gapped.connections().filter_map(|held| held.name()).collect::<Vec<_>>(),
        ["Alpha", "Gamma"],
        "the item that named nothing contributes no endpoint"
    );
    assert_eq!(
        gapped.connection(symbol(&fixture, "third")).and_then(|held| held.name()),
        Some("Gamma"),
        "and `third` still names position 2, which is written and did resolve"
    );
    assert!(
        gapped.connection(symbol(&fixture, "second")).is_none(),
        "`second` names the gap, and is never quietly handed the endpoint after it"
    );
    let keys: Vec<Option<&str>> = gapped
        .connection_edges()
        .map(|held| held.key.and_then(|key| fixture.interned.symbols().resolve(key)))
        .collect();
    assert_eq!(
        keys,
        [Some("first"), Some("third")],
        "the index agrees with the accessor about which endpoint each handle named"
    );
}

#[test]
fn a_handle_is_checked_against_the_positions_the_sequence_writes() {
    // The bound, asserted on its own: `third: 2` is legal over a three-item
    // sequence whatever the middle item resolved to, and it is the only
    // `E0225` this edge could earn.
    let fixture = open("edge-positions");
    let rendered = fixture.rendered();
    assert!(!rendered.contains("Gapped"), "no handle of `Gapped` is unbound:\n{rendered}");
    assert!(rendered.contains("E0213"), "and pass 4's codes reach the harness:\n{rendered}");
}

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

#[test]
fn a_position_has_one_spelling() {
    // The trim was the whole leniency, and its only observable effect was
    // accepting what should be rejected.
    let fixture = open("edge-positions");
    let image = image(&fixture);
    let spelling = image.model(by_name(&image, "Spelling")).expect("a node");
    for handle in ["padded", "signed", "leading"] {
        assert!(
            spelling.connection(symbol(&fixture, handle)).is_none(),
            "`{handle}` is not a position and does not name one"
        );
    }
    assert_eq!(spelling.connection_edges().count(), 1);
    assert!(spelling.connection_edges().all(|held| held.key.is_none()), "and labels nothing");
}

#[test]
fn a_handle_may_not_take_one_of_the_two_names_the_language_owns() {
    let fixture = open("edge-positions");
    let image = image(&fixture);
    let shadowing = image.model(by_name(&image, "Shadowing")).expect("a node");
    assert!(shadowing.connection(symbol(&fixture, CONNECTIONS)).is_none());
    assert!(
        fixture.rendered().contains("Shadowing"),
        "and it is reported rather than silently shadowing: {}",
        fixture.rendered()
    );
}

#[test]
fn a_handle_that_names_no_position_here_names_the_node_it_is_wrong_about() {
    // `E0225` over an inherited `definition`. The primary span is the base's,
    // which is correct for every edge of the family that reads the sequence
    // whole, so the message names the subject and a note points at it.
    let fixture = open("edge-positions");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::UnboundHandle), 6, "{rendered}");
    assert!(
        rendered.contains("`target` names no connection of `edges::positions/Narrowed`"),
        "the message names its subject:\n{rendered}"
    );
    assert!(
        rendered.contains("inherits this `definition` from `edges::positions/Pairwise`"),
        "and a note says where the declaration came from:\n{rendered}"
    );
    let image = image(&fixture);
    let narrowed = image.model(by_name(&image, "Narrowed")).expect("a node");
    assert_eq!(
        narrowed.connection(symbol(&fixture, "source")).and_then(|held| held.name()),
        Some("Alpha"),
        "and the handle that does name a position still names it"
    );
}

// ------------------------------------------------- the two members are owned

#[test]
fn an_edge_that_never_supplies_an_inherited_connections_relates_nothing() {
    // `E0223` and not `E0224`. The member is a declaration nobody satisfied,
    // which is the failure of writing no member at all; the shape code would
    // send the author looking for a sequence they never wrote.
    let fixture = open("edge-positions");
    assert_eq!(fixture.count(Code::EdgeWithoutConnections), 1, "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(
        image.model(by_name(&image, "Unsupplied")).expect("a node").connections().count(),
        0
    );
}

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
fn an_observer_is_told_what_relates_a_node_only_where_it_may_look() {
    // The reverse of `connections_readable_from`, and it needs the same two
    // gates: an edge in a scope the observer cannot see is not disclosed by
    // asking the node it relates, and neither is one whose `connections` is a
    // bare member.
    let fixture = open("edge-visibility");
    let image = image(&fixture);
    let outside = scope(&fixture, "edge-visibility/peer");
    let alpha = image.model(by_name(&image, "Alpha")).expect("a node");

    let all: Vec<&str> = alpha.incident_edges().filter_map(|held| held.name()).collect();
    assert_eq!(all, ["Shown", "Hidden", "Private"], "the unfiltered walk is unchanged");
    let seen: Vec<&str> =
        alpha.incident_edges_visible_from(outside).filter_map(|held| held.name()).collect();
    assert_eq!(
        seen,
        ["Shown"],
        "`Hidden` discloses no endpoints and `Private` is in a scope this observer cannot see"
    );
    assert!(alpha.is_visible_from(outside), "while the node itself is public");
}
