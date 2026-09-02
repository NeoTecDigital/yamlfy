// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inheritance clauses, and whether their operands are legal (`E0211`).
//!
//! Two clauses exist and they are different operations (D4.1):
//!
//! * `<<:` — **inclusion**. A has B as one of its members. B is unchanged and
//!   nothing else in the program observes anything. Governed in **both** file
//!   classes, because merge is YAML's and not ours (D6.6).
//! * `extends:` — **extension** with an alias or an inline mapping, and an
//!   **extended reference** with a `!ref`. The operand selects the operation,
//!   and only the `!ref` form reaches back into the base. Interpreted only in
//!   Yamlfication source.
//!
//! # Legal operands
//!
//! D1.6, as extended by D4.3: a mapping, an alias to one, a **path** resolving
//! to one, or a **flat** sequence whose every element is one of those three. A
//! path may be written plain or carry `!ref`; the tag changes what the clause
//! *does*, not whether the operand is legal. Anything else — a scalar that is
//! not a path, a nested sequence, an alias to either — is `E0211`. A path
//! resolving to nothing is `E0213` and is *not* also `E0211`: it has already
//! been reported once, and a second code about the same token would send the
//! author looking for a second fault.
//!
//! An `extends` entry whose operand is illegal is reported, never
//! reinterpreted. Treating it as an ordinary field instead would let a mistake
//! in the value silently decide whether the key is an operation, producing a
//! node that quietly stopped inheriting with nothing pointing at it.

use yfi_syntax::{Ast, Code, Diagnostic, Diagnostics, FileId, NodeId, Span};

use super::keys::is_extends_key;
use super::refs::References;
use super::Ctx;

/// Which operation a clause is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClauseKind {
    /// `<<:` — inclusion. Compositional, not definitional: a node that includes
    /// `water` is not a water.
    Inclusion,
    /// `extends:` — extension, or an extended reference when the operand is a
    /// `!ref`.
    Extension,
}

/// How an operand was written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OperandForm {
    /// `*name` — document-local, after imports (D6.7) are installed.
    Alias,
    /// An inline mapping written in place.
    Inline,
    /// A plain path — `../shared/Service`, `peer/Service`, `Service`. The
    /// reach itself, read-only.
    Path,
    /// `!ref` on a path. The same target, plus the declaration that this
    /// operand intends to modify it.
    Ref,
}

/// One legal operand of a clause.
pub struct Operand {
    /// The operand node as written, which is what a diagnostic points at.
    pub node: NodeId,
    /// How it was written. Together with the clause kind this decides the
    /// operation: `extends` plus [`OperandForm::Ref`] is the only spelling that
    /// installs a reverse edge.
    pub form: OperandForm,
    /// The mapping it names.
    pub target: (FileId, NodeId),
    /// The operand's span.
    pub span: Span,
}

/// One `<<` or `extends` entry.
pub struct Clause {
    /// The file it is written in.
    pub file: FileId,
    /// The mapping that writes it — `A` in `A << B`.
    pub owner: NodeId,
    /// Which operation it is.
    pub kind: ClauseKind,
    /// The clause key's span, which names the operator rather than an operand.
    pub site: Span,
    /// The operands that survived validation, in written order.
    pub operands: Vec<Operand>,
}

/// Collect every clause of the project, reporting `E0211` for each illegal
/// operand and keeping the legal ones.
pub(crate) fn collect(ctx: &Ctx, refs: &References, diagnostics: &mut Diagnostics) -> Vec<Clause> {
    let mut out = Vec::new();
    for file in ctx.project.files() {
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            in_mapping(ctx, refs, file.id, node, &mut out, diagnostics);
        }
    }
    out
}

fn in_mapping(
    ctx: &Ctx,
    refs: &References,
    file: FileId,
    owner: NodeId,
    out: &mut Vec<Clause>,
    diagnostics: &mut Diagnostics,
) {
    let ast = ctx.ast(file).expect("a discovered file has an arena");
    let Some(entries) = ast.entries(owner) else { return };
    let source = ctx.is_source(file);
    for entry in entries {
        let kind = if entry.merge {
            ClauseKind::Inclusion
        } else if source && is_extends_key(ast, entry.key) {
            ClauseKind::Extension
        } else {
            continue;
        };
        let operands = operands(ctx, refs, file, kind, entry.value, diagnostics);
        out.push(Clause { file, owner, kind, site: ast.node(entry.key).span, operands });
    }
}

/// Read a clause's operands, expanding the flat sequence form.
fn operands(
    ctx: &Ctx,
    refs: &References,
    file: FileId,
    kind: ClauseKind,
    value: NodeId,
    diagnostics: &mut Diagnostics,
) -> Vec<Operand> {
    let ast = ctx.ast(file).expect("a discovered file has an arena");
    let Some(items) = ast.items(value) else {
        return one(ctx, refs, file, kind, value, diagnostics).into_iter().collect();
    };
    items.iter().filter_map(|item| one(ctx, refs, file, kind, *item, diagnostics)).collect()
}

fn one(
    ctx: &Ctx,
    refs: &References,
    file: FileId,
    kind: ClauseKind,
    node: NodeId,
    diagnostics: &mut Diagnostics,
) -> Option<Operand> {
    let ast = ctx.ast(file).expect("a discovered file has an arena");
    let span = ast.node(node).span;
    if ast.entries(node).is_some() {
        return Some(Operand { node, form: OperandForm::Inline, target: (file, node), span });
    }
    if ast.alias(node).is_some() {
        return through_alias(ctx, file, kind, node, span, diagnostics);
    }
    if let Some(reference) = refs.get(file, node) {
        let target = reference.target?;
        let form = if reference.capability { OperandForm::Ref } else { OperandForm::Path };
        return accept(ctx, kind, node, span, form, target, diagnostics);
    }
    // Reached only for a directly written operand and for an element of the
    // flat sequence form, so a sequence here is the nested-sequence case.
    let found = if ast.items(node).is_some() {
        "a sequence, and a merge sequence must be flat".to_owned()
    } else {
        describe(ast, node)
    };
    diagnostics.push(illegal(kind, span, found));
    None
}

fn through_alias(
    ctx: &Ctx,
    file: FileId,
    kind: ClauseKind,
    node: NodeId,
    span: Span,
    diagnostics: &mut Diagnostics,
) -> Option<Operand> {
    let ast = ctx.ast(file).expect("a discovered file has an arena");
    // An alias that bound to nothing is already `E0100` or `E0130` from the
    // parse. Reporting it again as an illegal source would name a second fault
    // that does not exist.
    let target = ast.alias_binding(node)?;
    accept(ctx, kind, node, span, OperandForm::Alias, target, diagnostics)
}

/// Accept an operand that names `target`, or report what it named instead.
fn accept(
    ctx: &Ctx,
    kind: ClauseKind,
    node: NodeId,
    span: Span,
    form: OperandForm,
    target: (FileId, NodeId),
    diagnostics: &mut Diagnostics,
) -> Option<Operand> {
    let named = ctx.ast(target.0)?;
    if named.entries(target.1).is_none() {
        let what = describe(named, target.1);
        let through = match form {
            OperandForm::Ref => "a `!ref` to",
            OperandForm::Path => "a path to",
            _ => "an alias to",
        };
        diagnostics.push(illegal(kind, span, format!("{through} {what}")));
        return None;
    }
    Some(Operand { node, form, target, span })
}

fn illegal(kind: ClauseKind, span: Span, found: String) -> Diagnostic {
    let operator = match kind {
        ClauseKind::Inclusion => "a merge source",
        ClauseKind::Extension => "an `extends` operand",
    };
    Diagnostic::new(
        Code::IllegalMergeSource,
        span,
        format!(
            "{operator} must be a mapping, an alias to one, a path resolving to one, or a \
             flat sequence of those; this is {found}"
        ),
    )
}

fn describe(ast: &Ast, node: NodeId) -> String {
    if node.index() >= ast.nodes().len() {
        return "nothing".to_owned();
    }
    if ast.entries(node).is_some() {
        return "a mapping".to_owned();
    }
    if ast.items(node).is_some() {
        return "a sequence".to_owned();
    }
    if ast.alias(node).is_some() {
        return "an alias".to_owned();
    }
    "a scalar".to_owned()
}
