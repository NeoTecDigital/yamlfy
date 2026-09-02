// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Arbitrary input must never panic.
//!
//! The libfuzzer target under `fuzz/` needs a nightly toolchain, so the same
//! property is also exercised here deterministically: a fixed-seed generator,
//! a YAML-token generator that reaches deeper into the parser than random bytes
//! do, and structured mutations of every real fixture.

mod common;

use yfi_syntax::{parse, ParseOptions, SourceMap};

/// Deterministic xorshift64\*, so a failure is always reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn parse_text(text: &str) {
    let mut sources = SourceMap::new();
    let file = sources.add("fuzz.yml", text);
    let parsed = parse(&sources, file, &ParseOptions::default());
    let _ = parsed.diagnostics.render(&sources);
    let _ = parsed.ast.dump();
    for document in parsed.ast.documents() {
        let _ = parsed.ast.reachable_from(document.root);
        let _ = parsed.ast.is_cyclic_from(document.root);
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut rng = Rng(0x2026_0830_c0ff_ee01);
    for _ in 0..4000 {
        let len = rng.below(192);
        let bytes: Vec<u8> = (0..len).map(|_| (rng.next() & 0xff) as u8).collect();
        parse_text(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn arbitrary_yaml_tokens_never_panic() {
    const TOKENS: &[&str] = &[
        "---", "...", "&a", "&b", "*a", "*b", "<<", ":", "- ", "[", "]", "{", "}", ",", "\n",
        "  ", "#c\n", "!node", "!!merge", "!ref", "\"", "'", "?", "|", ">", "%YAML 1.2\n", "\t",
        "k", "v", "\u{feff}", "é",
    ];
    let mut rng = Rng(0x2026_0830_c0ff_ee02);
    for _ in 0..6000 {
        let count = rng.below(48);
        let mut text = String::new();
        for _ in 0..count {
            text.push_str(TOKENS[rng.below(TOKENS.len())]);
        }
        parse_text(&text);
    }
}

#[test]
fn mutated_fixtures_never_panic() {
    let mut rng = Rng(0x2026_0830_c0ff_ee03);
    for relative in common::all_fixtures() {
        let path = common::fixtures().join(&relative);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        for _ in 0..40 {
            let mut mutated = bytes.clone();
            mutate(&mut rng, &mut mutated);
            parse_text(&String::from_utf8_lossy(&mutated));
        }
    }
}

fn mutate(rng: &mut Rng, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    let at = rng.below(bytes.len());
    match rng.below(4) {
        0 => {
            bytes.remove(at);
        }
        1 => bytes.insert(at, (rng.next() & 0x7f) as u8),
        2 => bytes[at] = (rng.next() & 0xff) as u8,
        _ => bytes.truncate(at),
    }
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    for depth in [64usize, 512, 20_000] {
        let text = format!("a: {}{}", "[".repeat(depth), "]".repeat(depth));
        parse_text(&text);
        parse_text(&format!("a: {}", "[".repeat(depth)));
    }
}

#[test]
fn a_long_alias_chain_stays_linear() {
    let mut text = String::from("--- &n0\nk0: v\n");
    for i in 1..2000 {
        text.push_str(&format!("k{i}: *n0\n"));
    }
    let mut sources = SourceMap::new();
    let file = sources.add("chain.yml", &text);
    let parsed = parse(&sources, file, &ParseOptions::default());

    assert!(!parsed.diagnostics.has_errors());
    // One node per alias, one per key, plus the root and its single value.
    assert_eq!(parsed.ast.nodes().len(), 2 * 2000 + 1);
    let root = parsed.ast.documents()[0].root;
    assert_eq!(parsed.ast.reachable_from(root).len(), parsed.ast.nodes().len());
}

#[test]
fn a_cyclic_graph_is_traversed_once_not_forever() {
    let text = "--- &a\nself: *a\nnested: &b\n  back: *a\n  me: *b\n";
    let mut sources = SourceMap::new();
    let file = sources.add("ring.yml", text);
    let parsed = parse(&sources, file, &ParseOptions::default());
    let root = parsed.ast.documents()[0].root;

    assert!(parsed.ast.is_cyclic_from(root));
    assert_eq!(parsed.ast.reachable_from(root).len(), parsed.ast.nodes().len());
}
