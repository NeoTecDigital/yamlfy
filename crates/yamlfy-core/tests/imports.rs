// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Imports, the two file classes, and what crosses a file boundary.

mod common;

use yamlfy_core::{intern, FileClass, TagKind};
use yamlfy_syntax::{Code, NodeId};

#[test]
fn a_source_file_imports_a_source_file() {
    let project = common::open_clean("imports-source");
    let app = common::file_id(&project, "app.yfy");
    let net = common::file_id(&project, "net.yfy");
    assert_eq!(project.imports_of(app), [net]);
    assert_eq!(project.imports_of(net), [], "importing is not transitive and net imports nothing");
    assert_eq!(project.file(net).map(|f| f.class), Some(FileClass::Source));
}

#[test]
fn importing_one_file_twice_records_it_once() {
    let project = common::open_clean("imports-source");
    let app = common::file_id(&project, "app.yfy");
    assert_eq!(project.imports_of(app).len(), 1, "the fixture names `core/net.yfy` twice");
}

#[test]
fn a_source_file_imports_a_data_file() {
    let project = common::open_clean("imports-data");
    let app = common::file_id(&project, "app.yfy");
    let data = common::file_id(&project, "services.yaml");
    assert_eq!(project.imports_of(app), [data]);
    assert_eq!(project.file(data).map(|f| f.class), Some(FileClass::Data));
    assert!(project.file(data).is_some_and(|f| f.header.is_none()), "a data file has no header");
    assert!(project.file(data).is_some_and(|f| f.imports.is_empty()), "and cannot import");
    assert!(project.import_reaches(app, data), "and the importer can see it");
    assert_eq!(
        common::imported_names(&project, app),
        ["defaults"],
        "importing a `.yaml` brings its objects, which is the whole point of the fixture"
    );
}

#[test]
fn a_data_file_is_never_read_as_yamlfication() {
    // `nested-namespaces/net/edge.yaml` carries a `!yamlfy/header` document
    // claiming a namespace and a nonsense visibility, plus an `extends` key.
    // Read as source that is one E0231 and a scope claim; read as data it is
    // inert, which is the whole point of the two classes.
    let project = common::open_clean("nested-namespaces");
    let edge = common::file_id(&project, "edge.yaml");
    let file = project.file(edge).expect("edge.yaml");

    assert_eq!(file.class, FileClass::Data);
    assert!(file.header.is_none(), "`!yamlfy/header` in base YAML is not a header");
    assert_eq!(common::count(project.diagnostics(), Code::BadHeaderValue), 0);
    assert_eq!(common::count(project.diagnostics(), Code::DuplicateNamespace), 0);

    let scope = project.scopes().get(file.scope).expect("net scope");
    assert_ne!(
        scope.namespace.as_deref(),
        Some("not::a::namespace"),
        "a data file declares nothing about its scope"
    );
}

#[test]
fn yamlfication_tags_are_inert_in_a_data_file() {
    let project = common::open_clean("nested-namespaces");
    let interned = intern(&project);
    let edge = common::file_id(&project, "edge.yaml");
    let file = project.file(edge).expect("edge.yaml");

    let kinds: Vec<TagKind> = (0..file.ast.nodes().len())
        .filter_map(|i| interned.tag_kind(edge, NodeId(i as u32)))
        .collect();
    assert!(!kinds.is_empty(), "the file does carry tags");
    assert!(
        kinds.iter().all(|k| *k == TagKind::Other),
        "`!node` and `!yamlfy/header` are inert in base YAML: {kinds:?}"
    );
    assert_eq!(interned.class_of(edge), Some(FileClass::Data));

    let source = common::file_id(&project, "service.yfy");
    let source_kinds: Vec<TagKind> = (0..project
        .file(source)
        .expect("service.yfy")
        .ast
        .nodes()
        .len())
        .filter_map(|i| interned.tag_kind(source, NodeId(i as u32)))
        .collect();
    assert!(source_kinds.contains(&TagKind::Header), "the same tag is live in source");
}

#[test]
fn an_extends_key_in_a_data_file_stays_data() {
    let project = common::open_clean("imports-data");
    let interned = intern(&project);
    let data = common::file_id(&project, "services.yaml");
    let file = project.file(data).expect("services.yaml");

    let extends = interned.symbols().get("extends").expect("`extends` is interned as a key");
    let found = (0..file.ast.nodes().len())
        .filter(|i| interned.key_of(data, NodeId(*i as u32)) == Some(extends))
        .count();
    assert_eq!(found, 1, "`extends` is an ordinary mapping key here, nothing more");

    let merges = (0..file.ast.nodes().len())
        .filter_map(|i| file.ast.entries(NodeId(i as u32)))
        .flatten()
        .filter(|entry| entry.merge)
        .count();
    assert_eq!(merges, 1, "`<<` is plain YAML merge, still recognised syntactically");
    assert!(!project.diagnostics().has_errors(), "and neither is interpreted by us");
}

#[test]
fn an_import_cycle_is_legal_and_recorded() {
    let project = common::open_clean("import-cycle");
    let a = common::file_id(&project, "a.yfy");
    let b = common::file_id(&project, "b.yfy");

    assert_eq!(project.imports_of(a), [b]);
    assert_eq!(project.imports_of(b), [a]);
    assert!(!project.diagnostics().has_errors(), "an import cycle is not an error");

    let cycles = project.import_cycles();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0], vec![a, b], "members are listed in rank order");

    // A service and its network, each naming the other. What each side sees is
    // a one-step union — an import carries no value, so the *meaning* has no
    // fixed point in it (D6.7) — even though computing it takes a parse per
    // round.
    assert_eq!(
        project.file(a).expect("a").ast.alias_binding(common::entry_at(&project, a, 1, &["peer"])),
        Some((b, common::declaration(&project, b, "B")))
    );
    assert_eq!(
        project.file(b).expect("b").ast.alias_binding(common::entry_at(&project, b, 1, &["peer"])),
        Some((a, common::declaration(&project, a, "A")))
    );
    // `&Late` sits after `b`'s own cross-file alias, so `b` has to be bound
    // before it exists at all — and `b` cannot be bound before `a`, because
    // they import each other. A cycle has no imports-first order, so its
    // members are seeded from their text and rebound until their exported names
    // stop moving; `import-cycle-late-anchor` is the case where the seed is the
    // only thing that starts it.
    assert_eq!(
        project.file(a).expect("a").ast.alias_binding(common::entry_at(&project, a, 1, &["also"])),
        Some((b, common::declaration(&project, b, "Late")))
    );
}

#[test]
fn an_acyclic_import_graph_reports_no_cycle() {
    let project = common::open_clean("imports-source");
    assert!(project.import_cycles().is_empty());
}

#[test]
fn an_import_that_names_nothing_is_reported_once_per_entry() {
    let project = common::open("import-missing");
    let rendered = project.diagnostics().render(project.sources());
    assert_eq!(
        common::count(project.diagnostics(), Code::UnresolvedImport),
        2,
        "one absent path, one path leaving the project:\n{rendered}"
    );
    assert!(rendered.contains("does not name a file of this project"), "{rendered}");
    assert_eq!(
        common::count(project.diagnostics(), Code::BadHeaderValue),
        0,
        "an unresolved import is `E0240`, not a header value the language cannot read"
    );
    let app = common::file_id(&project, "app.yfy");
    assert!(project.imports_of(app).is_empty(), "nothing resolved, and nothing invented");
}

#[test]
fn importing_does_not_launder_a_private_definition() {
    let project = common::open("import-private");
    let user = common::file_id(&project, "user.yfy");
    let hidden = common::file_id(&project, "hidden.yfy");
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(project.imports_of(user), [hidden], "the edge is recorded either way");
    assert!(
        !project.import_reaches(user, hidden),
        "a `private` scope stays private; an import is not a way out of it"
    );

    // Reaching nothing is not the same as doing nothing quietly. The import is
    // `E0241` at its own entry, with the scope that closed the path in a note.
    assert_eq!(
        common::count(project.diagnostics(), Code::ImportNotVisible),
        1,
        "one entry, one diagnostic:\n{rendered}"
    );
    assert_eq!(
        common::count(project.diagnostics(), Code::UnresolvedImport),
        0,
        "the path names a real file, so this is not `E0240`:\n{rendered}"
    );
    assert!(
        rendered.contains(
            "open/user.yfy:7:11: `secret/hidden.yfy` names a file this scope cannot see"
        ),
        "the primary span is the import entry, in the header the author wrote:\n{rendered}"
    );
    assert!(
        rendered.contains("note: ")
            && rendered.contains(
                "secret/hidden.yfy:6:13 `import-private/secret` is declared `private`"
            ),
        "and the note names the scope that blocked it, at its `visibility:`:\n{rendered}"
    );

    let interned = intern(&project);
    let hidden_scope = project.file(hidden).expect("hidden").scope;
    let root = project.file(hidden).expect("hidden").ast.documents()[1].root;
    assert_eq!(
        interned.scope_of(hidden, root),
        Some(hidden_scope),
        "an imported definition keeps the exporting scope, never the importing one"
    );
    assert_eq!(interned.scope_path_of(hidden, root), Some(project.scopes().path(hidden_scope)));
    assert!(
        common::imported_names(&project, user).is_empty(),
        "an import that cannot reach its target installs nothing, rather than working quietly"
    );
}

#[test]
fn an_unreachable_import_is_diagnosed_at_the_import_and_not_at_the_alias() {
    // The failure this exists to prevent. `open/user.yfy` imports a `private`
    // definition *and* aliases it, so without `E0241` the only thing reported
    // would be `E0100` at `*Secret` — the wrong code, in the wrong file, about
    // the wrong construct: the alias is written correctly and the import is
    // what cannot reach. The alias failure is a consequence and is reported as
    // well; the diagnosis of the cause is what must be present.
    let project = common::open("import-private-alias");
    let user = common::file_id(&project, "user.yfy");
    let hidden = common::file_id(&project, "hidden.yfy");
    let rendered = project.diagnostics().render(project.sources());

    assert!(!project.import_reaches(user, hidden));
    assert_eq!(
        common::count(project.diagnostics(), Code::ImportNotVisible),
        1,
        "the cause is diagnosed, once:\n{rendered}"
    );
    assert!(
        rendered.contains(
            "open/user.yfy:7:11: `secret/hidden.yfy` names a file this scope cannot see"
        ),
        "at the import entry, not at line 9 where `*Secret` is written:\n{rendered}"
    );
    assert!(
        common::imported_names(&project, user).is_empty(),
        "and the reach itself is unchanged: an unreachable import still installs nothing"
    );

    // Recorded rather than asserted away: the unknown anchor is a *second*,
    // downstream error for one fault. It is not suppressed, because suppressing
    // it would mean an import that installs nothing sometimes hiding an alias
    // that would fail anyway — and `E0241` already names the cause.
    let cascade = common::count(project.diagnostics(), Code::SyntaxError);
    assert_eq!(cascade, 1, "the alias to the uninstalled name still fails:\n{rendered}");
    assert!(
        rendered.contains("open/user.yfy:9:7: while parsing node, found unknown anchor"),
        "{rendered}"
    );
}

#[test]
fn a_cross_file_alias_reaches_the_imported_definition() {
    // The point of imports is `extends: *Service` reaching a definition in
    // another file (D4.4). The import puts `Service` into this document before
    // its first event, so the alias is an ordinary alias and the operation is
    // an ordinary extension — nothing learned to travel.
    let project = common::open_clean("import-alias");
    let app = common::file_id(&project, "app.yfy");
    let net = common::file_id(&project, "net.yfy");

    assert_eq!(project.imports_of(app), [net], "the import edge resolves");
    assert!(project.import_reaches(app, net), "and the target is visible");
    assert_eq!(
        common::count(project.diagnostics(), Code::ImportNotVisible),
        0,
        "a reachable import raises nothing at all"
    );

    let alias = common::entry_at(&project, app, 1, &["extends"]);
    let service = common::declaration(&project, net, "Service");
    assert_eq!(
        project.file(app).expect("app").ast.alias_binding(alias),
        Some((net, service)),
        "`*Service` resolves to the node `core/net.yfy` anchored, in that file"
    );
    assert_eq!(
        project.file(app).expect("app").ast.alias_target(alias),
        None,
        "and not to anything in the importing file's own arena"
    );
}

#[test]
fn an_imported_definition_keeps_the_span_it_was_written_at() {
    // A diagnostic about an imported definition must point at the file that
    // wrote it, with that file's line and column — never at the importing file
    // and never at the synthetic text the parser is handed.
    let project = common::open("import-shadowed-locally");
    let app = common::file_id(&project, "app.yfy");
    let net = common::file_id(&project, "net.yfy");
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(common::count(project.diagnostics(), Code::AnchorShadowed), 1, "{rendered}");
    assert!(
        rendered
            .contains("import-shadowed-locally/app.yfy:8:16: anchor `&Service` enters a new state"),
        "the local definition is the new state, and it is what is pointed at:\n{rendered}"
    );
    assert!(
        rendered.contains("note: ")
            && rendered.contains("core/net.yfy:7:11 the state it supersedes"),
        "the superseded state is in the imported file, at its own line and column:\n{rendered}"
    );

    let alias = common::entry_at(&project, app, 1, &["Api", "extends"]);
    let local = common::declaration(&project, app, "Service");
    assert_eq!(
        project.file(app).expect("app").ast.alias_binding(alias),
        Some((app, local)),
        "a local definition after the import wins for aliases after it (D2.1)"
    );
    assert!(project.import_reaches(app, net));
}

#[test]
fn two_imports_of_one_name_shadow_in_authored_order() {
    // `alpha` sorts before `omega`, so discovery ranks it first; the header
    // names `omega` first, so `alpha` is the later import and the one a bare
    // `*Service` denotes. Import order is authored, not discovered (D6.7).
    let project = common::open("import-shadowing");
    let app = common::file_id(&project, "app.yfy");
    let alpha = common::file_id(&project, "alpha/defs.yfy");
    let omega = common::file_id(&project, "omega/defs.yfy");
    let rendered = project.diagnostics().render(project.sources());

    assert!(
        project.rank(alpha) < project.rank(omega),
        "the fixture is only discriminating if discovery order is the other way round"
    );
    assert_eq!(project.imports_of(app), [omega, alpha], "resolved in written order");

    let alias = common::entry_at(&project, app, 1, &["extends"]);
    assert_eq!(
        project.file(app).expect("app").ast.alias_binding(alias),
        Some((alpha, common::declaration(&project, alpha, "Service"))),
        "the last import wins, and last is a fact about the header:\n{rendered}"
    );

    assert_eq!(common::count(project.diagnostics(), Code::AnchorShadowed), 1, "{rendered}");
    assert!(
        rendered.contains("alpha/defs.yfy:7:11: anchor `&Service` enters a new state"),
        "W0300 points at the new state, in the file that wrote it:\n{rendered}"
    );
    assert!(
        rendered.contains("omega/defs.yfy:7:11 the state it supersedes is here"),
        "and its note at the state it supersedes, likewise:\n{rendered}"
    );
}

#[test]
fn an_import_does_not_relax_the_document_boundary() {
    // D2.6 is preserved, not weakened. `*Service` is legal in both documents
    // because the import is re-installed at the start of each; `*Api` is not,
    // because `&Api` is an ordinary definition of an earlier document.
    let project = common::open("import-cross-document");
    let app = common::file_id(&project, "app.yfy");
    let net = common::file_id(&project, "net.yfy");
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(common::count(project.diagnostics(), Code::SyntaxError), 0, "{rendered}");
    assert_eq!(
        common::count(project.diagnostics(), Code::CrossDocumentAlias),
        1,
        "exactly one alias crosses a document boundary:\n{rendered}"
    );
    assert!(rendered.contains("app.yfy:11:7: alias `*Api`"), "{rendered}");

    let ast = &project.file(app).expect("app").ast;
    let service = common::declaration(&project, net, "Service");
    for document in [1, 2] {
        let alias = common::entry_at(&project, app, document, &["extends"]);
        assert_eq!(
            ast.alias_binding(alias),
            Some((net, service)),
            "every document starts with the same imported bindings and nothing else"
        );
    }
}

#[test]
fn an_import_is_not_transitive() {
    // `a` imports `b`, `b` imports `c`. `a` receives what `b` wrote and nothing
    // `b` imported, so `*B` binds and `*C` is an unknown anchor in `a` while
    // resolving perfectly well inside `b`.
    let project = common::open("import-not-transitive");
    let (a, b, c) = (
        common::file_id(&project, "a/a.yfy"),
        common::file_id(&project, "b/b.yfy"),
        common::file_id(&project, "c/c.yfy"),
    );
    let rendered = project.diagnostics().render(project.sources());

    assert_eq!(project.imports_of(a), [b]);
    assert_eq!(project.imports_of(b), [c]);

    assert_eq!(
        common::imported_names(&project, a),
        ["B", "Late"],
        "`a` receives all of `own(b)` and none of `own(c)`"
    );
    assert_eq!(common::imported_names(&project, b), ["C"]);
    // `&Late` is written *after* `b`'s own cross-file alias, so it exists only
    // in a parse of `b` that already had `c` bound into it. Binding runs one
    // strongly connected component at a time, imports first, which is what
    // makes a chain converge without iterating.
    assert_eq!(
        project.file(a).expect("a").ast.alias_binding(common::entry_at(&project, a, 1, &["late"])),
        Some((b, common::declaration(&project, b, "Late")))
    );

    let base = common::entry_at(&project, b, 1, &["base"]);
    assert_eq!(
        project.file(b).expect("b").ast.alias_binding(base),
        Some((c, common::declaration(&project, c, "C"))),
        "the clause is discharged where it is written (D4.9)"
    );

    assert_eq!(
        common::count(project.diagnostics(), Code::SyntaxError),
        1,
        "and `*C` in `a` names nothing `a` can see:\n{rendered}"
    );
    assert!(
        rendered.contains("a/a.yfy:12:8: while parsing node, found unknown anchor"),
        "{rendered}"
    );
}

#[test]
fn the_shared_parser_reads_both_classes_and_only_the_class_differs() {
    // `fixtures/valid/header-document.yfy` is Yamlfication source living in the
    // parser corpus. The front end knows nothing about classes; it parses this
    // file exactly as it parses a `.yml`. Only `discover` treats it as source,
    // and that is the sole reason its header is read at all.
    let source = common::open_at("fixtures/valid/header-document.yfy");
    let file = &source.files()[0];
    assert_eq!(file.class, FileClass::Source);
    let header = file.header.as_ref().expect("a source file's header is read");
    assert_eq!(header.namespace.as_ref().map(|(n, _)| &**n), Some("acme::billing"));
    assert!(header.imports.is_empty());
    assert!(!source.diagnostics().has_errors(), "an unknown header key such as `schema` is ignored");

    let tags = common::open_at("fixtures/valid/tags.yfy");
    let interned = intern(&tags);
    let id = tags.files()[0].id;
    let kinds: Vec<TagKind> = (0..tags.files()[0].ast.nodes().len())
        .filter_map(|i| interned.tag_kind(id, NodeId(i as u32)))
        .collect();
    assert!(kinds.contains(&TagKind::Node), "`!node` is live in source: {kinds:?}");
    assert!(kinds.contains(&TagKind::Edge));
    assert!(kinds.contains(&TagKind::Ref));
}
