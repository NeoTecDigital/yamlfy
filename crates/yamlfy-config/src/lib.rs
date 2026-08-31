// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! Layered configuration.
//!
//! Four layers, applied in order, each overriding the last:
//!
//! 1. compiled-in defaults,
//! 2. a `yamlfy.toml` file,
//! 3. `YAMLFY_*` environment variables,
//! 4. command-line flags, applied by the caller through [`Config`]'s fields.
//!
//! The file format is TOML rather than YAML on purpose: configuration is read
//! before the YAML front end exists, and a compiler that needs itself to start
//! is a bootstrapping problem, not a feature.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod env;
mod layers;
mod log;

pub use env::{Environment, ProcessEnvironment, StaticEnvironment};
pub use layers::effective_severity;
pub use log::{LogConfig, LogFormat};

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yamlfy_syntax::{Code, ParseOptions, Severity, SeverityMap};

/// Why a configuration could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The configuration file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The configuration file is not valid TOML, or has unknown keys.
    #[error("cannot parse {path}: {source}")]
    Parse {
        /// The file that could not be parsed.
        path: PathBuf,
        /// The underlying failure.
        source: toml::de::Error,
    },
    /// A value was syntactically fine but meaningless.
    #[error("{key}: {message}")]
    Value {
        /// The setting that was wrong.
        key: String,
        /// What was wrong with it.
        message: String,
    },
}

/// Diagnostic behaviour.
#[derive(Clone, Debug)]
pub struct DiagnosticsConfig {
    /// Per-code severity overrides, keyed by printed code such as `W0300`.
    pub severities: SeverityMap,
    /// How many times a file may be restarted after a syntax error.
    pub max_recovery_attempts: u32,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        DiagnosticsConfig { severities: SeverityMap::new(), max_recovery_attempts: 16 }
    }
}

/// The resolved configuration.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Logging.
    pub log: LogConfig,
    /// Diagnostics.
    pub diagnostics: DiagnosticsConfig,
}

impl Config {
    /// Defaults, then `path` if it exists, then the environment.
    ///
    /// # Errors
    /// Returns [`ConfigError`] if the file exists but cannot be read or parsed,
    /// or if any value is not meaningful.
    pub fn load(path: Option<&Path>, env: &dyn Environment) -> Result<Self, ConfigError> {
        let mut config = Config::default();
        if let Some(path) = path.filter(|p| p.exists()) {
            let raw = std::fs::read_to_string(path)
                .map_err(|source| ConfigError::Read { path: path.to_owned(), source })?;
            let file: FileConfig = toml::from_str(&raw)
                .map_err(|source| ConfigError::Parse { path: path.to_owned(), source })?;
            layers::apply_file(&mut config, file)?;
        }
        layers::apply_env(&mut config, env)?;
        Ok(config)
    }

    /// The parser options implied by this configuration.
    #[must_use]
    pub fn parse_options(&self) -> ParseOptions {
        ParseOptions {
            severities: self.diagnostics.severities.clone(),
            max_recovery_attempts: self.diagnostics.max_recovery_attempts,
        }
    }

    /// Override one diagnostic code's severity, as a `--deny`/`--allow` flag
    /// would.
    ///
    /// # Errors
    /// Returns [`ConfigError::Value`] if the code or severity is unknown.
    pub fn set_severity(&mut self, code: &str, severity: &str) -> Result<(), ConfigError> {
        let code = Code::parse(code).ok_or_else(|| ConfigError::Value {
            key: code.to_owned(),
            message: format!("unknown diagnostic code (known: {})", known_codes()),
        })?;
        let severity = Severity::parse(severity).ok_or_else(|| ConfigError::Value {
            key: code.as_str().to_owned(),
            message: "severity must be one of allow, warning, error".to_owned(),
        })?;
        self.diagnostics.severities.insert(code, severity);
        Ok(())
    }
}

/// Every diagnostic code, comma separated, for error messages.
#[must_use]
pub fn known_codes() -> String {
    Code::all().iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
}

/// The on-disk shape. Unknown keys are rejected so a typo is never silent.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileConfig {
    #[serde(default)]
    pub(crate) log: Option<log::FileLogConfig>,
    #[serde(default)]
    pub(crate) diagnostics: Option<FileDiagnosticsConfig>,
}

/// The on-disk diagnostics section.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileDiagnosticsConfig {
    #[serde(default)]
    pub(crate) max_recovery_attempts: Option<u32>,
    /// Code to severity, for example `W0300 = "error"`.
    #[serde(default)]
    pub(crate) severity: std::collections::BTreeMap<String, String>,
}
