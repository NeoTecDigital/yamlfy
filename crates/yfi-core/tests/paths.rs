// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 4 — path reach, over the project corpus.
//!
//! **The path is the reach.** These tests hold the shape of that claim: what a
//! bare name means, what a peer name means, what `..` means, where a plain
//! scalar is read as a path and where it stays data, and what `!ref` adds on
//! top — a binding whose members are addressable with `.`.
//!
//! Two codes are owed here: `E0213` for a path that names nothing and `E0218`
//! for one that lands and then addresses a member the target does not hold.
//! They are two codes because the fixes are different — the path, or the member
//! name.

mod common;

use std::path::Path;

use yfi_core::intern::intern;
use yfi_core::link::{link, Linked};
use yfi_core::Project;
use yfi_syntax::{Code, FileId};

/// A project taken all the way through pass 4.
struct Linked3 {
    project: Project,
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
    Linked3 { project, linked }
}

// ---------------------------------------------------------------- E0213

#[test]
fn e0213_names_every_path_that_resolves_to_nothing() {
    // One code, four failures, each with its own note: a path can miss because
    // the definition is not there, because it climbed out of the project, or
    // because it is not a path at all. The note is what tells them apart.
    let fixture = open("link-unresolved-ref");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::UnresolvedRef), 4, "{rendered}");
    assert!(
        rendered.contains(
            "app.yfy:10:15: `Nowhere` names nothing: no definition called \
                           `Nowhere` in `app.yfy`"
        ) && rendered.contains("only an anchored collection is addressable"),
        "a bare name is sought in the file that wrote it, and the message says which:\n{rendered}"
    );
    assert!(
        rendered.contains("app.yfy:11:17: `../absent/Base` ascends past the project root")
            && rendered.contains("the root has no parent"),
        "`..` walks the scope tree and stops at the top:\n{rendered}"
    );
    assert!(
        rendered.contains("app.yfy:12:19: `refs::Base` is not a path")
            && rendered.contains("a path is written `../dir/Name`"),
        "the old `namespace::name` spelling is not a path at all:\n{rendered}"
    );
}

#[test]
fn a_bare_name_is_this_file_and_a_peer_has_to_be_named() {
    // `&Peer` is written in `peer.yfy`, which sits beside `app.yfy` in one
    // directory and one namespace. `Peer` alone does not find it — a bare name
    // is this file — and `peer/Peer` does, because naming the peer file is what
    // reaches into it.
    let fixture = open("link-unresolved-ref");
    let rendered = fixture.rendered();
    assert!(rendered.contains("app.yfy:14:17: `Peer` names nothing"), "{rendered}");
    let peer = fixture.file("peer.yfy");
    let named = fixture
        .linked
        .references()
        .iter()
        .find(|held| &*held.text == "peer/Peer")
        .expect("the path is recorded");
    assert_eq!(named.target.map(|target| target.0), Some(peer), "{rendered}");
}

#[test]
fn e0218_fires_when_the_path_lands_and_the_member_does_not() {
    // The walk succeeded and the `.` step did not, so this is not `E0213`:
    // the fix is the member name, not the path.
    let fixture = open("link-unresolved-ref");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::UnresolvedMember), 1, "{rendered}");
    assert!(
        rendered.contains(
            "app.yfy:13:21: `Base.nope` addresses `nope`, which the node it names does not hold"
        ),
        "{rendered}"
    );
}

#[test]
fn a_ref_binding_is_addressable_by_member_and_chains() {
    // `service: !ref ../core/Service` binds `service` with the capability, and
    // `service.port` addresses into it. The capability is established at the
    // binding; the `.` steps are addressing within it.
    let fixture = open("link-ref-binding");
    let app = fixture.file("app/app.yfy");
    let core = fixture.file("core/service.yfy");
    let resolved: Vec<(&str, bool)> = fixture
        .linked
        .references()
        .iter()
        .filter(|held| held.file == app)
        .map(|held| (&*held.text, held.target.is_some_and(|t| t.0 == core)))
        .collect();
    assert_eq!(
        resolved,
        [
            ("../core/Service", true),
            ("service.port", true),
            ("service.tls.enabled", true),
            ("service.absent", false),
        ],
        "a chained member lands in the file the binding named:\n{}",
        fixture.rendered()
    );
    assert_eq!(fixture.count(Code::UnresolvedMember), 1, "{}", fixture.rendered());
}

#[test]
fn a_plain_path_binds_no_name() {
    // Only `!ref` establishes the capability that member access addresses
    // through, so a plain path at a key is a data edge and nothing more.
    let fixture = open("link-ref-binding");
    let app = fixture.file("app/app.yfy");
    let bound: Vec<&str> = fixture
        .linked
        .references()
        .iter()
        .filter(|held| held.file == app)
        .filter_map(|held| held.binds.as_deref())
        .collect();
    assert_eq!(bound, ["service", "a", "b", "c"], "every one of them is a `!ref`");
}

#[test]
fn a_segment_naming_both_a_directory_and_a_file_resolves_to_the_directory() {
    // `core/` and `core.yfy` sit side by side at the root. `../core/Service`
    // means the directory: a directory is what a namespace is claimed on, and
    // resolving to the file instead would let adding a directory silently move
    // a path that already worked.
    let fixture = open("link-ref-binding");
    let directory = fixture.file("core/service.yfy");
    let found = fixture
        .linked
        .references()
        .iter()
        .find(|held| &*held.text == "../core/Service")
        .expect("the path is recorded");
    assert_eq!(found.target.map(|target| target.0), Some(directory), "{}", fixture.rendered());
}

#[test]
fn two_ascents_walk_two_scopes_up() {
    // `..` composes: `core/deep` climbs to `core`, then to the root, and comes
    // back down through `core/` to the definition. One `..` per directory,
    // exactly as a filesystem reads it.
    let fixture = open("link-ref-binding");
    let deep = fixture.file("core/deep/deep.yfy");
    let service = fixture.file("core/service.yfy");
    let found = fixture
        .linked
        .references()
        .iter()
        .find(|held| held.file == deep)
        .expect("the path is recorded");
    assert_eq!(found.text.as_ref(), "../../core/Service");
    assert_eq!(found.target.map(|target| target.0), Some(service), "{}", fixture.rendered());
}

#[test]
fn an_unanchored_scalar_in_a_data_position_stays_data() {
    // `d: MyClass` names something this file defines, and is still a string.
    // In a data position a reach must be written `./…` or `../…`, or every
    // value in the language would become a reference to whatever happened to
    // share its spelling.
    let fixture = open("link-ref-binding");
    let app = fixture.file("app/app.yfy");
    assert!(
        fixture
            .linked
            .references()
            .iter()
            .filter(|held| held.file == app)
            .all(|held| &*held.text != "MyClass"),
        "{}",
        fixture.rendered()
    );
}

#[test]
fn a_ref_in_base_yaml_is_an_ordinary_tag_and_resolves_nothing() {
    // D6.6: in a `.yaml` the operators are not interpreted. `!ref` is an
    // unrecognised tag on a value and `extends:` is a field, so the same text
    // that is three errors in the `.yfy` is silent here.
    let fixture = open("link-unresolved-ref");
    let data = fixture.file("data.yaml");
    assert!(
        fixture.linked.references().iter().all(|held| held.file != data),
        "no `!ref` is recorded for a base YAML file"
    );
    assert!(
        fixture.linked.clauses().iter().all(|held| held.file != data),
        "and its `extends:` is a field, not a clause"
    );
}
