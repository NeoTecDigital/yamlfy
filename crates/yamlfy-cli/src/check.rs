// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `yamlfy check` subcommand.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use tracing::info;
use yamlfy_config::Config;
use yamlfy_syntax::{parse_file, Severity, SourceMap};

/// Parse every file and print diagnostics. Exit code is 0 when no error-level
/// diagnostic was raised, 1 otherwise.
pub fn run(config: &Config, files: &[impl AsRef<Path>], dump: bool) -> ExitCode {
    let options = config.parse_options();
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut out = std::io::stdout().lock();
    for path in files {
        let mut sources = SourceMap::new();
        let parsed = parse_file(&mut sources, path.as_ref(), &options);
        info!(path = %path.as_ref().display(), nodes = parsed.ast.nodes().len(), "parsed");
        let _ = write!(out, "{}", parsed.diagnostics.render(&sources));
        if dump {
            let _ = write!(out, "{}", parsed.ast.dump());
        }
        errors += parsed.diagnostics.error_count();
        warnings += count(&parsed, Severity::Warning);
    }
    let _ = writeln!(out, "{errors} error(s), {warnings} warning(s)");
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn count(parsed: &yamlfy_syntax::Parsed, severity: Severity) -> usize {
    parsed.diagnostics.items().iter().filter(|d| d.severity == severity).count()
}
