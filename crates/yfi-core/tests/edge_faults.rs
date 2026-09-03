// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an `!edge` is told about, and what it is not — D4.13.
//!
//! `E0223`, `E0224` and `E0225` are the three codes the two members the
//! language owns can earn, and `E0213` is the one a `connections` item earns by
//! being an ordinary failed reach. Split from `tests/edges.rs`, which reads the
//! model back out of a compiled project: the claims here are about what the
//! **compiler says**, and a file that grows both kinds at once ends up ordered
//! by fixture rather than by argument.
//!
//! The file opens with the sweep asserting that every fixture meant to be
//! silent is silent, because a diagnostic that fires where it should not is the
//! same failure as one that does not fire where it should.

mod common;

use common::edge::{by_name, endpoints, image, symbol, CLEAN};
use common::pipeline::open;
use yfi_core::edge::CONNECTIONS;
use yfi_syntax::Code;

// -------------------------------------------------------- nothing to say

#[test]
fn every_edge_fixture_that_should_be_clean_is_clean() {
    for name in CLEAN {
        let fixture = open(name);
        assert!(fixture.linked.diagnostics().is_empty(), "{name}: {}", fixture.rendered());
        assert!(fixture.checked.diagnostics().is_empty(), "{name}: {}", fixture.rendered());
    }
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
    assert_eq!(broken.count(Code::EdgeWithoutConnections), 2, "{}", broken.rendered());
}

#[test]
fn a_handle_naming_no_position_is_reported_and_the_others_still_bind() {
    // Two conditions, one code: past the end, and not an index at all.
    // Diagnostics accumulate, so a bad handle does not cost the good ones.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::UnboundHandle), 4, "{}", fixture.rendered());
    let image = image(&fixture);
    let bad = image.model(by_name(&image, "BadHandles")).expect("a node");
    assert_eq!(
        bad.connection(symbol(&fixture, "source")).and_then(|held| held.name()),
        Some("Alpha"),
        "the handle that does name a position still names it"
    );
    assert!(bad.connection(symbol(&fixture, "target")).is_none());
}

#[test]
fn a_reserved_member_of_the_wrong_shape_is_one_code_over_two_conditions() {
    // `connections` that is not a sequence and `definition` that is not a
    // mapping are one fault with one fix, in two places.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::EdgeMemberShape), 5, "{}", fixture.rendered());
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
    // fails. Inventing a code for it would give one fault two numbers. Three
    // items of this project name nothing: one path that names no definition,
    // and the two spellings of an anchored scalar below.
    let fixture = open("edge-errors");
    assert_eq!(fixture.count(Code::UnresolvedRef), 3, "{}", fixture.rendered());
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "Nowhere"),
        Vec::<String>::new(),
        "and the endpoint that named nothing is not invented as one"
    );
}

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

// -------------------------------------------------- endpoints and file class

#[test]
fn an_edge_cannot_take_its_endpoints_from_a_base_yaml_file() {
    // The silence D4.13 used to record as a known gap. `.yaml` is data, not
    // language (D6.6): its scalars are never read as paths, so an edge that
    // includes a `.yaml` mapping holding `connections: [Alpha]` ends up with
    // the member and with no endpoints. The member is present and is a
    // sequence, so neither `E0223`'s other conditions nor `E0213` fired, and a
    // wrong graph was accepted without a word — which D2.1 names as the worst
    // failure this design has.
    let fixture = open("edge-base-yaml");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::EdgeWithoutConnections), 1, "{rendered}");
    assert!(
        rendered.contains("base YAML"),
        "and the message says which of `E0223`'s conditions fired:\n{rendered}"
    );

    // The premise, asserted rather than assumed: the member really does arrive,
    // which is what made the old silence a wrong graph rather than a no-op.
    let at = fixture.node("graph.yfy", "FromData");
    assert!(
        fixture.resolved_keys(at).iter().any(|key| key == CONNECTIONS),
        "the resolved view is composed across file classes: {:?}",
        fixture.resolved_keys(at)
    );
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "FromData"),
        Vec::<String>::new(),
        "and the edge relates nothing, which is now said rather than left to a paragraph"
    );
}

#[test]
fn a_definition_from_a_base_yaml_file_is_read_like_any_other() {
    // The other half of the rule, and the reason it is `connections` alone that
    // is refused: a handle's value is a **position**, not a reach, so nothing
    // about reading it depends on the file class. The fixture's `.yaml` writes
    // both members; only one of them is a fault.
    let fixture = open("edge-base-yaml");
    assert_eq!(fixture.count(Code::EdgeMemberShape), 0, "{}", fixture.rendered());
    assert_eq!(fixture.count(Code::UnboundHandle), 0, "{}", fixture.rendered());
}

#[test]
fn an_anchored_scalar_is_not_an_endpoint_by_either_spelling() {
    // The asymmetry that was: a *path* naming an anchored scalar was `E0213`
    // and an *alias* naming the same anchor was a legal endpoint, so two
    // spellings of one question — is this addressable? — had two answers. An
    // endpoint is a node; D6.1 makes only an anchored collection one, and the
    // inline spelling agreed already (`connections: [7]` has always been
    // `E0213`). The alias was the outlier and now follows the same rule.
    let fixture = open("edge-errors");
    let rendered = fixture.rendered();
    assert!(
        rendered.contains("`*limit` names an anchored scalar"),
        "the alias spelling is reported, and names what it found:\n{rendered}"
    );
    assert!(
        rendered.contains("`limit` names nothing"),
        "beside the path spelling, which always was:\n{rendered}"
    );
    let image = image(&fixture);
    assert_eq!(
        endpoints(&image, "ToAValue"),
        Vec::<String>::new(),
        "and neither position holds an endpoint"
    );
}

#[test]
fn an_edge_that_empties_its_own_connections_is_not_told_it_inherited_one() {
    // `E0223`'s empty-member arm covers two situations and used to have one
    // wording for both. `pub connections:` in a base that no concrete edge
    // supplies is a declaration nobody satisfied; `connections: ~` on the node
    // itself is not a declaration at all, and the inherited wording sent the
    // author looking for a base there is none of.
    let own = open("edge-errors");
    let rendered = own.rendered();
    assert!(
        rendered.contains("an `!edge`'s own `connections` is empty"),
        "the own-key case has its own phrasing:\n{rendered}"
    );
    assert!(
        !rendered.contains("declared here, and left empty"),
        "and does not wear the inherited one:\n{rendered}"
    );

    let inherited = open("edge-positions");
    let rendered = inherited.rendered();
    assert!(
        rendered.contains("is a declaration nothing supplied"),
        "while the inherited case keeps the wording that is right for it:\n{rendered}"
    );
    assert!(
        rendered.contains("declared here, and left empty"),
        "and still points at the declaration:\n{rendered}"
    );
}

#[test]
fn a_malformed_connections_removes_the_bound_and_not_the_handle_rules() {
    // `E0224` on `connections` used to end the read, so a `definition` full of
    // nonsense was accepted without a word whenever the member above it was
    // malformed. A handle is now checked against what is **knowable**: the two
    // rejections that do not depend on the sequence are reported, and the one
    // that does — a position past the end — is not, because with no bound every
    // handle would fail against zero and one fault would print as four.
    let fixture = open("edge-errors");
    let rendered = fixture.rendered();
    assert!(
        rendered.contains("`connections` is one of the two member names the language owns"),
        "a handle taking an owned name is wrong however many endpoints there are:\n{rendered}"
    );
    assert!(
        rendered.contains("`nope` names no connection"),
        "and so is a value that is not a position at all:\n{rendered}"
    );
    assert!(
        !rendered.contains("`late` names no connection"),
        "while a position past the end of a sequence never read is a cascade:\n{rendered}"
    );
}

#[test]
fn definitions_own_shape_is_checked_whatever_connections_holds() {
    // The two members are read independently (D4.13), and that has to survive
    // one of them being malformed: `definition: 1` is not a mapping whatever
    // the sequence above it is, and the node writing both earns both codes.
    let fixture = open("edge-errors");
    let rendered = fixture.rendered();
    let both: Vec<&str> = rendered
        .lines()
        .filter(|line| line.contains("must be a mapping") || line.contains("must be a sequence"))
        .collect();
    assert_eq!(both.len(), 5, "{both:#?}");
}
