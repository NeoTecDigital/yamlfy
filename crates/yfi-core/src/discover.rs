// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pass 1 — discovery.
//!
//! The input is a **project**: a root directory whose tree becomes one
//! namespace hierarchy. A single file is a project of one file.
//!
//! The pass walks the tree, loads every accepted file into **one**
//! [`SourceMap`] — never one per file, which would make every file `FileId(0)`
//! and stop two files' diagnostics from ever coexisting — reads each file's
//! header, builds the scope tree from the directory hierarchy, and resolves
//! both scope axes by inheritance.
//!
//! Nothing here returns a `Result`. A directory that cannot be read, a file
//! that cannot be decoded and a header that states nonsense are all
//! diagnostics, so one bad file never decides the fate of the rest of the tree.
//!
//! A file that imports anything is parsed **twice** — a header's `imports:` can
//! only be read from a parse, and what it imports must be installed before that
//! file's first document event (D6.7). Only the second parse's arena and
//! diagnostics survive; see [`crate::bind`].

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{debug, info};
use yfi_syntax::{
    parse_file, Ast, Code, Diagnostic, Diagnostics, Dialect, FileId, ParseOptions, SourceMap, Span,
};

use crate::bind;
use crate::claims::{check_namespace_uniqueness, Claims};
use crate::header::{self, Header};
use crate::imports;
use crate::reserved;
use crate::scope::{ScopeId, ScopeTree};
use crate::walk::{self, Candidate};

pub use crate::walk::identity;

/// Which language a file is read as.
///
/// The extension is a **classification, not a filter**. A project holds two
/// kinds of file and they mean different things, so one list of accepted
/// extensions cannot express the distinction.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FileClass {
    /// Yamlfication source, `.yfy`. The full language: a header, namespaces,
    /// scope axes, `!type`/`!node`, `!ref`, and the inheritance operators.
    Source,
    /// Base YAML, `.yml` / `.yaml`. The objects and definitions the engine
    /// compiles or runs over. **Yamlfication semantics are not interpreted
    /// here**: `extends` is an ordinary field, `<<` is plain YAML merge and
    /// nothing more, and a `!yfi/header` document is not a header.
    Data,
}

impl FileClass {
    /// The class name, for logging and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FileClass::Source => "source",
            FileClass::Data => "data",
        }
    }

    /// The front end this class is read with. Source gets the `.yfy` pre-pass
    /// (`//`, `<?-- --!>`, `<?-- -->`); data reaches the parser as written.
    #[must_use]
    pub fn dialect(self) -> Dialect {
        match self {
            FileClass::Source => Dialect::Yamlfication,
            FileClass::Data => Dialect::BaseYaml,
        }
    }
}

/// Extensions read as Yamlfication source when nothing says otherwise.
pub const DEFAULT_SOURCE_EXTENSIONS: [&str; 1] = ["yfy"];

/// Extensions read as base YAML when nothing says otherwise.
pub const DEFAULT_DATA_EXTENSIONS: [&str; 2] = ["yml", "yaml"];

/// Knobs for [`discover`].
#[derive(Clone, Debug)]
pub struct DiscoverOptions {
    /// Extensions read as Yamlfication source, without the leading dot.
    pub source_extensions: Vec<String>,
    /// Extensions read as base YAML, without the leading dot.
    pub data_extensions: Vec<String>,
    /// Options handed to the parser for every file.
    pub parse: ParseOptions,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        DiscoverOptions {
            source_extensions: owned(&DEFAULT_SOURCE_EXTENSIONS),
            data_extensions: owned(&DEFAULT_DATA_EXTENSIONS),
            parse: ParseOptions::default(),
        }
    }
}

fn owned(extensions: &[&str]) -> Vec<String> {
    extensions.iter().map(|e| (*e).to_owned()).collect()
}

impl DiscoverOptions {
    /// Which class `path` belongs to, or `None` when the project ignores it.
    ///
    /// Source wins a tie. Listing one extension in both lists is a
    /// configuration mistake, and reading a file as the *fuller* language is
    /// the failure that produces diagnostics rather than the one that silently
    /// stops interpreting a header.
    #[must_use]
    pub fn class_of(&self, path: &Path) -> Option<FileClass> {
        let found = path.extension().and_then(|e| e.to_str())?;
        let matches = |list: &[String]| list.iter().any(|w| w.eq_ignore_ascii_case(found));
        if matches(&self.source_extensions) {
            return Some(FileClass::Source);
        }
        matches(&self.data_extensions).then_some(FileClass::Data)
    }
}

/// One discovered file.
pub struct ProjectFile {
    /// Its handle in the project's source map.
    pub id: FileId,
    /// Which language it is read as.
    pub class: FileClass,
    /// The files its header imports, resolved, deduplicated, in written order.
    /// Always empty for a [`FileClass::Data`] file, which has no header.
    pub imports: Vec<FileId>,
    /// Its rank in relative-path order; also its index in [`Project::files`].
    pub rank: u32,
    /// Path relative to the project root — the sort key.
    pub relative: PathBuf,
    /// Path as read, formed from the root the caller supplied.
    pub path: PathBuf,
    /// The directory scope the file's nodes belong to.
    pub scope: ScopeId,
    /// The parsed arena.
    pub ast: Ast,
    /// The file's header. Always `None` for a [`FileClass::Data`] file.
    pub header: Option<Header>,
}

/// A discovered project: one source map, one scope tree, one diagnostic
/// collection.
pub struct Project {
    root: PathBuf,
    sources: SourceMap,
    files: Vec<ProjectFile>,
    ranks: HashMap<FileId, u32>,
    scopes: ScopeTree,
    diagnostics: Diagnostics,
    import_cycles: Vec<Vec<FileId>>,
}

impl Project {
    /// The project root as the caller wrote it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every file registered while discovering, for rendering spans.
    #[must_use]
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// Give the source map up so a *further* project can be discovered into it.
    ///
    /// `FileId` is an index into one map. Two projects discovered into two maps
    /// therefore both start at `FileId(0)`, and a diagnostic from one would
    /// render against the other's file — so a caller that discovers more than
    /// one project in a single run passes the map along instead of starting
    /// again. This consumes the project because its diagnostics can no longer
    /// be rendered once its map has moved on.
    #[must_use]
    pub fn into_sources(self) -> SourceMap {
        self.sources
    }

    /// Every discovered file, in rank order.
    #[must_use]
    pub fn files(&self) -> &[ProjectFile] {
        &self.files
    }

    /// Look up a file by handle.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&ProjectFile> {
        self.rank(id).and_then(|rank| self.files.get(rank as usize))
    }

    /// The file's rank in discovery order.
    #[must_use]
    pub fn rank(&self, id: FileId) -> Option<u32> {
        self.ranks.get(&id).copied()
    }

    /// The scope tree built from the directory hierarchy.
    #[must_use]
    pub fn scopes(&self) -> &ScopeTree {
        &self.scopes
    }

    /// Everything discovery found.
    #[must_use]
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// The files `id`'s header imports, in written order.
    #[must_use]
    pub fn imports_of(&self, id: FileId) -> &[FileId] {
        self.file(id).map_or(&[], |f| f.imports.as_slice())
    }

    /// Every cycle in the import graph, each listed in rank order.
    ///
    /// A cycle is **not** an error; see `imports.rs` for why. It is recorded so
    /// a later pass and a human can both see it.
    #[must_use]
    pub fn import_cycles(&self) -> &[Vec<FileId>] {
        &self.import_cycles
    }

    /// Whether an import may actually reach what it names.
    ///
    /// **An import does not re-home a definition.** The imported file keeps its
    /// own scope, and therefore its own visibility path; importing is not a way
    /// to launder a `private` definition into a `public` scope. Discovery
    /// records the edge either way and answers the question here, so the pass
    /// that resolves names can report on it rather than silently missing.
    #[must_use]
    pub fn import_reaches(&self, importer: FileId, imported: FileId) -> bool {
        let (Some(from), Some(to)) = (self.file(importer), self.file(imported)) else {
            return false;
        };
        self.scopes.visible(to.scope, from.scope)
    }
}

/// Walk `root`, parse every accepted file, and build the scope tree.
#[must_use]
pub fn discover(root: impl AsRef<Path>, options: &DiscoverOptions) -> Project {
    discover_in(SourceMap::new(), root, options)
}

/// [`discover`], into a source map that already holds files.
///
/// Every file a run touches must live in **one** map, because `FileId` is an
/// index into it; see [`Project::into_sources`].
#[must_use]
pub fn discover_in(
    sources: SourceMap,
    root: impl AsRef<Path>,
    options: &DiscoverOptions,
) -> Project {
    let root = root.as_ref();
    let severities = options.parse.severities.clone();
    let mut diagnostics = Diagnostics::with_severities(severities.clone());
    let mut sources = sources;
    let (base, candidates) = candidates(root, options, &mut sources, &mut diagnostics);
    let mut scopes = ScopeTree::new();
    let directories = directory_scopes(&base, &candidates, &mut scopes);
    let (mut files, mut per_file) = load(&candidates, &directories, options, &mut sources);

    // Claims first: visibility is declared in headers, and the binding pass
    // reads it back to decide whether an import may reach what it names.
    let mut late = Diagnostics::with_severities(severities);
    let mut claims = Claims::new(scopes.scopes().len());
    for file in &files {
        if let Some(header) = &file.header {
            claims.declare(file.scope, file.id, header, &mut late);
        }
    }
    claims.apply(&mut scopes);
    check_namespace_uniqueness(&scopes, &mut late);

    imports::resolve(&mut files, &candidates, &base, &scopes, &mut late);
    let components = imports::components(&files);
    let inputs = bind::Inputs { sources: &sources, scopes: &scopes, options: &options.parse };
    bind::bind(&mut files, &mut per_file, &components, &inputs);
    bind::rebind(&mut files);

    reserved::check(&files, &mut late);

    let import_cycles = imports::cycles(&components, &files);
    for found in per_file {
        diagnostics.extend(found);
    }
    diagnostics.extend(late);
    let ranks = files.iter().map(|f| (f.id, f.rank)).collect();
    info!(
        files = files.len(),
        scopes = scopes.scopes().len(),
        import_cycles = import_cycles.len(),
        "discovered project"
    );
    Project { root: base, sources, files, ranks, scopes, diagnostics, import_cycles }
}

/// Resolve the root into a base directory and the files beneath it. A root that
/// is itself a file is a project of one file rooted at its parent directory.
fn candidates(
    root: &Path,
    options: &DiscoverOptions,
    sources: &mut SourceMap,
    diagnostics: &mut Diagnostics,
) -> (PathBuf, Vec<Candidate>) {
    if root.is_file() {
        let name = root.file_name().map(PathBuf::from).unwrap_or_default();
        let base = root.parent().unwrap_or(Path::new(".")).to_path_buf();
        let Some(class) = options.class_of(root) else {
            unreadable(
                sources,
                diagnostics,
                root,
                "this file's extension classifies it as neither Yamlfication source nor base YAML",
            );
            return (base, Vec::new());
        };
        let candidate = Candidate {
            relative: name,
            identity: walk::identity(root),
            path: root.to_path_buf(),
            class,
        };
        return (base, vec![candidate]);
    }
    if !root.is_dir() {
        unreadable(sources, diagnostics, root, "project root is neither a file nor a directory");
        return (root.to_path_buf(), Vec::new());
    }
    let found = walk::collect(root, options);
    for error in &found.errors {
        unreadable(
            sources,
            diagnostics,
            &error.path,
            &format!("cannot read directory: {}", error.message),
        );
    }
    (root.to_path_buf(), found.candidates)
}

/// Register a path that carries no text so a diagnostic about it still has a
/// span. `parse_file` does the same for a file it cannot read.
fn unreadable(sources: &mut SourceMap, diagnostics: &mut Diagnostics, path: &Path, message: &str) {
    let id = sources.add(path, "");
    let span = Span::empty(id, sources.file(id).pos_at_char(0));
    diagnostics.push(Diagnostic::new(Code::IoError, span, message.to_owned()));
}

/// Create one scope per directory, parents before children. A `BTreeMap` keyed
/// by relative path gives that ordering for free: a directory's path is a
/// prefix of every path beneath it, so it sorts first.
fn directory_scopes(
    base: &Path,
    candidates: &[Candidate],
    scopes: &mut ScopeTree,
) -> BTreeMap<PathBuf, ScopeId> {
    let mut wanted: BTreeMap<PathBuf, ScopeId> = BTreeMap::new();
    let root_name = base.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let root = scopes.push(None, root_name);
    wanted.insert(PathBuf::new(), root);
    for candidate in candidates {
        let mut chain: Vec<PathBuf> = Vec::new();
        let mut dir = candidate.relative.parent().map(Path::to_path_buf);
        while let Some(path) = dir.filter(|p| !p.as_os_str().is_empty()) {
            dir = path.parent().map(Path::to_path_buf);
            chain.push(path);
        }
        for path in chain.into_iter().rev() {
            insert_scope(scopes, &mut wanted, path);
        }
    }
    wanted
}

fn insert_scope(scopes: &mut ScopeTree, wanted: &mut BTreeMap<PathBuf, ScopeId>, path: PathBuf) {
    if wanted.contains_key(&path) {
        return;
    }
    let parent_path = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let parent = wanted.get(&parent_path).copied().or_else(|| scopes.root());
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let id = scopes.push(parent, name);
    debug!(scope = id.0, path = %path.display(), "directory scope");
    wanted.insert(path, id);
}

/// Parse every candidate, in rank order, into the one source map.
///
/// Each file's own diagnostics are returned beside it rather than merged, so a
/// file the binding pass re-parses can have them **replaced** — the survey
/// parse of a file that imports anything reports an unknown anchor for every
/// cross-file alias, which is an artefact of not having bound the imports yet
/// and not a finding about the file.
fn load(
    candidates: &[Candidate],
    directories: &BTreeMap<PathBuf, ScopeId>,
    options: &DiscoverOptions,
    sources: &mut SourceMap,
) -> (Vec<ProjectFile>, Vec<Diagnostics>) {
    let mut files = Vec::with_capacity(candidates.len());
    let mut per_file = Vec::with_capacity(candidates.len());
    for (rank, candidate) in candidates.iter().enumerate() {
        let parsed =
            parse_file(sources, &candidate.path, &options.parse, candidate.class.dialect());
        let mut found = parsed.diagnostics;
        // A data file has no header. Reading one would make `!yfi/header`
        // meaningful in base YAML, which is exactly what the two classes exist
        // to prevent.
        let header = match candidate.class {
            FileClass::Source => header::read(&parsed.ast, &mut found),
            FileClass::Data => None,
        };
        per_file.push(found);
        let directory = candidate.relative.parent().map(Path::to_path_buf).unwrap_or_default();
        // Every candidate's directory got a scope above, so the fallback is
        // unreachable; it is the root rather than a panic because a missing
        // scope must not cost the caller the rest of the project.
        let scope = directories.get(&directory).copied().unwrap_or(ScopeId(0));
        debug!(
            rank,
            path = %candidate.relative.display(),
            scope = scope.0,
            class = candidate.class.as_str(),
            "loaded"
        );
        files.push(ProjectFile {
            id: parsed.ast.file(),
            class: candidate.class,
            imports: Vec::new(),
            rank: u32::try_from(rank).expect("project file count overflow"),
            relative: candidate.relative.clone(),
            path: candidate.path.clone(),
            scope,
            ast: parsed.ast,
            header,
        });
    }
    (files, per_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_decides_the_class_and_is_matched_case_insensitively() {
        let options = DiscoverOptions::default();
        assert_eq!(options.class_of(Path::new("a.yfy")), Some(FileClass::Source));
        assert_eq!(options.class_of(Path::new("a.YFY")), Some(FileClass::Source));
        assert_eq!(options.class_of(Path::new("a.yml")), Some(FileClass::Data));
        assert_eq!(options.class_of(Path::new("a.yaml")), Some(FileClass::Data));
        assert_eq!(options.class_of(Path::new("a.txt")), None);
        assert_eq!(options.class_of(Path::new("a")), None);
        assert_eq!(options.class_of(Path::new("yfy")), None);
    }

    #[test]
    fn the_two_lists_are_configurable_and_source_wins_a_tie() {
        let options = DiscoverOptions {
            source_extensions: vec!["yml".to_owned()],
            data_extensions: vec!["yml".to_owned(), "json".to_owned()],
            ..DiscoverOptions::default()
        };
        assert_eq!(options.class_of(Path::new("a.yml")), Some(FileClass::Source));
        assert_eq!(options.class_of(Path::new("a.json")), Some(FileClass::Data));
        assert_eq!(
            options.class_of(Path::new("a.yfy")),
            None,
            "the default is replaced, not added"
        );
    }
}
