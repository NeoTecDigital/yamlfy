// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for the project corpus.
//!
//! `fixtures/` holds single-file fixtures and is driven by
//! `yfi-syntax/tests/common::all_fixtures`, which walks exactly one
//! directory level and treats every file as an independent parse. Cross-file
//! behaviour cannot be expressed there, so project fixtures live in a **sibling
//! tree**, `projects/<name>/`, where one directory is one compilable project.
//! Keeping them apart is what leaves `fixtures/` and the three corpus-wide
//! invariants that walk it untouched.
//!
//! Both corpora hold both file classes. `.yfy` is Yamlfication source and
//! `.yml`/`.yaml` is base YAML; the front end parses them identically, and only
//! `discover` tells them apart.

#![allow(dead_code)]

use std::path::PathBuf;

use yfi_core::{discover, DiscoverOptions, Project, ScopeId};
use yfi_syntax::{Code, Diagnostics, FileId, NodeId};

/// Repository root.
pub fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Root of the project corpus.
pub fn projects() -> PathBuf {
    repository().join("projects")
}

/// Discover any path in the repository as a project. Used to point a project
/// at a single-file fixture, which is legal: a file is a project of one file.
pub fn open_at(relative: &str) -> Project {
    discover(repository().join(relative), &DiscoverOptions::default())
}

/// Discover a project fixture by name.
pub fn open(name: &str) -> Project {
    open_with(name, &DiscoverOptions::default())
}

/// Discover a project fixture with explicit options.
pub fn open_with(name: &str, options: &DiscoverOptions) -> Project {
    discover(projects().join(name), options)
}

/// Discover a project fixture, asserting it raised nothing at all.
pub fn open_clean(name: &str) -> Project {
    let project = open(name);
    assert!(
        project.diagnostics().is_empty(),
        "{name} was expected to be clean:\n{}",
        project.diagnostics().render(project.sources())
    );
    project
}

/// Number of diagnostics carrying `code`.
pub fn count(diagnostics: &Diagnostics, code: Code) -> usize {
    diagnostics.with_code(code).count()
}

/// The relative paths of every discovered file, in rank order.
pub fn relative_paths(project: &Project) -> Vec<String> {
    project.files().iter().map(|f| f.relative.display().to_string()).collect()
}

/// The scope whose `root/dir/sub` name is `qualified`.
pub fn scope_by(project: &Project, qualified: &str) -> ScopeId {
    project
        .scopes()
        .scopes()
        .iter()
        .find(|s| project.scopes().qualified(s.id) == qualified)
        .unwrap_or_else(|| panic!("no scope `{qualified}`"))
        .id
}

/// The file whose relative path ends with `ends_with`. Test fixtures name a
/// file by its own name and its directory, never by rank, so a fixture can gain
/// a file without renumbering every assertion about it.
pub fn file_id(project: &Project, ends_with: &str) -> FileId {
    project
        .files()
        .iter()
        .find(|f| f.relative.ends_with(ends_with))
        .unwrap_or_else(|| panic!("no file ending `{ends_with}`"))
        .id
}

/// The node reached from `document`'s root by following `path` key by key.
pub fn entry_at(project: &Project, file: FileId, document: usize, path: &[&str]) -> NodeId {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    let mut at = ast.documents()[document].root;
    for key in path {
        at = ast
            .entries(at)
            .unwrap_or_else(|| panic!("{at:?} of {file:?} is not a mapping"))
            .iter()
            .find(|e| ast.scalar(e.key).is_some_and(|s| s.value.as_ref() == *key))
            .unwrap_or_else(|| panic!("no key `{key}`"))
            .value;
    }
    at
}

/// The node the last definition of `name` written in `file` names.
pub fn declaration(project: &Project, file: FileId, name: &str) -> NodeId {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    ast.anchors()
        .defs()
        .iter()
        .filter(|d| !d.is_imported() && &*d.name == name)
        .next_back()
        .unwrap_or_else(|| panic!("`&{name}` is not defined in {file:?}"))
        .node
}

/// The names a header import installed into `file`, in authored order, each
/// listed once however many parser segments re-installed it.
pub fn imported_names(project: &Project, file: FileId) -> Vec<String> {
    let ast = &project.file(file).unwrap_or_else(|| panic!("no file {file:?}")).ast;
    let mut out: Vec<String> = Vec::new();
    for def in ast.anchors().defs().iter().filter(|d| d.is_imported()) {
        let name = def.name.to_string();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// A project taken all the way through pass 5, and the questions a pass-5 test
/// asks of one. Kept here rather than in one test binary because the check-pass
/// assertions split across two files and a second copy would drift.
pub mod pipeline {
    use yfi_core::check::{check, Checked};
    use yfi_core::intern::{intern, Interned};
    use yfi_core::link::{link, Linked};
    use yfi_core::{Project, Symbol};
    use yfi_syntax::{Code, FileId, NodeId};

    /// A project taken all the way through pass 5.
    pub struct Compiled {
        pub project: Project,
        pub interned: Interned,
        pub linked: Linked,
        pub checked: Checked,
    }

    impl Compiled {
        /// Every diagnostic the whole pipeline raised, not one pass's.
        ///
        /// `link` and `check` each keep their own collection, so asking only
        /// the later one silently loses every code the earlier one owns —
        /// `E0211`, `E0213`, `E0214`, `W0303` among them. A harness that cannot
        /// see half the diagnostics makes a test that asserts one of them pass
        /// or fail for the wrong reason.
        pub fn rendered(&self) -> String {
            let mut out = String::new();
            for held in
                [self.project.diagnostics(), self.linked.diagnostics(), self.checked.diagnostics()]
            {
                out.push_str(&held.render(self.project.sources()));
            }
            out
        }

        pub fn count(&self, code: Code) -> usize {
            [self.project.diagnostics(), self.linked.diagnostics(), self.checked.diagnostics()]
                .iter()
                .map(|held| super::count(held, code))
                .sum()
        }

        pub fn file(&self, ends_with: &str) -> FileId {
            super::file_id(&self.project, ends_with)
        }

        /// The node an anchor names, wherever in the project it is written.
        pub fn node(&self, file: &str, anchor: &str) -> (FileId, NodeId) {
            let id = self.file(file);
            (id, super::declaration(&self.project, id, anchor))
        }

        pub fn symbol(&self, text: &str) -> Symbol {
            self.interned.symbols().get(text).unwrap_or_else(|| panic!("`{text}` is never written"))
        }

        /// The keys of a node's resolved view, highest precedence first.
        pub fn resolved_keys(&self, at: (FileId, NodeId)) -> Vec<String> {
            self.view_keys(self.checked.resolved(at.0, at.1))
        }

        /// The keys a node declares.
        pub fn declared_keys(&self, at: (FileId, NodeId)) -> Vec<String> {
            self.view_keys(self.checked.declared(at.0, at.1))
        }

        pub fn view_keys(&self, view: Option<&yfi_core::check::View>) -> Vec<String> {
            view.expect("a view")
                .fields()
                .iter()
                .map(|field| {
                    self.interned.symbols().resolve(field.name).unwrap_or_default().to_owned()
                })
                .collect()
        }

        /// The text a node's resolved view ends up holding for a key.
        pub fn value_of(&self, at: (FileId, NodeId), key: &str) -> String {
            let field = self
                .checked
                .resolved(at.0, at.1)
                .expect("a view")
                .get(self.symbol(key))
                .unwrap_or_else(|| panic!("no key `{key}`"));
            let ast = &self.project.file(field.value.0).expect("file").ast;
            ast.scalar(field.value.1).map_or_else(String::new, |s| s.value.to_string())
        }
    }

    pub fn through(project: Project) -> Compiled {
        let interned = intern(&project);
        let linked = link(&project, &interned);
        let checked = check(&project, &interned, &linked);
        Compiled { project, interned, linked, checked }
    }

    /// Discover, intern, link and check a project fixture, asserting `discover`
    /// found nothing — so every diagnostic in a test below belongs to pass 4 or 5.
    pub fn open(name: &str) -> Compiled {
        through(super::open_clean(name))
    }

    /// The same for a single-file fixture, which is a project of one file.
    pub fn open_at(relative: &str) -> Compiled {
        through(super::open_at(relative))
    }
}

/// A scratch project tree that removes itself. Built in code rather than
/// checked in because the thing under test is a *symlink*, which a source
/// corpus cannot carry portably.
#[cfg(unix)]
pub mod scratch {
    use std::path::{Path, PathBuf};

    pub struct Tree(PathBuf);

    impl Tree {
        pub fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            path.push(format!("yamlfy-{name}-{unique}"));
            std::fs::create_dir_all(&path).expect("scratch tree");
            Tree(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write(&self, relative: &str, body: &str) {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("scratch directory");
            }
            std::fs::write(path, body).expect("scratch file");
        }

        pub fn link(&self, target: &str, link: &str) {
            let path = self.0.join(link);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("scratch directory");
            }
            std::os::unix::fs::symlink(self.0.join(target), path).expect("scratch symlink");
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
