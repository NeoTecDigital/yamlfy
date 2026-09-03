// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Import resolution — how a definition crosses a file (D6.7).
//!
//! # Paths
//!
//! An import is resolved **against the project root** and must name a file the
//! project already discovered.
//!
//! **Membership is decided by discovery, not by where the bytes live.** A file
//! the walk found, ranked and gave a scope to is a member of the project by
//! every other measure, so its own relative path — the path that discovered it
//! — always names it. That is what makes `imports: [vendor.yfy]` work when
//! `vendor.yfy` is a symlink to a file outside the tree, which is the ordinary
//! way a vendored directory is brought in. Deciding membership by canonical
//! identity instead resolves the link first and then rejects the result for
//! being outside the root, so a file that is discovered, ranked and scoped can
//! never be imported — and D6.2 already refuses to let a symlink's target
//! decide anything, for the same reason.
//!
//! A path that does *not* name a discovered file is then resolved by canonical
//! identity, reusing the identity the walk deduplicates on, so a route through
//! a symlinked directory reaches the same file the walk kept. That second
//! lookup is guarded: the path must stay inside the project. The guard is not
//! redundant with the first, because the symlinked-in file's identity *is* in
//! the table — without it, `../outside/vendor.yfy` would resolve to the very
//! file `vendor.yfy` names, importing a file by a path that leaves the project.
//!
//! A path that satisfies neither is `E0240`.
//!
//! # Reach, and cycles
//!
//! `E0241` is raised here rather than in [`crate::bind`], once per resolved
//! entry; the edge is recorded either way. Import cycles are recorded rather
//! than diagnosed, so a later pass and a human can both see the shape (D6.7).

use std::collections::HashMap;
// `Component` is taken here by the import graph's own SCC type, below.
use std::path::{Component as PathPart, Path, PathBuf};

use tracing::debug;
use yfi_syntax::{Code, Diagnostic, Diagnostics, FileId, Span};

use crate::discover::ProjectFile;
use crate::scope::{ScopeId, ScopeTree};
use crate::walk::{self, Candidate};

/// Resolve every header's `imports:` onto the files they name.
///
/// `scopes` must already carry every header's claims: reach is read off it, and
/// a tree still at its inherited defaults would answer a different question.
pub(crate) fn resolve(
    files: &mut [ProjectFile],
    candidates: &[Candidate],
    root: &Path,
    scopes: &ScopeTree,
    diagnostics: &mut Diagnostics,
) {
    let resolver = Resolver {
        root,
        root_identity: walk::identity(root),
        by_relative: candidates
            .iter()
            .zip(files.iter())
            .map(|(candidate, file)| (candidate.relative.as_path(), file.id))
            .collect(),
        by_identity: candidates
            .iter()
            .zip(files.iter())
            .map(|(candidate, file)| (&candidate.identity, file.id))
            .collect(),
        scope_of: files.iter().map(|file| (file.id, file.scope)).collect(),
        scopes,
    };
    for file in files.iter_mut() {
        let Some(header) = &file.header else { continue };
        let resolved = resolver.entries(file.scope, &header.imports, diagnostics);
        file.imports = resolved;
    }
}

/// Everything one project's import entries resolve against. Held apart from the
/// files so the pass can read every file's scope while writing one file's
/// resolved edges.
struct Resolver<'a> {
    root: &'a Path,
    root_identity: PathBuf,
    /// Every discovered file under the root-relative path the walk found it by.
    /// This is what membership means, so it is consulted first.
    by_relative: HashMap<&'a Path, FileId>,
    by_identity: HashMap<&'a PathBuf, FileId>,
    scope_of: HashMap<FileId, ScopeId>,
    scopes: &'a ScopeTree,
}

impl Resolver<'_> {
    /// Resolve one header's entries, in written order, reporting each fault
    /// once. A repeat of a file already imported is neither re-recorded nor
    /// re-reported, because it installs nothing the first entry did not.
    fn entries(
        &self,
        importer: ScopeId,
        imports: &[(Box<str>, Span)],
        diagnostics: &mut Diagnostics,
    ) -> Vec<FileId> {
        let mut resolved: Vec<FileId> = Vec::with_capacity(imports.len());
        for (text, span) in imports {
            match self.lookup(text) {
                Some(id) if resolved.contains(&id) => {
                    debug!(import = %text, "already imported by this file; ignoring the repeat");
                }
                Some(id) => {
                    if let Some(blocker) = self.blocked_by(importer, id) {
                        diagnostics.push(not_visible(self.scopes, text, *span, importer, blocker));
                    }
                    resolved.push(id);
                }
                None => diagnostics.push(unresolved(text, *span)),
            }
        }
        resolved
    }

    /// Find the file an import names, or `None`.
    fn lookup(&self, text: &str) -> Option<FileId> {
        self.discovered_as(text).or_else(|| self.by_real_path(text))
    }

    /// The file the walk filed under this exact relative path.
    ///
    /// Discovery decides membership, so this answers before the filesystem is
    /// consulted at all — a discovered file is importable by the path that
    /// discovered it whatever a symlink on that path points at.
    fn discovered_as(&self, text: &str) -> Option<FileId> {
        let relative = within_root(text)?;
        self.by_relative.get(relative.as_path()).copied()
    }

    /// The discovered file whose real identity this path resolves to, provided
    /// the path itself stays inside the project.
    ///
    /// This reaches a file named by some *other* route through the project —
    /// a symlinked directory, say — and the guard is what stops it reaching a
    /// symlinked-in file by the outside path its link points at.
    fn by_real_path(&self, text: &str) -> Option<FileId> {
        let identity = walk::identity(&self.root.join(text));
        if !identity.starts_with(&self.root_identity) {
            debug!(import = %text, "resolves outside the project root");
            return None;
        }
        self.by_identity.get(&identity).copied()
    }

    /// The scope that stops `importer` reaching `imported`, if one does.
    fn blocked_by(&self, importer: ScopeId, imported: FileId) -> Option<ScopeId> {
        let target = self.scope_of.get(&imported).copied()?;
        self.scopes.blocked_by(target, importer)
    }
}

/// An import path as a root-relative one, with `.` and `..` resolved
/// **lexically** — the filesystem is deliberately not consulted, because the
/// question is which discovered file the author named, and discovery keys files
/// by exactly this path.
///
/// `None` when the path is absolute or climbs above the root, neither of which
/// can be a discovered file's relative path. Both fall through to the identity
/// lookup, which decides them on the filesystem's terms.
fn within_root(text: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for part in Path::new(text).components() {
        match part {
            PathPart::Normal(name) => relative.push(name),
            PathPart::CurDir => {}
            PathPart::ParentDir if relative.pop() => {}
            PathPart::ParentDir | PathPart::RootDir | PathPart::Prefix(_) => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn unresolved(text: &str, span: Span) -> Diagnostic {
    // `E0240`, not `E0231`. A `.yfy` path that names nothing is not a header
    // *value* the language cannot read — the value is a perfectly good path
    // scalar — it is a reach that lands outside the project, which is a
    // different fault with a different fix, and §4 allocates it its own code.
    Diagnostic::new(
        Code::UnresolvedImport,
        span,
        format!(
            "`{text}` does not name a file of this project; imports resolve against the \
             project root and must name a discovered source or data file"
        ),
    )
}

fn not_visible(
    scopes: &ScopeTree,
    text: &str,
    span: Span,
    importer: ScopeId,
    blocker: ScopeId,
) -> Diagnostic {
    // The importing header is what the author can act on, so it is the primary
    // span; the scope that closed the path is why, so it is the note. Without
    // the note the message would name a file the author already wrote and no
    // reason, and the blocking scope is frequently neither the target's own
    // directory nor one the author has ever opened.
    let (at, how) = match scopes.get(blocker).and_then(|scope| scope.visibility_span) {
        Some(at) => (Some(at), "declared"),
        None => (None, "inherited"),
    };
    Diagnostic::new(
        Code::ImportNotVisible,
        span,
        format!(
            "`{text}` names a file this scope cannot see; an import is not a visibility \
             grant, so nothing is installed"
        ),
    )
    .with_note(
        format!(
            "`{}` is {how} `private` and `{}` is outside it; visibility composes over the \
             whole path from the root",
            scopes.qualified(blocker),
            scopes.qualified(importer)
        ),
        at,
    )
}

/// One strongly connected component of the import graph: file ranks, sorted,
/// plus whether the component is a cycle rather than a lone acyclic file.
pub(crate) struct Component {
    /// The files it holds, in rank order.
    pub ranks: Vec<usize>,
    /// Whether its members import each other, directly or through the group.
    pub cyclic: bool,
}

/// Every strongly connected component of the import graph, **imports first**.
///
/// Tarjan's algorithm, iterative: the graph is a project's worth of files and
/// recursion depth would be a function of the input. It closes a component only
/// after every component reachable from it, so the emission order is a reverse
/// topological one — which is exactly the order the binding pass wants, because
/// a file's imports are then already in their final form when its own prelude
/// is assembled.
pub(crate) fn components(files: &[ProjectFile]) -> Vec<Component> {
    let ranks: HashMap<FileId, usize> =
        files.iter().enumerate().map(|(rank, file)| (file.id, rank)).collect();
    let graph = Graph { files, ranks: &ranks };
    let mut state = Tarjan::new(files);
    for start in 0..files.len() {
        if state.index_of[start].is_none() {
            state.run(&graph, start);
        }
    }
    state.found
}

/// Every cycle in the import graph, each member list in rank order and the
/// cycles themselves ordered by their first member.
pub(crate) fn cycles(components: &[Component], files: &[ProjectFile]) -> Vec<Vec<FileId>> {
    let mut found: Vec<&Vec<usize>> =
        components.iter().filter(|c| c.cyclic).map(|c| &c.ranks).collect();
    found.sort();
    found.iter().map(|group| group.iter().map(|r| files[*r].id).collect()).collect()
}

/// The import graph, addressed by rank so every array indexes directly.
struct Graph<'a> {
    files: &'a [ProjectFile],
    ranks: &'a HashMap<FileId, usize>,
}

impl Graph<'_> {
    fn edge(&self, node: usize, edge: usize) -> Option<Option<usize>> {
        let target = self.files[node].imports.get(edge)?;
        Some(self.ranks.get(target).copied())
    }

    fn imports_itself(&self, node: usize) -> bool {
        self.files[node].imports.iter().any(|id| self.ranks.get(id) == Some(&node))
    }
}

/// Ranks, not `FileId`s, because a rank indexes the arrays directly.
struct Tarjan {
    index_of: Vec<Option<u32>>,
    low: Vec<u32>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: u32,
    found: Vec<Component>,
}

impl Tarjan {
    fn new(files: &[ProjectFile]) -> Self {
        Tarjan {
            index_of: vec![None; files.len()],
            low: vec![0; files.len()],
            on_stack: vec![false; files.len()],
            stack: Vec::new(),
            next: 0,
            found: Vec::new(),
        }
    }

    fn run(&mut self, graph: &Graph<'_>, start: usize) {
        let mut work: Vec<(usize, usize)> = vec![(start, 0)];
        self.enter(start);
        while let Some((node, edge)) = work.pop() {
            match self.step(graph, node, edge) {
                Step::Descend(child) => {
                    work.push((node, edge + 1));
                    work.push((child, 0));
                    self.enter(child);
                }
                Step::Skip => work.push((node, edge + 1)),
                Step::Done => {
                    let parent = work.last().map(|(parent, _)| *parent);
                    self.leave(graph, node, parent);
                }
            }
        }
    }

    fn enter(&mut self, node: usize) {
        self.index_of[node] = Some(self.next);
        self.low[node] = self.next;
        self.next += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
    }

    fn step(&mut self, graph: &Graph<'_>, node: usize, edge: usize) -> Step {
        let Some(target) = graph.edge(node, edge) else { return Step::Done };
        let Some(child) = target else { return Step::Skip };
        if self.index_of[child].is_none() {
            return Step::Descend(child);
        }
        if self.on_stack[child] {
            self.low[node] = self.low[node].min(self.index_of[child].unwrap_or(u32::MAX));
        }
        Step::Skip
    }

    fn leave(&mut self, graph: &Graph<'_>, node: usize, parent: Option<usize>) {
        if let Some(parent) = parent {
            self.low[parent] = self.low[parent].min(self.low[node]);
        }
        if self.index_of[node] != Some(self.low[node]) {
            return;
        }
        let mut group = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack[member] = false;
            group.push(member);
            if member == node {
                break;
            }
        }
        group.sort_unstable();
        // A one-member component is a cycle only when the file imports itself.
        let cyclic = group.len() > 1 || graph.imports_itself(node);
        self.found.push(Component { ranks: group, cyclic });
    }
}

enum Step {
    Descend(usize),
    Skip,
    Done,
}
