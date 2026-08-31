// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! Source registry and span model.
//!
//! `saphyr-parser` reports positions as [`saphyr_parser::Marker`]s whose `index`
//! is a **character** offset and whose `col` is **zero based**. Neither is what a
//! diagnostic needs, so every marker is converted exactly once, here, into a
//! [`Pos`] carrying a byte offset into the original file and a one-based column.

use std::fmt;
use std::path::{Path, PathBuf};

/// Handle to a file registered in a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(pub u32);

/// A resolved position in a source file.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Pos {
    /// Byte offset into the original file contents, BOM included.
    pub byte: u32,
    /// One-based line number.
    pub line: u32,
    /// One-based column number, counted in characters.
    pub col: u32,
}

/// A half-open source range, `start` inclusive and `end` exclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    /// The file the range belongs to.
    pub file: FileId,
    /// Inclusive start.
    pub start: Pos,
    /// Exclusive end.
    pub end: Pos,
}

impl Span {
    /// An empty span at `pos`.
    #[must_use]
    pub fn empty(file: FileId, pos: Pos) -> Self {
        Span { file, start: pos, end: pos }
    }

    /// A span covering both `self` and `other`. `other` is assumed to be in the
    /// same file; if it is not, the file of `self` wins.
    #[must_use]
    pub fn to(self, other: Span) -> Self {
        let start = if self.start.byte <= other.start.byte { self.start } else { other.start };
        let end = if self.end.byte >= other.end.byte { self.end } else { other.end };
        Span { file: self.file, start, end }
    }
}

/// Why a source file could not be registered.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The bytes are not valid UTF-8; the payload is the byte offset of the
    /// first invalid sequence.
    NotUtf8(usize),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{e}"),
            LoadError::NotUtf8(at) => write!(f, "invalid UTF-8 at byte {at}"),
        }
    }
}

/// One registered source file.
pub struct SourceFile {
    id: FileId,
    path: PathBuf,
    /// Contents with any leading byte-order mark removed.
    text: String,
    /// Byte offset of every character of `text`, plus a trailing sentinel.
    /// `None` when `text` is ASCII, where the character index *is* the offset.
    char_offsets: Option<Vec<u32>>,
    /// Bytes stripped from the front of the file (a BOM, or zero).
    byte_base: u32,
    /// Character index at which each line begins.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// The file's handle.
    #[must_use]
    pub fn id(&self) -> FileId {
        self.id
    }

    /// The path this file was registered under.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The BOM-stripped contents.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of characters in [`SourceFile::text`].
    #[must_use]
    pub fn char_len(&self) -> usize {
        match &self.char_offsets {
            Some(o) => o.len() - 1,
            None => self.text.len(),
        }
    }

    /// Byte offset of character `index` within [`SourceFile::text`], clamped to
    /// the end of the text.
    #[must_use]
    pub fn char_to_local_byte(&self, index: usize) -> u32 {
        match &self.char_offsets {
            Some(o) => o[index.min(o.len() - 1)],
            None => u32::try_from(index.min(self.text.len())).unwrap_or(u32::MAX),
        }
    }

    /// The character at `index`, or `None` past the end.
    #[must_use]
    pub fn char_at(&self, index: usize) -> Option<char> {
        if index >= self.char_len() {
            return None;
        }
        let at = self.char_to_local_byte(index) as usize;
        self.text[at..].chars().next()
    }

    /// The text between two character indices.
    #[must_use]
    pub fn slice_chars(&self, from: usize, to: usize) -> &str {
        let (a, b) = (self.char_to_local_byte(from) as usize, self.char_to_local_byte(to) as usize);
        self.text.get(a..b.max(a)).unwrap_or("")
    }

    /// Number of lines, counted as line starts.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.line_starts.len()).unwrap_or(u32::MAX)
    }

    /// Character index at which the one-based `line` begins.
    #[must_use]
    pub fn line_start_char(&self, line: u32) -> usize {
        let index = (line.max(1) - 1) as usize;
        self.line_starts.get(index).copied().unwrap_or(0) as usize
    }

    /// Resolve a character index of [`SourceFile::text`] into a [`Pos`].
    #[must_use]
    pub fn pos_at_char(&self, index: usize) -> Pos {
        let index = index.min(self.char_len());
        let line = self.line_starts.partition_point(|&s| s as usize <= index).max(1);
        let start = self.line_starts[line - 1] as usize;
        Pos {
            byte: self.byte_base + self.char_to_local_byte(index),
            line: u32::try_from(line).unwrap_or(u32::MAX),
            col: u32::try_from(index - start).unwrap_or(u32::MAX).saturating_add(1),
        }
    }

    /// Convert a raw marker into a [`Pos`]. `char_base` and `line_base` rebase
    /// markers produced by a parser restarted part-way through the file.
    #[must_use]
    pub fn pos(&self, marker: &saphyr_parser::Marker, char_base: usize, line_base: u32) -> Pos {
        let index = marker.index() + char_base;
        Pos {
            byte: self.byte_base + self.char_to_local_byte(index),
            line: u32::try_from(marker.line()).unwrap_or(u32::MAX) + line_base,
            col: u32::try_from(marker.col()).unwrap_or(u32::MAX).saturating_add(1),
        }
    }
}

/// Registry of every file a compilation touched.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `text` under `path` without touching the filesystem.
    pub fn add(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> FileId {
        let raw: String = text.into();
        let (text, byte_base) = match raw.strip_prefix('\u{feff}') {
            Some(rest) => (rest.to_owned(), 3),
            None => (raw, 0),
        };
        let char_offsets = (!text.is_ascii()).then(|| build_char_offsets(&text));
        let line_starts = build_line_starts(&text);
        let id = FileId(u32::try_from(self.files.len()).expect("source map overflow"));
        self.files.push(SourceFile {
            id,
            path: path.into(),
            text,
            char_offsets,
            byte_base,
            line_starts,
        });
        id
    }

    /// Read `path` and register its contents.
    ///
    /// # Errors
    /// Returns [`LoadError`] if the file cannot be read or is not valid UTF-8.
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<FileId, LoadError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(LoadError::Io)?;
        let text = String::from_utf8(bytes)
            .map_err(|e| LoadError::NotUtf8(e.utf8_error().valid_up_to()))?;
        Ok(self.add(path, text))
    }

    /// Look up a registered file.
    #[must_use]
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    /// Render a span as `path:line:col`.
    #[must_use]
    pub fn location(&self, span: Span) -> String {
        let file = self.file(span.file);
        format!("{}:{}:{}", file.path().display(), span.start.line, span.start.col)
    }
}

fn build_line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    let mut index = 0u32;
    for c in text.chars() {
        index = index.saturating_add(1);
        if c == '\n' {
            starts.push(index);
        }
    }
    starts
}

fn build_char_offsets(text: &str) -> Vec<u32> {
    let mut offsets = Vec::with_capacity(text.len() + 1);
    offsets.extend(text.char_indices().map(|(i, _)| u32::try_from(i).unwrap_or(u32::MAX)));
    offsets.push(u32::try_from(text.len()).unwrap_or(u32::MAX));
    offsets
}

#[cfg(test)]
mod tests {
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
}
