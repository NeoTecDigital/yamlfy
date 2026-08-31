// Written by Richard Christopher, Copyright 2026 Richard Christopher

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
fn is_name_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ',' | '[' | ']' | '{' | '}')
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

/// Find the anchor property that belongs to a node whose content starts at
/// `hi`, searching the region `[lo, hi)`.
///
/// Returns the character range of the `&name` token, `&` included.
pub(crate) fn find_anchor_token(
    file: &SourceFile,
    lo: usize,
    hi: usize,
) -> Option<TokenRange> {
    let hi = hi.min(file.char_len());
    let mut found = None;
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
                    found = Some(TokenRange { start: i, end });
                }
                end.max(i + 1)
            }
            _ => i + 1,
        };
        prev = file.char_at(next.saturating_sub(1));
        i = next;
    }
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
    fn reads_an_alias_name_from_its_token() {
        let mut map = SourceMap::new();
        let id = map.add("t.yml", "a: *name\n");
        let file = map.file(id);
        assert_eq!(alias_name(file, 3, 8), Some("name"));
        assert_eq!(alias_name(file, 0, 1), None, "not an alias token");
    }
}
