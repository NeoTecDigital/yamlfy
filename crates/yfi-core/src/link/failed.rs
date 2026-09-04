// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a path that did not land is reported as.
//!
//! Split from [`super::refs`], which decides *where* a path is read and *what*
//! it resolves to. This is the table of answers for when it resolves to
//! nothing, and the table is the interesting part: five failures are `E0213`
//! because the walk did not land, a member miss is `E0218` because the fix is
//! the field name rather than the address, and an invisible landing is `E0216`
//! because the gate stands in front of the lookup (D4.12).

use yfi_syntax::{Code, Diagnostic, Span};

use super::path::Failure;

/// The diagnostic a failed resolution earns. Every failure is `E0213` — the
/// path named nothing — except a member miss, which is `E0218` (the path landed
/// and the member did not, and the two have different fixes), and a landing the
/// referencing scope cannot see, which is `E0216`.
///
/// `E0216`'s note carries **no span**. Every other note here points at
/// something the author can read; that one would point inside the scope the
/// gate just refused, which is the disclosure the gate exists to prevent.
pub(super) fn failed(text: &str, span: Span, failure: &Failure) -> Diagnostic {
    let (code, message, note) = explain(text, failure);
    Diagnostic::new(code, span, message).with_note(note, None)
}

/// The code, the message and the note one failure earns. Split out because the
/// table is the interesting part and a reader comparing two rows should not
/// have to step over the diagnostic plumbing to do it.
fn explain(text: &str, failure: &Failure) -> (Code, String, String) {
    match failure {
        Failure::AboveRoot => (
            Code::UnresolvedRef,
            format!("`{text}` ascends past the project root"),
            "`..` walks up the scope tree the way it walks up directories, and the root has no \
             parent"
                .to_owned(),
        ),
        Failure::NoSegment(segment) => (
            Code::UnresolvedRef,
            format!("`{text}` names nothing: there is no `{segment}` here"),
            "a segment names a directory of this project or a `.yfy` beside the file that wrote \
             the path"
                .to_owned(),
        ),
        Failure::NotADirectory(segment) => (
            Code::UnresolvedRef,
            format!("`{text}` looks for `{segment}` inside a file"),
            "a file holds definitions, not directories; address a member with `.` instead"
                .to_owned(),
        ),
        Failure::NoDefinition(name, at) => (
            Code::UnresolvedRef,
            format!("`{text}` names nothing: no definition called `{name}` in `{at}`"),
            "only an anchored collection is addressable; an anchored scalar is a value, not a \
             type"
                .to_owned(),
        ),
        Failure::BindingCycle => (
            Code::UnresolvedRef,
            format!("`{text}` resolves through a `!ref` binding that resolves back to itself"),
            "a binding names a target, so a binding that names itself names nothing".to_owned(),
        ),
        Failure::NotAPath => (
            Code::UnresolvedRef,
            format!("`{text}` is not a path"),
            "a path is written `../dir/Name`, `peer/Name`, `Name` or `Name.member`; `!ref` takes \
             one and nothing else"
                .to_owned(),
        ),
        Failure::NotVisible(blocker, observer) => (
            Code::RefNotVisible,
            format!(
                "`{text}` names a definition this scope cannot see; the path grants the reach, \
                 and `private` decides that you may not have it"
            ),
            format!(
                "`{blocker}` is `private` and `{observer}` is outside it; both axes compose \
                 over the whole path from the root"
            ),
        ),
        Failure::NoMember(name) => (
            Code::UnresolvedMember,
            format!("`{text}` addresses `{name}`, which the node it names does not hold"),
            "member access reads the keys the target writes; a key it inherits is not addressable \
             until it is written"
                .to_owned(),
        ),
    }
}
