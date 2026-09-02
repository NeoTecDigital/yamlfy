// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication: the umbrella entry point.
//!
//! Re-exports the layers and offers the one convenience the layers deliberately
//! do not: read a file, parse it, and hand back everything needed to print
//! `file:line:col` diagnostics.

pub use yfi_config as config;
pub use yfi_core as core;
pub use yfi_syntax as syntax;

use yfi_core::{DiscoverOptions, FileClass};
use yfi_syntax::{parse_file, ParseOptions, Parsed, SourceMap};

/// Parse `path`, returning the source registry alongside the result so spans
/// can be rendered.
///
/// The file's class — and therefore whether the `.yfy` front end runs over it —
/// is decided by the default extension lists (D6.6). A caller with its own
/// lists resolves the class itself and calls
/// [`parse_file`](yfi_syntax::parse_file).
///
/// Read and encoding failures are reported as diagnostics, not as a `Result`.
pub fn check_file(
    path: impl AsRef<std::path::Path>,
    options: &ParseOptions,
) -> (SourceMap, Parsed) {
    let path = path.as_ref();
    let class = DiscoverOptions::default().class_of(path).unwrap_or(FileClass::Data);
    let mut sources = SourceMap::new();
    let parsed = parse_file(&mut sources, path, options, class.dialect());
    (sources, parsed)
}
