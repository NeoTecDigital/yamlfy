// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Accumulating diagnostics.
//!
//! Nothing in this crate returns on the first problem. A pass runs to
//! completion, pushing every finding into a [`Diagnostics`], and the caller
//! decides what to do with the collection.
//!
//! The vocabulary itself — every [`Code`] and its [`Severity`] — lives in
//! [`code`], because the list of what can go wrong and the machinery that
//! orders and renders findings are read by different people for different
//! reasons.

use std::fmt;

use crate::span::{SourceMap, Span};

mod code;

pub use code::{Code, Severity, SeverityMap};

/// A secondary location attached to a diagnostic.
#[derive(Clone, Debug)]
pub struct Note {
    /// What the location means.
    pub message: String,
    /// Where it is, when it has a location.
    pub span: Option<Span>,
}

/// One recorded problem.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The problem class.
    pub code: Code,
    /// Effective severity after configuration.
    pub severity: Severity,
    /// One-line description.
    pub message: String,
    /// The location the diagnostic points at, when it has one.
    pub span: Option<Span>,
    /// Supporting locations.
    pub notes: Vec<Note>,
}

impl Diagnostic {
    /// A diagnostic at `span` with the default severity for `code`.
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: code.default_severity(),
            message: message.into(),
            span: Some(span),
            notes: Vec::new(),
        }
    }

    /// Attach a supporting location.
    #[must_use]
    pub fn with_note(mut self, message: impl Into<String>, span: Option<Span>) -> Self {
        self.notes.push(Note { message: message.into(), span });
        self
    }
}

/// An ordered, de-duplicated collection of diagnostics.
#[derive(Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    severities: SeverityMap,
}

impl Diagnostics {
    /// An empty collection using default severities.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty collection using `severities` as overrides.
    #[must_use]
    pub fn with_severities(severities: SeverityMap) -> Self {
        Diagnostics { items: Vec::new(), severities }
    }

    /// Effective severity for `code`.
    #[must_use]
    pub fn severity_of(&self, code: Code) -> Severity {
        self.severities.get(&code).copied().unwrap_or_else(|| code.default_severity())
    }

    /// Record `diagnostic` unless its code is configured to `allow`.
    pub fn push(&mut self, mut diagnostic: Diagnostic) {
        let severity = self.severity_of(diagnostic.code);
        if severity == Severity::Allow {
            return;
        }
        diagnostic.severity = severity;
        self.items.push(diagnostic);
    }

    /// Every recorded diagnostic, in the order they were found.
    #[must_use]
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Number of recorded diagnostics whose severity is `error`.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::Error).count()
    }

    /// Whether any recorded diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Whether anything at all was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every diagnostic carrying `code`.
    pub fn with_code(&self, code: Code) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(move |d| d.code == code)
    }

    /// Append another collection's contents.
    pub fn extend(&mut self, other: Diagnostics) {
        self.absorb(other.items);
    }

    /// Append diagnostics that have **already been through configuration**,
    /// keeping the severity each one carries.
    ///
    /// [`Diagnostics::push`] decides severity; this does not. Severity is
    /// decided exactly once, by the pass that raised the finding, because
    /// `Allow` has to suppress *recording* and a collection cannot un-record
    /// what it never saw. A merger that re-decided would also let its own map
    /// silently override the one the raising pass was configured with — which
    /// is a second source of truth for a question that has one.
    pub fn absorb(&mut self, items: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(items);
    }

    /// Every diagnostic, ordered by **where it points**: file, then line, then
    /// column. Ties keep the order they were found in, the sort being stable.
    ///
    /// This is D6.3's `(file rank, document index, node index)` expressed in the
    /// terms a diagnostic actually carries. A `FileId` is an index into the one
    /// source map and files are registered in discovery order, so ordering by it
    /// is ordering by file rank; and within a file, position ascends with
    /// document and node index, so line and column decide the rest.
    ///
    /// Insertion order cannot be the printed order. Findings arrive by *pass*,
    /// not by position — every file's parse diagnostics, then everything the
    /// project-wide passes found — so a cause routinely prints after its
    /// consequence (`E0241` at line 7 after the `E0100` at line 9 it explains)
    /// and files interleave. A reader fixes faults top-down through a file.
    ///
    /// A diagnostic with no span sorts last: it belongs to no position, and
    /// putting it first would push the file it is about below it.
    #[must_use]
    pub fn in_position_order(&self) -> Vec<&Diagnostic> {
        let mut ordered: Vec<&Diagnostic> = self.items.iter().collect();
        ordered.sort_by_key(|item| match item.span {
            Some(span) => (span.file.0, span.start.line, span.start.col),
            None => (u32::MAX, u32::MAX, u32::MAX),
        });
        ordered
    }

    /// Render every diagnostic as `severity[CODE] path:line:col: message`, in
    /// [`Diagnostics::in_position_order`].
    #[must_use]
    pub fn render(&self, sources: &SourceMap) -> String {
        let mut out = String::new();
        for item in self.in_position_order() {
            render_one(&mut out, sources, item);
        }
        out
    }
}

fn render_one(out: &mut String, sources: &SourceMap, item: &Diagnostic) {
    use fmt::Write as _;
    let at = item.span.map_or_else(|| "<unknown>".to_owned(), |s| sources.location(s));
    let _ = writeln!(out, "{}[{}] {}: {}", item.severity, item.code, at, item.message);
    for note in &item.notes {
        let at = note.span.map_or_else(String::new, |s| format!(" {}", sources.location(s)));
        let _ = writeln!(out, "  note:{at} {}", note.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{FileId, Pos};

    fn span() -> Span {
        Span::empty(FileId(0), Pos { byte: 0, line: 7, col: 3 })
    }

    #[test]
    fn an_allowed_code_is_never_recorded() {
        let mut severities = SeverityMap::new();
        severities.insert(Code::DuplicateKey, Severity::Allow);
        let mut diagnostics = Diagnostics::with_severities(severities);
        diagnostics.push(Diagnostic::new(Code::DuplicateKey, span(), "ignored"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn configuration_can_promote_a_warning_to_an_error() {
        let mut severities = SeverityMap::new();
        severities.insert(Code::AnchorShadowed, Severity::Error);
        let mut diagnostics = Diagnostics::with_severities(severities);
        diagnostics.push(Diagnostic::new(Code::AnchorShadowed, span(), "shadowed"));
        assert_eq!(diagnostics.error_count(), 1);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn diagnostics_accumulate_in_the_order_they_were_found() {
        let mut diagnostics = Diagnostics::new();
        for i in 0..3 {
            diagnostics.push(Diagnostic::new(Code::DuplicateKey, span(), format!("dup {i}")));
        }
        let messages: Vec<&str> = diagnostics.items().iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["dup 0", "dup 1", "dup 2"]);
    }

    #[test]
    fn rendering_orders_by_position_rather_than_by_the_pass_that_found_it() {
        let mut sources = SourceMap::new();
        let first = sources.add("a.yml", "a: 1\n");
        let second = sources.add("b.yml", "c: 3\n");
        let at = |file, line| Span::empty(file, Pos { byte: 0, line, col: 1 });
        let mut diagnostics = Diagnostics::new();
        // The order a pipeline finds them in: another file first, then one
        // file's own parse, then the project-wide pass that explains it.
        diagnostics.push(Diagnostic::new(Code::SyntaxError, at(second, 1), "another file"));
        diagnostics.push(Diagnostic::new(Code::SyntaxError, at(first, 9), "the consequence"));
        diagnostics.push(Diagnostic::new(Code::UnresolvedImport, at(first, 2), "the cause"));

        let printed: Vec<String> =
            diagnostics.render(&sources).lines().map(ToOwned::to_owned).collect();
        assert!(printed[0].contains("a.yml:2:1"), "{printed:?}");
        assert!(
            printed[1].contains("a.yml:9:1"),
            "a cause prints above what it caused: {printed:?}"
        );
        assert!(printed[2].contains("b.yml:1:1"), "and one file does not interleave with another");
        assert_eq!(
            diagnostics.items()[0].message,
            "another file",
            "the collection itself still holds them in the order they were found"
        );
    }

    #[test]
    fn rendering_prints_severity_code_and_location() {
        let mut sources = SourceMap::new();
        sources.add("t.yml", "a: 1\n");
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(
            Diagnostic::new(Code::DuplicateKey, span(), "duplicate mapping key `a`")
                .with_note("first defined here", Some(span())),
        );
        let rendered = diagnostics.render(&sources);
        assert!(rendered.starts_with("error[E0110] t.yml:7:3: duplicate"), "{rendered}");
        assert!(rendered.contains("note: t.yml:7:3 first defined here"), "{rendered}");
    }
}
