// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The documented diagnostic vocabulary is the implemented one.
//!
//! §4 of `docs/semantics.md` claims its table is **the whole vocabulary**, and
//! that claim is only worth making if something keeps it true. Twice now a code
//! has been raised, made configurable and left out of the table — `W0304` from
//! the day D6.4b landed — and both times it was a review that caught it, late.
//! So the claim is checked here instead: the table is parsed and diffed against
//! [`Code::all`] in both directions, and the next divergence fails a build.
//!
//! Two numbers are named rather than matched, because §4 names them.
//! `E0215` is a row of the table that says **retired, and never reused**: the
//! hole is documented on purpose, so the row must be there and the code must
//! not. `W0302` is a label for a deferred discussion with **no number
//! allocated**, so it must appear in the prose and never as a row. Both are
//! spelled out rather than skipped, because a test that ignored every
//! unmatched code would ignore a real omission just as quietly.

use std::collections::BTreeSet;
use std::path::PathBuf;

use yfi_syntax::Code;

/// A row of §4's table that must never be a code: the hole is the record.
const RETIRED: &str = "E0215";

/// A spelling §4 records as a discussion with no number, so never a row.
const UNNUMBERED: &str = "W0302";

fn specification() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/semantics.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every code the §4 table lists, read off the first cell of each row.
///
/// A code appears on more than one row where one fault is raised by two passes
/// over disjoint inputs — `E0110`, `E0213` and `W0303` all do — so this is a
/// set and the repetition is not a finding.
fn documented(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines().filter(|line| line.starts_with("| `")) {
        let Some(cell) = line[3..].split('`').next() else { continue };
        let printed = cell.as_bytes();
        let shaped = printed.len() == 5
            && matches!(printed[0], b'E' | b'W')
            && printed[1..].iter().all(u8::is_ascii_digit);
        if shaped {
            out.insert(cell.to_owned());
        }
    }
    out
}

#[test]
fn the_documented_table_is_exactly_the_implemented_vocabulary() {
    let text = specification();
    let mut documented = documented(&text);
    assert!(documented.len() > 20, "the table was not found, so nothing was compared");
    assert!(documented.remove(RETIRED), "the retired number's row is the record of the hole");
    assert!(!documented.contains(UNNUMBERED), "`{UNNUMBERED}` was never allocated a number");
    let implemented: BTreeSet<String> =
        Code::all().iter().map(|code| code.as_str().to_owned()).collect();
    let undocumented: Vec<&String> = implemented.difference(&documented).collect();
    assert!(undocumented.is_empty(), "raised and configurable, and in no table: {undocumented:?}");
    documented.retain(|code| !implemented.contains(code));
    assert!(documented.is_empty(), "documented and unraisable: {documented:?}");
}

#[test]
fn a_retired_number_is_documented_as_retired_and_is_not_a_code() {
    // The other half of the same guarantee. A project pinning `--deny E0215`
    // must get a clean "unknown code" rather than a silent redirection to an
    // unrelated rule, so the number stays burned in both places at once.
    let text = specification();
    for held in [RETIRED, UNNUMBERED] {
        assert_eq!(Code::parse(held), None, "`{held}` must name nothing");
        assert!(text.contains(held), "`{held}` is burned in the code and unrecorded");
    }
}
