// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! D7.4 — `!oneof` is reserved, and writing it is reported.

mod common;

use yamlfy_core::{classify, TagKind};
use yamlfy_syntax::Code;

#[test]
fn the_reserved_spelling_classifies_rather_than_falling_through_to_other() {
    let tag = yamlfy_syntax::Tag { handle: "!".into(), suffix: "oneof".into() };
    assert_eq!(classify(&tag), TagKind::OneOf, "an `Other` here is a silently ignored tag");
}

#[test]
fn writing_the_reserved_tag_in_source_is_an_error() {
    let project = common::open("reserved-tag");
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(
        common::count(project.diagnostics(), Code::ReservedTag),
        1,
        "one `!oneof` in the source file, none in the data file:\n{rendered}"
    );
    assert!(rendered.contains("modes.yfy:7:14"), "{rendered}");
    assert!(rendered.contains("reserved and not implemented"), "{rendered}");
}

#[test]
fn the_reservation_does_not_reach_into_base_yaml() {
    // D6.6: the tag vocabulary is not interpreted in a `.yaml` file, so
    // `objects.yaml` writes the same spelling and hears nothing about it.
    let project = common::open("reserved-tag");
    let data = project
        .files()
        .iter()
        .find(|file| file.relative.ends_with("objects.yaml"))
        .expect("objects.yaml");
    let reported: Vec<_> = project
        .diagnostics()
        .with_code(Code::ReservedTag)
        .filter(|item| item.span.is_some_and(|span| span.file == data.id))
        .collect();

    assert!(reported.is_empty(), "reserving a name in data the engine does not own");
}
