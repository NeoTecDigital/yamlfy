// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Accumulating diagnostics.
//!
//! Nothing in this crate returns on the first problem. A pass runs to
//! completion, pushing every finding into a [`Diagnostics`], and the caller
//! decides what to do with the collection.

use std::collections::BTreeMap;
use std::fmt;

use crate::span::{SourceMap, Span};

/// How seriously a [`Code`] is taken. Configurable per code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// Suppressed entirely; the diagnostic is never recorded.
    Allow,
    /// Recorded, does not fail the build.
    Warning,
    /// Recorded, fails the build.
    Error,
}

impl Severity {
    /// The lower-case name used in configuration files.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Allow => "allow",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }

    /// Parse a configuration value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "allow" | "off" => Some(Severity::Allow),
            "warn" | "warning" => Some(Severity::Warning),
            "error" | "deny" => Some(Severity::Error),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stable identifier for one class of problem.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Code {
    /// The YAML stream is malformed.
    SyntaxError,
    /// The file is not valid UTF-8.
    InvalidUtf8,
    /// The file could not be read.
    IoError,
    /// Recovery gave up before reaching the end of the file.
    RecoveryLimitExceeded,
    /// A mapping contains the same key twice.
    DuplicateKey,
    /// A mapping contains more than one merge key.
    DuplicateMergeKey,
    /// An anchor's name could not be recovered from the source text.
    AnchorNameUnrecoverable,
    /// Recovered anchor positions are not in definition order.
    AnchorOrderInconsistent,
    /// An alias refers to an anchor defined in an earlier document.
    CrossDocumentAlias,
    /// An anchor name is redefined, shadowing an earlier definition.
    AnchorShadowed,
}

impl Code {
    /// The printed code, for example `E0100`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Code::SyntaxError => "E0100",
            Code::InvalidUtf8 => "E0101",
            Code::IoError => "E0102",
            Code::RecoveryLimitExceeded => "E0103",
            Code::DuplicateKey => "E0110",
            Code::AnchorNameUnrecoverable => "E0120",
            Code::AnchorOrderInconsistent => "E0121",
            Code::CrossDocumentAlias => "E0130",
            Code::DuplicateMergeKey => "E0210",
            Code::AnchorShadowed => "W0300",
        }
    }

    /// Severity used when configuration says nothing.
    #[must_use]
    pub fn default_severity(self) -> Severity {
        match self {
            Code::AnchorShadowed => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// Every code, for configuration validation and documentation.
    #[must_use]
    pub fn all() -> &'static [Code] {
        &[
            Code::SyntaxError,
            Code::InvalidUtf8,
            Code::IoError,
            Code::RecoveryLimitExceeded,
            Code::DuplicateKey,
            Code::DuplicateMergeKey,
            Code::AnchorNameUnrecoverable,
            Code::AnchorOrderInconsistent,
            Code::CrossDocumentAlias,
            Code::AnchorShadowed,
        ]
    }

    /// Resolve a printed code back to its variant.
    #[must_use]
    pub fn parse(text: &str) -> Option<Code> {
        let wanted = text.trim().to_ascii_uppercase();
        Code::all().iter().copied().find(|c| c.as_str() == wanted)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-code severity overrides.
pub type SeverityMap = BTreeMap<Code, Severity>;

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

    /// A diagnostic about a whole file rather than a range in it.
    pub fn detached(code: Code, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: code.default_severity(),
            message: message.into(),
            span: None,
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
        self.items.extend(other.items);
    }

    /// Render every diagnostic as `severity[CODE] path:line:col: message`.
    #[must_use]
    pub fn render(&self, sources: &SourceMap) -> String {
        let mut out = String::new();
        for item in &self.items {
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
    fn codes_round_trip_through_their_printed_form() {
        for code in Code::all() {
            assert_eq!(Code::parse(code.as_str()), Some(*code));
        }
        assert_eq!(Code::parse("E9999"), None);
    }

    #[test]
    fn severity_accepts_the_names_a_configuration_would_use() {
        assert_eq!(Severity::parse("Deny"), Some(Severity::Error));
        assert_eq!(Severity::parse("off"), Some(Severity::Allow));
        assert_eq!(Severity::parse("warn"), Some(Severity::Warning));
        assert_eq!(Severity::parse("loud"), None);
    }

    #[test]
    fn only_shadowing_defaults_to_a_warning() {
        for code in Code::all() {
            let expected =
                if *code == Code::AnchorShadowed { Severity::Warning } else { Severity::Error };
            assert_eq!(code.default_severity(), expected, "{code}");
        }
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
