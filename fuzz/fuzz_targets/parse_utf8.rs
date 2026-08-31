// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The same property over well-formed UTF-8, which reaches deeper into the
//! parser than random bytes usually do.
//!
//! Run with a nightly toolchain, seeding from the real corpus so the fuzzer
//! starts from valid YAML rather than noise:
//!
//! ```sh
//! cargo +nightly fuzz run parse_utf8 fixtures
//! ```
//!
//! The seed corpus is `fixtures/` itself; it is not copied.

#![no_main]

use libfuzzer_sys::fuzz_target;
use yamlfy_syntax::{parse, ParseOptions, SourceMap};

fuzz_target!(|text: String| {
    let mut sources = SourceMap::new();
    let file = sources.add("fuzz.yml", text);
    let parsed = parse(&sources, file, &ParseOptions::default());

    let _ = parsed.diagnostics.render(&sources);
    let _ = parsed.ast.dump();
    for document in parsed.ast.documents() {
        let _ = parsed.ast.reachable_from(document.root);
    }
});
