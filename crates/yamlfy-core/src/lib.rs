// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication's semantic layer.
//!
//! `yamlfy-syntax` is a pure front end and stays one: its [`Ast`] is read-only
//! outside its own crate and is not `Clone`. Everything here is therefore a
//! **side structure** keyed by `(FileId, NodeId)` rather than a mutation of the
//! arena. The image *is* the side table.
//!
//! Two passes live here so far.
//!
//! * [`discover`] — pass 1. Walk a project tree, classify each file as
//!   Yamlfication source (`.yfy`) or base YAML (`.yml`/`.yaml`), load every one
//!   into one [`SourceMap`](yamlfy_syntax::SourceMap), read source headers,
//!   resolve their imports, build the scope tree from the directory hierarchy
//!   and resolve both scope axes. A file that imports anything is then parsed a
//!   second time with those imports bound into it (`bind`, pass 1b), because a
//!   header can only be read from a parse and its imports have to be installed
//!   before that file's first document event.
//! * [`intern`] — pass 3. Intern every key, tag suffix and namespace component;
//!   classify tags; build the node→document and node→parent maps; record each
//!   node's resolved scope path.
//!
//! * [`link`] — pass 4. Build the definition table, walk every path,
//!   validate inheritance-clause operands and build the stratified inheritance
//!   graph pass 5 runs SCC over.
//! * [`check`] — pass 5. Detect cyclic inheritance, resolve inheritance into a
//!   view per node, and validate every concrete node against its abstract
//!   ancestors' declarations.
//!
//! Pass 2 is `yamlfy_syntax::parse`, driven by pass 1. Pass 6 — `emit` — is not
//! written yet and is deliberately absent rather than stubbed.
//!
//! # Example
//!
//! ```no_run
//! use yamlfy_core::{discover, intern, DiscoverOptions};
//!
//! let project = discover::discover("projects/nested-namespaces", &DiscoverOptions::default());
//! let interned = intern::intern(&project);
//! let placement: Vec<(String, String)> = project
//!     .files()
//!     .iter()
//!     .map(|file| {
//!         (file.relative.display().to_string(), project.scopes().qualified(file.scope))
//!     })
//!     .collect();
//! assert_eq!(placement.len(), project.files().len());
//! let _ = interned.symbols().len();
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod bind;
mod claims;
mod imports;
mod reserved;
mod walk;

pub mod check;
pub mod discover;
pub mod header;
pub mod intern;
pub mod link;
pub mod member;
pub mod order;
pub mod scope;
pub mod symbol;
pub mod tags;

pub use discover::{
    discover, discover_in, DiscoverOptions, FileClass, Project, ProjectFile,
    DEFAULT_DATA_EXTENSIONS, DEFAULT_SOURCE_EXTENSIONS,
};
pub use header::Header;
pub use intern::{intern, FileIndex, Interned, Member};
pub use member::MemberFlags;
pub use order::NodeOrder;
pub use scope::{Declared, Mutability, Scope, ScopeId, ScopeKind, ScopeTree, Visibility};
pub use symbol::{Symbol, SymbolTable};
pub use tags::{classify, TagKind};
