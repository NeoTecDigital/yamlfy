// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Path references: where one is written, what it names, and what `!ref` adds.
//!
//! **The path is the reach** (D4.12): a path resolves against the project's
//! directories and files and needs no `imports:` entry to precede it. The three
//! positions a plain scalar is read as a path in, what each form resolves
//! against, and what `!ref` declares on top of a plain path are all D4.12's;
//! the `connections` row is D4.13's. Whether the reach is *permitted* is
//! `E0216`, and `check::reach` says why that one gate sits elsewhere.
//!
//! That third position is the set [`crate::edge::endpoint_sequences`] names,
//! and a clause operand may itself be a path this pass resolves, so **the pass
//! runs twice** (D4.13): [`probe`] resolves with no `connections` item read as
//! a reach and reports nothing, `link` builds the clauses from it, then calls
//! [`resolve`] with the answer. Only the third position moves between the runs.
//!
//! In a base YAML file `!ref` is an unrecognised tag on a value and nothing
//! else (D6.6), so pass 3 classifies it as [`TagKind::Other`] there and this
//! pass never sees it.

use std::collections::{HashMap, HashSet};

use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, NodeId, ScalarStyle, Span};

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
    /// An item of a `connections` sequence some `!edge` reads — an endpoint of
    /// that edge (D4.13). A reach position by declaration, exactly as a clause
    /// operand is, so a bare name is a path there and quoting escapes nothing:
    /// there is no prefix in this position for a quote to escape.
    Connection,
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
            RefRole::Connection => "connections",
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
///
/// `endpoints` is the set of nodes whose `connections` some `!edge` reads; a
/// sequence item under that member is a reach on those nodes and an ordinary
/// value everywhere else.
pub(crate) fn resolve(
    ctx: &Ctx,
    table: &Table,
    space: &Space,
    endpoints: &HashSet<(FileId, NodeId)>,
    diagnostics: &mut Diagnostics,
) -> References {
    let occurrences = collect(ctx, endpoints);
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

/// The first of the pass's two runs: every reference resolved with no
/// `connections` item read as a reach, and nothing reported.
///
/// Its only consumer is the clause collection `link` derives the endpoint
/// holders from. Every clause operand resolves here exactly as it does in
/// [`resolve`], so the two runs produce the same clauses and only the second
/// raises the diagnostics.
pub(crate) fn probe(ctx: &Ctx, table: &Table, space: &Space) -> References {
    let mut discarded = Diagnostics::new();
    resolve(ctx, table, space, &HashSet::new(), &mut discarded)
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
fn collect(ctx: &Ctx, endpoints: &HashSet<(FileId, NodeId)>) -> Vec<Occurrence> {
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
            let document = document.unwrap_or_default();
            if let Some(found) = one(ctx, endpoints, file.id, node, document) {
                out.push(found);
            }
        }
    }
    out
}

/// Read `node` as a path occurrence, or `None` if it is not one.
fn one(
    ctx: &Ctx,
    endpoints: &HashSet<(FileId, NodeId)>,
    file: FileId,
    node: NodeId,
    document: u32,
) -> Option<Occurrence> {
    let ast = ctx.ast(file)?;
    let capability = ctx.interned.tag_kind(file, node) == Some(TagKind::Ref);
    let scalar = ast.scalar(node)?;
    let plain = scalar.style == ScalarStyle::Plain && ast.tag(node).is_none();
    let Site { role, binds, owner } = site(ctx, endpoints, file, node)?;
    // Two positions are reaches by declaration rather than by spelling: a
    // `!ref`, and an item of an edge's `connections`. In both the scalar names
    // a node whatever its style, and a scalar that is not a path at all is
    // still an occurrence — it declared a reach and named nothing, and silence
    // there would lose it.
    let declared = capability || role == RefRole::Connection;
    if !declared && !plain {
        return None;
    }
    let text: Box<str> = scalar.value.trim().into();
    // An untagged scalar in a data position is data unless the path is anchored.
    let path = match path::parse(&text) {
        Some(path) if declared || role != RefRole::Data || path.anchored => Some(path),
        _ if declared => None,
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
///
/// `endpoints` is the set of **sequences** an `!edge` reads, not of the nodes
/// holding the key, so only the sequence form is a reach position: a
/// `connections` that is not one never enters the set, and is reported once as
/// `E0224` rather than twice as a shape fault and a failed path.
fn site(
    ctx: &Ctx,
    endpoints: &HashSet<(FileId, NodeId)>,
    file: FileId,
    node: NodeId,
) -> Option<Site> {
    let loose = Site { role: RefRole::Data, binds: None, owner: None };
    let ast = ctx.ast(file)?;
    let Some(parent) = ctx.interned.parent_of(file, node) else { return Some(loose) };
    let endpoint = endpoints.contains(&(file, parent));
    let (holder, operand, direct) = match (ast.items(parent), ctx.interned.parent_of(file, parent))
    {
        // A sequence reached by an alias may be a document root, which no
        // mapping wrote. It still holds endpoints, and it owns nothing: the
        // reverse edge a `!ref` contributes lands on the context that declared
        // the dependency, and here there is no such context to name.
        (Some(_), None) => {
            return Some(if endpoint { connection(None) } else { loose });
        }
        (Some(_), Some(held)) => (held, parent, false),
        (None, _) => (parent, node, true),
    };
    // **An item of a sequence an edge reads is an endpoint, whatever wrote the
    // sequence.** D4.13 says a sequence an edge reads is a reach position *for
    // every reader of it*, so this is decided before anything about the
    // sequence's own surroundings is consulted.
    //
    // It used to be decided last, behind four exits — a sequence nested in
    // another, one standing as a complex mapping key, and one written as a `<<`
    // or `extends` operand list each reached its own answer first. All four
    // left the edge relating **nothing**, with no diagnostic, on the ordinary
    // shape of an aliased shared sequence.
    if !direct && endpoint {
        return Some(connection(ctx.interned.parent_of(file, parent)));
    }
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

/// An item of a sequence some `!edge` reads: an endpoint of that edge.
fn connection(owner: Option<NodeId>) -> Site {
    Site { role: RefRole::Connection, binds: None, owner }
}

/// The diagnostic a failed resolution earns. Every failure is `E0213` — the
/// path named nothing — except a member miss, which is `E0218` (the path landed
/// and the member did not, and the two have different fixes), and a landing the
/// referencing scope cannot see, which is `E0216`.
///
/// `E0216`'s note carries **no span**. Every other note here points at
/// something the author can read; that one would point inside the scope the
/// gate just refused, which is the disclosure the gate exists to prevent.
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
