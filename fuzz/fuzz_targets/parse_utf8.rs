// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The same property over well-formed UTF-8, which reaches deeper into the
//! parser than random bytes usually do.
//!
//! Run with a nightly toolchain:
//!
//! ```sh
//! cargo +nightly fuzz run parse_utf8 -- -max_total_time=90
//! ```
//!
//! Do NOT pass `fixtures/` as the corpus directory. libFuzzer *writes*
//! new inputs into its corpus dir, which would bury the 47 curated
//! fixtures under thousands of hash-named files. To seed from them,
//! copy first:
//!
//! ```sh
//! mkdir -p fuzz/corpus/parse_utf8
//! cp fixtures/*/*.yml fuzz/corpus/parse_utf8/
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use yfi_syntax::{parse, ParseOptions, SourceMap};

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
