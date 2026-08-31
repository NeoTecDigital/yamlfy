// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration layering: defaults, then file, then environment, then flags.

use std::io::Write;

use yamlfy_config::{Config, LogFormat, StaticEnvironment};
use yamlfy_syntax::{Code, Severity};

fn write_config(body: &str) -> tempdir::Temp {
    tempdir::Temp::new(body)
}

/// A minimal scratch file that removes itself.
mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct Temp(PathBuf);

    impl Temp {
        pub fn new(body: &str) -> Self {
            let mut path = std::env::temp_dir();
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            path.push(format!("yamlfy-config-{unique}-{:p}.toml", body.as_ptr()));
            std::fs::write(&path, body).expect("scratch file writable");
            Temp(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

#[test]
fn defaults_stand_when_nothing_overrides_them() {
    let env = StaticEnvironment::default();
    let config = Config::load(None, &env).expect("defaults load");

    assert_eq!(config.log.filter, "warn");
    assert!(config.log.directory.is_none());
    assert_eq!(config.diagnostics.max_recovery_attempts, 16);
    assert!(config.diagnostics.severities.is_empty());
}

#[test]
fn the_file_layer_is_applied() {
    let file = write_config(
        "[log]\nfilter = \"yamlfy_syntax=debug\"\ndirectory = \"/tmp/yamlfy-logs\"\n\
         format = \"json\"\n\n[diagnostics]\nmax_recovery_attempts = 3\n\
         severity = { W0300 = \"error\" }\n",
    );
    let env = StaticEnvironment::default();
    let config = Config::load(Some(file.path()), &env).expect("file loads");

    assert_eq!(config.log.filter, "yamlfy_syntax=debug");
    assert_eq!(config.log.format, LogFormat::Json);
    assert_eq!(config.diagnostics.max_recovery_attempts, 3);
    assert_eq!(
        yamlfy_config::effective_severity(&config, Code::AnchorShadowed),
        Severity::Error
    );
}

#[test]
fn the_environment_layer_overrides_the_file() {
    let file = write_config("[log]\nfilter = \"warn\"\n\n[diagnostics]\nmax_recovery_attempts = 3\n");
    let env = StaticEnvironment::new([
        ("YAMLFY_LOG", "trace"),
        ("YAMLFY_MAX_RECOVERY_ATTEMPTS", "9"),
        ("YAMLFY_SEVERITY_E0110", "allow"),
    ]);
    let config = Config::load(Some(file.path()), &env).expect("layers load");

    assert_eq!(config.log.filter, "trace");
    assert_eq!(config.diagnostics.max_recovery_attempts, 9);
    assert_eq!(yamlfy_config::effective_severity(&config, Code::DuplicateKey), Severity::Allow);
}

#[test]
fn flags_override_everything_else() {
    let env = StaticEnvironment::new([("YAMLFY_SEVERITY_W0300", "allow")]);
    let mut config = Config::load(None, &env).expect("env loads");
    assert_eq!(yamlfy_config::effective_severity(&config, Code::AnchorShadowed), Severity::Allow);

    config.set_severity("W0300", "deny").expect("flag applies");
    assert_eq!(yamlfy_config::effective_severity(&config, Code::AnchorShadowed), Severity::Error);
}

#[test]
fn the_parse_options_carry_the_resolved_configuration() {
    let env = StaticEnvironment::new([("YAMLFY_MAX_RECOVERY_ATTEMPTS", "2")]);
    let mut config = Config::load(None, &env).expect("env loads");
    config.set_severity("E0110", "warning").expect("flag applies");
    let options = config.parse_options();

    assert_eq!(options.max_recovery_attempts, 2);
    assert_eq!(options.severities.get(&Code::DuplicateKey), Some(&Severity::Warning));
}

#[test]
fn an_unknown_code_is_rejected_rather_than_ignored() {
    let env = StaticEnvironment::new([("YAMLFY_SEVERITY_E9999", "error")]);
    let error = Config::load(None, &env).expect_err("unknown code must fail");
    assert!(error.to_string().contains("unknown diagnostic code"), "{error}");
}

#[test]
fn an_unknown_severity_is_rejected() {
    let env = StaticEnvironment::default();
    let mut config = Config::load(None, &env).expect("defaults load");
    let error = config.set_severity("W0300", "loud").expect_err("unknown severity must fail");
    assert!(error.to_string().contains("allow, warning, error"), "{error}");
}

#[test]
fn an_unknown_configuration_key_is_rejected() {
    let file = write_config("[log]\nfilter = \"warn\"\ncolour = true\n");
    let env = StaticEnvironment::default();
    let error = Config::load(Some(file.path()), &env).expect_err("typo must fail");
    assert!(error.to_string().contains("cannot parse"), "{error}");
}

#[test]
fn a_bad_recovery_budget_is_rejected() {
    let env = StaticEnvironment::new([("YAMLFY_MAX_RECOVERY_ATTEMPTS", "lots")]);
    let error = Config::load(None, &env).expect_err("non-numeric must fail");
    assert!(error.to_string().contains("not a non-negative integer"), "{error}");
}

#[test]
fn a_missing_configuration_file_is_not_an_error() {
    let env = StaticEnvironment::default();
    let missing = std::path::Path::new("/nonexistent/yamlfy.toml");
    let config = Config::load(Some(missing), &env).expect("absent file is fine");
    assert_eq!(config.log.filter, "warn");
    let _ = std::io::stdout().flush();
}
