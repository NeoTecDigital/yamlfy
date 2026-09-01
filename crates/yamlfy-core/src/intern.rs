// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 3 — interning and indexing.
//!
//! Three things the front end deliberately does not provide, built here as side
//! tables keyed by `(FileId, NodeId)` so the read-only [`Ast`] stays read-only:
//!
//! * a **symbol table** over every mapping key, tag suffix and namespace
//!   component,
//! * a **node → document** map and a **node → parent** map, neither of which
//!   exists today,
//! * each node's **resolved scope path**.
//!
//! Both maps are built by a single forward scan of `ast.nodes()`. The arena is
//! post-order — a collection's `NodeId` exceeds every one of its children's — so
//! a forward scan *is* a bottom-up pass, and a child's parent is always assigned
//! by a later index than the child's own. No recursion, no visitor.

use std::collections::HashMap;

use tracing::debug;
use yamlfy_syntax::{Ast, FileId, NodeId};

use crate::discover::{FileClass, Project};
use crate::order::NodeOrder;
use crate::scope::ScopeId;
use crate::symbol::{Symbol, SymbolTable};
use crate::tags::{classify, TagKind};

/// Everything pass 3 knows about one file.
pub struct FileIndex {
    /// The file this index describes.
    pub file: FileId,
    /// Which language it was read as.
    pub class: FileClass,
    /// Its rank in discovery order.
    pub rank: u32,
    /// The directory scope its nodes belong to.
    pub scope: ScopeId,
    /// The scope's stored `root → scope` path.
    scope_path: Vec<ScopeId>,
    /// Zero-based document index per node, or `None` for a node orphaned by
    /// syntax-error recovery.
    document_of: Vec<Option<u32>>,
    /// Enclosing collection per node; `None` for a document root and for an
    /// orphan.
    parent_of: Vec<Option<NodeId>>,
    /// Classified tag and interned suffix per node.
    tag_of: Vec<Option<(TagKind, Symbol)>>,
    /// Interned text per node used as a scalar mapping key.
    key_of: Vec<Option<Symbol>>,
}

impl FileIndex {
    /// Number of nodes the index covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.document_of.len()
    }

    /// Whether the file parsed to nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.document_of.is_empty()
    }
}

/// The project's symbols and per-file indexes.
pub struct Interned {
    symbols: SymbolTable,
    files: Vec<FileIndex>,
    ranks: HashMap<FileId, u32>,
    namespaces: Vec<Vec<Symbol>>,
}

impl Interned {
    /// Every distinct name the project mentions.
    #[must_use]
    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }

    /// Every file index, in discovery order.
    #[must_use]
    pub fn files(&self) -> &[FileIndex] {
        &self.files
    }

    /// The index for one file.
    #[must_use]
    pub fn index(&self, file: FileId) -> Option<&FileIndex> {
        self.ranks.get(&file).and_then(|rank| self.files.get(*rank as usize))
    }

    /// The interned components of a scope's namespace, outermost first. Empty
    /// when the scope claims none.
    #[must_use]
    pub fn namespace_of(&self, scope: ScopeId) -> &[Symbol] {
        self.namespaces.get(scope.index()).map_or(&[], Vec::as_slice)
    }

    /// The document a node belongs to.
    #[must_use]
    pub fn document_of(&self, file: FileId, node: NodeId) -> Option<u32> {
        self.index(file)?.document_of.get(node.index()).copied().flatten()
    }

    /// The collection a node is written inside.
    #[must_use]
    pub fn parent_of(&self, file: FileId, node: NodeId) -> Option<NodeId> {
        self.index(file)?.parent_of.get(node.index()).copied().flatten()
    }

    /// Which language a file was read as.
    #[must_use]
    pub fn class_of(&self, file: FileId) -> Option<FileClass> {
        Some(self.index(file)?.class)
    }

    /// A node's classified tag, if it carries one.
    ///
    /// In a [`FileClass::Data`] file every tag classifies as
    /// [`TagKind::Other`]: Yamlfication semantics are not interpreted in base
    /// YAML, so a `!node` there is a tag the engine carries and does not read.
    /// Putting that rule here rather than in every later pass is what keeps it
    /// from being forgotten once. The lexical answer is still available from
    /// [`crate::tags::classify`], and the suffix is still interned.
    #[must_use]
    pub fn tag_kind(&self, file: FileId, node: NodeId) -> Option<TagKind> {
        self.index(file)?.tag_of.get(node.index()).copied().flatten().map(|(kind, _)| kind)
    }

    /// A node's interned tag suffix, if it carries a tag.
    #[must_use]
    pub fn tag_suffix(&self, file: FileId, node: NodeId) -> Option<Symbol> {
        self.index(file)?.tag_of.get(node.index()).copied().flatten().map(|(_, symbol)| symbol)
    }

    /// The interned text of a node used as a scalar mapping key.
    #[must_use]
    pub fn key_of(&self, file: FileId, node: NodeId) -> Option<Symbol> {
        self.index(file)?.key_of.get(node.index()).copied().flatten()
    }

    /// The scope a node resolves to. Phase 1 has no sub-file scoping, so every
    /// node of a file resolves to that file's directory scope.
    #[must_use]
    pub fn scope_of(&self, file: FileId, node: NodeId) -> Option<ScopeId> {
        let index = self.index(file)?;
        (node.index() < index.len()).then_some(index.scope)
    }

    /// A node's resolved scope path, `root → scope`. This is what visibility
    /// and mutability compose over; evaluating either axis on the last element
    /// alone would make an enclosing `private` or `readonly` scope mean nothing.
    #[must_use]
    pub fn scope_path_of(&self, file: FileId, node: NodeId) -> Option<&[ScopeId]> {
        let index = self.index(file)?;
        (node.index() < index.len()).then_some(index.scope_path.as_slice())
    }

    /// A node's position in the project's total order.
    #[must_use]
    pub fn order(&self, file: FileId, node: NodeId) -> Option<NodeOrder> {
        let index = self.index(file)?;
        let document = index.document_of.get(node.index()).copied().flatten()?;
        Some(NodeOrder {
            file: index.rank,
            document,
            node: u32::try_from(node.index()).ok()?,
        })
    }
}

/// Build the symbol table and every side index for `project`.
#[must_use]
pub fn intern(project: &Project) -> Interned {
    let mut symbols = SymbolTable::new();
    let namespaces = namespace_symbols(project, &mut symbols);
    let mut files = Vec::with_capacity(project.files().len());
    for file in project.files() {
        files.push(index_file(file, project, &mut symbols));
    }
    let ranks = files.iter().map(|f| (f.file, f.rank)).collect();
    debug!(symbols = symbols.len(), files = files.len(), "interned project");
    Interned { symbols, files, ranks, namespaces }
}

fn namespace_symbols(project: &Project, symbols: &mut SymbolTable) -> Vec<Vec<Symbol>> {
    project
        .scopes()
        .scopes()
        .iter()
        .map(|scope| match &scope.namespace {
            Some(namespace) => namespace.split("::").map(|part| symbols.intern(part)).collect(),
            None => Vec::new(),
        })
        .collect()
}

fn index_file(
    file: &crate::discover::ProjectFile,
    project: &Project,
    symbols: &mut SymbolTable,
) -> FileIndex {
    let ast = &file.ast;
    let count = ast.nodes().len();
    let mut index = FileIndex {
        file: ast.file(),
        class: file.class,
        rank: file.rank,
        scope: file.scope,
        scope_path: project.scopes().path(file.scope).to_vec(),
        document_of: document_map(ast),
        parent_of: vec![None; count],
        tag_of: vec![None; count],
        key_of: vec![None; count],
    };
    for position in 0..count {
        let id = NodeId(u32::try_from(position).expect("arena overflow"));
        link_children(ast, id, &mut index, symbols);
        if let Some(tag) = ast.tag(id) {
            index.tag_of[position] = Some((kind_in(file.class, tag), symbols.intern(&tag.suffix)));
        }
    }
    index
}

/// Classify a tag for the class of file it was written in.
fn kind_in(class: FileClass, tag: &yamlfy_syntax::Tag) -> TagKind {
    match class {
        FileClass::Source => classify(tag),
        FileClass::Data => TagKind::Other,
    }
}

/// Record `id` as the parent of each of its children, and intern every scalar
/// key it holds. Children always have a lower index than `id`, so one forward
/// scan assigns every parent exactly once.
fn link_children(ast: &Ast, id: NodeId, index: &mut FileIndex, symbols: &mut SymbolTable) {
    if let Some(items) = ast.items(id) {
        for item in items {
            index.parent_of[item.index()] = Some(id);
        }
        return;
    }
    let Some(entries) = ast.entries(id) else { return };
    let mut keys: Vec<(usize, Symbol)> = Vec::new();
    for entry in entries {
        index.parent_of[entry.key.index()] = Some(id);
        index.parent_of[entry.value.index()] = Some(id);
        if let Some(scalar) = ast.scalar(entry.key) {
            keys.push((entry.key.index(), symbols.intern(&scalar.value)));
        }
    }
    for (position, symbol) in keys {
        index.key_of[position] = Some(symbol);
    }
}

/// Map every node to its document.
///
/// A document's root is emitted after all of its children, so roots partition
/// the arena into ascending ranges. Recovery from a syntax error can leave
/// nodes behind that belong to a document which was never completed; those fall
/// outside every document's span and are recorded as `None` rather than
/// silently attributed to the next document that happens to follow them.
fn document_map(ast: &Ast) -> Vec<Option<u32>> {
    let mut out = vec![None; ast.nodes().len()];
    let mut document = 0usize;
    for (position, slot) in out.iter_mut().enumerate() {
        while document < ast.documents().len()
            && ast.documents()[document].root.index() < position
        {
            document += 1;
        }
        let Some(current) = ast.documents().get(document) else { break };
        let node = ast.nodes()[position].span;
        let within = node.start.byte >= current.span.start.byte
            && node.start.byte <= current.span.end.byte;
        if within {
            *slot = Some(u32::try_from(document).expect("document count overflow"));
        }
    }
    out
}
