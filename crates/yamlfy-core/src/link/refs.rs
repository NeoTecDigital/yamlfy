// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Path references: where one is written, what it names, and what `!ref` adds.
//!
//! **The path is the reach** (D4.12). A path resolves against the project's
//! directories and files, it needs no `imports:` entry to precede it, and
//! writing it is what performs the reach. Visibility still decides whether the
//! reach is *permitted* — that is `check`'s `E0216` — but nothing has to be
//! declared twice for a path to mean something.
//!
//! # Where a plain scalar is read as a path
//!
//! | position | read as a path when |
//! |---|---|
//! | operand of `<<:` or `extends:` | it parses as a path at all |
//! | anywhere else (a data edge) | it parses **and** was written `./…` or `../…` |
//!
//! The asymmetry is exact rather than a matter of taste. A scalar under `<<:`
//! or `extends:` was `E0211` in every previous version of the language, so
//! reading it as a path cannot change the meaning of anything that used to be
//! legal. A scalar in a data position has always been data, so there the reach
//! must be *anchored* — a leading `./` or `../` — and `region: eu-west` stays a
//! string no matter what the project happens to contain.
//!
//! # What `!ref` adds
//!
//! `!ref` is not the way to write a reference; a plain path is. `!ref` is a
//! **declaration of intent**, and it is legal wherever a path is:
//!
//! * **mutation** — the target must be writable from here, which `check`
//!   enforces as `E0217`. An extended reference is a write performed at compile
//!   time, so the mutability axis is a real gate rather than a record;
//! * **dependency** — the target depends on this context, the same direction
//!   `extends: !ref` establishes, and therefore the same reverse edge into
//!   `own(A)` the graph already builds;
//! * **access** — written at a mapping entry it binds that key as a name
//!   carrying the capability, so `service.member_one` addresses into it. Access
//!   is granted to *that member*, not to the file, which is D4.12's rule that
//!   access is a relationship rather than a flag.
//!
//! A plain path binds nothing. `service: ../core/Service` is a data edge and
//! `service.member_one` elsewhere in the document will not find it: only a
//! `!ref` establishes the capability that member access addresses through.
//!
//! In a base YAML file `!ref` is an unrecognised tag on a value and nothing
//! else (D6.6), so pass 3 classifies it as [`TagKind::Other`] there and this
//! pass never sees it.

use std::collections::HashMap;

use yamlfy_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId, ScalarStyle, Span};

use super::keys::is_extends_key;
use super::path::{self, Failure, Path, Space};
use super::table::Table;
use super::Ctx;
use crate::tags::TagKind;

/// Which operator a reference is an operand of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefRole {
    /// `<<:` — inclusion. Safe: it absorbs the referent's keys and changes
    /// nothing about the referent.
    Inclusion,
    /// `extends:` — extension, or an extended reference when written `!ref`.
    Extension,
    /// Anywhere else — a data edge.
    Data,
}

impl RefRole {
    /// The spelling used in a diagnostic.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RefRole::Inclusion => "<<",
            RefRole::Extension => "extends",
            RefRole::Data => "data",
        }
    }
}

/// One path reference.
pub struct Reference {
    /// The file it is written in.
    pub file: FileId,
    /// The scalar node holding the path.
    pub node: NodeId,
    /// The path exactly as written, which is what a diagnostic quotes.
    pub text: Box<str>,
    /// The parsed path, or `None` for a `!ref` whose operand is not a path at
    /// all — which is still a reference, and still has to be reported.
    pub path: Option<Path>,
    /// What operator it is an operand of.
    pub role: RefRole,
    /// Whether it was written `!ref`: mutation intent, a reverse dependency
    /// edge, and a capability bound to the key it sits under.
    pub capability: bool,
    /// The key this reference binds, when it is a `!ref` written as the value
    /// of a mapping entry. `None` for a plain path and for a sequence element.
    pub binds: Option<Box<str>>,
    /// The mapping the reference was written in — the context a `!ref` declares
    /// its target to depend on, and therefore where the reverse edge lands.
    pub owner: Option<NodeId>,
    /// The node it names, or `None` when it resolved to nothing.
    pub target: Option<(FileId, NodeId)>,
    /// The reference's span.
    pub span: Span,
}

/// Every path reference of the project, indexed by the node that wrote it.
pub(crate) struct References {
    items: Vec<Reference>,
    index: HashMap<(FileId, NodeId), usize>,
}

impl References {
    /// The reference written at `node`, if that node is one.
    pub(crate) fn get(&self, file: FileId, node: NodeId) -> Option<&Reference> {
        self.index.get(&(file, node)).map(|at| &self.items[*at])
    }

    /// How many references the project holds.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// Give up the collection once indexing is no longer needed.
    pub(crate) fn into_items(self) -> Vec<Reference> {
        self.items
    }
}

/// One reference before it is resolved.
struct Occurrence {
    file: FileId,
    node: NodeId,
    document: u32,
    text: Box<str>,
    path: Option<Path>,
    role: RefRole,
    capability: bool,
    binds: Option<Box<str>>,
    owner: Option<NodeId>,
    span: Span,
}

/// Resolution state: the memo, and the stack that makes a binding cycle a
/// failure rather than a hang.
struct Pass<'a> {
    ctx: &'a Ctx<'a>,
    space: &'a Space,
    table: &'a Table,
    occurrences: &'a [Occurrence],
    bindings: HashMap<(FileId, u32, Box<str>), usize>,
    memo: Vec<Option<Result<(FileId, NodeId), Failure>>>,
    visiting: Vec<usize>,
}

/// Resolve every path reference in the project, reporting `E0213` for each that
/// names nothing and `E0218` for each that names a member its target does not
/// hold.
pub(crate) fn resolve(
    ctx: &Ctx,
    table: &Table,
    space: &Space,
    diagnostics: &mut Diagnostics,
) -> References {
    let occurrences = collect(ctx);
    let mut pass = Pass {
        ctx,
        space,
        table,
        occurrences: &occurrences,
        bindings: bindings(&occurrences),
        memo: vec![None; occurrences.len()],
        visiting: Vec::new(),
    };
    let outcomes: Vec<Result<(FileId, NodeId), Failure>> =
        (0..occurrences.len()).map(|at| pass.resolve_at(at)).collect();
    let mut refs = References { items: Vec::new(), index: HashMap::new() };
    for (held, outcome) in occurrences.into_iter().zip(outcomes) {
        if let Err(failure) = &outcome {
            diagnostics.push(failed(&held, failure));
        }
        refs.index.insert((held.file, held.node), refs.items.len());
        refs.items.push(Reference {
            file: held.file,
            node: held.node,
            text: held.text,
            path: held.path,
            role: held.role,
            capability: held.capability,
            binds: held.binds,
            owner: held.owner,
            target: outcome.ok(),
            span: held.span,
        });
    }
    refs
}

/// The `!ref` bindings of each document, keyed by the name they bind. A name
/// bound twice in one document is a state sequence and the last state is what
/// the name denotes (D5.2), so the later entry replaces the earlier.
fn bindings(occurrences: &[Occurrence]) -> HashMap<(FileId, u32, Box<str>), usize> {
    let mut out = HashMap::new();
    for (at, held) in occurrences.iter().enumerate() {
        if let (true, Some(name)) = (held.capability, held.binds.as_ref()) {
            out.insert((held.file, held.document, name.clone()), at);
        }
    }
    out
}

impl Pass<'_> {
    /// Resolve occurrence `at`, memoized, with a binding cycle reported rather
    /// than followed.
    fn resolve_at(&mut self, at: usize) -> Result<(FileId, NodeId), Failure> {
        if let Some(done) = &self.memo[at] {
            return done.clone();
        }
        if self.visiting.contains(&at) {
            return Err(Failure::BindingCycle);
        }
        self.visiting.push(at);
        let outcome = self.one(at);
        self.visiting.pop();
        self.memo[at] = Some(outcome.clone());
        outcome
    }

    fn one(&mut self, at: usize) -> Result<(FileId, NodeId), Failure> {
        let held = &self.occurrences[at];
        let Some(path) = held.path.as_ref() else { return Err(Failure::NotAPath) };
        let Some(through) = self.binding_root(held) else {
            return path::resolve(self.ctx, self.space, self.table, held.file, path);
        };
        let base = self.resolve_at(through)?;
        let held = &self.occurrences[at];
        let members = held.path.as_ref().map_or(&[][..], |path| &path.members);
        path::members(self.ctx, base, members)
    }

    /// The binding a bare path roots at, if it roots at one. Only the bare
    /// one-segment form may: an anchored or multi-segment path is addressing
    /// the tree, and letting a local name capture it would make adding a `!ref`
    /// silently redirect a path that names a directory.
    fn binding_root(&self, held: &Occurrence) -> Option<usize> {
        let path = held.path.as_ref().filter(|path| path.is_bare())?;
        let name = path.segments.first()?;
        let at = *self.bindings.get(&(held.file, held.document, name.clone()))?;
        (self.occurrences[at].node != held.node).then_some(at)
    }
}

/// Every position in the project that holds a path.
fn collect(ctx: &Ctx) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for file in ctx.project.files() {
        if !ctx.is_source(file.id) {
            continue;
        }
        let header = file.header.as_ref().and_then(|h| ctx.interned.document_of(file.id, h.node));
        for position in 0..file.ast.nodes().len() {
            let node = NodeId(u32::try_from(position).expect("arena overflow"));
            let document = ctx.interned.document_of(file.id, node);
            if document.is_some() && document == header {
                continue;
            }
            if let Some(found) = one(ctx, file.id, node, document.unwrap_or_default()) {
                out.push(found);
            }
        }
    }
    out
}

/// Read `node` as a path occurrence, or `None` if it is not one.
fn one(ctx: &Ctx, file: FileId, node: NodeId, document: u32) -> Option<Occurrence> {
    let ast = ctx.ast(file)?;
    let capability = ctx.interned.tag_kind(file, node) == Some(TagKind::Ref);
    let scalar = ast.scalar(node)?;
    if !capability && (scalar.style != ScalarStyle::Plain || ast.tag(node).is_some()) {
        return None;
    }
    let Site { role, binds, owner } = site(ctx, file, node)?;
    let text: Box<str> = scalar.value.trim().into();
    // An untagged scalar in a data position is data unless the path is anchored;
    // a `!ref` that is not a path at all is still an occurrence, because it
    // declared an intent and named nothing, and silence there would lose it.
    let path = match path::parse(&text) {
        Some(path) if capability || role != RefRole::Data || path.anchored => Some(path),
        _ if capability => None,
        _ => return None,
    };
    let span = ast.node(node).span;
    Some(Occurrence { file, node, document, text, path, role, capability, binds, owner, span })
}

/// Where a reference sits.
struct Site {
    role: RefRole,
    binds: Option<Box<str>>,
    owner: Option<NodeId>,
}

/// Which operator `node` is an operand of, the key it would bind, and the
/// mapping that wrote it.
///
/// The operand may be written directly or as an element of the flat sequence
/// form, so one level of sequence is stepped through before the entry is found.
/// A mapping **key** is never a path: it names a field.
fn site(ctx: &Ctx, file: FileId, node: NodeId) -> Option<Site> {
    let loose = Site { role: RefRole::Data, binds: None, owner: None };
    let ast = ctx.ast(file)?;
    let Some(parent) = ctx.interned.parent_of(file, node) else { return Some(loose) };
    let (holder, operand, direct) = match ast.items(parent) {
        Some(_) => (ctx.interned.parent_of(file, parent)?, parent, false),
        None => (parent, node, true),
    };
    let Some(entries) = ast.entries(holder) else { return Some(loose) };
    let entry = entries.iter().find(|entry| entry.value == operand)?;
    let owner = Some(holder);
    if entry.merge {
        return Some(Site { role: RefRole::Inclusion, binds: None, owner });
    }
    if is_extends_key(ast, entry.key) {
        return Some(Site { role: RefRole::Extension, binds: None, owner });
    }
    let binds = direct.then(|| ast.scalar(entry.key).map(|key| key.value.clone())).flatten();
    Some(Site { role: RefRole::Data, binds, owner })
}

/// The diagnostic a failed resolution earns. Every failure is `E0213` — the
/// path named nothing — except a member miss, which is `E0218`: the path landed
/// and the member did not, and the two have different fixes.
fn failed(held: &Occurrence, failure: &Failure) -> Diagnostic {
    let (code, message, note) = explain(&held.text, failure);
    Diagnostic::new(code, held.span, message).with_note(note, None)
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
        Failure::NoMember(name) => (
            Code::UnresolvedMember,
            format!("`{text}` addresses `{name}`, which the node it names does not hold"),
            "member access reads the keys the target writes; a key it inherits is not addressable \
             until it is written"
                .to_owned(),
        ),
    }
}
