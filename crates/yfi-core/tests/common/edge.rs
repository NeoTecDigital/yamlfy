// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Helpers shared by the two `!edge` test binaries.
//!
//! D4.13's claims split into two files — `tests/edges.rs` reads the **model**
//! back out of a compiled project, `tests/edge_faults.rs` reads the
//! **diagnostics** — and both ask a compiled fixture the same four questions.
//! Kept here rather than copied, because a second copy of `endpoints` that
//! drifted would let one file assert something the other had stopped checking.

use yfi_core::emit::emit;
use yfi_core::image::{Image, ModelId};
use yfi_core::{ScopeId, Symbol};

use super::pipeline::Compiled;

/// Every edge fixture that is meant to compile without a word said about it.
/// Two corpus-wide sweeps read it, so a fixture added here is held to both.
pub const CLEAN: [&str; 8] = [
    "edge-binary",
    "edge-nary",
    "edge-handles",
    "edge-extends",
    "edge-visibility",
    "edge-mixin",
    "edge-not-a-reach",
    "edge-shared-sequence",
];

/// A project taken all the way through pass 6.
pub fn image(fixture: &Compiled) -> Image<'_> {
    emit(&fixture.project, &fixture.interned, &fixture.linked, &fixture.checked)
}

/// The node an anchor names, whatever its kind.
pub fn by_name<'a>(image: &'a Image<'a>, name: &str) -> ModelId {
    image
        .nodes()
        .find(|held| held.name() == Some(name))
        .unwrap_or_else(|| panic!("no node called `{name}`"))
        .id()
}

/// The anchor names of the nodes an edge connects, in written order.
pub fn endpoints<'a>(image: &'a Image<'a>, edge: &str) -> Vec<String> {
    image
        .model(by_name(image, edge))
        .expect("a node")
        .connections()
        .map(|held| held.name().unwrap_or("<anonymous>").to_owned())
        .collect()
}

/// An interned name, which is how a handle is addressed.
pub fn symbol(fixture: &Compiled, text: &str) -> Symbol {
    fixture.interned.symbols().get(text).unwrap_or_else(|| panic!("`{text}` is never written"))
}

/// The scope a project fixture's directory claims.
pub fn scope(fixture: &Compiled, qualified: &str) -> ScopeId {
    super::scope_by(&fixture.project, qualified)
}
