// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `override` — the keyword that replaces rather than defers (D4.14).
//!
//! Two spellings and two entirely different times. `extends: !ref override P`
//! is a **compile-time redefinition**: the contribution outranks the base's own
//! keys instead of ranking below them, so every node that is a `P` and every
//! node that merely includes `P` sees the new value. `<<: override P` is a
//! **runtime claim** and moves nothing at all: the resolved views either side
//! of it are byte-identical to `<<: P`'s, and what the compiler does is record
//! the claim, gate it, and emit it.
//!
//! Both are writes, so both answer the mutability gate `!ref` already answers,
//! with the same `E0217` and the same `W0304` when a `brute` member forces it.
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
    assert_eq!(read(claimed), [("<<", true, true)], "the claim, on the inclusion it qualifies");
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
fn neither_spelling_applies_without_mut_on_the_target() {
    // Both forms are writes, and the predicate is the composed one `!ref`
    // already uses — not a second one. `ovg::lib` is public and says nothing
    // about mutability, so it is immutable and every unforced write is
    // refused.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotWritable), 4, "{rendered}");
    assert!(
        rendered.contains(
            "`override-gate/lib` is `immutable` and `override-gate/patch` is \
             outside it"
        ),
        "the note is the composed one, not a second predicate's: {rendered}"
    );
    // The message names the word the author wrote. `<<: override P` carries no
    // `!ref`, and telling them to drop one sends them looking for a token that
    // is not there.
    assert!(rendered.contains("`override ../lib/Shared` declares"), "{rendered}");
    assert!(!rendered.contains("`!ref ../lib/Shared` declares"), "{rendered}");
    // `Bare` writes `extends: override P` with no `!ref` beside it and earns
    // the same refusal as `Amend`, which writes both: the keyword declares
    // more than the tag does, so it entails it, and the two are one operation.
    assert!(rendered.contains("patch.yfy:18:12"), "the bare spelling is a write too: {rendered}");
}

#[test]
fn brute_forces_an_overriding_write_exactly_as_it_forces_any_other() {
    // `brute` is a prefix on the **member** and `override` one on the
    // **operand**, so the two compose without either learning about the other.
    let fixture = open("override-gate");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::ForcedWrite), 1, "{rendered}");
    assert!(rendered.contains("`brute` forces this write"), "{rendered}");
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
    assert!(
        fixture.rendered().is_empty(),
        "and nothing tried to resolve one: {}",
        fixture.rendered()
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
