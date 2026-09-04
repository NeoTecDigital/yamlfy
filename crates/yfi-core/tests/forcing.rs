// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! `brute` — the one act that overrides a refusal, and how far it reaches
//! (D6.4b).
//!
//! Split out of `access.rs`, which owns the two axes themselves: forcing is a
//! third question asked of the second axis and only of it, and the file that
//! answers it grew past the limit that keeps a file readable.
//!
//! **The reach is one level and is never transitive.** A `brute` member forces
//! the `!ref`s written in that member's own value — the value itself when it is
//! one, and the operands of the clauses that value writes. Nothing further
//! down: clauses are not members, so a member of that value keeps its own
//! refusal until its own key says otherwise. The author who writes `brute` sees
//! the block it governs; a `brute` that reached past it would silence a refusal
//! they never read.
//!
//! And it forces the mutability gate alone. `brute` applies only within the
//! scope it can already see, so it can never be used to find out whether a
//! private scope holds a name.

mod common;

use common::pipeline::open;
use yfi_syntax::Code;

#[test]
fn brute_forces_the_write_the_mutability_axis_refused() {
    // `brute::lib` is public and says nothing about mutability, so it is
    // immutable by default. Two `!ref`s into it from the same node: the bare
    // member is refused, the `brute` member performs the same write. The write
    // stands and the forcing is recorded, which is the whole of the bargain —
    // forcing is allowed and it is never quiet.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::ForcedWrite), 3, "{rendered}");
    assert_eq!(
        fixture.count(Code::RefNotWritable),
        3,
        "the bare member is still refused:\n{rendered}"
    );
    assert!(
        rendered.contains(
            "`brute` forces this write: `../lib/Shared` names a target that may \
                           not be written from here, and the write is performed anyway"
        ),
        "{rendered}"
    );
}

#[test]
fn the_record_of_a_forced_write_names_the_scope_that_refused_it() {
    // W0304 carries the same composed note E0217 does, so a reader learns which
    // scope was overridden and where it said so — a forcing whose target is
    // unnamed would be a worse record than no record.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert!(
        rendered.contains("`check-brute/lib` is `immutable` and `check-brute/force` is outside it"),
        "{rendered}"
    );
}

#[test]
fn brute_never_forces_visibility() {
    // The asymmetry is the point. Mutability is a policy about what may be
    // changed, and an author can be entitled to override a policy in the open.
    // Visibility is not a policy: `E0216` says you may not have this at all,
    // and a member cannot grant itself sight of what it was never shown. The
    // `brute` member reaching into a private scope is refused exactly as a bare
    // one would be, and nothing is recorded as forced.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotVisible), 2, "{rendered}");
    assert!(
        !rendered.lines().any(|line| line.contains("blind.yfy") && line.contains("W0304")),
        "a forced write must never be recorded for a reach that was never visible:\n{rendered}"
    );
}

#[test]
fn a_forced_write_is_a_warning_and_does_not_fail_a_build_by_itself() {
    // W0304 defaults to a warning: the write was asked for explicitly and
    // performed. A project that wants forcing to be fatal denies the code,
    // which is what the severity map is for.
    assert_eq!(Code::ForcedWrite.default_severity(), yfi_syntax::Severity::Warning);
}

#[test]
fn brute_forces_a_clause_operand_the_member_below_it_writes() {
    // `extends:` is an operator, not a member, so a clause operand binds no
    // key of its own and there is no member on it for `brute` to sit on. The
    // member that declares it is one level up — the one whose value writes the
    // clause — and that is the one that forces it. Without this, `brute` is
    // required for `!ref override` into an immutable scope and unreachable
    // there, which is a rule with no spelling.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert!(rendered.contains("force.yfy:22:17"), "the document-root form: {rendered}");
    assert!(rendered.contains("force.yfy:27:19"), "the nested member form: {rendered}");
    for line in ["force.yfy:22:17", "force.yfy:27:19"] {
        let forced = rendered
            .lines()
            .find(|held| held.contains(line))
            .unwrap_or_else(|| panic!("nothing reported at {line}:\n{rendered}"));
        assert!(forced.contains("W0304"), "{forced}");
    }
}

#[test]
fn brute_does_not_descend_past_the_member_it_is_written_on() {
    // One level, and never transitive. `Deep.outer` is `brute`; the clause is
    // written by a member of `outer`'s value rather than by that value, so the
    // refusal stands. A `brute` that reached further than the author expects is
    // worse than one that reaches too little: it silences a refusal nobody
    // asked it to silence, in a block nobody read.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    let deep = rendered
        .lines()
        .find(|held| held.contains("force.yfy:34:21"))
        .unwrap_or_else(|| panic!("nothing reported for the nested clause:\n{rendered}"));
    assert!(deep.contains("E0217"), "the refusal stands two levels down: {deep}");
}

#[test]
fn brute_never_forces_visibility_from_a_clause_either() {
    // The new reach changes nothing about the axis it never touched. `brute`
    // applies only within the scope it can already see, so it can never be
    // used to learn whether a private scope holds a name: the refusal has the
    // same shape whether the target is there or not.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert_eq!(fixture.count(Code::RefNotVisible), 2, "{rendered}");
    assert!(
        !rendered.lines().any(|line| line.contains("blind.yfy") && line.contains("W0304")),
        "nothing is recorded as forced for a reach that was never visible:\n{rendered}"
    );
}

#[test]
fn a_brute_that_forces_nothing_is_silent() {
    // `brute::open` grants the write, so `Vacuous.allowed` forces nothing.
    // Forcing is recorded **where it takes effect**, and it does not take
    // effect here — so there is no `W0304`, and no `E0217` either, because
    // nothing refused. The compiler has no opinion about a `brute` that was
    // not needed.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    assert!(
        !rendered.contains("../open/Open"),
        "a brute with nothing to force is not a finding:\n{rendered}"
    );
}

#[test]
fn quoting_escapes_the_brute_prefix_as_it_escapes_every_other() {
    // D4.2's escape, one level down: a flag prefix is read off a **plain**
    // scalar and nothing else, so `"brute quoted"` is a member genuinely
    // called `brute quoted` and forces nothing. Reading the prefix off a
    // quoted key would make the one escape the language offers stop working
    // at exactly the word where forcing is decided.
    let fixture = open("check-brute");
    let rendered = fixture.rendered();
    let quoted = rendered
        .lines()
        .find(|held| held.contains("force.yfy:17:"))
        .unwrap_or_else(|| panic!("nothing reported for the quoted member:\n{rendered}"));
    assert!(quoted.contains("E0217"), "a quoted prefix is a name, not a flag: {quoted}");
}
