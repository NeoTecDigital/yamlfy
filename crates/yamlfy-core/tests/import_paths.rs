// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an import path may name, and what it may not.
//!
//! `imports.rs` next door is about what crossing a file *means* — the two file
//! classes, transitivity, shadowing, cycles. This is the narrower question of
//! path resolution: which file a written path denotes, and where the project
//! boundary falls. The two rules pull in opposite directions and are tested
//! together for that reason, over one fixture that exercises both at once.
//!
//! The fixture is built in code rather than checked in because it turns on a
//! **symlink**, which a source corpus cannot carry portably; `common::scratch`
//! is the same helper `discover.rs` uses for the walk's own symlink cases.

mod common;

use yamlfy_core::DiscoverOptions;
use yamlfy_syntax::Code;

/// A file symlinked in from outside the tree is discovered, ranked and scoped
/// like any other, and must therefore be importable — by the path that
/// discovered it.
///
/// Resolving membership through the link instead canonicalises to the target,
/// finds it outside the root and rejects it, so a file the project has in every
/// other sense can never be named by one of its own headers. That also makes
/// vendoring impossible, which is the arrangement this exists for: a directory
/// of data owned by another team, linked in rather than copied.
///
/// The escaping import in the same fixture is the other half. Its path
/// canonicalises to *the same file*, so it is in the identity table and the
/// project-escape guard is the only thing that rejects it — which is what makes
/// the guard observable at all. `import-missing` cannot show it: its paths name
/// nothing that exists, so the table misses them first and the guard is never
/// reached.
#[cfg(unix)]
#[test]
fn a_symlinked_in_file_is_importable_by_the_path_that_discovered_it() {
    let tree = common::scratch::Tree::new("import-symlink");
    tree.write("vendor/vendor.yfy", VENDOR);
    tree.link("vendor/vendor.yfy", "proj/vendor.yfy");
    tree.write("proj/app.yfy", APP);
    tree.write("proj/escape.yfy", ESCAPE);

    let project = yamlfy_core::discover(tree.path().join("proj"), &DiscoverOptions::default());
    let rendered = project.diagnostics().render(project.sources());
    assert_eq!(
        common::relative_paths(&project),
        ["app.yfy", "escape.yfy", "vendor.yfy"],
        "the link is a member of the project by discovery:\n{rendered}"
    );

    let app = common::file_id(&project, "app.yfy");
    let vendor = common::file_id(&project, "vendor.yfy");
    assert_eq!(project.imports_of(app), [vendor], "and importable as one:\n{rendered}");
    assert_eq!(
        project.file(app).expect("app").ast.alias_binding(common::entry_at(
            &project,
            app,
            1,
            &["extends"]
        )),
        Some((vendor, common::declaration(&project, vendor, "Vendored"))),
        "so `*Vendored` binds, which is the whole point of importing it:\n{rendered}"
    );

    // The guard, still doing its job on the identity route.
    assert_eq!(
        common::count(project.diagnostics(), Code::UnresolvedImport),
        1,
        "the escaping path names the same real file and is still rejected:\n{rendered}"
    );
    assert!(rendered.contains("escape.yfy:3:11:"), "at that entry, and no other:\n{rendered}");
    assert!(project.imports_of(common::file_id(&project, "escape.yfy")).is_empty(), "{rendered}");
}

/// The vendored file. It carries no header, because every file in one
/// directory declares that directory's one scope and `app.yfy` already does.
const VENDOR: &str = "--- !type &Vendored\nport: !!int\n";

const APP: &str = concat!(
    "--- !yamlfy/header\nversion: 1\nnamespace: app\nimports: [vendor.yfy]\n",
    "--- !node &Api\nextends: *Vendored\n"
);

/// Names the same real file as `vendor.yfy`, by a path that leaves the project.
const ESCAPE: &str = concat!(
    "--- !yamlfy/header\nversion: 1\nimports: [../vendor/vendor.yfy]\n",
    "--- !node &Escape\nport: 1\n"
);
