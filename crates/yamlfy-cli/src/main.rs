// Written by Richard Christopher, Copyright 2026 Richard Christopher

//! `yamlfy` — the Yamlfication command line.
//!
//! Phase 1 step 2 ships one subcommand, `check`, which parses files and prints
//! `file:line:col` diagnostics. Later passes add their own subcommands; nothing
//! here is stubbed for them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod check;
mod logging;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing::debug;
use yamlfy_config::{Config, ProcessEnvironment};

/// Command-line surface.
#[derive(Parser)]
#[command(name = "yamlfy", version, about = "Yamlfication compiler")]
struct Cli {
    /// Configuration file. Defaults to `./yamlfy.toml` when present.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Log filter directive, for example `yamlfy_syntax=debug`.
    #[arg(long, global = true, value_name = "DIRECTIVE")]
    log: Option<String>,

    /// Raise a diagnostic code to an error, as `--deny W0300`.
    #[arg(long, global = true, value_name = "CODE")]
    deny: Vec<String>,

    /// Silence a diagnostic code, as `--allow W0300`.
    #[arg(long, global = true, value_name = "CODE")]
    allow: Vec<String>,

    #[command(subcommand)]
    command: Command,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Command {
    /// Parse files and report diagnostics.
    Check {
        /// Files to parse.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,

        /// Also print the arena node table for each file.
        #[arg(long)]
        dump: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match build_config(&cli) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("yamlfy: {message}");
            return ExitCode::from(2);
        }
    };
    let (guard, warning) = logging::init(&config.log);
    if let Some(warning) = warning {
        eprintln!("yamlfy: {warning}");
    }
    debug!(file_logging = guard.is_file_backed(), "logging ready");

    match &cli.command {
        Command::Check { files, dump } => check::run(&config, files, *dump),
    }
}

fn build_config(cli: &Cli) -> Result<Config, String> {
    let default_path = PathBuf::from("yamlfy.toml");
    let path = cli.config.clone().unwrap_or(default_path);
    let mut config =
        Config::load(Some(path.as_path()), &ProcessEnvironment).map_err(|e| e.to_string())?;
    if let Some(filter) = &cli.log {
        config.log.filter = filter.clone();
    }
    for code in &cli.deny {
        config.set_severity(code, "error").map_err(|e| e.to_string())?;
    }
    for code in &cli.allow {
        config.set_severity(code, "allow").map_err(|e| e.to_string())?;
    }
    Ok(config)
}
