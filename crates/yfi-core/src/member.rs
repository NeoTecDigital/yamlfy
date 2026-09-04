// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Member flags: the two scope axes and the one override, written one level
//! down.
//!
//! ```text
//! ClassA:
//!   - private_member                   // private, immutable
//!   - pub public_member
//!   - public public_member_two
//!   - mutable mutable_member
//!   - mut member_two
//!   - pub mut public_mutable_member
//!   - mutable public mutable_public_member
//!   - brute forced_member              // writes where it may not
//! ```
//!
//! `pub`/`public` and `mut`/`mutable` are **prefixes on the member name**, in
//! either order, either or both, and they are not tags. `- pub mut name` is
//! already the ordinary YAML string `"pub mut name"`, so nothing about the
//! parse changes and there is no collision with `!type`, `!node` or `!ref`; the
//! prefix is read off the scalar here.
//!
//! A bare member is `private` and `immutable` — D6.4 one level down — and the
//! escape is D4.2's: a quoted or tagged key is literal text, so a field
//! genuinely called `pub x` is written `"pub x"`.
//!
//! Composition needs no new rule and none is written here: the member's
//! declaration and the scope path are combined by the same
//! [`ScopeTree`](crate::scope::ScopeTree) walk every other reach uses (D6.5).
//!
//! # `brute` is not a third axis
//!
//! The two axes state what a member *is*. `brute` states that a member **writes
//! anyway** — it forces the mutation an immutable target would otherwise refuse
//! (`E0217`). It is therefore a flag and not an axis: forcing is present or
//! absent, with no second value and no default worth naming, and it is closed
//! like the axes are — a bare member forces nothing.
//!
//! It is a prefix rather than a tag for the reason the axes are: the position
//! already exists, the word is read off the scalar, and the parse does not
//! change. And it is spelled at all — rather than inferred, or allowed silently
//! — because a write that overrides a refusal is the one act in the language
//! that must be visible in the source that performs it. Forcing is never
//! quiet: it is written here and recorded as `W0304` where it takes effect.
//!
//! # `override` is a prefix on the operand, not on the member
//!
//! ```text
//! extends: !ref override ../lib/Shared   // redefinition, and a claim beside it
//! extends: override ../lib/Shared        // the claim alone: no write, no gate
//! <<: override ../lib/Shared             // a runtime claim, and no more
//! brute Amend: !node                     // the two prefixes, composed, and the
//!   extends: !ref override P             //   three declarations all distinct
//! ```
//!
//! Same lexing, different position and different vocabulary — [`split_operand`]
//! says why. `override` qualifies what an **operator** does with its operand
//! and inherits that operator's blast radius (D4.14), so there is nothing for
//! it to mean on a member name; `brute` qualifies what a **member** does with a
//! refusal, so there is nothing for it to mean on a path.
//!
//! Neither implies the other, and neither implies `!ref`. Three positions, three
//! declarations: the tag says *I intend to modify*, `override` says *my claim
//! outranks the other claimants*, and `brute` says *write anyway*. Only the
//! first is gated, only the third forces a gate, and the second asks for
//! nothing at all.

use crate::scope::{Mutability, Visibility};

/// What a member declared about itself.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct MemberFlags {
    /// Stated visibility. `None` is the closed default.
    pub visibility: Option<Visibility>,
    /// Stated mutability. `None` is the closed default.
    pub mutability: Option<Mutability>,
    /// Whether the member forces its write past a refusal. Absent is the closed
    /// default: a member that says nothing forces nothing.
    pub brute: bool,
}

impl MemberFlags {
    /// The member's visibility: what it declared, else `private`.
    #[must_use]
    pub fn visibility(self) -> Visibility {
        self.visibility.unwrap_or(Visibility::Private)
    }

    /// The member's mutability: what it declared, else `immutable`.
    #[must_use]
    pub fn mutability(self) -> Mutability {
        self.mutability.unwrap_or(Mutability::Immutable)
    }

    /// Whether the member forces its write past a refusal.
    #[must_use]
    pub fn is_brute(self) -> bool {
        self.brute
    }

    /// Whether the member stated anything at all.
    #[must_use]
    pub fn is_declared(self) -> bool {
        self.visibility.is_some() || self.mutability.is_some() || self.brute
    }
}

/// Split a member name into its flags and the name itself.
///
/// Flag words are consumed from the front while they last, so order is free and
/// repetition is idempotent. **The last word is never a flag**: `- pub` declares
/// a member called `pub`, because a prefix with nothing to qualify is a name.
#[must_use]
pub fn split(text: &str) -> (MemberFlags, &str) {
    let mut flags = MemberFlags::default();
    let rest = peel(text, |word| {
        match word {
            "pub" | "public" => flags.visibility = Some(Visibility::Public),
            "mut" | "mutable" => flags.mutability = Some(Mutability::Mutable),
            "brute" => flags.brute = true,
            _ => return false,
        }
        true
    });
    (flags, rest)
}

/// Split an **operand** prefix off a path, returning whether it said `override`
/// and the path itself (D4.14).
///
/// The same lexing as [`split`] — a word peeled off a plain scalar, the last
/// word never a flag — over a different vocabulary, because an operand is a
/// **path** and not a member. `pub`, `mut` and `brute` state what a member *is*
/// or *does* and have nothing to qualify on a path; consuming them here would
/// make `<<: pub Base` quietly resolve to `Base`, which is a spelling the
/// language never gave a meaning. `override` runs the other way: it qualifies
/// an operator's operand and has nothing to say about a member, so it is not in
/// [`split`]'s vocabulary either. And neither of them implies `!ref`, which is
/// a tag rather than a prefix and declares a third thing again (D4.14).
///
/// The two therefore compose without either learning about the other:
/// `brute claim: !ref override ../lib/Shared` reads `brute` off the key and
/// `override` off the value.
#[must_use]
pub fn split_operand(text: &str) -> (bool, &str) {
    let mut overrides = false;
    let rest = peel(text, |word| {
        if word != OVERRIDE {
            return false;
        }
        overrides = true;
        true
    });
    (overrides, rest)
}

/// The word that claims priority among a target's holders.
const OVERRIDE: &str = "override";

/// Consume whitespace-separated words from the front of `text` while `take`
/// accepts them, and return what is left.
///
/// **The last word is never offered**, which is the whole of the "a prefix with
/// nothing to qualify is a name" rule and is why both readers get it for free.
fn peel(text: &str, mut take: impl FnMut(&str) -> bool) -> &str {
    let mut rest = text.trim();
    while let Some((word, tail)) = rest.split_once(char::is_whitespace) {
        if !take(word) {
            break;
        }
        rest = tail.trim_start();
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (Visibility, Mutability, &str) {
        let (flags, name) = split(text);
        (flags.visibility(), flags.mutability(), name)
    }

    #[test]
    fn brute_is_read_off_the_name_like_the_axes() {
        let (flags, name) = split("brute valuation");
        assert!(flags.is_brute());
        assert_eq!(name, "valuation", "the member keeps its real name");
        assert_eq!(flags.visibility(), Visibility::Private, "brute states no axis");
        assert_eq!(flags.mutability(), Mutability::Immutable);
    }

    #[test]
    fn brute_composes_with_both_axes_in_any_order() {
        let (flags, name) = split("pub brute mut port");
        assert_eq!(
            (flags.visibility(), flags.mutability(), flags.is_brute(), name),
            (Visibility::Public, Mutability::Mutable, true, "port")
        );
        assert!(split("brute pub x").0.is_brute());
    }

    #[test]
    fn a_bare_member_forces_nothing() {
        assert!(!split("member").0.is_brute());
    }

    #[test]
    fn brute_with_nothing_to_qualify_is_a_name() {
        let (flags, name) = split("brute");
        assert!(!flags.is_brute());
        assert_eq!(name, "brute");
    }

    #[test]
    fn brute_alone_is_a_declaration() {
        assert!(split("brute x").0.is_declared());
    }

    #[test]
    fn a_bare_member_is_private_and_immutable() {
        assert_eq!(read("member"), (Visibility::Private, Mutability::Immutable, "member"));
    }

    #[test]
    fn either_spelling_of_either_axis_is_accepted() {
        assert_eq!(read("pub a").0, Visibility::Public);
        assert_eq!(read("public a").0, Visibility::Public);
        assert_eq!(read("mut a").1, Mutability::Mutable);
        assert_eq!(read("mutable a").1, Mutability::Mutable);
    }

    #[test]
    fn the_two_axes_combine_in_either_order() {
        assert_eq!(read("pub mut a"), (Visibility::Public, Mutability::Mutable, "a"));
        assert_eq!(read("mutable public a"), (Visibility::Public, Mutability::Mutable, "a"));
    }

    #[test]
    fn a_flag_word_with_nothing_to_qualify_is_a_name() {
        assert_eq!(read("pub"), (Visibility::Private, Mutability::Immutable, "pub"));
        assert_eq!(read("mut"), (Visibility::Private, Mutability::Immutable, "mut"));
    }

    #[test]
    fn a_word_that_is_not_a_flag_ends_the_prefix() {
        assert_eq!(read("port pub x"), (Visibility::Private, Mutability::Immutable, "port pub x"));
        assert_eq!(read("pub port x"), (Visibility::Public, Mutability::Immutable, "port x"));
    }

    #[test]
    fn nothing_is_declared_unless_a_flag_is_written() {
        assert!(!split("member").0.is_declared());
        assert!(split("pub member").0.is_declared());
    }
}
