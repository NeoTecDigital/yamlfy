// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The same property over the **Yamlfication** front end.
//!
//! The other two targets register their input with [`SourceMap::add`], which is
//! [`Dialect::BaseYaml`], so the pre-pass is never exercised: `//` comments,
//! `<?-- … -->` and `<?-- … --!>` blocks and the rewrite that turns them into
//! something a YAML parser will accept are all untouched by them. That is the
//! newest hand-index-arithmetic code in the tree, and the one place a CRLF
//! block-scalar corruption has already been found — the arithmetic is a
//! *substitution*, so every position it produces must still land on a character
//! boundary of the file as written.
//!
//! So this target asserts that invariant rather than merely surviving. Every
//! node's span is required to be a character boundary of the text the author
//! wrote, in order, and within the file: a rewrite that miscounts by one byte
//! is a wrong diagnostic location at best and a panic in any caller that slices
//! at worst, and neither shows up in a target that only checks for crashes.
//!
//! Run with a nightly toolchain:
//!
//! ```sh
//! cargo +nightly fuzz run parse_yfy -- -max_total_time=90
//! ```
//!
//! Do NOT pass `fixtures/` as the corpus directory. libFuzzer *writes* new
//! inputs into its corpus dir, which would bury the 47 curated fixtures under
//! thousands of hash-named files. To seed from them, copy first:
//!
//! ```sh
//! mkdir -p fuzz/corpus/parse_yfy
//! cp fixtures/*/*.yfy fixtures/*/*.yml fuzz/corpus/parse_yfy/
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use yfi_syntax::{parse, Dialect, ParseOptions, SourceMap, Span};

fuzz_target!(|text: String| {
    let mut sources = SourceMap::new();
    let file = sources.add_as("fuzz.yfy", text, Dialect::Yamlfication);
    let parsed = parse(&sources, file, &ParseOptions::default());

    // Rendering resolves every span back to a line and column, which is the
    // other half of the offset arithmetic.
    let _ = parsed.diagnostics.render(&sources);
    let _ = parsed.ast.dump();

    let held = sources.file(file);
    let written = held.text();
    let base = held.byte_base() as usize;
    let check = |span: Span| {
        // A `Pos::byte` is an offset into the file, mark included; `text()`
        // starts after the mark.
        let (start, end) = (span.start.byte as usize, span.end.byte as usize);
        assert!(start >= base && end >= base, "a position precedes the file it is in");
        let (start, end) = (start - base, end - base);
        assert!(start <= end, "a span ends before it starts");
        assert!(end <= written.len(), "a span leaves the file as written");
        assert!(written.is_char_boundary(start), "a span starts mid-character");
        assert!(written.is_char_boundary(end), "a span ends mid-character");
    };
    for node in parsed.ast.nodes() {
        check(node.span);
    }
    for block in held.blocks() {
        check(block.span);
    }

    for document in parsed.ast.documents() {
        let _ = parsed.ast.reachable_from(document.root);
        let _ = parsed.ast.is_cyclic_from(document.root);
    }
});
