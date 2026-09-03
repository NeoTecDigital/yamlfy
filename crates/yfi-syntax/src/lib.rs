// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication's syntax layer: YAML events in, arena AST and diagnostics out.
//!
//! **An alias is a reference, never a copy** (§0), so this crate consumes the
//! event stream rather than a loader and stores an alias as an
//! [`ast::AliasRef`] into a flat `u32`-indexed arena. Every node carries a
//! [`span::Span`] with a byte offset and one-based line and column, so every
//! diagnostic can print `file:line:col`; diagnostics accumulate, and nothing
//! returns on the first problem.
//!
//! `.yfy` is not YAML — `//`, `<?-- … --!>` and `<?-- … -->` are constructs a
//! YAML parser rejects — so a [`Dialect::Yamlfication`] file is rewritten by
//! [`front`] before the parser reads it, character for character, leaving every
//! span pointing at the file the author wrote. A [`Dialect::BaseYaml`] file
//! reaches the parser exactly as written (D6.6).
//!
//! An [`ast::Ast`] is one file's arena and stays one. The single thing that
//! crosses a file boundary is a **binding**, installed into every document by
//! [`parse::parse_with_imports`] (D6.7). Such an [`anchor::AnchorDef`] names a
//! node of the *writing* file's arena, which is why [`ast::Ast::alias_target`]
//! answers `None` for one and [`ast::Ast::alias_binding`] answers with a
//! [`span::FileId`] as well as a node. The crate still reads no directory,
//! resolves no path and knows nothing of projects; it is told what to bind.
//!
//! # Example
//!
//! ```
//! use yfi_syntax::{parse, ParseOptions, SourceMap};
//!
//! let mut sources = SourceMap::new();
//! let file = sources.add("ring.yml", "--- &ring\nself: *ring\n");
//! let parsed = parse(&sources, file, &ParseOptions::default());
//!
//! assert!(!parsed.diagnostics.has_errors());
//! let root = parsed.ast.documents()[0].root;
//! assert!(parsed.ast.is_cyclic_from(root));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod anchor;
pub mod ast;
pub mod diagnostic;
pub mod front;
pub mod mapping;
pub mod parse;
pub mod span;

mod builder;
mod scan;
mod walk;

pub use anchor::{AnchorDef, AnchorId, AnchorTable, Source};
pub use ast::{AliasRef, Ast, Document, Entry, Node, NodeId, NodeKind, Scalar, ScalarStyle, Tag};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Severity, SeverityMap};
pub use front::{Block, BlockKind, Dialect};
pub use mapping::{is_merge_key, MERGE_KEY};
pub use parse::{
    anchor_names, parse, parse_file, parse_with_imports, Import, ParseOptions, Parsed,
};
pub use span::{FileId, LoadError, Pos, SourceFile, SourceMap, Span};
