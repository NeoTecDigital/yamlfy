// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Member flags, and the `.yfy` front end reaching the semantic passes.
//!
//! `pub`/`public` and `mut`/`mutable` are prefixes on a member name, not tags:
//! `- pub mut name` is the ordinary YAML string `"pub mut name"` and the prefix
//! is read off the scalar. A bare member is `private` and `immutable`, which is
//! D6.4's rule one level down — a member that says nothing grants nothing — and
//! the gate is composed with its scope's, which is D6.5's rule one level down.

mod common;

use common::pipeline::{open, Compiled};
use yfi_core::check::Acquisition;
use yfi_core::scope::{Mutability, Visibility};
use yfi_syntax::{Code, ScalarStyle};

/// Every member of a node's resolved view, as `(name, visibility, mutability)`.
fn members(fixture: &Compiled, file: &str, anchor: &str) -> Vec<(String, Visibility, Mutability)> {
    let at = fixture.node(file, anchor);
    fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .fields()
        .iter()
        .map(|field| {
            let name = fixture.interned.symbols().resolve(field.name).unwrap_or_default();
            (name.to_owned(), field.reach.visibility, field.reach.mutability)
        })
        .collect()
}

#[test]
fn the_fixture_compiles_clean() {
    let fixture = open("member-flags");
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(fixture.linked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(
        fixture.project.diagnostics().is_empty(),
        "{}",
        fixture.project.diagnostics().render(fixture.project.sources())
    );
}

#[test]
fn a_sequence_of_names_is_a_member_list_and_each_prefix_is_read_off_it() {
    let fixture = open("member-flags");
    assert_eq!(
        members(&fixture, "app/app.yfy", "ClassA"),
        [
            ("private_member".to_owned(), Visibility::Private, Mutability::Immutable),
            ("public_member".to_owned(), Visibility::Public, Mutability::Immutable),
            ("public_member_two".to_owned(), Visibility::Public, Mutability::Immutable),
            ("mutable_member".to_owned(), Visibility::Private, Mutability::Mutable),
            ("member_two".to_owned(), Visibility::Private, Mutability::Mutable),
            ("public_mutable_member".to_owned(), Visibility::Public, Mutability::Mutable),
            ("mutable_public_member".to_owned(), Visibility::Public, Mutability::Mutable),
        ],
        "both spellings of both axes, in either order, and a bare member closed on both"
    );
}

#[test]
fn the_mapping_form_declares_the_same_two_axes() {
    let fixture = open("member-flags");
    let held = members(&fixture, "app/app.yfy", "Service");
    assert!(held.contains(&("port".to_owned(), Visibility::Public, Mutability::Immutable)));
    assert!(held.contains(&("secret".to_owned(), Visibility::Private, Mutability::Immutable)));
    assert!(held.contains(&("handler".to_owned(), Visibility::Public, Mutability::Mutable)));
}

#[test]
fn a_quoted_name_keeps_its_prefix_because_quoting_is_the_escape() {
    // D4.2's mechanism, one level down: a quoted or tagged key is literal text.
    let fixture = open("member-flags");
    let held = members(&fixture, "app/app.yfy", "Service");
    assert!(
        held.contains(&("pub literal".to_owned(), Visibility::Private, Mutability::Immutable)),
        "the member is called `pub literal`, and it declared nothing: {held:?}"
    );
}

#[test]
fn a_public_member_is_readable_from_another_scope_and_a_private_one_is_not() {
    let fixture = open("member-flags");
    let at = fixture.node("app/app.yfy", "Service");
    let view = fixture.checked.resolved(at.0, at.1).expect("a view");
    let outside = common::scope_by(&fixture.project, "member-flags/reader");
    let surface: Vec<&str> = view
        .readable_from(fixture.project.scopes(), outside)
        .map(|field| fixture.interned.symbols().resolve(field.name).unwrap_or_default())
        .collect();
    assert_eq!(surface, ["port", "handler"], "the public surface, and nothing under it");
    assert!(view.len() > surface.len(), "the private members are resolved, just not readable");
}

#[test]
fn a_public_member_of_a_private_scope_is_public_only_inside_it() {
    // The composition, and the reason no narrowing rule is needed (D6.5): the
    // member is reachable from inside the private scope, where the scope's own
    // gate is already passed, and from nowhere else.
    let fixture = open("member-flags");
    let at = fixture.node("vault/vault.yfy", "Ledger");
    let entries = fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .get(fixture.symbol("entries"))
        .expect("`entries`");
    assert_eq!(entries.reach.visibility, Visibility::Private, "the scope composes over it");
    assert_eq!(entries.reach.mutability, Mutability::Mutable, "the scope says `mutable`");
    let scopes = fixture.project.scopes();
    assert!(entries.is_readable_from(scopes, common::scope_by(&fixture.project, "member-flags/vault")));
    assert!(!entries.is_readable_from(scopes, common::scope_by(&fixture.project, "member-flags/app")));
}

#[test]
fn every_nested_scalar_of_a_yfy_is_a_member_however_it_is_written() {
    // **A member is anything nested inside something else**, exactly as YAML
    // nests, and the discriminator is the file class. A `.yfy` is not a data
    // store: what is nested in it are members, and the data is what is
    // evaluated from that structure. Quoting is the escape for the *prefix*
    // (D4.2, one level down) and has never been a rule about membership —
    // letting it be one would put a signal inside the file in charge of a
    // semantic question, which is what D6.6 forbids one level up.
    let fixture = open("member-flags");
    let file = fixture.file("app/app.yfy");
    let tags = common::entry_at(&fixture.project, file, 1, &["Service", "tags"]);
    let ast = &fixture.project.file(file).expect("file").ast;
    let names: Vec<&str> = ast
        .items(tags)
        .expect("a sequence")
        .iter()
        .filter_map(|item| fixture.interned.key_of(file, *item))
        .map(|name| fixture.interned.symbols().resolve(name).unwrap_or_default())
        .collect();
    assert_eq!(names, ["one", "two"], "quoted items are members, and keep their text");
    assert!(fixture.checked.resolved(file, tags).is_some(), "so the sequence holds members");
}

#[test]
fn a_base_yaml_sequence_declares_no_members_at_all() {
    // The other side of the same discriminator: a `.yaml` is base YAML data,
    // has no yfi syntax in it (D6.6), and declares nothing. Nothing about how
    // its items are written can change that, which is what makes the rule one
    // question and not two.
    let fixture = open("imports-data");
    let file = fixture.file("services.yaml");
    let ast = &fixture.project.file(file).expect("file").ast;
    let sequences: Vec<yfi_syntax::NodeId> = (0..ast.nodes().len())
        .map(|at| yfi_syntax::NodeId(u32::try_from(at).expect("arena")))
        .filter(|node| ast.items(*node).is_some())
        .collect();
    assert!(!sequences.is_empty(), "the fixture writes at least one sequence");
    for node in sequences {
        for item in ast.items(node).expect("a sequence") {
            assert_eq!(fixture.interned.key_of(file, *item), None, "data declares nothing");
        }
    }
}

#[test]
fn both_axes_travel_together_across_an_extends_step() {
    let fixture = open("member-flags");
    let at = fixture.node("reader/reader.yfy", "Reader");
    let handler = fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .get(fixture.symbol("handler"))
        .expect("`handler`");
    assert_eq!(handler.acquired, Acquisition::Extended);
    assert_eq!(handler.reach.visibility, Visibility::Public);
    assert_eq!(handler.reach.mutability, Mutability::Mutable, "the `mut` came with it");
    assert!(handler.is_writable_from(
        fixture.project.scopes(),
        common::scope_by(&fixture.project, "member-flags/app")
    ));
}

#[test]
fn base_yaml_members_are_gated_by_their_scope_and_by_nothing_else() {
    // The flags are `.yfy` syntax and are **not interpreted** in base YAML
    // (D6.6), so there is no declaration to read there and the closed default
    // must not apply either — a data file cannot opt in, so gating it on a
    // prefix it has no way to write would make every imported `.yaml` private
    // for good.
    let fixture = open("imports-data");
    let file = fixture.file("services.yaml");
    let node = common::entry_at(&fixture.project, file, 0, &["web"]);
    let view = fixture.checked.resolved(file, node).expect("a view");
    let gates: Vec<Visibility> = view.fields().iter().map(|held| held.reach.visibility).collect();
    assert!(!gates.is_empty());
    assert!(
        gates.iter().all(|held| *held == Visibility::Public),
        "`fleet` is reachable from the root, so its data is readable: {gates:?}"
    );
}

#[test]
fn a_path_addresses_the_member_name_with_the_prefix_already_taken_off() {
    // `!ref ../app/Service.port` names `pub port:`. A flag is a declaration
    // about a member, never part of its name.
    let fixture = open("member-flags");
    let port = fixture
        .linked
        .references()
        .iter()
        .find(|held| &*held.text == "../app/Service.port")
        .expect("the member path");
    let target = port.target.expect("`Service.port` resolved");
    let ast = &fixture.project.file(target.0).expect("file").ast;
    assert_eq!(&*ast.scalar(target.1).expect("scalar").value, "8443");
}

#[test]
fn privacy_still_crosses_exactly_one_extends_step() {
    // `Reader extends ../app/Service`. Instantiation absorbs, so `secret`
    // arrives as Reader's own private member — re-gated onto `mf::reader`,
    // not laundered into it.
    let fixture = open("member-flags");
    let at = fixture.node("reader/reader.yfy", "Reader");
    let view = fixture.checked.resolved(at.0, at.1).expect("a view");
    let secret = view.get(fixture.symbol("secret")).expect("`secret` crossed one step");
    assert_eq!(secret.acquired, Acquisition::Extended);
    assert_eq!(secret.reach.visibility, Visibility::Private);
    assert_eq!(
        secret.reach.scope,
        fixture.interned.scope_of(at.0, at.1).expect("a scope"),
        "re-gated onto the inheritor"
    );
    assert!(!secret.is_readable_from(
        fixture.project.scopes(),
        common::scope_by(&fixture.project, "member-flags/app")
    ));
}

#[test]
fn a_code_block_is_a_members_value_and_carries_its_flag_through_the_passes() {
    let fixture = open("member-flags");
    let at = fixture.node("app/app.yfy", "Service");
    let handler = fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .get(fixture.symbol("handler"))
        .expect("`handler`");
    let ast = &fixture.project.file(handler.value.0).expect("file").ast;
    let scalar = ast.scalar(handler.value.1).expect("a code block is a scalar");
    assert_eq!(scalar.style, ScalarStyle::Code);
    assert_eq!(&*scalar.value, " fn(request) { return request.path: 1 } ");
}

#[test]
fn two_keys_naming_one_member_are_e0110_however_they_are_spelled() {
    // The parser compares keys by text and cannot see past a prefix, so
    // `port` and `pub port` reach the linker as two keys and one member.
    // Left-biased absorption would keep the first and drop the second in
    // silence.
    let fixture = open("member-collision");
    let found = fixture.linked.diagnostics();
    let rendered = found.render(fixture.project.sources());
    assert_eq!(common::count(found, Code::DuplicateKey), 1, "{rendered}");
    assert!(
        rendered.contains("app.yfy:9:3: `pub port` names the member `port`"),
        "{rendered}"
    );
    assert!(rendered.contains("first declared here"), "{rendered}");
}
