// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 5 — what a path may reach, and who may read a resolved member.
//!
//! Two questions, one model. **The epistemic gate** decides whether a reach is
//! allowed at all: `E0216` for a path into a scope this one cannot see,
//! `E0217` for a `!ref` — mutation intent — into a scope this one may not
//! write. They are asked in that order and in *different passes*: `E0216`
//! inside pass 4's path resolution, in front of the lookup, so that an
//! invisible target resolves to nothing at all; `E0217` in pass 5, of a target
//! that already resolved. **Access** then decides, per member, whether a reader may see it —
//! and that depends on the *relationship* that brought the member into the node
//! holding it, which is why `Acquisition` is asserted alongside `Visibility`
//! throughout.
//!
//! `E0215` — "`!ref` into a file this file does not import" — was retired by
//! the path amendment and the tests that asserted it are rewritten below rather
//! than deleted: the same fixture now asserts that the reach it used to reject
//! is **clean**, which is the change stated as a test.

mod common;

use common::pipeline::open;
use yfi_core::check::Acquisition;
use yfi_core::scope::Visibility;
use yfi_syntax::Code;

// ---------------------------------------------------------------- E0216/E0217

#[test]
fn a_path_into_a_file_this_file_never_imports_is_clean() {
    // This was `E0215`. Naming is reaching: the path performs the reach and no
    // `imports:` entry has to be kept in step with it. `stray.yfy` imports
    // nothing and reads `../lib/Shared` anyway.
    let fixture = open("check-ref-reach");
    let stray = fixture.file("stray/stray.yfy");
    let lib = fixture.file("lib/shared.yfy");
    assert!(
        fixture.project.imports_of(stray).is_empty(),
        "the fixture only proves anything while this file imports nothing"
    );
    let reached: Vec<bool> = fixture
        .linked
        .references()
        .iter()
        .filter(|held| held.file == stray)
        .map(|held| held.target.is_some_and(|target| target.0 == lib))
        .collect();
    assert_eq!(reached, [true], "the path landed in `lib/shared.yfy`");
    assert!(
        fixture
            .checked
            .diagnostics()
            .items()
            .iter()
            .all(|held| held.span.is_none_or(|span| span.file != stray)),
        "a path needs no import:\n{}",
        fixture.rendered()
    );
}

#[test]
fn e0216_fires_on_a_path_into_a_scope_the_referencing_scope_cannot_see() {
    let fixture = open("check-ref-reach");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotVisible), 2, "{rendered}");
    assert!(
        rendered
            .contains("peek.yfy:9:14: `../vault/Secret` names a definition this scope cannot see"),
        "{rendered}"
    );
    assert!(
        rendered.contains("`check-ref-reach/vault` is `private`")
            && rendered.contains("both axes compose over the whole path from the root"),
        "the note names the outermost gate that shut it out:\n{rendered}"
    );
}

#[test]
fn an_invisible_target_answers_the_same_whether_the_member_exists_or_not() {
    // The disclosure this closes: three probes at one private scope used to
    // earn three distinguishable answers -- `E0216` naming the definition's
    // file, line and column for a member that exists, `E0218` for one that does
    // not, and `E0213` for a node that does not. Between them an outsider
    // enumerates a private scope's node names and each node's member names,
    // which is exactly the access D4.12 says it has none of.
    let fixture = open("private-opacity");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotVisible), 3, "{rendered}");
    assert_eq!(fixture.count(Code::UnresolvedMember), 0, "a member miss is an oracle:\n{rendered}");
    assert_eq!(fixture.count(Code::UnresolvedRef), 0, "so is a missing name:\n{rendered}");
    for path in ["../vault/Secret.password", "../vault/Secret.nosuch", "../vault/NoSuchNode"] {
        assert!(
            rendered.contains(&format!("`{path}` names a definition this scope cannot see")),
            "one shape for all three, differing only in what the author wrote:\n{rendered}"
        );
    }
    assert!(
        !rendered.contains("hidden.yfy"),
        "and no location inside the scope the reader may not see:\n{rendered}"
    );
}

#[test]
fn the_visibility_gate_stands_in_front_of_member_resolution() {
    // Ordering, not wording: an invisible target must resolve to *nothing*, so
    // nothing downstream -- member addressing, an `is_a` edge, a required-field
    // check against the base -- ever runs against a node the reader may not
    // have. A gate that only decorates the diagnostic is not a gate.
    let fixture = open("private-opacity");
    let probe = fixture.file("outside/probe.yfy");
    let resolved: Vec<bool> = fixture
        .linked
        .references()
        .iter()
        .filter(|held| held.file == probe)
        .map(|held| held.target.is_some())
        .collect();
    assert_eq!(resolved, [false, false, false], "{}", fixture.rendered());
}

#[test]
fn e0217_fires_on_a_ref_whose_target_may_not_be_written_from_here() {
    // `reach::lib` is `public` and says nothing about mutability, so it is
    // `immutable` by default. A plain path into it is fine; `!ref` is not,
    // because `!ref` declares that this context intends to modify the target.
    let fixture = open("check-ref-reach");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotWritable), 1, "{rendered}");
    assert!(
        rendered.contains(
            "patch.yfy:9:16: `!ref ../lib/Shared` declares that this context intends to modify \
             the target"
        ),
        "{rendered}"
    );
    assert!(
        rendered.contains("`check-ref-reach/lib` is `immutable`"),
        "the note names the gate on the mutability axis:\n{rendered}"
    );
    assert!(
        rendered.contains("drop the `!ref` if `../lib/Shared` is meant to be read"),
        "the fix is the plain path:\n{rendered}"
    );
}

#[test]
fn a_ref_into_a_public_mutable_scope_is_clean() {
    let fixture = open("check-ref-reach");
    let rendered = fixture.rendered();
    assert!(
        !rendered.contains("../mut/Open"),
        "an explicit `mutable` admits the write:\n{rendered}"
    );
}

#[test]
fn visibility_is_decided_before_writability() {
    // `reach::vault` is `private` *and* `immutable`, and `peek.yfy` writes a
    // `!ref` into it — so both gates are shut and only one may be reported.
    // Naming the mutability one would send the author to change the keyword
    // that is not what stopped them, and the `public` they actually need would
    // still be missing when they came back.
    let fixture = open("check-ref-reach");
    let rendered = fixture.rendered();
    assert!(
        rendered.contains(
            "peek.yfy:10:17: `../vault/Secret` names a definition this scope \
                           cannot see"
        ),
        "the `!ref` into a private *and* immutable scope reports the visibility gate:\n{rendered}"
    );
    assert!(
        !rendered.contains("`!ref ../vault/Secret` declares that"),
        "and not the mutability one:\n{rendered}"
    );
}

#[test]
fn a_path_into_an_imported_visible_file_is_clean() {
    let fixture = open("check-ref-reach");
    let user = fixture.file("open/user.yfy");
    assert!(
        fixture
            .checked
            .diagnostics()
            .items()
            .iter()
            .all(|held| held.span.is_none_or(|span| span.file != user)),
        "{}",
        fixture.rendered()
    );
}

#[test]
fn a_path_inside_its_own_file_is_gated_by_nothing() {
    // A file can always see, and always write, what it wrote.
    let fixture = open("tagged");
    assert_eq!(fixture.count(Code::RefNotVisible), 0, "{}", fixture.rendered());
    assert_eq!(fixture.count(Code::RefNotWritable), 0, "{}", fixture.rendered());
}

// ---------------------------------------------------------------- visibility

#[test]
fn an_inherited_private_field_is_carried_in_private_rather_than_published() {
    // `keep::vault` is private and contributes `token` to `keep::api/Service`
    // through an extended reference. Every descendant of `Service` now carries
    // the field — but it is carried in *private to the inheritor*, not
    // laundered into the public scope it landed in.
    let fixture = open("check-private-inherit");
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    let api = fixture.node("api/api.yfy", "Api");
    let view = fixture.checked.resolved(api.0, api.1).expect("a view");
    let api_scope = fixture.interned.scope_of(api.0, api.1).expect("a scope");

    let token = view.get(fixture.symbol("token")).expect("`token` was installed");
    assert_eq!(token.reach.visibility, Visibility::Private, "privacy is not laundered");
    assert_eq!(token.reach.scope, api_scope, "and is re-based onto the inheritor");
    assert_ne!(
        token.reach.scope,
        fixture.interned.scope_of(token.origin.0, token.origin.1).expect("origin scope"),
        "the field is no longer gated by the scope it came from"
    );

    // The inheritor's own visibility is its own: only the field is restricted.
    let name = view.get(fixture.symbol("name")).expect("`name`");
    assert_eq!(name.reach.visibility, Visibility::Public);
    assert_eq!(name.reach.scope, api_scope);
}

#[test]
fn a_public_scopes_own_fields_stay_public_through_an_extension() {
    let fixture = open("check-private-inherit");
    let api = fixture.node("api/api.yfy", "Api");
    let port = fixture
        .checked
        .resolved(api.0, api.1)
        .expect("a view")
        .get(fixture.symbol("port"))
        .expect("`port`");
    assert_eq!(port.reach.visibility, Visibility::Public, "inherited from a public ancestor");
}

// ---------------------------------------------------------------- access

/// Every field of a node's resolved view, as `(key, how it arrived, gate)`.
fn members(
    fixture: &common::pipeline::Compiled,
    file: &str,
    anchor: &str,
) -> Vec<(String, Acquisition, Visibility)> {
    let at = fixture.node(file, anchor);
    fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .fields()
        .iter()
        .map(|field| {
            let name = fixture.interned.symbols().resolve(field.name).unwrap_or_default();
            (name.to_owned(), field.acquired, field.reach.visibility)
        })
        .collect()
}

/// Whether an observer sitting in the scope of `observer` may read `key` of the
/// node `anchor` in `file`.
fn readable(
    fixture: &common::pipeline::Compiled,
    holder: (&str, &str),
    key: &str,
    observer: &str,
) -> bool {
    let at = fixture.node(holder.0, holder.1);
    let field = fixture
        .checked
        .resolved(at.0, at.1)
        .expect("a view")
        .get(fixture.symbol(key))
        .unwrap_or_else(|| panic!("`{key}` is not in the view"));
    let scope = common::scope_by(&fixture.project, observer);
    field.is_readable_from(fixture.project.scopes(), scope)
}

#[test]
fn the_access_fixture_compiles_clean() {
    let fixture = open("check-access");
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    assert!(fixture.linked.diagnostics().is_empty());
}

#[test]
fn inclusion_carries_a_private_member_in_without_republishing_it() {
    // `Mix << Service`. `Service` is public and holds one private member,
    // installed on it from `acc::vault`. Containment brings it in and leaves it
    // gated by the context it already had: `Mix` addresses it, and a reader in
    // `Mix`'s own scope still cannot see it.
    let fixture = open("check-access");
    let held = members(&fixture, "mix/mix.yfy", "Mix");
    assert_eq!(
        held,
        [
            ("port".to_owned(), Acquisition::Included, Visibility::Public),
            ("token".to_owned(), Acquisition::Included, Visibility::Private),
        ]
    );
    assert!(!readable(&fixture, ("mix/mix.yfy", "Mix"), "token", "check-access/mix"));
    assert!(readable(&fixture, ("mix/mix.yfy", "Mix"), "port", "check-access/mix"));
}

#[test]
fn one_extends_step_absorbs_a_private_member_as_the_inheritors_own() {
    // `Sub extends *Service`. Instantiation absorbs: `token` becomes `Sub`'s
    // own private member, re-gated onto `acc::sub`, and is therefore readable
    // from inside `acc::sub` where it was not readable from `acc::mix`.
    let fixture = open("check-access");
    let held = members(&fixture, "sub/sub.yfy", "Sub");
    assert_eq!(
        held,
        [
            ("extra".to_owned(), Acquisition::Own, Visibility::Public),
            ("port".to_owned(), Acquisition::Extended, Visibility::Public),
            ("token".to_owned(), Acquisition::Extended, Visibility::Private),
        ]
    );
    assert!(readable(&fixture, ("sub/sub.yfy", "Sub"), "token", "check-access/sub"));
    assert!(!readable(&fixture, ("sub/sub.yfy", "Sub"), "token", "check-access/leaf"));
}

#[test]
fn a_private_member_does_not_survive_a_second_inheritance_step() {
    // `Leaf extends *Sub extends *Service`. Privacy crosses one step, not a
    // chain, so `token` does not arrive at all — while `port`, public at that
    // level, descends normally.
    let fixture = open("check-access");
    let held = members(&fixture, "leaf/leaf.yfy", "Leaf");
    assert_eq!(
        held,
        [
            ("extra".to_owned(), Acquisition::Extended, Visibility::Public),
            ("port".to_owned(), Acquisition::Descended, Visibility::Public),
        ],
        "a descendant reaches its grandparent's members only where they are public"
    );
}

#[test]
fn referencing_a_public_node_does_not_reach_into_its_private_members() {
    // `acc::api/Service` is public and referenceable from anywhere. Its private
    // member is not part of the surface a `!ref` yields: being able to name a
    // node is not being able to reach into it.
    let fixture = open("check-access");
    let service = fixture.node("api/api.yfy", "Service");
    let view = fixture.checked.resolved(service.0, service.1).expect("a view");
    let outside = common::scope_by(&fixture.project, "check-access/mix");
    let surface: Vec<&str> = view
        .readable_from(fixture.project.scopes(), outside)
        .map(|field| fixture.interned.symbols().resolve(field.name).unwrap_or_default())
        .collect();
    assert_eq!(surface, ["port"], "the public surface, and nothing under it");
    assert_eq!(view.len(), 2, "the private member is still resolved, just not readable");
}

#[test]
fn a_private_definition_is_ordinary_inside_its_own_scope() {
    // Privacy is a boundary against the outside, not secrecy from siblings.
    // `Peer` includes `Audit`, both in the private `acc::vault`, and reads its
    // private member without ceremony.
    let fixture = open("check-access");
    assert!(readable(&fixture, ("vault/patch.yfy", "Peer"), "token", "check-access/vault"));
    assert!(!readable(&fixture, ("vault/patch.yfy", "Peer"), "token", "check-access/api"));
}

#[test]
fn a_private_definition_outside_the_observers_scope_cannot_be_reached_at_all() {
    // The epistemic gate, checked before any of the three relationships: there
    // is no view of `reach::vault/Secret` to ask questions of from `reach::peek`,
    // because the reach itself is refused — whether it was asked for with a
    // plain path or with a `!ref`.
    let fixture = open("check-ref-reach");
    assert_eq!(fixture.count(Code::RefNotVisible), 2, "{}", fixture.rendered());
}
