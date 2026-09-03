// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `yamlfy check` subcommand.
//!
//! **`check <file>` and `check <dir>` are one operation at two scopes** (D6.1),
//! so this runs `discover` and never the parser alone. Parsing a file by itself
//! cannot see a header's `imports:`, so every cross-file alias in it reports as
//! an unknown anchor — the file compiles through the library and fails through
//! the command line, which is worse than not having the subcommand.
//!
//! # What the project root is, and why
//!
//! Imports resolve **root-relative** and must name a file the project already
//! discovered (D6.7), so the root decides what an import can reach. For a path
//! given on the command line it is:
//!
//! * `--root`, when the caller states one;
//! * the path itself, when it is a directory;
//! * otherwise the file's **parent directory**.
//!
//! The parent directory is the only root derivable from the argument alone. The
//! alternative — searching upward for a marker such as `yamlfy.toml` — makes the
//! compilation unit depend on files nobody named, and its failure is unbounded:
//! one stray marker in a home directory silently turns a one-file check into a
//! walk of everything below it.
//!
//! A file deeper in a tree whose imports reach above its own directory is
//! therefore checked against a project those imports leave, and each one is
//! `E0240` — reported, at the import's own span, naming the fix. That is the
//! trade the parent-directory rule makes on purpose: a wrong answer about which
//! project a file belongs to would resolve an import onto some *other* file of
//! the same relative path, which is a silently wrong graph (D2.1). An import
//! that resolves to nothing is loud, local and repairable with `--root`.
//!
//! # What is printed
//!
//! Discovering the project reads every file under the root; **reporting is
//! narrowed to the paths that were asked about.** A path that is a directory
//! reports its whole subtree. Findings that belong to no discovered file — an
//! unreadable directory, say — are always printed, because nothing else would
//! carry them.
//!
//! # One map, one render, one decision about severity
//!
//! Two invariants hold this together and each is load-bearing rather than
//! decorative:
//!
//! **One [`SourceMap`] for the whole invocation.** A [`FileId`] is an index
//! into it, so a second map would restart at `FileId(0)` and every span from
//! the first project would then name the second project's files. That is only
//! a real constraint because the report is rendered **once, at the end**, from
//! one accumulated collection: a per-group render would resolve each group's
//! spans against that group's own map and the invariant would buy nothing.
//! Rendering once is also what makes `--allow`/`--deny` counting and the exit
//! code cover the whole invocation rather than a group of it.
//!
//! **Severity is decided once, by the pass that raised the finding.** The map
//! is handed to `discover`, `link_with` and `check_with`, which is where it
//! must be: `allow` suppresses *recording*, and a collection cannot un-record
//! a diagnostic it never received. Everything here merges with
//! [`Diagnostics::absorb`], which keeps the severity each item already carries,
//! so there is exactly one place that answers "how serious is this".
//!
//! `--dump` prints each project's arenas as that project is checked, so the
//! report follows the dumps rather than preceding them. The dumps are a debug
//! aid tied to one project; the report is the invocation's.

use std::collections::HashSet;
use std::io::{StdoutLock, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tracing::{debug, info};
use yfi_config::Config;
use yfi_core::check::check_with;
use yfi_core::link::link_with;
use yfi_core::{discover_in, intern, DiscoverOptions, FileClass, Project};
use yfi_syntax::{parse_file, Diagnostics, FileId, Severity, SeverityMap, SourceMap};

/// Check every path. Exit code is 0 when no error-level diagnostic was raised
/// for the paths asked about, 1 otherwise.
pub fn run(config: &Config, paths: &[PathBuf], dump: bool, root: Option<&Path>) -> ExitCode {
    let options = DiscoverOptions { parse: config.parse_options(), ..DiscoverOptions::default() };
    let mut run = Run {
        severities: options.parse.severities.clone(),
        options,
        dump,
        found: Diagnostics::new(),
        // One map for the whole invocation; see the module documentation.
        sources: SourceMap::new(),
        out: std::io::stdout().lock(),
    };
    for group in group_by_root(paths, root) {
        run.group(&group);
    }
    run.finish()
}

/// The paths of one invocation that share a project root.
struct Group {
    root: PathBuf,
    paths: Vec<PathBuf>,
}

/// Bucket the requested paths by the project each belongs to, first appearance
/// first, so the report comes out in the order the paths were written.
fn group_by_root(paths: &[PathBuf], root: Option<&Path>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for path in paths {
        let root = root.map_or_else(|| project_root(path), Path::to_path_buf);
        match groups.iter_mut().find(|group| group.root == root) {
            Some(group) => group.paths.push(path.clone()),
            None => groups.push(Group { root, paths: vec![path.clone()] }),
        }
    }
    groups
}

/// The project a path is checked as part of. See the module documentation for
/// why this is the parent directory and not something cleverer.
fn project_root(path: &Path) -> PathBuf {
    if path.is_dir() {
        return path.to_path_buf();
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// One invocation's accumulating state.
struct Run {
    options: DiscoverOptions,
    severities: SeverityMap,
    dump: bool,
    /// Every finding of every group, already carrying its decided severity and
    /// waiting for the one render in [`Run::finish`].
    found: Diagnostics,
    sources: SourceMap,
    out: StdoutLock<'static>,
}

impl Run {
    /// Check one project's worth of requested paths.
    fn group(&mut self, group: &Group) {
        if !group.root.is_dir() {
            // There is no tree to walk, so there is no project. Read the paths
            // themselves, which is what reports *why* — an absent file is
            // `E0102` at its own path, not at a directory nobody named.
            for path in &group.paths {
                self.loose(path);
            }
            return;
        }
        let sources = std::mem::take(&mut self.sources);
        let project = discover_in(sources, &group.root, &self.options);
        let selection = select(&project, &group.paths);
        // Every pass, not just the two that read files. `discover` and `parse`
        // own a handful of codes; `link` and `check` own most of them, and a
        // compiler whose semantic errors never reach the command line is one
        // whose errors nobody can see.
        let interned = intern(&project);
        let linked = link_with(&project, &interned, self.severities.clone());
        let checked = check_with(&project, &interned, &linked, self.severities.clone());
        debug!(
            root = %group.root.display(),
            files = project.files().len(),
            reported = selection.reported.len(),
            "checked project"
        );
        self.collect(&project, &selection, linked.diagnostics(), checked.diagnostics());
        self.sources = project.into_sources();
        for path in &selection.absent {
            self.loose(path);
        }
    }

    /// Take the selected files' diagnostics into the invocation's collection,
    /// and print their arenas when asked.
    ///
    /// The three collections are kept apart by the passes that raise them and
    /// are merged here so one render, one count and one exit code cover all of
    /// them. Merge order does not decide print order: [`Diagnostics::render`]
    /// sorts by position, so a reader gets a file read top to bottom rather
    /// than grouped by which pass happened to find what.
    ///
    /// Severity is not re-decided. Each pass was handed the configuration and
    /// has already applied it, `Allow` included, so absorbing is the whole job.
    fn collect(
        &mut self,
        project: &Project,
        selection: &Selection,
        linked: &Diagnostics,
        checked: &Diagnostics,
    ) {
        let passes = [project.diagnostics(), linked, checked];
        let selected = passes
            .iter()
            .flat_map(|held| held.items())
            .filter(|item| !item.span.is_some_and(|span| selection.hidden.contains(&span.file)))
            .cloned();
        self.found.absorb(selected);
        if !self.dump {
            return;
        }
        for file in project.files().iter().filter(|f| selection.reported.contains(&f.id)) {
            let _ = write!(self.out, "{}", file.ast.dump());
        }
    }

    /// Read a path the project does not contain — one whose extension the
    /// project ignores, or one that cannot be read at all.
    ///
    /// Into the same map as everything else, so its spans render beside the
    /// projects' at the end rather than against a map of their own.
    fn loose(&mut self, path: &Path) {
        let dialect = self.options.class_of(path).unwrap_or(FileClass::Data).dialect();
        let parsed = parse_file(&mut self.sources, path, &self.options.parse, dialect);
        info!(path = %path.display(), nodes = parsed.ast.nodes().len(), "parsed outside a project");
        if self.dump {
            let _ = write!(self.out, "{}", parsed.ast.dump());
        }
        self.found.extend(parsed.diagnostics);
    }

    /// Render the whole invocation once, against the one map, and decide the
    /// exit code from what it holds.
    fn finish(mut self) -> ExitCode {
        let text = self.found.render(&self.sources);
        let errors = self.found.error_count();
        let warnings = count(&self.found, Severity::Warning);
        let _ = write!(self.out, "{text}");
        let _ = writeln!(self.out, "{errors} error(s), {warnings} warning(s)");
        if errors == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

/// Which of a project's files this invocation asked about.
struct Selection {
    /// Files the caller named, directly or by naming a directory above them.
    reported: HashSet<FileId>,
    /// Discovered files the caller did not name. Their findings are suppressed;
    /// anything with no discovered file behind it is not, which is why this is
    /// the complement rather than the selection.
    hidden: HashSet<FileId>,
    /// Requested paths the project does not contain.
    absent: Vec<PathBuf>,
}

fn select(project: &Project, paths: &[PathBuf]) -> Selection {
    let wanted: Vec<PathBuf> = paths.iter().map(|path| identity(path)).collect();
    let mut reported = HashSet::new();
    let mut hidden = HashSet::new();
    let mut matched = vec![false; wanted.len()];
    for file in project.files() {
        let real = identity(&file.path);
        let hits: Vec<usize> =
            (0..wanted.len()).filter(|index| real.starts_with(&wanted[*index])).collect();
        if hits.is_empty() {
            hidden.insert(file.id);
            continue;
        }
        reported.insert(file.id);
        for index in hits {
            matched[index] = true;
        }
    }
    let absent = paths
        .iter()
        .zip(&matched)
        // A named directory holding no accepted file is an empty project, not a
        // missing one; reading it as a file would invent an `E0102` about it.
        .filter(|(path, hit)| !**hit && !path.is_dir())
        .map(|(path, _)| path.clone())
        .collect();
    Selection { reported, hidden, absent }
}

/// A path's real identity, so a symlinked or `./`-prefixed spelling of a file
/// still matches the one the walk discovered.
fn identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn count(diagnostics: &Diagnostics, severity: Severity) -> usize {
    diagnostics.items().iter().filter(|d| d.severity == severity).count()
}
