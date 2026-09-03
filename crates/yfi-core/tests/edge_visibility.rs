// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility over an edge, in both directions — D4.13.
//!
//! An edge's endpoints are gated by the `connections` **member**, which is a
//! member like any other and is therefore private by default (D4.12), and the
//! reverse question — *what relates this node* — is gated by the same two
//! predicates. Both directions are one concern and are asserted together, so a
//! change to one that forgets the other fails here rather than in a file about
//! arity.

mod common;

use common::edge::{by_name, image, scope, CLEAN};
use common::pipeline::open;
use yfi_syntax::Code;

// ------------------------------------------------- what an edge discloses

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
