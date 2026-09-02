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
    /// A `<?--` block was opened and never closed.
    UnterminatedBlock,
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
    /// An anchor name is redefined: the name enters a new state.
    AnchorShadowed,
    /// A reserved tag was written that the language does not implement.
    ReservedTag,
    /// A merge key or an `extends` entry carries an operand the language does
    /// not accept as a source.
    IllegalMergeSource,
    /// The inheritance graph contains a cycle.
    CyclicInheritance,
    /// A path names nothing in the project.
    UnresolvedRef,
    /// Two extended references contribute the same key to one base with
    /// different values.
    ConflictingExtension,
    /// A path names a definition in a scope the referencing scope cannot see.
    RefNotVisible,
    /// A `!ref` names a definition in a scope the referencing scope may not
    /// write. `!ref` declares mutation intent, so the target must be mutable.
    RefNotWritable,
    /// A path addresses a member the node it resolved to does not hold.
    UnresolvedMember,
    /// An extended reference contributes a key the base already defines, so
    /// the contribution does nothing.
    InertContribution,
    /// A concrete node leaves an ancestor's required field unsupplied.
    RequiredFieldUnsatisfied,
    /// A concrete node's effective value contradicts an ancestor's declared
    /// tag.
    DeclaredTagMismatch,
    /// A concrete node carries a field no abstract ancestor declares.
    UndeclaredField,
    /// Two files claim the same namespace.
    DuplicateNamespace,
    /// A `!yamlfy/header` field carries a value the language does not define.
    BadHeaderValue,
    /// A header's `imports:` entry names no file of the project.
    UnresolvedImport,
    /// A header's `imports:` entry names a file the importer cannot see.
    ImportNotVisible,
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
            Code::UnterminatedBlock => "E0104",
            Code::DuplicateKey => "E0110",
            Code::AnchorNameUnrecoverable => "E0120",
            Code::AnchorOrderInconsistent => "E0121",
            Code::CrossDocumentAlias => "E0130",
            Code::DuplicateMergeKey => "E0210",
            Code::AnchorShadowed => "W0300",
            Code::IllegalMergeSource => "E0211",
            Code::CyclicInheritance => "E0212",
            Code::UnresolvedRef => "E0213",
            Code::ConflictingExtension => "E0214",
            Code::RefNotVisible => "E0216",
            Code::RefNotWritable => "E0217",
            Code::UnresolvedMember => "E0218",
            Code::RequiredFieldUnsatisfied => "E0220",
            Code::DeclaredTagMismatch => "E0221",
            Code::ReservedTag => "E0222",
            Code::UndeclaredField => "W0301",
            Code::InertContribution => "W0303",
            Code::DuplicateNamespace => "E0230",
            Code::BadHeaderValue => "E0231",
            Code::UnresolvedImport => "E0240",
            Code::ImportNotVisible => "E0241",
        }
    }

    /// Severity used when configuration says nothing.
    #[must_use]
    pub fn default_severity(self) -> Severity {
        match self {
            Code::AnchorShadowed | Code::InertContribution | Code::UndeclaredField => {
                Severity::Warning
            }
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
            Code::UnterminatedBlock,
            Code::DuplicateKey,
            Code::DuplicateMergeKey,
            Code::AnchorNameUnrecoverable,
            Code::AnchorOrderInconsistent,
            Code::CrossDocumentAlias,
            Code::AnchorShadowed,
            Code::IllegalMergeSource,
            Code::CyclicInheritance,
            Code::UnresolvedRef,
            Code::ConflictingExtension,
            Code::RefNotVisible,
            Code::RefNotWritable,
            Code::UnresolvedMember,
            Code::RequiredFieldUnsatisfied,
            Code::DeclaredTagMismatch,
            Code::UndeclaredField,
            Code::InertContribution,
            Code::ReservedTag,
            Code::DuplicateNamespace,
            Code::BadHeaderValue,
            Code::UnresolvedImport,
            Code::ImportNotVisible,
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
    fn every_code_defaults_to_an_error_unless_it_is_one_of_the_two_warnings() {
        // The `W` prefix and the default severity are two spellings of one
        // fact, so they are checked against each other rather than against a
        // hand-kept list that can drift from the printed code.
        for code in Code::all() {
            let expected = if code.as_str().starts_with('W') {
                Severity::Warning
            } else {
                Severity::Error
            };
            assert_eq!(code.default_severity(), expected, "{code}");
        }
        assert_eq!(Code::AnchorShadowed.default_severity(), Severity::Warning);
        assert_eq!(Code::InertContribution.default_severity(), Severity::Warning);
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
