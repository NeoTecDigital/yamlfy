// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication's semantic layer.
//!
//! `yfi-syntax` is a pure front end and stays one: its [`Ast`] is read-only
//! outside its own crate and is not `Clone`. Everything here is therefore a
//! **side structure** keyed by `(FileId, NodeId)` rather than a mutation of the
//! arena. The image *is* the side table.
//!
//! [`discover`] is pass 1, [`intern`] 3, [`link`] 4, [`check`] 5 and [`emit`]
//! 6; §9 tables what each owns. Pass 2 is `yfi_syntax::parse`, driven by pass 1
//! — and driven **twice** for a file that imports anything, because a header
//! can only be read from a parse and its imports must be installed before that
//! file's first document event (`bind`, pass 1b).
//!
//! # Example
//!
//! ```no_run
//! use yfi_core::{discover, intern, DiscoverOptions};
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
pub mod edge;
pub mod emit;
pub mod header;
pub mod image;
pub mod intern;
pub mod link;
pub mod member;
pub mod scope;
pub mod symbol;
pub mod tags;

pub use discover::{
    discover, discover_in, identity, DiscoverOptions, FileClass, Project, ProjectFile,
    DEFAULT_DATA_EXTENSIONS, DEFAULT_SOURCE_EXTENSIONS,
};
pub use edge::{CONNECTIONS, DEFINITION};
pub use emit::emit;
pub use header::Header;
// `Edge` and `EdgeKind` are deliberately absent: `link` and `image` each define
// a type of both names and they are not the same set of kinds. Both are reached
// through their own module, so `use yfi_core::EdgeKind` cannot silently pick one.
pub use image::{FieldView, Image, ModelId, ModelKind, ModelView, Named};
pub use intern::{intern, FileIndex, Interned, Member};
pub use member::MemberFlags;
pub use scope::{Declared, Mutability, Scope, ScopeId, ScopeKind, ScopeTree, Visibility};
pub use symbol::{Symbol, SymbolTable};
pub use tags::{classify, TagKind};
