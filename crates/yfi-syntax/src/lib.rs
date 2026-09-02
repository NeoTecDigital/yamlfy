// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication's syntax layer: YAML events in, arena AST and diagnostics out.
//!
//! # Why events and not a loader
//!
//! A document-level YAML loader returns an owned recursive enum. In Rust that
//! type *is* a tree, so it cannot represent a cyclic alias graph at all; its
//! only options are to expand aliases by deep copy or to diverge. Yamlfication
//! is a graph database, so cycles are the point, not an edge case. This crate
//! therefore consumes the event stream, where an alias is reported as an anchor
//! id and nothing else, and stores it as a [`ast::AliasRef`] pointing into a
//! flat arena indexed by `u32`.
//!
//! # What it guarantees
//!
//! * A cyclic alias graph parses without copying, recursing or diverging.
//! * Every node carries a [`span::Span`] with a byte offset, a one-based line
//!   and a one-based column, so every diagnostic can print `file:line:col`.
//! * Diagnostics accumulate. Nothing returns on the first problem.
//!
//! # Two dialects, one parser
//!
//! `.yfy` is not YAML. `//`, `<?-- … --!>` and `<?-- … -->` are constructs a
//! YAML parser rejects, so a [`Dialect::Yamlfication`] file is rewritten by
//! [`front`] before the parser reads it — character for character, so that
//! every span still points at the file the author wrote. A
//! [`Dialect::BaseYaml`] file gets none of that and reaches the parser exactly
//! as written (D6.6).
//!
//! # One file, and one exception
//!
//! An [`ast::Ast`] is one file's arena and stays one. The single thing that
//! crosses a file boundary is a **binding**: [`parse::parse_with_imports`]
//! takes the definitions a header imported and installs them into every
//! document of the file being parsed (D6.7), so an ordinary alias reaches them.
//! Such an [`anchor::AnchorDef`] carries the span it was written at, in the
//! file that wrote it, and names a node of *that* file's arena — which is why
//! [`ast::Ast::alias_target`] answers `None` for one and
//! [`ast::Ast::alias_binding`] answers with a [`span::FileId`] as well as a
//! node. The crate still reads no directory, resolves no path and knows nothing
//! of projects; it is told what to bind.
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
pub mod front;
pub mod diagnostic;
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
