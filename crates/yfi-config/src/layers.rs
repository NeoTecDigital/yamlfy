// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Applying the file and environment layers onto the defaults.

use yfi_syntax::{Code, Severity};

use crate::env::Environment;
use crate::log::LogFormat;
use crate::{Config, ConfigError, FileConfig};

/// Prefix every Yamlfication environment variable shares.
pub(crate) const ENV_PREFIX: &str = "YAMLFY_";
/// Prefix for per-code severity overrides, for example `YAMLFY_SEVERITY_W0300`.
pub(crate) const ENV_SEVERITY_PREFIX: &str = "SEVERITY_";

pub(crate) fn apply_file(config: &mut Config, file: FileConfig) -> Result<(), ConfigError> {
    if let Some(log) = file.log {
        apply_file_log(config, log)?;
    }
    let Some(diagnostics) = file.diagnostics else { return Ok(()) };
    if let Some(max) = diagnostics.max_recovery_attempts {
        config.diagnostics.max_recovery_attempts = max;
    }
    for (code, severity) in &diagnostics.severity {
        config.set_severity(code, severity)?;
    }
    Ok(())
}

fn apply_file_log(config: &mut Config, log: crate::log::FileLogConfig) -> Result<(), ConfigError> {
    if let Some(filter) = log.filter {
        config.log.filter = filter;
    }
    if let Some(directory) = log.directory {
        config.log.directory = Some(directory);
    }
    if let Some(prefix) = log.file_prefix {
        config.log.file_prefix = prefix;
    }
    if let Some(ansi) = log.ansi {
        config.log.ansi = ansi;
    }
    let Some(format) = log.format else { return Ok(()) };
    config.log.format = LogFormat::parse(&format).ok_or_else(|| ConfigError::Value {
        key: "log.format".to_owned(),
        message: "format must be one of compact, json".to_owned(),
    })?;
    Ok(())
}

pub(crate) fn apply_env(config: &mut Config, env: &dyn Environment) -> Result<(), ConfigError> {
    apply_env_log(config, env)?;
    if let Some(raw) = env.get("YAMLFY_MAX_RECOVERY_ATTEMPTS") {
        config.diagnostics.max_recovery_attempts =
            raw.trim().parse().map_err(|_| ConfigError::Value {
                key: "YAMLFY_MAX_RECOVERY_ATTEMPTS".to_owned(),
                message: format!("`{raw}` is not a non-negative integer"),
            })?;
    }
    for (suffix, value) in env.with_prefix(ENV_PREFIX) {
        let Some(code) = suffix.strip_prefix(ENV_SEVERITY_PREFIX) else { continue };
        config.set_severity(code, &value)?;
    }
    Ok(())
}

fn apply_env_log(config: &mut Config, env: &dyn Environment) -> Result<(), ConfigError> {
    if let Some(filter) = env.get("YAMLFY_LOG") {
        config.log.filter = filter;
    }
    if let Some(directory) = env.get("YAMLFY_LOG_DIR") {
        config.log.directory = Some(directory.into());
    }
    if let Some(prefix) = env.get("YAMLFY_LOG_PREFIX") {
        config.log.file_prefix = prefix;
    }
    if let Some(ansi) = env.get("YAMLFY_LOG_ANSI") {
        config.log.ansi = !matches!(ansi.trim(), "0" | "false" | "no");
    }
    let Some(format) = env.get("YAMLFY_LOG_FORMAT") else { return Ok(()) };
    config.log.format = LogFormat::parse(&format).ok_or_else(|| ConfigError::Value {
        key: "YAMLFY_LOG_FORMAT".to_owned(),
        message: "format must be one of compact, json".to_owned(),
    })?;
    Ok(())
}

/// Severity of `code` under `config`, for reporting what the layers resolved to.
#[must_use]
pub fn effective_severity(config: &Config, code: Code) -> Severity {
    config.diagnostics.severities.get(&code).copied().unwrap_or_else(|| code.default_severity())
}
