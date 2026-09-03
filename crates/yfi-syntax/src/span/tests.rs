// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for the source registry and span model.
//!
//! Kept beside [`super`] rather than inside it: the module is the compiler's
//! one answer to "where is this", and its offset arithmetic earns enough tests
//! that carrying them in the same file pushed it past the size the project
//! holds every file to.

use super::*;

fn file(text: &str) -> (SourceMap, FileId) {
    let mut map = SourceMap::new();
    let id = map.add("t.yml", text);
    (map, id)
}

#[test]
fn ascii_uses_the_identity_offset_table() {
    let (map, id) = file("abc\ndef\n");
    let f = map.file(id);
    assert_eq!(f.char_len(), 8);
    assert_eq!(f.char_to_local_byte(5), 5);
    assert_eq!(f.char_at(4), Some('d'));
}

#[test]
fn multi_byte_characters_shift_byte_offsets() {
    let (map, id) = file("a: héllo\n");
    let f = map.file(id);
    assert_eq!(f.char_at(4), Some('é'));
    assert_eq!(f.char_to_local_byte(4), 4);
    assert_eq!(f.char_to_local_byte(5), 6, "é occupies two bytes");
    assert_eq!(f.slice_chars(3, 8), "héllo");
}

#[test]
fn a_bom_is_stripped_and_accounted_for() {
    let (map, id) = file("\u{feff}key: 1\n");
    let f = map.file(id);
    assert_eq!(f.text(), "key: 1\n");
    assert_eq!(f.pos_at_char(0).byte, 3, "byte offsets stay relative to the file");
    assert_eq!(f.pos_at_char(0).line, 1);
    assert_eq!(f.pos_at_char(0).col, 1);
}

#[test]
fn a_byte_offset_is_the_files_and_not_an_index_into_the_stripped_text() {
    // The two are one apart by exactly the mark, and the documentation used
    // to claim they were the same string. They are not, and a caller that
    // believed it sliced three bytes into every BOM'd file.
    let (map, id) = file("\u{feff}key: 1\n");
    let f = map.file(id);
    let at = f.pos_at_char(0).byte as usize;
    assert_eq!(f.byte_base(), 3);
    assert!(f.text().get(at..).is_none_or(|rest| !rest.starts_with("key")));
    assert!(f.text()[at - f.byte_base() as usize..].starts_with("key"));

    // And nothing is subtracted when there is nothing to subtract, which is
    // what makes the correction safe to write unconditionally.
    let (map, id) = file("key: 1\n");
    let f = map.file(id);
    assert_eq!(f.byte_base(), 0);
    assert!(f.text()[f.pos_at_char(0).byte as usize..].starts_with("key"));
}

#[test]
fn positions_are_one_based_in_both_axes() {
    let (map, id) = file("ab\ncd\n");
    let f = map.file(id);
    assert_eq!((f.pos_at_char(0).line, f.pos_at_char(0).col), (1, 1));
    assert_eq!((f.pos_at_char(3).line, f.pos_at_char(3).col), (2, 1));
    assert_eq!((f.pos_at_char(4).line, f.pos_at_char(4).col), (2, 2));
}

#[test]
fn line_starts_locate_document_boundaries() {
    let (map, id) = file("a\n--- b\nc\n");
    let f = map.file(id);
    assert_eq!(f.line_count(), 4);
    assert_eq!(f.line_start_char(2), 2);
    assert_eq!(f.slice_chars(f.line_start_char(2), f.line_start_char(2) + 3), "---");
}

#[test]
fn out_of_range_indices_clamp_instead_of_panicking() {
    let (map, id) = file("ab");
    let f = map.file(id);
    assert_eq!(f.char_at(99), None);
    assert_eq!(f.slice_chars(99, 200), "");
    assert_eq!(f.pos_at_char(99).col, 3);
}
