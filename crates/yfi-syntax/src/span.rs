// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Source registry and span model.
//!
//! `saphyr-parser` reports positions as [`saphyr_parser::Marker`]s whose `index`
//! is a **character** offset and whose `col` is **zero based**. Neither is what a
//! diagnostic needs, so every marker is converted exactly once, here, into a
//! [`Pos`] carrying a byte offset into the original file and a one-based column.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::front::{self, Block, Dialect, Fault};

/// Handle to a file registered in a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(pub u32);

/// A resolved position in a source file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pos {
    /// Byte offset into the original file contents, BOM included.
    pub byte: u32,
    /// One-based line number.
    pub line: u32,
    /// One-based column number, counted in characters.
    pub col: u32,
}

impl Default for Pos {
    /// The start of a file. Written out rather than derived: `line` and `col`
    /// are one-based, so a derived zero would render `file:0:0`.
    fn default() -> Self {
        Pos { byte: 0, line: 1, col: 1 }
    }
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
///
/// A `.yfy` file is rewritten before the parser reads it (see [`crate::front`]),
/// so two texts exist and both are kept. They hold the **same characters in the
/// same lines** — the rewrite is a substitution — and differ only in which
/// characters those are, and therefore in how many bytes each one occupies. So
/// there are two offset tables: a [`Pos`] is resolved against the text **as
/// written**, because that is what a diagnostic points into, and text is sliced
/// against what the **parser read**, because that is what its markers index.
/// For a base YAML file the two are the same text and one table serves both.
pub struct SourceFile {
    id: FileId,
    path: PathBuf,
    /// Which language the file was read as.
    dialect: Dialect,
    /// The contents the parser read, with any leading byte-order mark removed.
    text: String,
    /// The contents as written, present only when the pre-pass changed them.
    written: Option<String>,
    /// Byte offset of every character of `text`, plus a trailing sentinel.
    /// `None` when `text` is ASCII, where the character index *is* the offset.
    char_offsets: Option<Vec<u32>>,
    /// The same table for [`SourceFile::text`], when the two texts differ in
    /// their byte layout. `None` means `char_offsets` answers for both.
    written_offsets: Option<Vec<u32>>,
    /// Bytes stripped from the front of the file (a BOM, or zero).
    byte_base: u32,
    /// Character index at which each line begins.
    line_starts: Vec<u32>,
    /// Every `<?-- … >` region the pre-pass captured, in source order.
    blocks: Vec<Block>,
    /// Every block the pre-pass found unterminated.
    faults: Vec<Fault>,
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

    /// The contents **as written**, with any byte-order mark removed.
    ///
    /// A [`Pos::byte`] does **not** index this string directly. It is an offset
    /// into the file's own bytes, mark included, because that is what a reader
    /// comparing a diagnostic against `hexdump` or an editor's byte column
    /// needs; this string starts after the mark. Subtract
    /// [`SourceFile::byte_base`] to cross between them, which is zero for the
    /// overwhelming majority of files and exactly the point of asking rather
    /// than assuming.
    #[must_use]
    pub fn text(&self) -> &str {
        self.written.as_deref().unwrap_or(&self.text)
    }

    /// Bytes stripped from the front of the file before [`SourceFile::text`]
    /// begins — the length of a byte-order mark, or zero.
    ///
    /// This is the whole difference between a [`Pos::byte`] and an index into
    /// [`SourceFile::text`], and it is exposed so that difference can be
    /// written down instead of guessed at.
    #[must_use]
    pub fn byte_base(&self) -> u32 {
        self.byte_base
    }

    /// The contents the **parser read**: [`SourceFile::text`] for base YAML,
    /// and the pre-pass's rewrite for Yamlfication source.
    #[must_use]
    pub fn parsed_text(&self) -> &str {
        &self.text
    }

    /// Which language the file was read as.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Every `<?-- … >` region the file holds, in source order.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Every block opened and never closed.
    pub(crate) fn faults(&self) -> &[Fault] {
        &self.faults
    }

    /// Number of characters in [`SourceFile::text`].
    #[must_use]
    pub fn char_len(&self) -> usize {
        match &self.char_offsets {
            Some(o) => o.len() - 1,
            None => self.text.len(),
        }
    }

    /// Byte offset of character `index` within [`SourceFile::parsed_text`],
    /// clamped to the end of the text.
    #[must_use]
    pub fn char_to_local_byte(&self, index: usize) -> u32 {
        offset(self.char_offsets.as_ref(), &self.text, index)
    }

    /// Byte offset of character `index` within [`SourceFile::text`] — the file
    /// as written, which is what a [`Pos`] must point into.
    fn char_to_written_byte(&self, index: usize) -> u32 {
        match &self.written_offsets {
            Some(offsets) => offset(Some(offsets), self.text(), index),
            None => self.char_to_local_byte(index),
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
            byte: self.byte_base + self.char_to_written_byte(index),
            line: u32::try_from(line).unwrap_or(u32::MAX),
            col: u32::try_from(index - start).unwrap_or(u32::MAX).saturating_add(1),
        }
    }

    /// Turn the pre-pass's character ranges into spans of this file.
    fn locate(&self, raw: &[front::RawBlock]) -> Vec<Block> {
        raw.iter()
            .map(|block| Block {
                kind: block.kind,
                text: block.text.as_str().into(),
                span: Span {
                    file: self.id,
                    start: self.pos_at_char(block.start),
                    end: self.pos_at_char(block.end),
                },
            })
            .collect()
    }

    /// Convert a raw marker into a [`Pos`], applying `rebase`.
    ///
    /// Crate-private on purpose: it is the only place a `saphyr-parser` type
    /// appears in a signature, and leaking it would force every downstream
    /// crate to depend on that parser at a matching version.
    pub(crate) fn pos(&self, marker: &saphyr_parser::Marker, rebase: Rebase) -> Pos {
        let index = rebase.char(marker.index());
        Pos {
            byte: self.byte_base + self.char_to_written_byte(index),
            line: rebase.line(marker.line()),
            col: u32::try_from(marker.col()).unwrap_or(u32::MAX).saturating_add(1),
        }
    }
}

/// The correction that turns a marker one parser instance produced into a
/// position in the original file.
///
/// Two things move a marker, and they move it in opposite directions. A parser
/// restarted at a document boundary after a syntax error sees only the tail of
/// the file, so its indices are **too small** (D3.6). A parser fed a synthetic
/// import prelude ahead of the file's own text sees more than the file, so its
/// indices are **too large** (D6.7). The correction is therefore signed, and
/// applying it is the one place either offset is allowed to be reasoned about.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Rebase {
    /// Characters to add to a marker's index.
    pub chars: i64,
    /// Lines to add to a marker's line.
    pub lines: i64,
}

impl Rebase {
    /// A marker's index as a character index into the original file.
    pub(crate) fn char(self, index: usize) -> usize {
        shift(index, self.chars)
    }

    /// A marker's line as a one-based line of the original file.
    fn line(self, line: usize) -> u32 {
        u32::try_from(shift(line, self.lines).max(1)).unwrap_or(u32::MAX)
    }
}

/// `value + by`, saturating at zero rather than wrapping.
fn shift(value: usize, by: i64) -> usize {
    let shifted = i64::try_from(value).unwrap_or(i64::MAX).saturating_add(by);
    usize::try_from(shifted.max(0)).unwrap_or(0)
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

    /// Register `text` under `path` as base YAML, without touching the
    /// filesystem.
    pub fn add(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> FileId {
        self.add_as(path, text, Dialect::BaseYaml)
    }

    /// Register `text` under `path`, read as `dialect`.
    ///
    /// The dialect is the caller's to state and is never inferred from the
    /// contents: the two file classes exist precisely so that no signal inside
    /// a file has to decide which language it is (D6.6).
    pub fn add_as(
        &mut self,
        path: impl Into<PathBuf>,
        text: impl Into<String>,
        dialect: Dialect,
    ) -> FileId {
        let raw: String = text.into();
        let (written, byte_base) = match raw.strip_prefix('\u{feff}') {
            Some(rest) => (rest.to_owned(), 3),
            None => (raw, 0),
        };
        let rewrite = front::preprocess(&written, dialect);
        let char_offsets = (!rewrite.text.is_ascii()).then(|| build_char_offsets(&rewrite.text));
        let written_offsets =
            (rewrite.changed && !written.is_ascii()).then(|| build_char_offsets(&written));
        let line_starts = build_line_starts(&rewrite.text);
        let id = FileId(u32::try_from(self.files.len()).expect("source map overflow"));
        self.files.push(SourceFile {
            id,
            path: path.into(),
            dialect,
            text: rewrite.text,
            written: rewrite.changed.then_some(written),
            char_offsets,
            written_offsets,
            byte_base,
            line_starts,
            blocks: Vec::new(),
            faults: rewrite.faults,
        });
        let blocks = self.files[id.0 as usize].locate(&rewrite.blocks);
        self.files[id.0 as usize].blocks = blocks;
        id
    }

    /// Read `path` and register its contents, read as `dialect`.
    ///
    /// # Errors
    /// Returns [`LoadError`] if the file cannot be read or is not valid UTF-8.
    pub fn load_as(
        &mut self,
        path: impl AsRef<Path>,
        dialect: Dialect,
    ) -> Result<FileId, LoadError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(LoadError::Io)?;
        let text = String::from_utf8(bytes)
            .map_err(|e| LoadError::NotUtf8(e.utf8_error().valid_up_to()))?;
        Ok(self.add_as(path, text, dialect))
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

/// Byte offset of character `index`, clamped to the end of `text`.
fn offset(offsets: Option<&Vec<u32>>, text: &str, index: usize) -> u32 {
    match offsets {
        Some(o) => o[index.min(o.len() - 1)],
        None => u32::try_from(index.min(text.len())).unwrap_or(u32::MAX),
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
mod tests;
