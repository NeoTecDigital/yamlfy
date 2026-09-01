// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication: the umbrella entry point.
//!
//! Re-exports the layers and offers the one convenience the layers deliberately
//! do not: read a file, parse it, and hand back everything needed to print
//! `file:line:col` diagnostics.

pub use yamlfy_config as config;
pub use yamlfy_core as core;
pub use yamlfy_syntax as syntax;

use yamlfy_syntax::{parse_file, ParseOptions, Parsed, SourceMap};

/// Parse `path`, returning the source registry alongside the result so spans
/// can be rendered.
///
/// Read and encoding failures are reported as diagnostics, not as a `Result`.
pub fn check_file(path: impl AsRef<std::path::Path>, options: &ParseOptions) -> (SourceMap, Parsed) {
    let mut sources = SourceMap::new();
    let parsed = parse_file(&mut sources, path, options);
    (sources, parsed)
}
