// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reserved spellings the language holds back (D7.4).
//!
//! `!oneof` is reserved and unimplemented; writing it is `E0222`. The check
//! belongs to discovery because it needs exactly two things discovery already
//! holds — the tag, and the **class** of the file that wrote it. In base YAML
//! the tag vocabulary is not interpreted (D6.6), so `!oneof` there stays
//! silent.

use tracing::debug;
use yfi_syntax::{Code, Diagnostic, Diagnostics, NodeId};

use crate::discover::{FileClass, ProjectFile};
use crate::tags::{classify, TagKind};

/// What a reader is told when they write the reserved spelling.
const MESSAGE: &str = "`!oneof` is reserved and not implemented; the spelling is held back so \
                       that enumerations can be given one later without breaking documents \
                       already written";

/// Report every reserved tag written in a Yamlfication source file.
pub(crate) fn check(files: &[ProjectFile], diagnostics: &mut Diagnostics) {
    for file in files.iter().filter(|file| file.class == FileClass::Source) {
        for id in reserved_in(file) {
            debug!(file = file.id.0, node = id.0, "reserved tag");
            diagnostics.push(Diagnostic::new(Code::ReservedTag, file.ast.node(id).span, MESSAGE));
        }
    }
}

/// Every node of `file` carrying a reserved tag, in arena order.
fn reserved_in(file: &ProjectFile) -> Vec<NodeId> {
    (0..file.ast.nodes().len())
        .map(|position| NodeId(u32::try_from(position).expect("arena overflow")))
        .filter(|id| file.ast.tag(*id).map(classify) == Some(TagKind::OneOf))
        .collect()
}
