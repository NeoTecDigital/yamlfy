// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Arbitrary bytes must never panic the parser.
//!
//! Run with a nightly toolchain:
//!
//! ```sh
//! cargo +nightly fuzz run parse -- -max_total_time=90
//! ```
//!
//! Do NOT pass `fixtures/` as the corpus directory. libFuzzer *writes*
//! new inputs into its corpus dir, which would bury the 47 curated
//! fixtures under thousands of hash-named files. To seed from them,
//! copy first:
//!
//! ```sh
//! mkdir -p fuzz/corpus/parse
//! cp fixtures/*/*.yml fuzz/corpus/parse/
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use yfi_syntax::{parse, ParseOptions, SourceMap};

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
