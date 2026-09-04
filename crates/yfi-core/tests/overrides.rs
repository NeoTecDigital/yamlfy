// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `override` — the keyword that claims priority among claimants (D4.14).
//!
//! Two spellings and two entirely different times. `extends: !ref override P`
//! is a **compile-time redefinition**: the contribution outranks the base's own
//! keys instead of ranking below them, so every node that is a `P` and every
//! node that merely includes `P` sees the new value. `<<: override P` is a
//! **runtime claim** and moves nothing at all: the resolved views either side
//! of it are byte-identical to `<<: P`'s, and what the compiler does is record
//! the claim, gate it, and emit it.
//!
//! **Neither spelling is a write on its own.** `!ref` declares intent to
//! modify and answers the mutability gate; `override` declares priority among
//! claimants and answers nothing. So `extends: override P` is legal into an
//! immutable scope and `extends: !ref override P` is not, and the difference
//! between them is one declaration rather than a word that changed nothing.
//! Neither touches visibility, which is decided a pass earlier and is never
//! forced by anything (D4.12).

mod common;

use common::pipeline::{open, Compiled};
use yfi_core::check::Acquisition;
use yfi_core::emit::emit;
use yfi_core::image::{EdgeKind, Image, ModelId};
use yfi_core::member;
use yfi_syntax::Code;

/// A project taken all the way through pass 6.
fn image<'a>(fixture: &'a Compiled) -> Image<'a> {
    emit(&fixture.project, &fixture.interned, &fixture.linked, &fixture.checked)
}

fn by_name<'a>(image: &'a Image<'a>, name: &str) -> ModelId {
    image
        .nodes()
        .find(|held| held.name() == Some(name))
        .unwrap_or_else(|| panic!("no node called `{name}`"))
        .id()
}

// ------------------------------------------------------------------ lexing

#[test]
fn override_is_read_off_the_operand_by_the_prefix_rule_the_axes_use() {
    // The word is a prefix on a **plain scalar**, exactly as `pub`, `mut` and
    // `brute` are — so no tag is introduced, `!ref` stays a real tag, and
    // nothing about the parse changes.
    assert_eq!(member::split_operand("override base/Potion"), (true, "base/Potion"));
    assert_eq!(member::split_operand("base/Potion"), (false, "base/Potion"));
    assert_eq!(member::split_operand("  override   ../lib/Shared "), (true, "../lib/Shared"));
}

#[test]
fn override_with_nothing_to_qualify_is_a_path() {
    // `split`'s rule, verbatim: the last word is never a flag, so a definition
    // genuinely called `override` is still reachable by its own name.
    assert_eq!(member::split_operand("override"), (false, "override"));
}

#[test]
fn the_operand_vocabulary_is_override_alone() {
    // An operand is a **path**, not a member. `pub`, `mut` and `brute` state
    // what a member is or does and have nothing to qualify on a path;
    // consuming them here would make `<<: pub Base` quietly resolve to `Base`.
    for text in ["pub Base", "mut Base", "brute Base"] {
        assert_eq!(member::split_operand(text), (false, text), "`{text}` is not an operand flag");
    }
    assert!(!member::split("override port").0.is_declared(), "and the reverse holds too");
}

// -------------------------------------------------- the compile-time change

#[test]
fn an_overriding_extended_reference_outranks_the_bases_own_key() {
    // D4.5 ranks an ordinary contribution below everything the base already
    // holds. `override` inverts exactly that, and only that: `cork`, which the
    // patch does not contribute, is untouched.
    let fixture = open("override-redefines");
    let potion = fixture.node("potion.yfy", "Potion");
    assert_eq!(fixture.value_of(potion, "vessel"), "flask", "{}", fixture.rendered());
    assert_eq!(fixture.value_of(potion, "cork"), "wax");
}

#[test]
fn the_redefinition_reaches_every_node_that_is_a_p_and_every_node_that_includes_one() {
    // The blast radius is the operator's, not the keyword's: `extends: !ref`
    // already reaches every `P` in the program, so this does too.
    let fixture = open("override-redefines");
    let draught = fixture.node("use.yfy", "Draught");
    let holder = fixture.node("use.yfy", "Holder");
    assert_eq!(fixture.value_of(draught, "vessel"), "flask", "a node that is a Potion");
    assert_eq!(fixture.value_of(holder, "vessel"), "flask", "a node that merely includes one");
}

#[test]
fn overriding_something_is_not_w0303_and_the_project_is_otherwise_clean() {
    // `W0303` says a contribution is inert because the base already defines
    // the key. That is the condition `override` inverts, so the warning has
    // nothing left to report — and reporting it anyway would make the correct
    // spelling of a redefinition noisier than the mistaken one.
    let fixture = open("override-redefines");
    assert_eq!(fixture.count(Code::InertContribution), 0, "{}", fixture.rendered());
    assert_eq!(fixture.count(Code::VacuousOverride), 0, "{}", fixture.rendered());
    assert!(fixture.rendered().is_empty(), "{}", fixture.rendered());
}

#[test]
fn installing_an_override_leaves_the_bases_own_members_the_bases_own() {
    // The overriding tier is absorbed *above* the base, so the base's composed
    // view is folded in underneath rather than carried across a relationship.
    // Carrying it would re-gate every member onto the scope it is already gated
    // to and demote its acquisition a second time — after which `Potion`'s
    // private `seal` would be one step from its author before `Draught` had
    // taken any step at all, and would silently vanish from the leaf. That is
    // `projects/check-diamond`'s fault, reached from the other side.
    let fixture = open("override-redefines");
    let draught = fixture.node("use.yfy", "Draught");
    assert!(
        fixture.resolved_keys(draught).contains(&"seal".to_owned()),
        "privacy crosses one step from the node that wrote it: {:?}",
        fixture.resolved_keys(draught)
    );
}

// --------------------------------------------------------- overriding nothing

#[test]
fn an_override_that_lands_on_no_key_of_the_base_is_w0305() {
    // The mirror of `W0303`, and it needs its own code for `W0303`'s reason:
    // by D4.5's identity result the author's own node looks correct either
    // way, so a mistyped `override` is invisible in the file that makes it.
    let fixture = open("override-nothing");
    assert_eq!(fixture.count(Code::VacuousOverride), 1, "{}", fixture.rendered());
    let rendered = fixture.rendered();
    assert!(rendered.contains("`vesel`"), "the typo is the subject: {rendered}");
    assert!(!rendered.contains("`vessel`"), "the key that does override is silent: {rendered}");
    assert_eq!(fixture.count(Code::InertContribution), 0, "never both codes: {rendered}");
}

#[test]
fn w0305_is_a_warning_and_is_configurable() {
    assert_eq!(Code::VacuousOverride.default_severity(), yfi_syntax::Severity::Warning);
    assert_eq!(Code::parse("W0305"), Some(Code::VacuousOverride));
    assert!(Code::all().contains(&Code::VacuousOverride), "`--deny W0305` validates against this");
}

// ------------------------------------------------------------ the two claims

#[test]
fn an_inclusion_claim_changes_no_resolved_value() {
    // The whole of `<<: override P`'s compile-time effect, asserted directly:
    // `Plain` and `Claimed` differ by one word in the source and by nothing in
    // the result.
    let fixture = open("override-claim");
    let plain = fixture.node("claim.yfy", "Plain");
    let claimed = fixture.node("claim.yfy", "Claimed");
    assert_eq!(fixture.resolved_keys(plain), fixture.resolved_keys(claimed));
    for key in ["port", "host"] {
        assert_eq!(fixture.value_of(plain, key), fixture.value_of(claimed, key), "{key}");
    }
}

#[test]
fn an_inclusion_claim_installs_nothing_on_its_target() {
    // A claim is a registered right, not an edit. Only `extends: !ref`
    // contributes keys (D4.3), and `override` qualifies a contribution rather
    // than creating one.
    let fixture = open("override-claim");
    let base = fixture.node("claim.yfy", "Base");
    assert_eq!(fixture.resolved_keys(base), ["port"], "{}", fixture.rendered());
    assert_eq!(fixture.declared_keys(base), ["port"]);
    assert!(fixture.rendered().is_empty(), "{}", fixture.rendered());
}

#[test]
fn the_claim_is_carried_into_the_image_where_a_runtime_can_find_it() {
    // Recorded, gated and emitted — never executed. It rides the inclusion
    // edge it qualifies, so the claim is found from either end of the
    // relationship by the index that already exists.
    let fixture = open("override-claim");
    let image = image(&fixture);
    let claimed = by_name(&image, "Claimed");
    let plain = by_name(&image, "Plain");
    let read = |id: ModelId| -> Vec<(&'static str, bool, bool)> {
        image
            .out(id)
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Inclusion)
            .map(|edge| (edge.kind.as_str(), edge.capability, edge.overrides))
            .collect()
    };
    assert_eq!(
        read(claimed),
        [("<<", false, true)],
        "the claim, on the inclusion it qualifies -- and no capability beside it, because \
         the node wrote no `!ref` and declared no intent to modify anything"
    );
    assert_eq!(read(plain), [("<<", false, false)], "and the plain spelling claims nothing");
    let base = by_name(&image, "Base");
    assert_eq!(
        image.inc(base).iter().filter(|edge| edge.overrides).count(),
        1,
        "the target's inbound run is where a runtime looks for its claimants"
    );
}

// ------------------------------------------------------------------ the gate

#[test]
fn only_the_tag_answers_the_mutability_gate() {
    // `ovg::lib` is public and says nothing about mutability, so it is
    // immutable. Two of the five reaches into it are refused and they are
    // exactly the two that wrote `!ref`: `Amend` and `Forced.refused`. `Take`
    // and `Bare` write `override` alone — a claim of priority among the nodes
    // that hold the target, which is not a mutation and has nothing to ask the
    // axis for.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotWritable), 2, "{rendered}");
    assert!(
        rendered.contains(
            "`override-gate/lib` is `immutable` and `override-gate/patch` is \
             outside it"
        ),
        "the note is the composed one, not a second predicate's: {rendered}"
    );
    for line in ["patch.yfy:15:7", "patch.yfy:17:12"] {
        assert!(!rendered.contains(line), "`override` alone is not a write: {rendered}");
    }
}

#[test]
fn the_refusal_names_the_declaration_that_was_refused() {
    // The gate is `!ref`'s, so the message is `!ref`'s. Naming `override`
    // would send an author to drop the word that is not being refused, and
    // dropping it would leave the write standing.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert!(rendered.contains("`!ref ../lib/Shared` declares"), "{rendered}");
    assert!(!rendered.contains("`override ../lib/Shared` declares"), "{rendered}");
    // And the fix it offers is truthful: what is left after dropping the tag
    // is `extends: override P`, which is legal and asks the axis for nothing.
    assert!(
        rendered.contains("the priority claim `override` makes is not what was refused"),
        "a diagnostic that says what survives is worth more than one that only \
         says what to remove: {rendered}"
    );
}

#[test]
fn brute_forces_an_overriding_write_exactly_as_it_forces_any_other() {
    // `brute` is a prefix on the **member** and `override` one on the
    // **operand**, so the two compose without either learning about the other.
    // Two forced writes: the data position `Forced.claim` and the clause
    // operand `Forceful` writes, which `brute` reaches because `brute` is
    // *required* for `!ref override` into a scope that is not mutable.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::ForcedWrite), 2, "{rendered}");
    assert!(rendered.contains("`brute` forces this write"), "{rendered}");
    assert!(rendered.contains("patch.yfy:27:17"), "the clause operand is forced: {rendered}");
}

#[test]
fn override_never_bypasses_visibility() {
    // Visibility is decided during path resolution, in pass 4, before the
    // final segment is sought (D4.12). A target out of view resolved to
    // nothing long before anything read the keyword, so the answer is `E0216`
    // and there is no reference left for `E0217` to be asked about.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotVisible), 1, "{rendered}");
    assert!(!rendered.contains("../vault/Secret` declares"), "not also E0217: {rendered}");
}

// -------------------------------------------------------------- two claimants

#[test]
fn two_overriding_references_may_not_redefine_one_key_differently() {
    // Nothing ranks two files' claims on a base except their path names
    // (D6.2), and a graph whose values depend on a filename is what D1.8
    // refuses. So it is `E0214`, exactly as an additive disagreement is: the
    // fault and the fix are the same, and a second number would say otherwise.
    let fixture = open("override-conflicting");
    assert_eq!(fixture.count(Code::ConflictingExtension), 1, "{}", fixture.rendered());
    assert!(fixture.rendered().contains("`vessel`"), "{}", fixture.rendered());
}

// -------------------------------------------------------------------- cycles

#[test]
fn override_participates_in_a_cycle_no_differently() {
    // One graph, one cycle rule (D4.10). `override` qualifies what an extended
    // reference installs; it adds no edge and no stratum, so the component is
    // the one the same shape without the word would close.
    let fixture = open("override-cycle");
    assert_eq!(fixture.count(Code::CyclicInheritance), 1, "{}", fixture.rendered());
    assert!(fixture.checked.is_cyclic());
    // D1.8: nothing read off the recovered view is reported, so the keyword's
    // own diagnostics are withheld too rather than fired against a repair.
    assert_eq!(fixture.count(Code::VacuousOverride), 0, "{}", fixture.rendered());
}

// ------------------------------------------------------- what it is not read as

#[test]
fn a_data_position_is_not_made_a_reach_by_the_keyword() {
    // D4.12's asymmetry, applied to the new word. A scalar under `<<` or
    // `extends` has been an operand in every version of this language, so
    // reading a prefix there cannot change the meaning of anything that used to
    // be legal; a scalar in a data position has always been data, and
    // `region: override eu-west` stays a string no matter what the project
    // happens to contain.
    let fixture = open("override-claim");
    let loose = fixture.node("claim.yfy", "Loose");
    assert_eq!(fixture.value_of(loose, "region"), "override eu-west");
    assert_eq!(fixture.value_of(loose, "note"), "override Base", "not a path to `Base`");
    assert_eq!(fixture.value_of(loose, "anchored"), "override ./Base");
    assert!(
        fixture.rendered().is_empty(),
        "and nothing tried to resolve one: {}",
        fixture.rendered()
    );
    // The unresolvable spellings above prove nothing on their own: a prefix
    // read off them lands on no path and so is invisible either way. `./Base`
    // is the one that would land — an anchored path is a reach in a data
    // position — so the edge index is where the rule is actually observable.
    let image = image(&fixture);
    assert!(
        image.out(by_name(&image, "Loose")).is_empty(),
        "reading the prefix here would write a data edge nothing asked for"
    );
}

#[test]
fn quoting_escapes_the_prefix_exactly_as_it_does_for_a_member() {
    // The prefix is read off a **plain** scalar and nothing else — D4.2's
    // escape one level down, and the same one that makes `"pub literal"` a
    // member called `pub literal`. A reader who knows how to write a literal
    // `extends` key already knows how to write this.
    let fixture = open("override-escaped");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::UnresolvedRef), 1, "{rendered}");
    assert!(rendered.contains("`override Base` is not a path"), "{rendered}");
}

#[test]
fn base_yaml_does_not_interpret_the_keyword() {
    // D6.6: a `.yaml` holds no yfi syntax, so it writes no references at all
    // and `<<: override source/Thing` is the ordinary scalar merge source
    // `E0211` refuses. It is emphatically not an inclusion of `Thing`.
    let fixture = open("override-base-yaml");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::IllegalMergeSource), 1, "{rendered}");
    assert_eq!(fixture.count(Code::UnresolvedRef), 0, "no path was resolved: {rendered}");
    let record = fixture.node("source.yfy", "Thing");
    assert_eq!(fixture.resolved_keys(record), ["port"], "and `Thing` gained nothing");
}

#[test]
fn installing_an_override_does_not_republish_what_the_base_merely_includes() {
    // The sharper half of the same rule. `Potion` includes `../mix/Mixin`, so
    // `tint` is the mixin's member addressed through `Potion` and is gated
    // where the mixin is (D4.12's containment row). Folding the base in through
    // *any* relationship — installation included — would re-gate it onto
    // `Potion` and call it descended, which is republishing by accident, on the
    // strength of a keyword written in a third directory.
    let fixture = open("override-redefines");
    let potion = fixture.node("potion.yfy", "Potion");
    let tint = fixture.symbol("tint");
    let field = fixture
        .checked
        .resolved(potion.0, potion.1)
        .expect("a view")
        .get(tint)
        .expect("`tint` arrives through the inclusion");
    assert_eq!(field.acquired, Acquisition::Included, "still the mixin's");
    assert_eq!(
        fixture.project.scopes().qualified(field.reach.scope),
        "override-redefines/mix",
        "and still gated where the mixin is"
    );
}

#[test]
fn the_base_declares_what_the_override_installed_and_not_what_it_wrote() {
    // D4.8 validates a concrete node against each abstract ancestor's
    // **declared** view, never against a flattened one. So the inversion has to
    // reach `declared` as well as `resolved`: a redefinition visible only in
    // the second would leave every descendant of `Potion` checked against a
    // declaration the program no longer has, and `W0301`, `E0220` and `E0221`
    // would all answer from the superseded one.
    let fixture = open("override-redefines");
    let potion = fixture.node("potion.yfy", "Potion");
    let patch = fixture.file("patch.yfy");
    let vessel = fixture.symbol("vessel");
    let declared = fixture.checked.declared(potion.0, potion.1).expect("a declared view");
    let resolved = fixture.checked.resolved(potion.0, potion.1).expect("a resolved view");
    for (name, view) in [("declared", declared), ("resolved", resolved)] {
        let field = view.get(vessel).expect("`vessel`");
        assert_eq!(field.key.0, patch, "{name} takes `vessel` from the patch");
        assert_eq!(field.acquired, Acquisition::Installed, "{name}");
    }
}

// ------------------------------------- priority without the mutation declaration

#[test]
fn extends_override_is_legal_with_no_ref_beside_it() {
    // The two declarations are orthogonal. `ovp::base` says nothing about
    // mutability, so it is immutable and any `!ref` into it is `E0217`; a
    // priority claim is not a mutation, so this project is clean.
    let fixture = open("override-priority");
    assert_eq!(fixture.count(Code::RefNotWritable), 0, "{}", fixture.rendered());
    assert!(fixture.rendered().is_empty(), "{}", fixture.rendered());
}

#[test]
fn a_priority_claim_writes_nothing_to_the_node_it_claims() {
    // No global write: `Potion` holds exactly what `Potion` wrote. Only
    // `extends: !ref` installs (D4.3), and `override` qualifies a contribution
    // rather than creating one.
    let fixture = open("override-priority");
    let potion = fixture.node("potion.yfy", "Potion");
    assert_eq!(fixture.value_of(potion, "vessel"), "vial");
    assert_eq!(fixture.resolved_keys(potion), ["vessel"]);
    assert_eq!(fixture.declared_keys(potion), ["vessel"]);
}

#[test]
fn a_priority_claim_changes_nothing_another_node_sees() {
    // `Draught` names neither the patch nor anything it claims, and is a
    // `Potion`. If a bare `override` had any blast radius at all it would land
    // here, because that is where an `extends: !ref override` one lands.
    let fixture = open("override-priority");
    let draught = fixture.node("use.yfy", "Draught");
    assert_eq!(fixture.value_of(draught, "vessel"), "vial");
}

#[test]
fn the_bare_form_is_the_ordinary_operation_plus_a_recorded_claim() {
    // The local instance, and nothing else moved: `Claimant` and `Ordinary`
    // differ by one word in the source and by nothing in the result. The
    // keyword inherits `extends`'s blast radius, which is this node.
    let fixture = open("override-priority");
    let claimant = fixture.node("patch.yfy", "Claimant");
    let ordinary = fixture.node("patch.yfy", "Ordinary");
    assert_eq!(fixture.resolved_keys(claimant), fixture.resolved_keys(ordinary));
    assert_eq!(fixture.value_of(claimant, "vessel"), "flask");
    assert_eq!(fixture.value_of(claimant, "vessel"), fixture.value_of(ordinary, "vessel"));
}

#[test]
fn the_claim_rides_the_extension_edge_and_carries_no_capability() {
    // What the compiler does with a priority claim is record it, gate what
    // needs gating and emit it — never execute it; there is no runtime here.
    // So the only trace is a flag on the edge it qualifies, and `capability`
    // beside it stays false because no `!ref` was written.
    let fixture = open("override-priority");
    let image = image(&fixture);
    let read = |name: &str| -> Vec<(&'static str, bool, bool)> {
        image
            .out(by_name(&image, name))
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Extension)
            .map(|edge| (edge.kind.as_str(), edge.capability, edge.overrides))
            .collect()
    };
    assert_eq!(read("Claimant"), [("extends", false, true)]);
    assert_eq!(read("Ordinary"), [("extends", false, false)]);
}

#[test]
fn a_bare_override_contributes_nothing_so_neither_warning_fires() {
    // `Extra` writes `vesel`, which the base does not hold — the exact shape
    // `W0305` exists for. It is silent because there is no contribution: the
    // keyword ranks holders, and only the tag installs keys.
    let fixture = open("override-priority");
    assert_eq!(fixture.count(Code::VacuousOverride), 0, "{}", fixture.rendered());
    assert_eq!(fixture.count(Code::InertContribution), 0, "{}", fixture.rendered());
    let potion = fixture.node("potion.yfy", "Potion");
    assert!(!fixture.resolved_keys(potion).contains(&"vesel".to_owned()));
}
