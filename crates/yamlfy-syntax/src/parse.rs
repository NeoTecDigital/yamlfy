// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser entry points and syntax-error recovery.
//!
//! A `saphyr-parser` event stream cannot be resumed after a [`ScanError`]: the
//! scanner's state is gone. To still report more than one syntax error per
//! file, the parser is *restarted* on the remainder of the file beginning at the
//! next document boundary, and every marker it then produces is rebased onto the
//! original file. Semantic diagnostics, which this crate raises itself, always
//! accumulate in full.

use saphyr_parser::{Event, Parser, ScanError};
use tracing::{debug, trace};

use crate::ast::Ast;
use crate::builder::Builder;
use crate::diagnostic::{Code, Diagnostic, Diagnostics, SeverityMap};
use crate::span::{FileId, LoadError, SourceFile, SourceMap, Span};

/// Knobs the caller may turn. Everything here is decided before parsing starts,
/// so it is a plain owned struct rather than a runtime lookup.
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// Per-code severity overrides.
    pub severities: SeverityMap,
    /// How many times a file may be restarted after a syntax error.
    pub max_recovery_attempts: u32,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions { severities: SeverityMap::new(), max_recovery_attempts: 16 }
    }
}

/// The result of parsing one file. An [`Ast`] is always produced, even when
/// diagnostics were raised; it holds everything that could be understood.
pub struct Parsed {
    /// The arena.
    pub ast: Ast,
    /// Everything found while building it.
    pub diagnostics: Diagnostics,
}

/// Parse a file that is already registered in `sources`.
#[must_use]
pub fn parse(sources: &SourceMap, file: FileId, options: &ParseOptions) -> Parsed {
    let source = sources.file(file);
    let mut builder = Builder::new(source, options.severities.clone());
    let mut segment = Segment { char_base: 0, line_base: 0 };
    let mut attempts = 0;
    while let Some(error) = run_segment(&mut builder, source, segment) {
        let span = error_span(source, &error, segment);
        attempts += 1;
        let resume = resume_point(source, span, segment, attempts, options);
        builder.diagnose(syntax_error(&error, span, &resume));
        match resume {
            Resume::At(next) => {
                debug!(char_base = next.char_base, "resuming after syntax error");
                builder.restart(next.char_base, next.line_base);
                segment = next;
            }
            Resume::Stop(limit) => {
                if limit {
                    builder.diagnose(Diagnostic::new(
                        Code::RecoveryLimitExceeded,
                        span,
                        format!(
                            "stopped after {attempts} syntax errors; \
                             the rest of this file was not parsed"
                        ),
                    ));
                }
                break;
            }
        }
    }
    let (ast, diagnostics) = builder.finish();
    Parsed { ast, diagnostics }
}

/// Register `path` in `sources` and parse it.
///
/// Read failures and encoding failures become diagnostics rather than a
/// `Result`, so a directory walk never has to decide between stopping and
/// silently skipping a file.
pub fn parse_file(
    sources: &mut SourceMap,
    path: impl AsRef<std::path::Path>,
    options: &ParseOptions,
) -> Parsed {
    let path = path.as_ref();
    match sources.load(path) {
        Ok(file) => parse(sources, file, options),
        Err(error) => {
            let file = sources.add(path, "");
            unreadable(sources, file, &error, options)
        }
    }
}

fn unreadable(
    sources: &SourceMap,
    file: FileId,
    error: &LoadError,
    options: &ParseOptions,
) -> Parsed {
    let source = sources.file(file);
    let span = Span::empty(file, source.pos_at_char(0));
    let (code, message) = match error {
        LoadError::Io(e) => (Code::IoError, format!("cannot read file: {e}")),
        LoadError::NotUtf8(at) => (
            Code::InvalidUtf8,
            format!("file is not valid UTF-8; first invalid byte at offset {at}"),
        ),
    };
    let mut builder = Builder::new(source, options.severities.clone());
    builder.diagnose(Diagnostic::new(code, span, message));
    let (ast, diagnostics) = builder.finish();
    Parsed { ast, diagnostics }
}

/// Where in the file the current parser instance started.
#[derive(Clone, Copy)]
struct Segment {
    char_base: usize,
    line_base: u32,
}

enum Resume {
    At(Segment),
    /// `true` when stopping because the recovery budget ran out.
    Stop(bool),
}

fn run_segment(
    builder: &mut Builder<'_>,
    source: &SourceFile,
    segment: Segment,
) -> Option<ScanError> {
    let text = source.slice_chars(segment.char_base, source.char_len());
    let mut parser = Parser::new_from_str(text).keep_tags(true);
    loop {
        match parser.next_event() {
            None => return None,
            Some(Ok((event, raw))) => {
                trace!(?event, "event");
                let done = matches!(event, Event::StreamEnd);
                builder.event(&event, raw);
                if done {
                    return None;
                }
            }
            Some(Err(error)) => return Some(error),
        }
    }
}

fn syntax_error(error: &ScanError, span: Span, resume: &Resume) -> Diagnostic {
    let diagnostic = Diagnostic::new(Code::SyntaxError, span, error.info().to_owned());
    match resume {
        Resume::At(_) => diagnostic.with_note(
            "the rest of this document was skipped; parsing resumed at the next `---`",
            None,
        ),
        Resume::Stop(_) => diagnostic,
    }
}

fn error_span(source: &SourceFile, error: &ScanError, segment: Segment) -> Span {
    let pos = source.pos(error.marker(), segment.char_base, segment.line_base);
    Span::empty(source.id(), pos)
}

fn resume_point(
    source: &SourceFile,
    error: Span,
    segment: Segment,
    attempts: u32,
    options: &ParseOptions,
) -> Resume {
    if attempts > options.max_recovery_attempts {
        return Resume::Stop(true);
    }
    match next_document_start(source, error.start.line, segment.char_base) {
        Some(char_base) => Resume::At(Segment {
            char_base,
            line_base: source.pos_at_char(char_base).line.saturating_sub(1),
        }),
        None => Resume::Stop(false),
    }
}

/// The character index of the next `---` that begins a line, at or after
/// `from_line` and strictly after the current segment's start.
fn next_document_start(source: &SourceFile, from_line: u32, after: usize) -> Option<usize> {
    let lines = source.line_count();
    for line in from_line..=lines {
        let start = source.line_start_char(line);
        if start <= after || !starts_document(source, start) {
            continue;
        }
        return Some(start);
    }
    None
}

fn starts_document(source: &SourceFile, start: usize) -> bool {
    if source.slice_chars(start, start + 3) != "---" {
        return false;
    }
    source.char_at(start + 3).is_none_or(char::is_whitespace)
}
