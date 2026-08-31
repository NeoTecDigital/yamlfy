// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end checks of the `yamlfy` binary over the real fixture corpus.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(relative)
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
    let b = fixture("valid/tags.yml");
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
