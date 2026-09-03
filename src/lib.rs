// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Yamlfication: one dependency that re-exports the three layers.
//!
//! No code of its own. A caller wanting a file parsed with spans renderable
//! calls [`syntax::parse_file`] against its own [`syntax::SourceMap`]; a caller
//! wanting a project calls [`core::discover()`].

pub use yfi_config as config;
pub use yfi_core as core;
pub use yfi_syntax as syntax;
