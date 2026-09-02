// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 1b — installing a header's imports into the importing file's parse.
//!
//! Discovery resolves import *edges*; this is what makes them mean something.
//! An import is a **binding operation over the importing file's anchor table**,
//! performed before that file's first document event (D6.7), and the only place
//! it can be performed is inside the parse: `saphyr-parser` rejects an unknown
//! alias while scanning, long before an event this crate could act on.
//!
//! So the file is parsed twice. The first parse — pass 1's — reads the header,
//! which is the only way to learn what the file imports. The second is given
//! the imported names and is the one that is kept.
//!
//! # What crosses, and what does not
//!
//! * **Only a file's own definitions cross.** [`exports_of`] skips every
//!   imported binding, so importing `b` brings what `b` wrote and never what
//!   `b` imported. That is D4.9 at file level, and it is the condition the
//!   legality of import cycles rests on (D6.7).
//! * **Import order is authored.** `file.imports` is the header's sequence in
//!   written order, so two imports defining one name shadow deterministically —
//!   `W0300`, both spans, last one winning (D2.1, D5.1).
//! * **An import is not a visibility grant.** Reach is composed on the imported
//!   node's own canonical path, so a definition the importer cannot see is
//!   never installed and the alias to it fails as an unknown anchor. The
//!   *diagnosis* is `E0241` and belongs to `discover` — see [`crate::imports`]
//!   for why it is raised there and not here, where a cyclic component would
//!   report it once per rebinding round.
//! * **Nothing is re-homed.** The installed definition keeps the span it was
//!   written at, in the file it was written in, so every diagnostic about it
//!   points at the exporting file's real line and column.
//!
//! # Order of work
//!
//! Files are bound one strongly connected component of the import graph at a
//! time, in the order Tarjan closes them, which is imports-first. An acyclic
//! file therefore sees its imports in final form on the first attempt and is
//! parsed exactly twice in total. Inside a genuine import cycle there is no
//! such order, so the component is rebound until its exported names stop
//! moving — see [`bind`] for the seed the iteration starts from, why it is
//! needed, and why the loop ends.
//!
//! The node an imported definition names is filled in last, by [`rebind`],
//! once every file's parse is final. Doing it during the parse would capture a
//! [`NodeId`] into an arena a later round may replace.

use std::collections::HashMap;

use tracing::{debug, trace};
use yfi_syntax::{
    anchor_names, parse_with_imports, AnchorId, Ast, Diagnostics, FileId, Import, NodeId,
    ParseOptions, SourceMap, Span,
};

use crate::discover::{FileClass, ProjectFile};
use crate::header;
use crate::imports::Component;
use crate::scope::ScopeTree;

/// One definition a file makes available to the files that import it.
struct Export {
    /// The name it is bound to.
    name: Box<str>,
    /// The node it names, in the exporting file's arena.
    node: NodeId,
    /// Its `&name` token, in the exporting file.
    span: Span,
}

/// Everything the binding pass needs that is not a file.
pub(crate) struct Inputs<'a> {
    /// Every file registered by discovery.
    pub sources: &'a SourceMap,
    /// The scope tree, with header claims already applied — visibility is read
    /// off it, so it must be final before this runs.
    pub scopes: &'a ScopeTree,
    /// Parser options, so a re-parse honours the same severities.
    pub options: &'a ParseOptions,
}

/// Re-parse every importing file with its imports installed, replacing that
/// file's arena, header and diagnostics with the bound parse's.
///
/// # The cyclic case, and why it iterates
///
/// A file's export set is read off a parse, and a parse stops at the first
/// alias it cannot bind — an unknown alias is a *scan* error, so recovery
/// resumes only at the next document boundary and **every anchor written after
/// that alias is lost**. For a member of an import cycle the first cross-file
/// alias is exactly such an alias, because the other side is not bound either.
/// Seeding the iteration from those parses therefore seeds it from ∅ for any
/// member whose anchors are written after its aliases, and ∅ is a fixed point:
/// nothing to install, nothing new to parse, nothing ever grows. Whether a
/// cycle compiled would then depend on where in a file its anchors happen to
/// sit, which is not a rule anyone could follow.
///
/// So a cyclic component is seeded from [`yfi_syntax::anchor_names`] — the
/// `&name` tokens read straight out of each member's text. That is an
/// over-approximation and it is used as a **prelude, never as an answer**:
/// round 0 installs it so each member's parse can reach the end of the file,
/// and that parse's own definitions immediately replace the seed.
///
/// # Termination
///
/// Every member is re-parsed in every round, so after round 0 each member's
/// export set is one a parse produced, and the seed is gone from the state. The
/// loop exits early when a whole round moves no export set: at that moment
/// every member's kept parse was made against export sets that did not change
/// during the round, so each kept parse used the final ones. From round 1 the
/// sets can only grow, because a prelude that binds more names carries a parse
/// at least as far and therefore reveals at least as many anchors, and they are
/// bounded above by the finitely many names the members write — so growth
/// stops. Independently of that argument the round count is bounded by
/// `members + 1`, one round for a name to cross each member plus one to observe
/// stability, and a pathological input costs that many parses rather than a
/// hang.
pub(crate) fn bind(
    files: &mut [ProjectFile],
    pending: &mut [Diagnostics],
    components: &[Component],
    inputs: &Inputs<'_>,
) {
    let ranks: HashMap<FileId, usize> =
        files.iter().enumerate().map(|(rank, file)| (file.id, rank)).collect();
    let mut exports: Vec<Vec<Export>> = files.iter().map(|f| exports_of(&f.ast)).collect();
    for component in components {
        let rounds = if component.cyclic {
            seed(component, files, &mut exports, inputs.sources);
            component.ranks.len() + 1
        } else {
            1
        };
        for round in 0..rounds {
            let mut changed = false;
            for rank in component.ranks.iter().copied() {
                changed |= bind_one(rank, files, pending, &mut exports, &ranks, inputs);
            }
            trace!(round, members = component.ranks.len(), changed, "import component rebound");
            if !changed {
                break;
            }
        }
    }
}

/// Start a cyclic component's export sets from the names its members *write*,
/// read from their text rather than from a parse none of them can complete yet.
///
/// A member's own file is scanned, so no member's seed depends on another's, and
/// the seed is order-free. It is replaced by that member's parsed definitions in
/// round 0 — see [`bind`].
fn seed(
    component: &Component,
    files: &[ProjectFile],
    exports: &mut [Vec<Export>],
    sources: &SourceMap,
) {
    for rank in component.ranks.iter().copied() {
        let written = anchor_names(sources, files[rank].id);
        debug!(file = files[rank].id.0, names = written.len(), "seeding a cyclic import");
        exports[rank] = last_state_of_each(
            written
                .into_iter()
                .map(|found| Export { name: found.name, node: NodeId(0), span: found.span })
                .collect(),
        );
    }
}

/// Bind one file, returning whether the set of names it exports moved.
///
/// A file with nothing to install is not re-parsed: an empty prelude produces a
/// stream identical to the survey's, character for character, so the parse
/// already in hand *is* that file's final parse. Its export set is still taken
/// from that parse, which is what keeps a seeded approximation from surviving a
/// round and reaching another member's prelude.
fn bind_one(
    rank: usize,
    files: &mut [ProjectFile],
    pending: &mut [Diagnostics],
    exports: &mut [Vec<Export>],
    ranks: &HashMap<FileId, usize>,
    inputs: &Inputs<'_>,
) -> bool {
    let imports = prelude_for(rank, files, exports, ranks, inputs.scopes);
    if !imports.is_empty() {
        reparse(rank, files, pending, &imports, inputs);
    }
    let refreshed = exports_of(&files[rank].ast);
    let changed = !same_names(&refreshed, &exports[rank]);
    exports[rank] = refreshed;
    changed
}

/// Re-read one file with `imports` installed, keeping the new parse in place of
/// the survey's.
fn reparse(
    rank: usize,
    files: &mut [ProjectFile],
    pending: &mut [Diagnostics],
    imports: &[Import],
    inputs: &Inputs<'_>,
) {
    let id = files[rank].id;
    debug!(file = id.0, imports = imports.len(), "re-parsing with imported definitions");
    let parsed = parse_with_imports(inputs.sources, id, inputs.options, imports);
    let mut found = parsed.diagnostics;
    let header = match files[rank].class {
        FileClass::Source => header::read(&parsed.ast, &mut found),
        FileClass::Data => None,
    };
    files[rank].ast = parsed.ast;
    files[rank].header = header;
    pending[rank] = found;
}

/// The definitions to install ahead of `rank`'s first document, in the order
/// its header names the files that export them.
fn prelude_for(
    rank: usize,
    files: &[ProjectFile],
    exports: &[Vec<Export>],
    ranks: &HashMap<FileId, usize>,
    scopes: &ScopeTree,
) -> Vec<Import> {
    let file = &files[rank];
    let mut out = Vec::new();
    for imported in &file.imports {
        let Some(other) = ranks.get(imported).copied() else { continue };
        if !scopes.visible(files[other].scope, file.scope) {
            debug!(
                importer = file.id.0,
                imported = imported.0,
                "the importer cannot reach what it imported; nothing is installed"
            );
            continue;
        }
        out.extend(exports[other].iter().map(|e| Import { name: e.name.clone(), span: e.span }));
    }
    out
}

/// What a file exports: the definitions it **wrote**, one per name.
///
/// Imported bindings are excluded, which is the whole of non-transitivity.
fn exports_of(ast: &Ast) -> Vec<Export> {
    let nodes = ast.nodes().len();
    let written = ast
        .anchors()
        .defs()
        .iter()
        .filter(|def| !def.is_imported() && !def.name.is_empty() && def.node.index() < nodes)
        .map(|def| Export { name: def.name.clone(), node: def.node, span: def.span })
        .collect();
    last_state_of_each(written)
}

/// Keep one definition per name — the last — preserving source order.
///
/// D5.2 says a bare name denotes the final state of its sequence, and
/// re-exporting the shadowed states would replay the exporting file's own
/// `W0300` in every file that imports it.
fn last_state_of_each(found: Vec<Export>) -> Vec<Export> {
    let mut latest: HashMap<Box<str>, usize> = HashMap::new();
    for (index, export) in found.iter().enumerate() {
        latest.insert(export.name.clone(), index);
    }
    let mut retained = vec![false; found.len()];
    for index in latest.into_values() {
        retained[index] = true;
    }
    found.into_iter().zip(retained).filter_map(|(export, keep)| keep.then_some(export)).collect()
}

/// Whether two export lists name the same things in the same order.
fn same_names(left: &[Export], right: &[Export]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.name == b.name)
}

/// Point every imported definition at the node it names, now that no file will
/// be parsed again.
///
/// A binding whose name has since vanished from the exporting file is left
/// unbound rather than pointed anywhere: `AnchorDef::target` then answers
/// `None`, which is the honest answer and one a later pass can report on.
pub(crate) fn rebind(files: &mut [ProjectFile]) {
    let table: HashMap<FileId, HashMap<Box<str>, NodeId>> = files
        .iter()
        .map(|file| {
            let names = exports_of(&file.ast).into_iter().map(|e| (e.name, e.node)).collect();
            (file.id, names)
        })
        .collect();
    for file in files.iter_mut() {
        let wanted: Vec<(AnchorId, NodeId)> = file
            .ast
            .anchors()
            .defs()
            .iter()
            .filter(|def| def.is_imported())
            .filter_map(|def| Some((def.id, *table.get(&def.span.file)?.get(&def.name)?)))
            .collect();
        for (id, node) in wanted {
            file.ast.rebind_import(id, node);
        }
    }
}
