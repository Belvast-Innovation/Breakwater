//! `breakwater` command-line interface.
//!
//! Exit codes follow `docs/ARCHITECTURE.md` §6: `0` all invariants held,
//! `1` a violation was found, `2` usage or configuration error. `snapshot`
//! uses the same codes: `1` is a resource regression.
#![forbid(unsafe_code)]

mod driver;
mod export;
mod html;
mod render;
mod snapshot;
mod style;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use breakwater::RunReport;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::style::Style;

const EXIT_USAGE: u8 = 2;

/// Foundry-style invariant testing for Soroban.
#[derive(Debug, Parser)]
#[command(name = "breakwater", version, about)]
struct Cli {
    /// Disable colours (also honoured: NO_COLOR, non-terminal output).
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a suite and report the first invariant violation.
    Run(RunArgs),
    /// Export the shrunk failing sequence of a suite as a Rust regression test.
    Export(ExportArgs),
    /// Render a JSON run report in the terminal or as a static HTML page.
    Report(ReportArgs),
    /// Record per-handler resource usage, or check it against the recorded snapshot.
    Snapshot(snapshot::SnapshotArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Auth {
    Mock,
    Real,
}

impl Auth {
    fn as_str(self) -> &'static str {
        match self {
            Auth::Mock => "mock",
            Auth::Real => "real",
        }
    }
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Directory of the suite crate (one with a `tests/suite.rs`).
    suite_dir: PathBuf,
    /// Seed for deterministic sequence generation.
    #[arg(long)]
    seed: Option<u64>,
    /// Inclusive step range, e.g. `1..=20`.
    #[arg(long, value_parser = parse_steps)]
    steps: Option<String>,
    /// Authorization mode.
    #[arg(long, value_enum)]
    auth: Option<Auth>,
    /// Keep the JSON report at this path.
    #[arg(long)]
    json: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ExportArgs {
    /// Directory of the suite crate.
    suite_dir: PathBuf,
    /// Destination file for the generated `#[test]`.
    #[arg(long)]
    out: PathBuf,
    /// Seed of the run to export.
    #[arg(long)]
    seed: Option<u64>,
    /// Use an existing JSON report instead of running the suite.
    #[arg(long)]
    from: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReportArgs {
    /// JSON report produced by `breakwater run --json`.
    json: PathBuf,
    /// Also write a single-file static HTML page.
    #[arg(long)]
    html: Option<PathBuf>,
}

fn parse_steps(s: &str) -> Result<String, String> {
    breakwater::parse_steps(s)
        .map(|_| s.to_owned())
        .ok_or_else(|| format!("expected a range like 1..=20, got `{s}`"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::detect(cli.no_color);
    let result = match cli.command {
        Command::Run(args) => run(&style, args),
        Command::Export(args) => export(&style, args),
        Command::Report(args) => report(&style, args),
        Command::Snapshot(args) => snapshot::run(&style, args),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("{} {err:#}", style.red("error:"));
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run(style: &Style, args: RunArgs) -> anyhow::Result<u8> {
    let auth = args.auth.map(Auth::as_str);
    println!(
        "{} {}",
        style.bold("running"),
        style.dim(&format!(
            "cargo test --test suite breakwater_suite in {}",
            args.suite_dir.display()
        ))
    );
    let (report, path) = driver::run(
        &args.suite_dir,
        &driver::RunParams {
            seed: args.seed,
            steps: args.steps.as_deref(),
            auth,
            report: args.json.as_deref(),
        },
    )?;
    render::header(
        style,
        &report,
        &render::Header {
            auth: auth.unwrap_or("suite default"),
            steps: args.steps.as_deref().unwrap_or("suite default"),
        },
    );
    let code = render::result(style, &report, Some(&path));
    Ok(u8::try_from(code).unwrap_or(EXIT_USAGE))
}

fn export(style: &Style, args: ExportArgs) -> anyhow::Result<u8> {
    let report = match &args.from {
        Some(path) => read_report(path)?,
        None => {
            driver::run(
                &args.suite_dir,
                &driver::RunParams {
                    seed: args.seed,
                    steps: None,
                    auth: None,
                    report: None,
                },
            )?
            .0
        }
    };
    let source = export::test_source(&report)?;
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&args.out, source).with_context(|| format!("writing {}", args.out.display()))?;
    println!(
        "{} {} {}",
        style.green("exported"),
        args.out.display(),
        style.dim(&format!("(suggested name: {})", export::file_name(&report)))
    );
    Ok(0)
}

fn report(style: &Style, args: ReportArgs) -> anyhow::Result<u8> {
    let report = read_report(&args.json)?;
    if let Some(html_path) = &args.html {
        let page = html::render(&report);
        if let Some(parent) = html_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(html_path, page)
            .with_context(|| format!("writing {}", html_path.display()))?;
        println!("{} {}", style.green("wrote"), html_path.display());
    }
    render::header(
        style,
        &report,
        &render::Header {
            auth: "see report",
            steps: "see report",
        },
    );
    let code = render::result(style, &report, Some(&args.json));
    Ok(u8::try_from(code).unwrap_or(EXIT_USAGE))
}

fn read_report(path: &std::path::Path) -> anyhow::Result<RunReport> {
    let json =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("parsing {}", path.display()))
}
