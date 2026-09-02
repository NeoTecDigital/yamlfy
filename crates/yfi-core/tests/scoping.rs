// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility and mutability, composed over the whole scope path.
//!
//! `projects/scope-matrix/` declares every visibility × mutability pair nested
//! inside every other, sixteen parent/child pairs, alongside a sibling
//! `outside/` scope that observes from beyond all of them.
//!
//! The expected answers here are derived from the **directory names**, not from
//! the parsed tree, so this is a second implementation of the rule reading a
//! different source. Every ordered pair of scopes is checked, which is what
//! makes it a test of composition in both directions rather than of one lucky
//! vantage point: a bug that evaluates an axis node-locally passes any
//! single-scope assertion and still gets a `private` or `immutable` parent wrong.

mod common;

use yfi_core::{Mutability, Project, ScopeId, Visibility};

/// The two axes of one scope, as its directory name spells them, reduced to
/// "open to an outside observer".
#[derive(Clone, Copy)]
struct Axes {
    visible: bool,
    writable: bool,
}

/// Read a `scope-matrix` directory name. The root and `outside` are named
/// separately because they are not part of the generated grid.
fn axes_of(name: &str) -> Axes {
    match name {
        "scope-matrix" => Axes { visible: false, writable: true },
        "outside" => Axes { visible: true, writable: true },
        other => {
            let (visibility, mutability) = other.split_once('-').expect("`vis-mut` directory name");
            Axes {
                visible: visibility == "pub",
                writable: mutability == "mu",
            }
        }
    }
}

/// Every scope, as the `root/dir/sub` name the tree reports.
fn qualified_names(project: &Project) -> Vec<String> {
    project.scopes().scopes().iter().map(|s| project.scopes().qualified(s.id)).collect()
}

/// The independently derived answer: every scope on the target's path must be
/// open, unless the observer is inside it.
fn expected(target: &str, observer: &str, axis: fn(Axes) -> bool) -> bool {
    let target: Vec<&str> = target.split('/').collect();
    let observer: Vec<&str> = observer.split('/').collect();
    (0..target.len()).all(|depth| {
        let prefix = &target[..=depth];
        axis(axes_of(target[depth])) || observer.starts_with(prefix)
    })
}

fn id(project: &Project, qualified: &str) -> ScopeId {
    common::scope_by(project, qualified)
}

#[test]
fn every_nesting_is_composed_over_the_whole_path_from_every_observer() {
    let project = common::open_clean("scope-matrix");
    let names = qualified_names(&project);
    assert_eq!(names.len(), 22, "root, outside, four parents and sixteen children");

    let mut checked = 0usize;
    for target in &names {
        for observer in &names {
            let (t, o) = (id(&project, target), id(&project, observer));
            assert_eq!(
                project.scopes().visible(t, o),
                expected(target, observer, |a| a.visible),
                "visible({target}, {observer})"
            );
            assert_eq!(
                project.scopes().writable(t, o),
                expected(target, observer, |a| a.writable),
                "writable({target}, {observer})"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, names.len() * names.len(), "both directions of every pair");
}

#[test]
fn a_public_scope_inside_a_private_one_is_not_reachable_from_outside() {
    let project = common::open_clean("scope-matrix");
    let target = id(&project, "scope-matrix/pri-mu/pub-mu");
    let outside = id(&project, "scope-matrix/outside");
    let parent = id(&project, "scope-matrix/pri-mu");

    assert!(!project.scopes().visible(target, outside), "the enclosing scope gates reach");
    assert!(project.scopes().visible(target, parent), "reachable from within");
    assert!(project.scopes().visible(target, target), "and from itself");
}

#[test]
fn the_blocking_scope_is_the_outermost_gate_and_not_the_target() {
    // What `E0241` puts in its note. The target here is `public`; the scope
    // that shut the observer out is its *parent*, and in a deeper tree it could
    // be several levels further up still. Naming the target would tell an
    // author to change the one marking that is already correct.
    let project = common::open_clean("scope-matrix");
    let target = id(&project, "scope-matrix/pri-mu/pub-mu");
    let outside = id(&project, "scope-matrix/outside");
    let parent = id(&project, "scope-matrix/pri-mu");

    assert_eq!(project.scopes().blocked_by(target, outside), Some(parent));
    assert_eq!(project.scopes().blocked_by(target, parent), None, "reachable from within");
    assert_eq!(
        project.scopes().blocked_by(id(&project, "scope-matrix/pub-mu"), outside),
        None,
        "a visible target blocks nothing, which is what the predicate answers"
    );
    for scope in project.scopes().scopes() {
        assert_eq!(
            project.scopes().blocked_by(scope.id, outside).is_none(),
            project.scopes().visible(scope.id, outside),
            "the reason and the verdict are one rule: {}",
            project.scopes().qualified(scope.id)
        );
    }
}

#[test]
fn a_mutable_scope_inside_an_immutable_one_is_not_writable_from_outside() {
    let project = common::open_clean("scope-matrix");
    let target = id(&project, "scope-matrix/pub-ro/pub-mu");
    let outside = id(&project, "scope-matrix/outside");
    let parent = id(&project, "scope-matrix/pub-ro");

    assert!(project.scopes().visible(target, outside), "visibility is the other axis");
    assert!(!project.scopes().writable(target, outside), "an immutable parent must mean something");
    assert!(project.scopes().writable(target, parent));
}

#[test]
fn the_axes_are_orthogonal() {
    let project = common::open_clean("scope-matrix");
    let outside = id(&project, "scope-matrix/outside");
    let public_immutable = id(&project, "scope-matrix/pub-ro");
    let private_mutable = id(&project, "scope-matrix/pri-mu");

    assert!(project.scopes().visible(public_immutable, outside));
    assert!(!project.scopes().writable(public_immutable, outside));
    assert!(!project.scopes().visible(private_mutable, outside));
    assert!(project.scopes().writable(private_mutable, outside));
}

#[test]
fn a_private_root_never_hides_the_project_from_itself() {
    let project = common::open_clean("scope-matrix");
    let root = project.scopes().root().expect("root");
    assert_eq!(project.scopes().get(root).map(|s| s.visibility), Some(Visibility::Private));
    assert_eq!(project.scopes().get(root).map(|s| s.mutability), Some(Mutability::Mutable));
    for scope in project.scopes().scopes() {
        assert!(project.scopes().encloses(root, scope.id), "every scope is inside the root");
    }
    let public = id(&project, "scope-matrix/pub-mu");
    let outside = id(&project, "scope-matrix/outside");
    assert!(project.scopes().visible(public, outside));
}

#[test]
fn each_scope_stores_its_whole_root_path() {
    let project = common::open_clean("scope-matrix");
    let deep = id(&project, "scope-matrix/pri-ro/pub-mu");
    let path = project.scopes().path(deep);
    let names: Vec<String> =
        path.iter().map(|s| project.scopes().get(*s).expect("scope").name.to_string()).collect();
    assert_eq!(names, ["scope-matrix", "pri-ro", "pub-mu"]);
    assert_eq!(path.last(), Some(&deep), "the path is inclusive at both ends");
}

#[test]
fn an_unknown_scope_answers_no_rather_than_panicking() {
    let project = common::open_clean("scope-matrix");
    let root = project.scopes().root().expect("root");
    let missing = ScopeId(9_999);
    assert!(!project.scopes().visible(missing, root));
    assert!(!project.scopes().writable(root, missing));
}

/// `projects/nested-gates/` is three levels deep and puts **two** closed gates
/// on one path, at different depths:
///
/// ```text
/// gates/                     private   (the root, which encloses every observer)
///   outside/                 public    <- the observer
///   outer/                   private   <- outermost gate
///     middle/                private   <- innermost gate
///       inner/               public    <- the target
/// ```
///
/// `scope-matrix` cannot express this. It is two levels below its root, so a
/// blocked path there has exactly one gate on it and "outermost" and
/// "innermost" name the same scope — which is why every assertion about
/// [`ScopeTree::blocked_by`] passed against an implementation that answered
/// either. Two gates at different depths is the smallest shape that tells them
/// apart, and it is not a contrived one: it is a package private to its
/// subsystem inside a subsystem private to the project.
#[test]
fn two_gates_on_one_path_are_reported_by_the_outermost() {
    let project = common::open("nested-gates");
    let outside = id(&project, "nested-gates/outside");
    let outer = id(&project, "nested-gates/outer");
    let middle = id(&project, "nested-gates/outer/middle");
    let inner = id(&project, "nested-gates/outer/middle/inner");

    // The fixture is only discriminating while both gates are shut. Asserted
    // rather than assumed, so editing a `visibility:` degrades the test loudly
    // instead of quietly making it pass either way again.
    let gates: Vec<ScopeId> = project
        .scopes()
        .path(inner)
        .iter()
        .copied()
        .filter(|s| {
            let scope = project.scopes().get(*s).expect("scope on the path");
            scope.visibility == Visibility::Private && !project.scopes().encloses(*s, outside)
        })
        .collect();
    assert_eq!(gates, [outer, middle], "two closed gates, outermost first");

    assert_eq!(
        project.scopes().blocked_by(inner, outside),
        Some(outer),
        "the outermost gate is the one that has to open first; opening `middle` changes nothing"
    );
    assert_eq!(project.scopes().blocked_by(middle, outside), Some(outer), "and likewise for it");
    assert_eq!(
        project.scopes().blocked_by(inner, middle),
        None,
        "an observer inside every gate is stopped by none of them"
    );
    assert_eq!(
        project.scopes().blocked_by(inner, outer),
        Some(middle),
        "an observer inside only the outer gate is stopped by the inner one, which is then \
         the outermost gate still shut to it"
    );
}

/// The same property, read off the diagnostic rather than the predicate:
/// `E0241`'s note names the scope an author has to change, and naming the inner
/// one would send them to edit a marking that is already correct.
#[test]
fn e0241_names_the_outermost_gate_in_its_note() {
    let project = common::open("nested-gates");
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(
        common::count(project.diagnostics(), yfi_syntax::Code::ImportNotVisible),
        1,
        "{rendered}"
    );
    assert!(
        rendered.contains("outer/outer.yfy:6:13 `nested-gates/outer` is declared `private`"),
        "the note points at the outermost gate's own `visibility:`:\n{rendered}"
    );
    assert!(
        !rendered.contains("`nested-gates/outer/middle` is declared"),
        "and never at the inner gate, which opening would not help:\n{rendered}"
    );
}
