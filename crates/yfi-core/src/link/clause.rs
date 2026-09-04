// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inheritance clauses, and whether their operands are legal (`E0211`).
//!
//! Two clauses, two operations (D4.1). `<<:` is inclusion and is governed in
//! **both** file classes, because merge is YAML's and not ours (D6.6);
//! `extends:` is extension, or an extended reference when the operand carries
//! `!ref`, and is interpreted only in Yamlfication source.
//!
//! Legal operands are D1.6's as extended by D4.3, and the `!ref` tag changes
//! what the clause *does* rather than whether the operand is legal. Two rules
//! about the reporting: a path resolving to nothing is `E0213` and is *not*
//! also `E0211`, because a second code about one token sends the author looking
//! for a second fault; and an illegal operand is reported, never reinterpreted
//! as an ordinary field, which would let a mistake in the value silently decide
//! whether the key is an operation at all.

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
    /// Whether the operand was written `override` (D4.14). It qualifies the
    /// clause rather than replacing it: under `extends: !ref` the contribution
    /// **outranks** the base instead of ranking below it, and under `<<` it is
    /// a runtime claim that moves no compile-time value at all.
    pub overrides: bool,
    /// The mapping it names.
    pub target: (FileId, NodeId),
    /// The operand's span.
    pub span: Span,
}

/// An operand's written form, before it is known to name a mapping.
#[derive(Clone, Copy)]
struct Written {
    node: NodeId,
    form: OperandForm,
    overrides: bool,
    span: Span,
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
        let form = OperandForm::Inline;
        return Some(Operand { node, form, overrides: false, target: (file, node), span });
    }
    if ast.alias(node).is_some() {
        return through_alias(ctx, file, kind, node, span, diagnostics);
    }
    if let Some(reference) = refs.get(file, node) {
        let target = reference.target?;
        let form = if reference.capability { OperandForm::Ref } else { OperandForm::Path };
        let written = Written { node, form, overrides: reference.overrides, span };
        return accept(ctx, kind, written, target, diagnostics);
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
    //
    // A cross-document alias is refused as well as unreported. The parser still
    // records the binding it found, so accepting it here built the `is_a` edge
    // and every rule downstream then answered against an ancestry the language
    // had already said does not exist: `extends: *Base` across a document
    // boundary earned `E0130` and then a required field of the base reported
    // unsatisfied -- the consequence printed above the cause, blaming a base
    // the compiler had just refused to let the node name. An operand the parse
    // rejected is not an operand.
    if ast.alias(node).is_some_and(|held| held.cross_document) {
        return None;
    }
    let target = ast.alias_binding(node)?;
    let written = Written { node, form: OperandForm::Alias, overrides: false, span };
    accept(ctx, kind, written, target, diagnostics)
}

/// Accept an operand that names `target`, or report what it named instead.
fn accept(
    ctx: &Ctx,
    kind: ClauseKind,
    written: Written,
    target: (FileId, NodeId),
    diagnostics: &mut Diagnostics,
) -> Option<Operand> {
    let named = ctx.ast(target.0)?;
    let Written { node, form, overrides, span } = written;
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
    Some(Operand { node, form, overrides, target, span })
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
