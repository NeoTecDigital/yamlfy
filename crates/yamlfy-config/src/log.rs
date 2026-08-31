// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Logging configuration.

use std::path::PathBuf;

use serde::Deserialize;

/// How log lines are formatted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable, one line per event.
    #[default]
    Compact,
    /// One JSON object per event.
    Json,
}

impl LogFormat {
    /// Parse a configuration value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "compact" | "text" => Some(LogFormat::Compact),
            "json" => Some(LogFormat::Json),
            _ => None,
        }
    }
}

/// Where and how much to log.
#[derive(Clone, Debug)]
pub struct LogConfig {
    /// A `tracing-subscriber` filter directive, for example `yamlfy=debug`.
    pub filter: String,
    /// Directory for the rolling log file. `None` logs to stderr only.
    pub directory: Option<PathBuf>,
    /// File name prefix inside [`LogConfig::directory`].
    pub file_prefix: String,
    /// Line format.
    pub format: LogFormat,
    /// Whether stderr output may use colour.
    pub ansi: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig {
            filter: "warn".to_owned(),
            directory: None,
            file_prefix: "yamlfy".to_owned(),
            format: LogFormat::Compact,
            ansi: true,
        }
    }
}

/// The on-disk logging section.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileLogConfig {
    #[serde(default)]
    pub(crate) filter: Option<String>,
    #[serde(default)]
    pub(crate) directory: Option<PathBuf>,
    #[serde(default)]
    pub(crate) file_prefix: Option<String>,
    #[serde(default)]
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) ansi: Option<bool>,
}
