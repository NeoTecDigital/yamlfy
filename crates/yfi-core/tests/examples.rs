// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every project under `examples/` compiles, and compiles to what its
//! `README.md` says it does.
//!
//! Examples are documentation, and documentation rots. These tests are what
//! stops it: an example that stops compiling, or that stops raising the one
//! diagnostic its README explains, fails here rather than being discovered by
//! the first person who trusts it.
//!
//! Note what is asserted about the two examples that are *not* silent. Their
//! warnings are the point being made, so the assertion is on the exact code and
//! count. Loosening either to "compiles" would let the demonstration disappear
//! while the test still passed.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use common::pipeline::Compiled;
use yfi_syntax::Code;

/// Every directory under `examples/`, so a new example is covered by being
/// written rather than by being remembered.
fn examples() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(&root).expect("examples/ exists").flatten() {
        if entry.path().is_dir() {
            out.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    assert!(!out.is_empty(), "examples/ holds no example");
    out.into_iter().collect()
}

fn open(name: &str) -> Compiled {
    common::pipeline::open_at(&format!("examples/{name}"))
}

#[test]
fn every_example_compiles_without_an_error() {
    for name in examples() {
        let fixture = open(&name);
        assert!(
            !fixture.linked.diagnostics().has_errors()
                && !fixture.checked.diagnostics().has_errors(),
            "examples/{name} does not compile:\n{}",
            fixture.rendered()
        );
    }
}

#[test]
fn every_example_carries_a_readme() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for name in examples() {
        assert!(
            root.join(&name).join("README.md").is_file(),
            "examples/{name} has no README.md; an example nobody can read is a fixture"
        );
    }
}

#[test]
fn the_operators_example_still_shows_an_undeclared_field() {
    // Its README explains this warning at length. If `reagent` ever becomes
    // declared, the explanation is describing something that no longer happens.
    let fixture = open("01-three-operators");
    assert_eq!(fixture.count(Code::UndeclaredField), 1, "{}", fixture.rendered());
}

#[test]
fn the_states_example_still_shows_a_state_transition() {
    // The whole example is that redefining an anchor is a transition and is
    // reported as one. A silent version of it would demonstrate nothing.
    let fixture = open("04-cycles-and-states");
    assert_eq!(fixture.count(Code::AnchorShadowed), 1, "{}", fixture.rendered());
}
