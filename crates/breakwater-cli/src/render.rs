//! Terminal rendering of a run report.

use std::path::Path;

use breakwater::{Outcome, RunReport, StepOutcome, StepRecord, LEDGERS_PER_DAY};

use crate::style::Style;

/// Run parameters that are not part of the report itself.
pub struct Header<'a> {
    /// `mock` or `real`.
    pub auth: &'a str,
    /// Steps range as given, e.g. `1..=10`.
    pub steps: &'a str,
}

/// Renders the one-line argument summary `k=v k2=v2`.
pub fn args_summary(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let mut parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{k}={}", scalar(v)))
                .collect();
            parts.sort();
            parts.join(" ")
        }
        serde_json::Value::Null => String::new(),
        other => scalar(other),
    }
}

fn scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn gap_text(ledgers: u32) -> String {
    if ledgers >= LEDGERS_PER_DAY {
        format!(
            "+{ledgers} ledgers ({:.1} d)",
            f64::from(ledgers) / f64::from(LEDGERS_PER_DAY)
        )
    } else {
        format!("+{ledgers} ledgers")
    }
}

fn outcome_text(style: &Style, outcome: &StepOutcome) -> String {
    match outcome {
        StepOutcome::Success => style.green("Success"),
        StepOutcome::ContractError(code) => style.yellow(&format!("ContractError(#{code})")),
        StepOutcome::Unauthorized => style.yellow("Unauthorized"),
        StepOutcome::HandlerError(m) => style.red(&format!("HandlerError({m})")),
        StepOutcome::Panic(m) => style.red(&format!("Panic({m})")),
    }
}

/// One step line: `#0 transfer      agent   +1 ledgers    amount_usdc=49 -> Success`.
pub fn step_line(style: &Style, step: &StepRecord) -> String {
    format!(
        "  {} {:<14} {:<10} {:<24} {} -> {}",
        style.dim(&format!("#{}", step.index)),
        step.handler,
        step.actor.name(),
        gap_text(step.gap.ledgers),
        args_summary(&step.args),
        outcome_text(style, &step.outcome)
    )
}

/// Prints the header block.
pub fn header(style: &Style, report: &RunReport, header: &Header<'_>) {
    println!("{} {}", style.bold("suite:"), style.cyan(&report.suite));
    println!(
        "{} {}   {} {}   {} {}",
        style.bold("seed:"),
        report.seed,
        style.bold("auth:"),
        header.auth,
        style.bold("steps:"),
        header.steps
    );
}

/// Prints the result block. Returns the process exit code (0 held, 1 violated).
pub fn result(style: &Style, report: &RunReport, report_path: Option<&Path>) -> i32 {
    match &report.outcome {
        Outcome::Held => {
            println!(
                "{}",
                style.green(&format!(
                    "all invariants held over {} cases and {} steps",
                    report.cases_run, report.steps_run
                ))
            );
            if let Some(p) = report_path {
                println!("{}", style.dim(&format!("report: {}", p.display())));
            }
            0
        }
        Outcome::Violated(failure) => {
            println!("{}", style.dim(&"─".repeat(72)));
            println!(
                "{} after {} cases and {} steps, shrunk to {} step(s):",
                style.red(&style.bold("violation found")),
                report.cases_run,
                report.steps_run,
                failure.log.len()
            );
            for step in failure.log.steps() {
                println!("{}", step_line(style, step));
            }
            println!(
                "{} {} {}",
                style.red("violation:"),
                failure.violation.message,
                style.dim(&format!("({})", failure.violation.source))
            );
            match report_path {
                Some(p) => println!(
                    "{}",
                    style.dim(&format!(
                        "reproduce with --seed {}; report: {}",
                        report.seed,
                        p.display()
                    ))
                ),
                None => println!(
                    "{}",
                    style.dim(&format!("reproduce with --seed {}", report.seed))
                ),
            }
            1
        }
    }
}
