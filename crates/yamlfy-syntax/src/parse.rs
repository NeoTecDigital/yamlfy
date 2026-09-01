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
//!
//! # Imports
//!
//! [`parse_with_imports`] takes the definitions a header imported and installs
//! them into every document of the file (D6.7). `saphyr-parser` rejects an
//! unknown alias at *scan* time, before any event this crate could act on, and
//! its anchor table is private with no seeding API — so the binding has to be
//! made the only way the scanner will accept, by giving it a document that
//! declares the names. That document is synthetic, it is stripped again before
//! anything reaches the arena, and every marker after it is rebased back onto
//! the real file, so no span ever points into it.

use std::borrow::Cow;

use saphyr_parser::{Event, Parser, ScanError};
use tracing::{debug, trace};

use crate::ast::Ast;
use crate::builder::Builder;
use crate::diagnostic::{Code, Diagnostic, Diagnostics, SeverityMap};
use crate::scan;
use crate::span::{FileId, LoadError, Rebase, SourceFile, SourceMap, Span};

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

/// One definition another file exports into the file being parsed.
///
/// The span is the `&name` token **in the file that wrote it**, which is what
/// makes a diagnostic about an imported definition point at the exporting file
/// with its real line and column. The node it names is deliberately absent: it
/// is not knowable while the exporting file may still be reparsed, and
/// [`Ast::rebind_import`] supplies it afterwards.
#[derive(Clone, Debug)]
pub struct Import {
    /// The name the definition is bound to.
    pub name: Box<str>,
    /// Span of its `&name` token, in the file that wrote it.
    pub span: Span,
}

/// Parse a file that is already registered in `sources`.
#[must_use]
pub fn parse(sources: &SourceMap, file: FileId, options: &ParseOptions) -> Parsed {
    parse_with_imports(sources, file, options, &[])
}

/// The `&name` tokens written in a file, read from its text without parsing it,
/// in source order and each carrying its own span.
///
/// **This is not a parse and does not pretend to be one.** A `&x` inside a plain
/// scalar reads exactly like a node property to a lexer, so the answer is an
/// over-approximation of what the file really anchors, and the shape of every
/// name is all it can honestly claim.
///
/// It exists for one caller: a file whose cross-file aliases cannot bind yet
/// cannot be parsed past the first of them, because an unknown alias is a *scan*
/// error and recovery resumes only at the next document boundary — so every
/// anchor written after it is lost. Files that import each other are all in that
/// state at once and none of them can go first, so their binding needs a
/// starting approximation that no parse can supply (D6.7).
#[must_use]
pub fn anchor_names(sources: &SourceMap, file: FileId) -> Vec<Import> {
    let source = sources.file(file);
    scan::all_anchor_tokens(source)
        .into_iter()
        .map(|token| Import {
            name: source.slice_chars(token.start + 1, token.end).into(),
            span: Span {
                file: source.id(),
                start: source.pos_at_char(token.start),
                end: source.pos_at_char(token.end),
            },
        })
        .collect()
}

/// Parse a file whose header imported `imports`, binding each imported name in
/// every document of the file (D6.7).
///
/// Passing an empty slice is exactly [`parse`]: no prelude is synthesised and
/// not a single position moves.
#[must_use]
pub fn parse_with_imports(
    sources: &SourceMap,
    file: FileId,
    options: &ParseOptions,
    imports: &[Import],
) -> Parsed {
    let source = sources.file(file);
    let prelude = Prelude::new(imports);
    let mut segment = Segment { char_base: 0, line_base: 0 };
    let mut builder = Builder::new(
        source,
        options.severities.clone(),
        prelude.imports.clone(),
        prelude.rebase(segment),
    );
    let mut attempts = 0;
    while let Some(error) = run_segment(&mut builder, source, &prelude, segment) {
        let span = error_span(source, &error, &prelude, segment);
        attempts += 1;
        let resume = resume_point(source, span, segment, attempts, options);
        builder.diagnose(syntax_error(&error, span, &resume));
        match resume {
            Resume::At(next) => {
                debug!(char_base = next.char_base, "resuming after syntax error");
                builder.restart(next.char_base, prelude.rebase(next));
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
    let mut builder =
        Builder::new(source, options.severities.clone(), Vec::new(), Rebase::default());
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

/// The synthetic document that declares a header's imported names, and the
/// measurements needed to rebase it back out of every position.
struct Prelude {
    /// The imports it declares, in authored order, nameless ones dropped.
    imports: Vec<Import>,
    /// The text prepended to every parser segment.
    text: String,
    /// Its length in characters.
    chars: i64,
    /// The number of lines it occupies.
    lines: i64,
}

impl Prelude {
    fn new(imports: &[Import]) -> Self {
        // A nameless anchor is one `E0120` already reported in the exporting
        // file; it cannot be aliased, so declaring it would only shift every
        // later import onto the wrong name.
        let imports: Vec<Import> = imports.iter().filter(|i| !i.name.is_empty()).cloned().collect();
        if imports.is_empty() {
            return Prelude { imports, text: String::new(), chars: 0, lines: 0 };
        }
        // One flow sequence of anchored empty scalars, closed with `...` so the
        // file's own first document may be implicit. Anchor names cannot hold a
        // flow indicator or a space (`scan::is_name_char`), so nothing here
        // needs quoting or escaping.
        let names: Vec<String> = imports.iter().map(|i| format!("&{} ", i.name)).collect();
        let text = format!("--- [{}]\n...\n", names.join(", "));
        let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
        let lines = i64::try_from(text.matches('\n').count()).unwrap_or(i64::MAX);
        Prelude { imports, text, chars, lines }
    }

    /// The correction that turns a marker of `segment`'s parser back into a
    /// position in the original file.
    fn rebase(&self, segment: Segment) -> Rebase {
        Rebase {
            chars: i64::try_from(segment.char_base).unwrap_or(i64::MAX) - self.chars,
            lines: i64::from(segment.line_base) - self.lines,
        }
    }
}

enum Resume {
    At(Segment),
    /// `true` when stopping because the recovery budget ran out.
    Stop(bool),
}

fn run_segment(
    builder: &mut Builder<'_>,
    source: &SourceFile,
    prelude: &Prelude,
    segment: Segment,
) -> Option<ScanError> {
    let tail = source.slice_chars(segment.char_base, source.char_len());
    let text: Cow<'_, str> = if prelude.text.is_empty() {
        Cow::Borrowed(tail)
    } else {
        Cow::Owned(format!("{}{tail}", prelude.text))
    };
    let mut parser = Parser::new_from_str(&text).keep_tags(true);
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

fn error_span(
    source: &SourceFile,
    error: &ScanError,
    prelude: &Prelude,
    segment: Segment,
) -> Span {
    let pos = source.pos(error.marker(), prelude.rebase(segment));
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
