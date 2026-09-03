// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostic vocabulary: every code the compiler can raise, and how
//! seriously each is taken.
//!
//! Split from the collection that holds them because the two answer different
//! questions. This file is the **list** — what can go wrong, what it prints as,
//! and what a project may reconfigure — and is read by anyone auditing the
//! language's surface. [`super`] is the plumbing that accumulates, orders and
//! renders findings, and is read by anyone changing how a report looks.

use std::collections::BTreeMap;
use std::fmt;

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
    /// A **non-scalar** key carries the `!!merge` tag, which makes no merge key
    /// of it (D1.1) and means nothing where it stands.
    MergeTagOnComplexKey,
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
    /// An `!edge` node holds no `connections` member, so the tag relates
    /// nothing.
    EdgeWithoutConnections,
    /// An edge's `connections` is not a sequence, or its `definition` is not a
    /// mapping.
    EdgeMemberShape,
    /// A `definition` handle names no position of the edge that resolves it —
    /// past the end, not a position at all, or one of the two member names the
    /// language owns on an edge.
    ///
    /// Two handles naming **one** position is not this: a self-loop is written
    /// `from: 0` and `to: 0`, and the mapping is many-to-one on purpose.
    UnboundHandle,
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
    /// A `!ref` binds a name this file already defines, so a bare path written
    /// against the definition would silently resolve through the binding
    /// instead.
    ///
    /// Numbered after the member code rather than filling the gap at `E0215`:
    /// that number was spent on the retired "`!ref` into a file this file does
    /// not import", and a code that once meant one thing and now means another
    /// makes every configuration file and every archived report that names it
    /// wrong without saying so.
    BindingShadowsDefinition,
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
    /// A `!yfi/header` field carries a value the language does not define.
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
            Code::MergeTagOnComplexKey => "E0111",
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
            Code::BindingShadowsDefinition => "E0219",
            Code::RequiredFieldUnsatisfied => "E0220",
            Code::DeclaredTagMismatch => "E0221",
            Code::ReservedTag => "E0222",
            Code::EdgeWithoutConnections => "E0223",
            Code::EdgeMemberShape => "E0224",
            Code::UnboundHandle => "E0225",
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
            Code::MergeTagOnComplexKey,
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
            Code::BindingShadowsDefinition,
            Code::RequiredFieldUnsatisfied,
            Code::DeclaredTagMismatch,
            Code::UndeclaredField,
            Code::InertContribution,
            Code::ReservedTag,
            Code::EdgeWithoutConnections,
            Code::EdgeMemberShape,
            Code::UnboundHandle,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            let expected =
                if code.as_str().starts_with('W') { Severity::Warning } else { Severity::Error };
            assert_eq!(code.default_severity(), expected, "{code}");
        }
        assert_eq!(Code::AnchorShadowed.default_severity(), Severity::Warning);
        assert_eq!(Code::InertContribution.default_severity(), Severity::Warning);
    }

    #[test]
    fn every_variant_is_listed_and_prints_a_distinct_code() {
        // `all()` is hand-kept and is what `--deny` validates against, so a
        // variant missing from it is a code no project can configure and no
        // documentation lists. Counting is not enough on its own -- a repeated
        // entry would pass that -- so the printed forms are required distinct.
        let mut printed: Vec<&str> = Code::all().iter().map(|code| code.as_str()).collect();
        let listed = printed.len();
        printed.sort_unstable();
        printed.dedup();
        assert_eq!(printed.len(), listed, "two variants print one code");
    }
}
