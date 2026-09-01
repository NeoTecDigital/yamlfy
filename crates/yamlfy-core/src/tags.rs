// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tag classification.
//!
//! A [`Tag`] arrives from the front end as two opaque strings. Which of them
//! carries the meaning is decided by YAML, not by us:
//!
//! * With no `%TAG` directive, `!node` is `handle = "!"`, `suffix = "node"`.
//! * A `%TAG ! tag:example.com,2026:` directive rewrites the *handle* to
//!   `tag:example.com,2026:` and leaves the suffix `node` untouched, so testing
//!   `handle == "!"` would silently stop recognising every tag in that file.
//! * A verbatim `!<node>` yields `handle = ""`, `suffix = "node"`.
//! * `!!node` is the core schema's `tag:yaml.org,2002:node`, a different tag
//!   that must not be mistaken for ours.
//!
//! The suffix is therefore the only field stable across all four spellings, and
//! classification reads the suffix while excluding the core schema by handle.

use yamlfy_syntax::Tag;

/// The core schema's handle. Anything carrying it is a YAML built-in, never a
/// Yamlfication tag.
const CORE_HANDLE: &str = "tag:yaml.org,2002:";

/// The suffix a header document carries.
pub const HEADER_SUFFIX: &str = "yamlfy/header";

/// What a tag means to Yamlfication.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TagKind {
    /// `!type` — abstract, inheritable, never emitted as a model.
    Type,
    /// `!node` — concrete, emitted as a model.
    Node,
    /// `!ref` — a cross-document reference.
    Ref,
    /// `!edge` — a typed edge.
    Edge,
    /// `!yamlfy/header` — the per-file header document.
    Header,
    /// `!oneof` — reserved (D7.4) and not implemented. Writing it is `E0222`;
    /// classifying it is what makes the reservation observable, because an
    /// unrecognised tag on a value would otherwise do nothing at all.
    OneOf,
    /// Anything else, including every core-schema tag such as `!!str`.
    Other,
}

impl TagKind {
    /// The canonical spelling, for logging and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TagKind::Type => "!type",
            TagKind::Node => "!node",
            TagKind::Ref => "!ref",
            TagKind::Edge => "!edge",
            TagKind::Header => "!yamlfy/header",
            TagKind::OneOf => "!oneof",
            TagKind::Other => "other",
        }
    }
}

/// Classify a tag by its suffix, excluding the core schema by its handle.
#[must_use]
pub fn classify(tag: &Tag) -> TagKind {
    if &*tag.handle == CORE_HANDLE {
        return TagKind::Other;
    }
    match &*tag.suffix {
        "type" => TagKind::Type,
        "node" => TagKind::Node,
        "ref" => TagKind::Ref,
        "edge" => TagKind::Edge,
        "oneof" => TagKind::OneOf,
        HEADER_SUFFIX => TagKind::Header,
        _ => TagKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(handle: &str, suffix: &str) -> Tag {
        Tag { handle: handle.into(), suffix: suffix.into() }
    }

    #[test]
    fn the_default_primary_handle_is_classified() {
        assert_eq!(classify(&tag("!", "node")), TagKind::Node);
        assert_eq!(classify(&tag("!", "type")), TagKind::Type);
        assert_eq!(classify(&tag("!", "ref")), TagKind::Ref);
        assert_eq!(classify(&tag("!", "edge")), TagKind::Edge);
        assert_eq!(classify(&tag("!", HEADER_SUFFIX)), TagKind::Header);
        assert_eq!(classify(&tag("!", "oneof")), TagKind::OneOf);
    }

    #[test]
    fn a_reserved_spelling_is_classified_rather_than_ignored() {
        // D7.4: a reservation with no diagnostic behind it is not a
        // reservation, and the diagnostic needs the classification to exist.
        assert_eq!(classify(&tag("", "oneof")), TagKind::OneOf);
        assert_eq!(classify(&tag(CORE_HANDLE, "oneof")), TagKind::Other);
    }

    #[test]
    fn a_tag_directive_rewrites_the_handle_and_must_not_defeat_classification() {
        assert_eq!(classify(&tag("tag:example.com,2026:", "node")), TagKind::Node);
    }

    #[test]
    fn a_verbatim_tag_has_an_empty_handle() {
        assert_eq!(classify(&tag("", "node")), TagKind::Node);
    }

    #[test]
    fn the_core_schema_is_never_ours() {
        assert_eq!(classify(&tag(CORE_HANDLE, "node")), TagKind::Other);
        assert_eq!(classify(&tag(CORE_HANDLE, "str")), TagKind::Other);
    }

    #[test]
    fn an_unknown_local_tag_is_other() {
        assert_eq!(classify(&tag("!", "widget")), TagKind::Other);
    }
}
