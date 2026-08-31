// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! Arbitrary bytes must never panic the parser.
//!
//! Run with a nightly toolchain, seeding from the real corpus so the fuzzer
//! starts from valid YAML rather than noise:
//!
//! ```sh
//! cargo +nightly fuzz run parse fixtures
//! ```
//!
//! The seed corpus is `fixtures/` itself; it is not copied.

#![no_main]

use libfuzzer_sys::fuzz_target;
use yamlfy_syntax::{parse, ParseOptions, SourceMap};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut sources = SourceMap::new();
    let file = sources.add("fuzz.yml", text.as_ref());
    let parsed = parse(&sources, file, &ParseOptions::default());

    // Exercise everything a caller would touch, including graph traversal,
    // which is where an alias cycle would otherwise diverge.
    let _ = parsed.diagnostics.render(&sources);
    let _ = parsed.ast.dump();
    for document in parsed.ast.documents() {
        let _ = parsed.ast.reachable_from(document.root);
        let _ = parsed.ast.is_cyclic_from(document.root);
    }
});
