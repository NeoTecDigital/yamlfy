// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery of anchor names from the source text.
//!
//! `saphyr-parser` reports an anchor as an opaque numeric id and never yields
//! the name that was written. The name is required — it is the node's
//! identifier inside a document — so it is read back out of the source.
//!
//! The read is bounded, not a guess. When an event carries a non-zero anchor id
//! the source between the previous event and this node's content can only hold
//! separation white space, comments, a tag property and the anchor property.
//! That region is scanned forward for the last `&name` token in it.

use crate::span::SourceFile;

/// Character range of a recovered token, `&`/`*` included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct TokenRange {
    pub start: usize,
    pub end: usize,
}

/// Whether `c` may appear in an anchor or alias name (YAML `ns-anchor-char`).
///
/// This mirrors `saphyr_parser::char_traits::is_anchor_char` exactly. It must not
/// be written as `!c.is_whitespace()`: Rust's `char::is_whitespace` is the Unicode
/// `White_Space` property, which is far wider than the six characters YAML excludes.
/// Using it truncates any anchor whose name contains U+00A0, U+2000-200A, U+3000 and
/// the rest — silently, because the name is only recovered text and the
/// `AnchorId` binding still succeeds. The result is a wrong node identifier and a
/// fabricated `W0300`, with neither `E0120` nor `E0121` able to see it.
pub(crate) fn is_name_char(c: char) -> bool {
    // is_yaml_non_space: not a line break, not a BOM, not a blank.
    !matches!(c, '\n' | '\r' | '\u{FEFF}' | ' ' | '\t')
        // is_flow
        && !matches!(c, ',' | '[' | ']' | '{' | '}')
        // is_z
        && c != '\0'
}

/// Whether a property token may start at a character preceded by `prev`.
fn can_start_token(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '-' | ':' | '?' | ',' | '[' | '{'),
    }
}

/// Read a name starting at `from`, returning the index one past its end.
fn name_end(file: &SourceFile, from: usize, hi: usize) -> usize {
    let mut i = from;
    while i < hi && file.char_at(i).is_some_and(is_name_char) {
        i += 1;
    }
    i
}

/// Skip a quoted scalar whose opening quote is at `from`.
fn skip_quoted(file: &SourceFile, from: usize, hi: usize, quote: char) -> usize {
    let mut i = from + 1;
    while i < hi {
        let Some(c) = file.char_at(i) else { break };
        if c == '\\' && quote == '"' {
            i += 2;
            continue;
        }
        i += 1;
        if c == quote {
            break;
        }
    }
    i
}

/// Skip from a `#` to the end of its line.
fn skip_comment(file: &SourceFile, from: usize, hi: usize) -> usize {
    let mut i = from;
    while i < hi && file.char_at(i) != Some('\n') {
        i += 1;
    }
    i
}

/// Visit every `&name` token in `[lo, hi)`, in source order.
///
/// Quoted scalars and comments are skipped, so an ampersand inside either is
/// not a token. Nothing else is interpreted: this is a lexer, not a parser, and
/// it has no way to know whether a `&name` it finds is a node property or text
/// inside a plain scalar. Both callers can afford that — one searches a region
/// the grammar has already restricted to properties, the other over-approximates
/// on purpose ([`all_anchor_tokens`]).
fn each_anchor_token(file: &SourceFile, lo: usize, hi: usize, mut visit: impl FnMut(TokenRange)) {
    let hi = hi.min(file.char_len());
    let mut i = lo;
    let mut prev = if lo == 0 { None } else { file.char_at(lo - 1) };
    while i < hi {
        let Some(c) = file.char_at(i) else { break };
        let next = match c {
            '\'' | '"' => skip_quoted(file, i, hi, c),
            '#' if can_start_token(prev) => skip_comment(file, i, hi),
            '&' if can_start_token(prev) => {
                let end = name_end(file, i + 1, hi);
                if end > i + 1 {
                    visit(TokenRange { start: i, end });
                }
                end.max(i + 1)
            }
            _ => i + 1,
        };
        prev = file.char_at(next.saturating_sub(1));
        i = next;
    }
}

/// Find the anchor property that belongs to a node whose content starts at
/// `hi`, searching the region `[lo, hi)`.
///
/// Returns the character range of the `&name` token, `&` included.
pub(crate) fn find_anchor_token(
    file: &SourceFile,
    lo: usize,
    hi: usize,
) -> Option<TokenRange> {
    let mut found = None;
    each_anchor_token(file, lo, hi, |token| found = Some(token));
    found
}

/// Every `&name` token in the whole file, in source order.
///
/// This is the one thing about a file that can be learned **without parsing
/// it**, and it exists because a parse is not always available: an unknown
/// alias is a scan error, and a scan error costs every anchor written after it
/// in that document. A file in an import cycle is exactly that case — its
/// cross-file aliases cannot bind until the other side is bound — so the names
/// it might export are read from its text to start the binding fixed point off
/// (see `yamlfy-core`'s `bind`).
///
/// The answer is an **over-approximation**: an `&x` inside a plain scalar is
/// indistinguishable from a node property to a lexer. That is safe for the one
/// use it has, because a name that no parse confirms is never exported.
pub(crate) fn all_anchor_tokens(file: &SourceFile) -> Vec<TokenRange> {
    let mut found = Vec::new();
    each_anchor_token(file, 0, file.char_len(), |token| found.push(token));
    found
}

/// The name of an alias whose `*name` token occupies `[start, end)`.
pub(crate) fn alias_name(file: &SourceFile, start: usize, end: usize) -> Option<&str> {
    if file.char_at(start) != Some('*') {
        return None;
    }
    let text = file.slice_chars(start + 1, end.max(start + 1));
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::SourceMap;

    fn anchor(text: &str, lo: usize, hi: usize) -> Option<String> {
        let mut map = SourceMap::new();
        let id = map.add("t.yml", text);
        let file = map.file(id);
        find_anchor_token(file, lo, hi).map(|t| file.slice_chars(t.start, t.end).to_owned())
    }

    #[test]
    fn finds_a_plain_anchor_property() {
        assert_eq!(anchor("a: &x 1\n", 1, 6).as_deref(), Some("&x"));
    }

    #[test]
    fn skips_a_tag_property_written_either_side_of_the_anchor() {
        assert_eq!(anchor("a: &t !!str v\n", 1, 12).as_deref(), Some("&t"));
        assert_eq!(anchor("a: !!str &t v\n", 1, 12).as_deref(), Some("&t"));
    }

    #[test]
    fn skips_a_comment_between_the_anchor_and_the_node() {
        assert_eq!(anchor("a: &x  # note\n  1\n", 1, 16).as_deref(), Some("&x"));
    }

    #[test]
    fn ignores_an_ampersand_inside_a_quoted_scalar() {
        assert_eq!(anchor("a: \"x &y\"\nb: 1\n", 1, 10), None);
    }

    #[test]
    fn takes_the_last_anchor_in_the_region() {
        assert_eq!(anchor("&a &b v\n", 0, 6).as_deref(), Some("&b"));
    }

    #[test]
    fn requires_a_name_after_the_ampersand() {
        assert_eq!(anchor("a: & 1\n", 1, 5), None);
    }

    #[test]
    fn every_anchor_token_in_a_file_is_found_in_source_order() {
        let mut map = SourceMap::new();
        let id = map.add("t.yml", "--- &a\nk: &b 1\nq: \"&c\"\n# &d\ne: &f 2\n");
        let file = map.file(id);
        let found: Vec<String> = all_anchor_tokens(file)
            .into_iter()
            .map(|t| file.slice_chars(t.start, t.end).to_owned())
            .collect();
        assert_eq!(found, ["&a", "&b", "&f"], "quoted text and comments carry no properties");
    }

    #[test]
    fn reads_an_alias_name_from_its_token() {
        let mut map = SourceMap::new();
        let id = map.add("t.yml", "a: *name\n");
        let file = map.file(id);
        assert_eq!(alias_name(file, 3, 8), Some("name"));
        assert_eq!(alias_name(file, 0, 1), None, "not an alias token");
    }
}
