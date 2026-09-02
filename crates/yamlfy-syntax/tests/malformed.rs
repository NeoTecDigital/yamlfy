// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The malformed corpus: clean diagnostics, never a panic, never a silent pass.

mod common;

use common::{count, parse, parse_with};
use yamlfy_syntax::{Code, ParseOptions, SourceMap};

#[test]
fn every_malformed_fixture_is_reported_and_none_panics() {
    for fixture in [
        "malformed/alias-before-definition.yml",
        "malformed/bad-indent.yml",
        "malformed/duplicate-key.yml",
        "malformed/invalid-utf8.yml",
        "malformed/multi-error-multidoc.yml",
        "malformed/tab-after-colon.yml",
        "malformed/unclosed-flow-sequence.yml",
        "malformed/unclosed-quote.yml",
        "malformed/unknown-alias.yml",
    ] {
        let (sources, parsed) = parse(fixture);
        assert!(parsed.diagnostics.has_errors(), "{fixture} should be rejected");
        let rendered = parsed.diagnostics.render(&sources);
        assert!(rendered.contains(".yml:"), "{fixture} must report file:line:col, got {rendered}");
    }
}

#[test]
fn an_empty_file_is_not_an_error() {
    let (_, parsed) = parse("malformed/empty.yml");
    assert!(parsed.diagnostics.is_empty());
    assert!(parsed.ast.documents().is_empty());
}

#[test]
fn every_duplicate_key_is_reported_not_just_the_first() {
    let (sources, parsed) = parse("malformed/duplicate-key.yml");
    assert_eq!(
        count(&parsed.diagnostics, Code::DuplicateKey),
        2,
        "{}",
        parsed.diagnostics.render(&sources)
    );
    let locations: Vec<String> = parsed
        .diagnostics
        .with_code(Code::DuplicateKey)
        .map(|d| sources.location(d.span.unwrap()))
        .collect();
    assert!(locations[0].ends_with(":4:1"), "{locations:?}");
    assert!(locations[1].ends_with(":6:1"), "{locations:?}");
}

#[test]
fn recovery_reports_a_syntax_error_in_each_broken_document() {
    let (_, parsed) = parse("malformed/multi-error-multidoc.yml");
    assert_eq!(count(&parsed.diagnostics, Code::SyntaxError), 2);
    assert_eq!(
        parsed.ast.documents().len(),
        1,
        "the one intact document is still parsed and kept"
    );
}

#[test]
fn recovery_can_be_bounded() {
    let options = ParseOptions { max_recovery_attempts: 0, ..ParseOptions::default() };
    let (_, parsed) = parse_with("malformed/multi-error-multidoc.yml", &options);

    assert_eq!(count(&parsed.diagnostics, Code::SyntaxError), 1);
    assert_eq!(count(&parsed.diagnostics, Code::RecoveryLimitExceeded), 1);
}

#[test]
fn a_file_that_is_not_utf8_is_a_diagnostic_not_a_crash() {
    let (_, parsed) = parse("malformed/invalid-utf8.yml");
    assert_eq!(count(&parsed.diagnostics, Code::InvalidUtf8), 1);
    assert!(parsed.ast.nodes().is_empty());
}

#[test]
fn a_missing_file_is_a_diagnostic_not_a_crash() {
    let mut sources = SourceMap::new();
    let parsed = yamlfy_syntax::parse_file(
        &mut sources,
        common::fixtures().join("no/such/file.yml"),
        &ParseOptions::default(),
        yamlfy_syntax::Dialect::BaseYaml,
    );
    assert_eq!(count(&parsed.diagnostics, Code::IoError), 1);
    assert!(parsed.diagnostics.render(&sources).contains("file.yml:1:1"));
}

#[test]
fn a_forward_reference_is_rejected() {
    // `*later` before `&later` is an unknown anchor, which is what positional
    // resolution means at the boundary.
    let (sources, parsed) = parse("malformed/alias-before-definition.yml");
    let rendered = parsed.diagnostics.render(&sources);
    assert!(rendered.contains("unknown anchor"), "{rendered}");
    assert!(rendered.contains(":4:6"), "{rendered}");
}

#[test]
fn diagnostics_from_the_whole_corpus_never_panic() {
    for relative in common::all_fixtures() {
        let (sources, parsed) = common::parse(&relative);
        let _ = parsed.diagnostics.render(&sources);
        let _ = parsed.ast.dump();
    }
}
