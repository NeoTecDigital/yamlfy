// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! When a cross-file binding is computed, and what it is computed against.
//!
//! Two questions `projects/` answers that the import *edges* do not: that the
//! binding pass iterates over a cycle from a seed no parse can give it (D6.7),
//! and that which of an import and a local definition an alias means is a
//! question about *where* the alias is written (D2.6).

mod common;

use yfi_syntax::Code;

#[test]
fn a_cycle_binds_when_every_anchor_is_written_after_its_alias() {
    // The shape a cycle cannot survive by luck. Neither document root is
    // anchored and every `&name` sits *below* the cross-file alias, so the
    // unbound survey parse of each member dies at the alias and records no
    // definition at all — the state in which "install what is known, re-parse"
    // has nothing to install and never starts. The fixture holds one 2-cycle
    // (`a`, `b`) and one 3-cycle (`x`, `y`, `z`), and both compile clean.
    let project = common::open_clean("import-cycle-late-anchor");
    let (a, b) = (common::file_id(&project, "a/a.yfy"), common::file_id(&project, "b/b.yfy"));
    let (x, y, z) = (
        common::file_id(&project, "x/x.yfy"),
        common::file_id(&project, "y/y.yfy"),
        common::file_id(&project, "z/z.yfy"),
    );

    assert_eq!(project.import_cycles(), [vec![a, b], vec![x, y, z]], "both are recorded");

    for (importer, exporter, name) in
        [(a, b, "B"), (b, a, "A"), (x, y, "Y"), (y, z, "Z"), (z, x, "X")]
    {
        let alias = common::entry_at(&project, importer, 1, &["peer"]);
        assert_eq!(
            project.file(importer).expect("member").ast.alias_binding(alias),
            Some((exporter, common::declaration(&project, exporter, name))),
            "`*{name}` binds to the definition written after the other side's own alias"
        );
    }
}

#[test]
fn an_import_survives_a_local_definition_of_the_same_name_in_an_earlier_document() {
    // `app.yfy` imports `&Name` and then writes its own `&Name` in document 1.
    // That local state dies with its document (D2.6), and document 2 starts
    // with the imported bindings and nothing else (D6.7) — so `*Name` there is
    // an ordinary alias to the import, not an alias reaching back across a
    // document boundary. Reporting `E0130` for it would make an import stop
    // working because of a name used in a document that has already ended.
    let project = common::open("import-reinstalled-after-shadow");
    let app = common::file_id(&project, "app.yfy");
    let lib = common::file_id(&project, "lib/l.yfy");
    let rendered = project.diagnostics().render(project.sources());
    let ast = &project.file(app).expect("app").ast;

    assert_eq!(
        common::count(project.diagnostics(), Code::CrossDocumentAlias),
        0,
        "the name is in scope in both documents, by two different definitions:\n{rendered}"
    );
    assert_eq!(
        common::count(project.diagnostics(), Code::SyntaxError),
        0,
        "and neither alias is unknown:\n{rendered}"
    );
    assert_eq!(
        ast.alias_binding(common::entry_at(&project, app, 1, &["self"])),
        Some((app, common::declaration(&project, app, "Name"))),
        "inside the shadowing document the local definition wins (D2.1)"
    );
    assert_eq!(
        ast.alias_binding(common::entry_at(&project, app, 2, &["v"])),
        Some((lib, common::declaration(&project, lib, "Name"))),
        "and the next document is back to the imported binding"
    );

    // The shadow itself is still a shadow, reported once, in the document that
    // wrote it.
    assert_eq!(common::count(project.diagnostics(), Code::AnchorShadowed), 1, "{rendered}");
}
