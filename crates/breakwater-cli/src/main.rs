//! `breakwater` command-line interface.
//!
//! Exit codes follow `docs/ARCHITECTURE.md` §6: `0` all invariants held,
//! `1` a violation was found, `2` usage or configuration error.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

/// Exit code for usage or configuration errors.
const EXIT_USAGE: u8 = 2;

/// Foundry-style invariant testing for Soroban.
#[derive(Debug, Parser)]
#[command(name = "breakwater", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a suite and report the first invariant violation.
    Run(RunArgs),
    /// Export the shrunk failing sequence of a suite as a Rust unit test.
    Export(ExportArgs),
    /// Print a human-readable summary of a JSON run report.
    Report(ReportArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Directory containing the suite (a crate with a `tests/suite.rs`).
    suite_dir: PathBuf,
    /// Seed for deterministic sequence generation.
    #[arg(long)]
    seed: Option<u64>,
    /// Inclusive step range, e.g. `1..=20`.
    #[arg(long)]
    steps: Option<String>,
    /// Authorization mode: `mock` or `real`.
    #[arg(long, default_value = "mock")]
    auth: String,
    /// Emit a machine-readable JSON report instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ExportArgs {
    /// Directory containing the suite.
    suite_dir: PathBuf,
    /// Destination file for the generated `#[test]`.
    #[arg(long)]
    out: PathBuf,
    /// Seed of the failing run to export.
    #[arg(long)]
    seed: Option<u64>,
}

#[derive(Debug, Args)]
struct ReportArgs {
    /// JSON report produced by `breakwater run --json`.
    json: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let name = match cli.command {
        Command::Run(_) => "run",
        Command::Export(_) => "export",
        Command::Report(_) => "report",
    };
    eprintln!("error: `breakwater {name}` is not implemented yet");
    ExitCode::from(EXIT_USAGE)
}
