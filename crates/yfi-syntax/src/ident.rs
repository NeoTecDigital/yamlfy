// Written by Richard Christopher, Copyright 2026 Richard Christopher
// SPDX-License-Identifier: GPL-3.0-or-later

//! What may spell a name.
//!
//! One rule, stated once: a namespace component, and the root scope's name
//! when a project states it, are both a non-empty run of `[A-Za-z0-9_-]`.
//! Every layer that validates such a name asks here, so the compiler and the
//! configuration cannot drift apart on which spellings they accept.

/// Whether `text` is a legal identifier: non-empty, and made only of ASCII
/// letters, digits, `_` and `-`.
#[must_use]
pub fn is_identifier(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The spelling rule, for diagnostics that refuse a name.
pub const IDENTIFIER_RULE: &str = "letters, digits, `_` and `-`";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_is_a_non_empty_run_of_the_allowed_characters() {
        assert!(is_identifier("pdl"));
        assert!(is_identifier("acme-core_2"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("a::b"));
        assert!(!is_identifier("a/b"));
        assert!(!is_identifier("a b"));
        assert!(!is_identifier("ünicode"));
    }
}
