// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 5 — the check pass, over the project corpus.
//!
//! Five codes are owed here and each has a project fixture that fires it:
//! `E0212` (cyclic inheritance), `E0217` (what a `!ref` may change),
//! `E0220` (required field unsatisfied), `E0221` (declared-tag mismatch) and
//! `W0301` (undeclared field), plus `W0303` widened to a resolved base.
//!
//! `E0216` is pass 4's and is asserted in `tests/access.rs`, beside the gate it
//! belongs to.
//!
//! The rest is about the two things a plausible wrong implementation gets wrong
//! and still passes a diagnostic-count test: a cycle reported against the
//! lowest *arena* index rather than the textually first member, and validation
//! run against the flattened view rather than against declarations.

mod common;

use common::pipeline::{open, open_at, through};
use yfi_core::check::{check, check_with};
use yfi_core::intern::intern;
use yfi_core::link::link;
use yfi_syntax::{Code, Severity, SeverityMap};

// ---------------------------------------------------------------- E0212

#[test]
fn e0212_reports_once_per_component_and_names_the_forward_edges() {
    // `Service extends: !ref cyc/Probe` with `Probe << *Service`. The cycle
    // closes through the two *forward* edges; the reverse edge ends at an `own`
    // vertex and cannot participate, so naming it would blame the innocent half
    // while the author is already sure the `!ref` is at fault.
    let fixture = open("check-cycle");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::CyclicInheritance), 1, "{rendered}");
    assert!(
        rendered.contains("cycle.yfy:8:3: cyclic inheritance: the resolved view of `cyc/Service`"),
        "{rendered}"
    );
    for note in [
        "cycle.yfy:11:7 `cyc/Probe` is in the same cycle",
        "cycle.yfy:8:17 closed via `extends !ref`",
        "cycle.yfy:11:11 closed via `<<`",
    ] {
        assert!(rendered.contains(note), "expected note `{note}`:\n{rendered}");
    }
    assert_eq!(rendered.matches("closed via").count(), 2, "two edges, not three:\n{rendered}");
}

#[test]
fn e0212_points_at_the_textually_first_member_not_the_lowest_arena_index() {
    // `Probe` is nested inside `Service`, and the arena is post-order, so
    // `Probe` has the *lower* index while `Service` is written first. Ordering
    // by the arena would point the whole diagnostic at the wrong node.
    let fixture = open("check-cycle");
    let service = fixture.node("cycle.yfy", "Service");
    let probe = fixture.node("cycle.yfy", "Probe");
    assert!(probe.1.index() < service.1.index(), "the nested node has the lower arena index");
    let primary = fixture
        .checked
        .diagnostics()
        .with_code(Code::CyclicInheritance)
        .next()
        .and_then(|held| held.span)
        .expect("a span");
    let ast = &fixture.project.file(service.0).expect("file").ast;
    assert_eq!(primary.start.byte, ast.node(service.1).span.start.byte);
}

#[test]
fn recovery_leaves_every_node_a_view_and_compilation_still_fails() {
    let fixture = open("check-cycle");
    assert!(fixture.checked.is_cyclic(), "the recovered view is not a semantic");
    let service = fixture.node("cycle.yfy", "Service");
    assert!(
        fixture.resolved_keys(service).contains(&"kind".to_owned()),
        "a back edge was dropped so later passes still have something to read"
    );
}

#[test]
fn the_merge_cycle_corpus_is_rejected_here() {
    // `yfi-syntax/tests/cycles.rs` asserts these four parse cleanly and
    // documents that `E0212` belongs to a later pass. This is that pass.
    for fixture in [
        "fixtures/cycles/merge-self-cycle.yml",
        "fixtures/cycles/merge-mutual-cycle.yml",
        "fixtures/cycles/merge-deep-cycle.yml",
        "fixtures/cycles/merge-oscillating.yml",
    ] {
        let held = open_at(fixture);
        assert!(held.checked.is_cyclic(), "{fixture} is a cycle");
        assert!(held.count(Code::CyclicInheritance) >= 1, "{fixture}:\n{}", held.rendered());
    }
}

#[test]
fn a_self_merge_is_a_one_cycle_even_though_it_is_a_no_op() {
    let fixture = open_at("fixtures/cycles/merge-self-cycle.yml");
    assert_eq!(fixture.count(Code::CyclicInheritance), 1, "{}", fixture.rendered());
}

#[test]
fn cyclic_data_over_an_acyclic_inheritance_graph_stays_legal() {
    // `ring` merges `base` and points at itself. Only cycles through
    // inheritance edges are rejected; the data cycle is the point of the system.
    let fixture = open_at("fixtures/cycles/alias-cycle-with-merge-dag.yml");
    assert!(!fixture.checked.is_cyclic());
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
}

#[test]
fn a_diamond_is_a_dag_and_not_a_cycle() {
    let fixture = open_at("fixtures/cycles/merge-diamond.yml");
    assert!(!fixture.checked.is_cyclic(), "{}", fixture.rendered());
}

// ---------------------------------------------------------------- E0220

#[test]
fn e0220_names_the_required_field_and_the_declaration_that_demanded_it() {
    let fixture = open("check-required");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RequiredFieldUnsatisfied), 1, "{rendered}");
    assert!(
        rendered.contains("app.yfy:12:3: `port` is required by `req/Service`"),
        "the primary span is the node that failed to supply it:\n{rendered}"
    );
    assert!(
        rendered.contains("app.yfy:8:3 declared here, with a tag and no value"),
        "and the note is the declaration:\n{rendered}"
    );
}

#[test]
fn a_tagged_declaration_carrying_a_value_is_a_default_and_not_a_requirement() {
    // `host: !!str localhost` is optional, `region:` is declared and
    // unconstrained. Only `port: !!int` is required, so exactly one fires.
    let fixture = open("check-required");
    let api = fixture.node("app.yfy", "Api");
    assert_eq!(fixture.value_of(api, "host"), "localhost");
    assert_eq!(fixture.value_of(api, "region"), "eu");
    assert_eq!(fixture.count(Code::RequiredFieldUnsatisfied), 1);
}

// ---------------------------------------------------------------- E0221

#[test]
fn e0221_fires_even_though_the_inclusion_wins_the_flatten() {
    // An ancestor declares `port: !!int`. A local `<<` mixin supplies
    // `port: !!str "8080"`, which outranks it, so the *flattened* node holds a
    // perfectly consistent `!!str` and checking it would confirm only that the
    // winner agrees with itself.
    let fixture = open("check-tag-mismatch");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::DeclaredTagMismatch), 1, "{rendered}");
    assert!(
        rendered.contains(
            "app.yfy:10:15: `port` is declared `!!int` by `mismatch/Service`, and this value is \
             !!str"
        ),
        "the primary span is the line that wrote the offending value:\n{rendered}"
    );
    assert!(
        rendered.contains("app.yfy:8:3 declared here")
            && rendered.contains("app.yfy:10:3 `mismatch/Overrides` supplies the value"),
        "with the declaration and the mixin that outranked it:\n{rendered}"
    );
}

#[test]
fn the_flattened_view_agrees_with_itself_which_is_why_it_cannot_be_the_check() {
    let fixture = open("check-tag-mismatch");
    let api = fixture.node("app.yfy", "Api");
    assert_eq!(fixture.value_of(api, "port"), "8080", "the mixin won the flatten");
}

// ---------------------------------------------------------------- W0301

#[test]
fn w0301_names_an_undeclared_field_and_the_shape_it_was_measured_against() {
    let fixture = open("check-undeclared");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::UndeclaredField), 1, "{rendered}");
    assert!(
        rendered.contains("app.yfy:11:3: `prot` is declared by no ancestor of this node"),
        "the primary span is the key:\n{rendered}"
    );
    assert!(rendered.contains("app.yfy:8:3 `undecl/Service` is one of the shapes"), "{rendered}");
}

#[test]
fn w0301_is_silent_on_a_concrete_node_with_no_abstract_ancestor() {
    // `Standalone` writes `anything: 1` and claims to be nothing. It declares
    // its own shape and cannot deviate from it; warning here would fire on
    // every field of every standalone node and be `--allow`ed into uselessness.
    let fixture = open("check-undeclared");
    let standalone = fixture.node("app.yfy", "Standalone");
    assert_eq!(fixture.resolved_keys(standalone), ["anything"]);
    assert_eq!(fixture.count(Code::UndeclaredField), 1, "{}", fixture.rendered());
}

#[test]
fn w0301_is_a_warning_and_a_project_may_deny_it() {
    let project = common::open_clean("check-undeclared");
    let interned = intern(&project);
    let linked = link(&project, &interned);
    assert!(!check(&project, &interned, &linked).diagnostics().has_errors());
    let mut severities = SeverityMap::new();
    severities.insert(Code::UndeclaredField, Severity::Error);
    let denied = check_with(&project, &interned, &linked, severities);
    assert_eq!(denied.diagnostics().error_count(), 1, "`--deny W0301` is available");
}

#[test]
fn an_extended_reference_silences_w0301_for_that_key_across_the_family() {
    // `plugin.yfy` contributes `prot` to `fam/Service`, so `prot` becomes
    // declared vocabulary for every descendant of `Service` — including `Api`,
    // in a file that has never heard of the plugin. That is the feature working
    // as designed, and it is the strongest argument for the three operations
    // looking different at the point of writing.
    let fixture = open("check-extref-silences");
    assert_eq!(fixture.count(Code::UndeclaredField), 0, "{}", fixture.rendered());
    let service = fixture.node("base.yfy", "Service");
    assert!(
        fixture.declared_keys(service).contains(&"prot".to_owned()),
        "the base's declared view holds what was installed on it"
    );
    // The same key on the same node, with the contributing file removed, is
    // exactly `check-undeclared`'s warning.
    assert_eq!(open("check-undeclared").count(Code::UndeclaredField), 1);
}

// ---------------------------------------------------------------- resolution

#[test]
fn precedence_is_own_keys_then_inclusions_then_extensions() {
    let fixture = open("check-tag-mismatch");
    let api = fixture.node("app.yfy", "Api");
    // `Api` writes nothing of its own; the inclusion's `port` beats the
    // extension's, which is the only ordering under which adding a `<<` to a
    // node cannot be silently ignored.
    assert_eq!(fixture.value_of(api, "port"), "8080");
}

#[test]
fn an_inheritance_clause_is_consumed_where_it_is_written() {
    // `<<` and `extends` are resolved in the mapping that writes them and then
    // cease to exist. Neither appears in any view, and neither is re-exported.
    let fixture = open("check-tag-mismatch");
    for anchor in ["Api", "Service", "Overrides"] {
        let at = fixture.node("app.yfy", anchor);
        let keys = fixture.resolved_keys(at);
        assert!(!keys.contains(&"extends".to_owned()), "{anchor}: {keys:?}");
        assert!(!keys.contains(&"<<".to_owned()), "{anchor}: {keys:?}");
    }
}

#[test]
fn a_cross_file_diamond_reaches_its_base_once_and_resolves_in_written_order() {
    let fixture = open("check-diamond");
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    let leaf = fixture.node("leaf.yfy", "Leaf");
    assert_eq!(fixture.value_of(leaf, "kind"), "base", "reached twice, idempotently");
    assert_eq!(fixture.value_of(leaf, "side"), "left", "`*Left` is written first");
    assert_eq!(fixture.value_of(leaf, "port"), "8443", "and an own key beats both");
    let keys = fixture.resolved_keys(leaf);
    assert_eq!(keys.iter().filter(|held| *held == "kind").count(), 1, "{keys:?}");
}

#[test]
fn declaring_is_not_including() {
    // An inclusion is compositional, not definitional (D4.1).
    let fixture = open("check-tag-mismatch");
    let api = fixture.node("app.yfy", "Api");
    assert!(fixture.declared_keys(api).is_empty(), "{:?}", fixture.declared_keys(api));
    assert!(fixture.resolved_keys(api).contains(&"port".to_owned()));
}

// ---------------------------------------------------------------- W0303

#[test]
fn w0303_widens_to_a_base_that_holds_the_key_through_its_own_inheritance() {
    // Pass 4 tests a contribution against `own(base)` only, because the wider
    // reading needs a resolved base and resolution is this pass's. The two sets
    // are disjoint, so nothing is reported twice.
    let fixture = open("check-inert-inherited");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::InertContribution), 1, "{rendered}");
    assert_eq!(
        common::count(fixture.linked.diagnostics(), Code::InertContribution),
        0,
        "pass 4 cannot see it, and must not report it twice when pass 5 can"
    );
    assert!(
        rendered.contains(
            "patch.yfy:10:3: `vessel` is contributed to `inh/Potion`, which \
                           already holds it through its own inheritance"
        ),
        "{rendered}"
    );
    assert!(rendered.contains("base.yfy:8:3 the base already inherits it from here"), "{rendered}");
}

// ---------------------------------------------------------------- the corpus

#[test]
fn inheritance_is_transitive_over_resolved_views() {
    // D1.3: a source contributes its *resolved* view, and its clause has
    // already been discharged in producing it. `leaf` receives `root`'s keys
    // through `mid` without `mid`'s clause being re-applied at `leaf`'s level.
    let fixture = open_at("fixtures/merge/transitive.yml");
    assert!(!fixture.checked.is_empty());
    assert!(fixture.checked.diagnostics().is_empty(), "{}", fixture.rendered());
    let file = fixture.file("transitive.yml");
    let leaf = (file, common::entry_at(&fixture.project, file, 0, &["leaf"]));
    assert_eq!(fixture.resolved_keys(leaf), ["l", "m", "r"]);
}

#[test]
fn merge_is_shallow() {
    // D1.5: `nested` is taken whole from the own mapping and is not
    // deep-merged with the base's.
    let fixture = open_at("fixtures/merge/shallow-not-deep.yml");
    let file = fixture.file("shallow-not-deep.yml");
    let derived = (file, common::entry_at(&fixture.project, file, 0, &["derived"]));
    let nested = (file, common::entry_at(&fixture.project, file, 0, &["derived", "nested"]));
    assert_eq!(fixture.resolved_keys(derived), ["nested"]);
    assert_eq!(fixture.resolved_keys(nested), ["c"], "the whole value is taken, not merged into");
}

#[test]
fn only_an_extended_reference_installs_keys_on_its_target() {
    // D4.3: of the three things `!ref` declares, contribution belongs to
    // `extends: !ref` alone. Every `!ref` contributes a reverse edge -- that is
    // what carries the dependency -- so a `check` pass that reads every reverse
    // edge lets `key: !ref P` and `<<: !ref P` push their whole mapping's keys
    // onto P and every descendant of it, project-wide and silently.
    //
    // The symptom is a warning going quiet, which is why this asserts the
    // warning is still there: `Impl`'s `junk` is declared by no ancestor, and
    // it stays undeclared however many capabilities other files declare on
    // `Base`. Assert the count, not merely that it compiles.
    let fixture = open("ref-installs-nothing");
    assert_eq!(fixture.count(Code::UndeclaredField), 1, "{}", fixture.rendered());

    let base = fixture.node("base.yfy", "Base");
    let declared = fixture.declared_keys(base);
    for absent in ["spurious", "also_spurious"] {
        assert!(
            !declared.iter().any(|held| held == absent),
            "`{absent}` was installed on `Base` by a reference that must not contribute: {declared:?}"
        );
    }
}

// ---------------------------------------------------------------- E0221 origin

#[test]
fn e0221_names_the_node_it_is_reported_against() {
    // Two nodes failing against one shared mixin. The primary span is the
    // effective value, which is the mixin's -- so the two findings share a
    // location, and without a note naming the subject they print byte for byte
    // identically and neither says which node to fix.
    let fixture = open("check-shared-mixin");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::DeclaredTagMismatch), 2, "{rendered}");
    for name in ["shared/Bad", "shared/Also"] {
        assert!(
            rendered.contains(&format!("`{name}` is the node this is reported against")),
            "each finding names its own subject:\n{rendered}"
        );
    }
    let blocks: Vec<&str> = rendered.split("error[E0221]").skip(1).collect();
    assert_eq!(blocks.len(), 2, "{rendered}");
    assert_ne!(blocks[0], blocks[1], "two findings, two texts:\n{rendered}");
}

// ---------------------------------------------------------------- recovery

#[test]
fn nothing_read_off_a_recovered_view_is_reported_once_e0212_has_fired() {
    // Recovery exists so the pass does not bail: every node keeps a defined
    // view and the walk terminates. D1.8 is explicit that the recovered value
    // is not a language semantic and is never emitted, so a finding read off it
    // is a claim about a program that does not exist. `W0303` was the proof --
    // it called this contribution inert because the base "already inherits" the
    // key, through the edge the compiler had just invented, and its note
    // pointed at the very line being contributed.
    let fixture = open("cycle-recovered-view");
    let rendered = fixture.rendered();
    assert!(fixture.checked.is_cyclic(), "{rendered}");
    assert_eq!(fixture.count(Code::CyclicInheritance), 1, "{rendered}");
    assert_eq!(fixture.count(Code::InertContribution), 0, "{rendered}");
    assert!(
        !rendered.contains("already holds it through"),
        "no note pointing at the line being contributed:\n{rendered}"
    );
}

// ---------------------------------------------------------------- E0130

#[test]
fn an_alias_rejected_by_e0130_forms_no_inheritance_edge() {
    // An operand the parser has already refused is not an operand. Accepting it
    // anyway built the `is_a` edge, so a required field of the base was then
    // reported unsatisfied -- `E0220` naming `xdoc/Base`, a base `E0130` had
    // just said this node cannot name, and printed *above* it in position
    // order. One fault, one code, and the cause is the only thing to fix.
    let fixture = through(common::open("cross-document-extends"));
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::CrossDocumentAlias), 1, "{rendered}");
    assert_eq!(fixture.count(Code::RequiredFieldUnsatisfied), 0, "{rendered}");
    assert_eq!(fixture.count(Code::IllegalMergeSource), 0, "and not a second code:\n{rendered}");
    let app = fixture.node("app.yfy", "App");
    assert!(
        fixture.checked.ancestors(&fixture.linked, app.0, app.1).is_empty(),
        "the rejected alias is on no node's `is_a` axis:\n{rendered}"
    );
}
