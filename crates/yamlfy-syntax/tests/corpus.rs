// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Guards on the shape of the fixture corpus itself.
//!
//! Three integration tests iterate every fixture, so anything that lands in
//! `fixtures/` becomes part of the specification by accident. A fuzz run once
//! pointed libFuzzer at `fixtures/` as its corpus directory and grew it from 47
//! files to 3,539; the `.gitignore` guard now catches that, and this test is the
//! cheaper detector that fails loudly the moment the count moves.
//!
//! The corpus holds both file classes. That is deliberate and it changes
//! nothing here: **the parser is shared.** Classification into Yamlfication
//! source and base YAML happens in `yamlfy-core::discover`, not in this crate,
//! and every `.yfy` construct so far is still valid YAML — so the front end
//! parses both identically and `all_fixtures()` keeps sweeping both.
//!
//! The counts are asserted per class rather than in total, so a stray file
//! landing in either one is still caught.

mod common;

/// Base YAML fixtures: the parser-level corpus.
const EXPECTED_YML: usize = 45;

/// Yamlfication source fixtures: the surface syntax of the language.
const EXPECTED_YFY: usize = 2;

/// Changing any of these is a deliberate act; update them in the same commit.
const EXPECTED_FIXTURES: usize = EXPECTED_YML + EXPECTED_YFY;

fn count_with(suffix: &str) -> usize {
    common::all_fixtures().iter().filter(|f| f.ends_with(suffix)).count()
}

#[test]
fn the_corpus_has_exactly_the_fixtures_it_is_meant_to() {
    let fixtures = common::all_fixtures();
    assert_eq!(
        fixtures.len(),
        EXPECTED_FIXTURES,
        "fixture count moved; if that was intended, update the constants:\n{fixtures:#?}"
    );
}

#[test]
fn each_class_holds_exactly_the_fixtures_it_is_meant_to() {
    assert_eq!(count_with(".yml"), EXPECTED_YML, "base YAML fixture count moved");
    assert_eq!(count_with(".yfy"), EXPECTED_YFY, "Yamlfication source fixture count moved");
}

#[test]
fn every_fixture_belongs_to_one_of_the_two_classes() {
    for relative in common::all_fixtures() {
        assert!(
            relative.ends_with(".yml") || relative.ends_with(".yfy"),
            "`{relative}` is neither base YAML nor Yamlfication source; fuzz artefacts and \
             scratch files do not belong in the corpus"
        );
    }
}
