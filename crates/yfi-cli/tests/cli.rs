// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end checks of the `yamlfy` binary over the real fixture corpus.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(relative)
}

fn project(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects").join(relative)
}

fn yamlfy(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yamlfy"))
        .args(args)
        // Keep the process environment out of the test's way.
        .env_remove("YAMLFY_LOG")
        .env_remove("YAMLFY_SEVERITY_W0300")
        .output()
        .expect("yamlfy runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn check_reports_file_line_col_for_a_real_document() {
    let path = fixture("shadowing/basic-shadow.yml");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success(), "warnings alone must not fail the run: {text}");
    assert!(text.contains("basic-shadow.yml:6:13"), "{text}");
    assert!(text.contains("warning[W0300]"), "{text}");
    assert!(text.contains("0 error(s), 1 warning(s)"), "{text}");
}

#[test]
fn check_fails_on_an_error_and_reports_every_one() {
    let path = fixture("malformed/duplicate-key.yml");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert_eq!(text.matches("error[E0110]").count(), 2, "{text}");
    assert!(text.contains("duplicate-key.yml:4:1"), "{text}");
    assert!(text.contains("duplicate-key.yml:6:1"), "{text}");
}

#[test]
fn check_parses_a_cyclic_document_without_hanging() {
    let path = fixture("cycles/deep-cycle.yml");
    let output = yamlfy(&["check", "--dump", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("alias(*n1)"), "{text}");
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");
}

#[test]
fn allow_and_deny_flags_change_the_outcome() {
    let path = fixture("shadowing/basic-shadow.yml");
    let quiet = yamlfy(&["--allow", "W0300", "check", path.to_str().unwrap()]);
    assert!(quiet.status.success());
    assert!(stdout(&quiet).contains("0 error(s), 0 warning(s)"), "{}", stdout(&quiet));

    let strict = yamlfy(&["--deny", "W0300", "check", path.to_str().unwrap()]);
    assert!(!strict.status.success());
    assert!(stdout(&strict).contains("1 error(s)"), "{}", stdout(&strict));
}

#[test]
fn several_files_are_checked_in_one_run() {
    let a = fixture("malformed/duplicate-key.yml");
    let b = fixture("valid/tags.yfy");
    let output = yamlfy(&["check", a.to_str().unwrap(), b.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert!(text.contains("2 error(s), 0 warning(s)"), "{text}");
}

#[test]
fn a_missing_file_is_reported_not_a_crash() {
    let output = yamlfy(&["check", "/nonexistent/file.yml"]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert!(text.contains("error[E0102]"), "{text}");
    assert!(text.contains("/nonexistent/file.yml:1:1"), "{text}");
}

#[test]
fn an_unknown_diagnostic_code_on_a_flag_is_rejected() {
    let output = yamlfy(&["--deny", "E9999", "check", "whatever.yml"]);
    let text = String::from_utf8_lossy(&output.stderr).into_owned();

    assert_eq!(output.status.code(), Some(2), "{text}");
    assert!(text.contains("unknown diagnostic code"), "{text}");
}

#[test]
fn check_resolves_a_header_import_because_it_runs_discovery() {
    // Parsing this file alone reports `E0100 unknown anchor` on `*Service`,
    // which is defined in `core/net.yfy` and reaches it through the header. The
    // library has always compiled it; `check` did not, and that gap is the one
    // thing worse than no subcommand — a file that passes as a project and
    // fails as a file. D6.1: they are one operation at two scopes.
    let path = project("import-alias/app.yfy");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");
}

#[test]
fn only_the_paths_asked_about_are_reported() {
    // The project root of a lone file is its parent directory, so discovery
    // reads every sibling. `defs.yfy` shadows an import and warns; asking about
    // `app.yfy` must not print that.
    let path = project("import-shadowing/app.yfy");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");

    let whole = yamlfy(&["check", project("import-shadowing").to_str().unwrap()]);
    let all = stdout(&whole);
    assert!(all.contains("warning[W0300]"), "a directory reports its whole subtree: {all}");
}

#[test]
fn an_explicit_root_overrides_the_derived_one() {
    // `core/net.yfy` sits below the project root. Rooted at its own directory
    // it is a project of one file; rooted at the project it is a member of one,
    // and `--root` is what says which.
    let path = project("import-alias/core/net.yfy");
    let root = project("import-alias");
    let output = yamlfy(&["check", "--root", root.to_str().unwrap(), path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");
    assert!(!text.contains("app.yfy"), "the sibling was discovered, not reported: {text}");
}

#[test]
fn a_root_that_is_not_a_directory_is_a_flag_error() {
    let path = fixture("valid/scalars.yml");
    let output = yamlfy(&["check", "--root", path.to_str().unwrap(), path.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is not a directory"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_import_naming_nothing_is_reported_as_an_unresolved_import() {
    let path = project("import-missing/app.yfy");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert_eq!(text.matches("error[E0240]").count(), 2, "{text}");
    assert!(!text.contains("E0231"), "an unresolved import is not a bad header value: {text}");
}

#[test]
fn an_import_that_cannot_see_its_target_is_reported_at_the_import() {
    let path = project("import-private/open/user.yfy");
    let root = project("import-private");
    let output = yamlfy(&["check", "--root", root.to_str().unwrap(), path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert_eq!(text.matches("error[E0241]").count(), 1, "{text}");
    assert!(text.contains("open/user.yfy:7:11"), "at the import entry: {text}");
    assert!(text.contains("secret/hidden.yfy:6:13"), "noting the scope that blocked it: {text}");
    assert!(!text.contains("E0240"), "the file exists; only its reach is the problem: {text}");
}

#[test]
fn the_reserved_tag_is_reported_rather_than_silently_ignored() {
    let path = project("reserved-tag/modes.yfy");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success());
    assert!(text.contains("error[E0222]"), "{text}");
    assert!(text.contains("modes.yfy:7:14"), "{text}");
}

#[test]
fn the_semantic_passes_reach_the_command_line() {
    // Every code asserted elsewhere in this file is owned by `discover` or
    // `parse`. This is the only test that fails if `check` stops running
    // `intern`, `link` and `check` -- which is what it did until recently,
    // reporting `0 error(s)` on a project raising every one of them. A compiler whose
    // semantic errors are invisible from a terminal is one whose errors
    // nobody sees, so the assertion is on codes no file reader can raise.
    let path = project("edge-errors");
    let output = yamlfy(&["check", path.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success(), "a project raising errors must exit 1: {text}");
    for code in ["E0213", "E0223", "E0224", "E0225"] {
        assert!(text.contains(&format!("error[{code}]")), "{code} never reached stdout: {text}");
    }
    assert!(text.contains("14 error(s)"), "{text}");
}

// ------------------------------------------------- the invocation-wide report

#[test]
fn two_projects_in_one_run_each_render_against_their_own_files() {
    // The single-`SourceMap` invariant, made observable. `FileId` is an index
    // into that map and the report is rendered once, at the end, from one
    // accumulated collection -- so a second map would restart at `FileId(0)`
    // and the second project's findings would name the first project's files.
    // Two roots, two codes, and each must point at the file that earned it.
    let a = project("import-missing/app.yfy");
    let b = project("reserved-tag/modes.yfy");
    let output = yamlfy(&["check", a.to_str().unwrap(), b.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(!output.status.success(), "{text}");
    for (code, file) in [("E0240", "import-missing/app.yfy"), ("E0222", "reserved-tag/modes.yfy")] {
        let line = text
            .lines()
            .find(|line| line.contains(&format!("error[{code}]")))
            .unwrap_or_else(|| panic!("no {code} in:\n{text}"));
        assert!(line.contains(file), "{code} rendered against the wrong file: {line}");
    }
    assert!(text.contains("3 error(s), 0 warning(s)"), "one count over the whole run: {text}");
}

#[test]
fn a_deny_flag_reaches_a_code_only_the_semantic_passes_raise() {
    // `W0303` is `link`'s, not the parser's, so this fails unless the severity
    // map is handed to `link_with` -- which is where severity is decided, once,
    // because `allow` has to suppress *recording* and nothing downstream can
    // un-record what it never received.
    let path = project("link-inert-contribution");
    let quiet = yamlfy(&["check", path.to_str().unwrap()]);
    assert!(quiet.status.success(), "{}", stdout(&quiet));
    assert!(stdout(&quiet).contains("warning[W0303]"), "{}", stdout(&quiet));

    let strict = yamlfy(&["--deny", "W0303", "check", path.to_str().unwrap()]);
    let text = stdout(&strict);
    assert!(!strict.status.success(), "{text}");
    assert!(text.contains("error[W0303]"), "{text}");
    assert!(!text.contains("warning[W0303]"), "decided once, not re-decided: {text}");

    let allowed = yamlfy(&["--allow", "W0303", "check", path.to_str().unwrap()]);
    assert!(!stdout(&allowed).contains("W0303"), "{}", stdout(&allowed));
}

#[test]
fn a_deny_flag_reaches_the_override_warning() {
    // `W0305` is `check`'s and is the newest code in the table, so this is the
    // end-to-end proof of the promise §4 makes about every code: it prints, it
    // is denied, and it is allowed. `--deny` is validated against `Code::all()`,
    // so a variant missing from that list would fail here as an unknown code
    // rather than as a missing diagnostic.
    let path = project("override-nothing");
    let quiet = yamlfy(&["check", path.to_str().unwrap()]);
    assert!(quiet.status.success(), "{}", stdout(&quiet));
    assert!(stdout(&quiet).contains("warning[W0305]"), "{}", stdout(&quiet));

    let strict = yamlfy(&["--deny", "W0305", "check", path.to_str().unwrap()]);
    let text = stdout(&strict);
    assert!(!strict.status.success(), "{text}");
    assert!(text.contains("error[W0305]"), "{text}");

    let allowed = yamlfy(&["--allow", "W0305", "check", path.to_str().unwrap()]);
    assert!(!stdout(&allowed).contains("W0305"), "{}", stdout(&allowed));
}
