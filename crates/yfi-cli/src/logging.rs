// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-blocking logging setup.
//!
//! The returned guard must be kept alive for the process's lifetime; dropping
//! it flushes the background writer.

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::EnvFilter;
use yfi_config::{LogConfig, LogFormat};

/// Keeps the non-blocking writer's worker thread alive.
pub struct LogGuard(Option<WorkerGuard>);

impl LogGuard {
    /// Whether logs are going to a file through the non-blocking writer.
    #[must_use]
    pub fn is_file_backed(&self) -> bool {
        self.0.is_some()
    }
}

/// Install the global subscriber described by `config`.
///
/// Returns the guard, and an error string if the filter directive was rejected;
/// logging still starts in that case, at `warn`.
pub fn init(config: &LogConfig) -> (LogGuard, Option<String>) {
    let (filter, error) = match EnvFilter::try_new(&config.filter) {
        Ok(filter) => (filter, None),
        Err(e) => (
            EnvFilter::new("warn"),
            Some(format!("invalid log filter `{}`: {e}; using `warn`", config.filter)),
        ),
    };
    let (writer, guard) = make_writer(config);
    install(config, filter, writer);
    (LogGuard(guard), error)
}

fn make_writer(config: &LogConfig) -> (BoxMakeWriter, Option<WorkerGuard>) {
    let Some(directory) = config.directory.as_ref() else {
        return (BoxMakeWriter::new(std::io::stderr), None);
    };
    if let Err(e) = std::fs::create_dir_all(directory) {
        eprintln!("yamlfy: cannot create log directory {}: {e}", directory.display());
        return (BoxMakeWriter::new(std::io::stderr), None);
    }
    let appender = tracing_appender::rolling::daily(directory, &config.file_prefix);
    let (writer, guard) = tracing_appender::non_blocking(appender);
    (BoxMakeWriter::new(writer), Some(guard))
}

fn install(config: &LogConfig, filter: EnvFilter, writer: BoxMakeWriter) {
    let ansi = config.ansi && config.directory.is_none();
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_writer(writer);
    let result = match config.format {
        LogFormat::Compact => builder.with_ansi(ansi).compact().try_init(),
        LogFormat::Json => builder.with_ansi(false).json().try_init(),
    };
    if let Err(e) = result {
        eprintln!("yamlfy: logging already initialised: {e}");
    }
}
