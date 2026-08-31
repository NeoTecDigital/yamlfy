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
//! # Example
//!
//! ```
//! use yamlfy_syntax::{parse, ParseOptions, SourceMap};
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
pub mod mapping;
pub mod parse;
pub mod span;

mod builder;
mod scan;
mod walk;

pub use anchor::{AnchorDef, AnchorId, AnchorTable};
pub use ast::{AliasRef, Ast, Document, Entry, Node, NodeId, NodeKind, Scalar, ScalarStyle, Tag};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Severity, SeverityMap};
pub use mapping::{is_merge_key, MERGE_KEY};
pub use parse::{parse, parse_file, ParseOptions, Parsed};
pub use span::{FileId, LoadError, Pos, SourceFile, SourceMap, Span};
