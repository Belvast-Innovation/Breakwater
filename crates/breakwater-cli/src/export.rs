//! Generates a self-contained regression test from a run report.

use anyhow::bail;
use breakwater::{ReplayStep, RunReport};

/// The Rust source of a `#[test]` replaying the report's shrunk sequence
/// through the suite's `common::replay_json`.
pub fn test_source(report: &RunReport) -> anyhow::Result<String> {
    let Some(failure) = report.failure() else {
        bail!("the run held every invariant; nothing to export");
    };
    let steps: Vec<ReplayStep> = ReplayStep::from_log(&failure.log);
    let json = serde_json::to_string_pretty(&steps)?;
    let ident = sanitize(&report.suite);
    let test_name = format!("repro_{ident}_{}", report.seed);
    let message = failure
        .violation
        .message
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    let mut src = String::new();
    src.push_str(&format!(
        "//! Reproduction exported by `breakwater export` for suite `{}` with seed {}.\n",
        report.suite, report.seed
    ));
    src.push_str("//! Replays the shrunk sequence through `common::replay_json` and asserts\n");
    src.push_str("//! that the same invariant fails at the same step.\n\n");
    src.push_str("mod common;\n\n");
    src.push_str("const STEPS: &str = r#\"");
    src.push_str(&json);
    src.push_str("\"#;\n\n");
    src.push_str("#[test]\n");
    src.push_str(&format!("fn {test_name}() {{\n"));
    src.push_str("    let report = common::replay_json(STEPS);\n");
    src.push_str("    let failure = report.failure().expect(\"the replayed sequence violates an invariant\");\n");
    src.push_str(&format!(
        "    assert_eq!(failure.violation.source, \"{}\");\n",
        failure.violation.source
    ));
    src.push_str(&format!(
        "    assert_eq!(failure.failing_step, {});\n",
        failure.failing_step
    ));
    src.push_str(&format!(
        "    assert_eq!(failure.violation.message, \"{message}\");\n"
    ));
    src.push_str("}\n");
    Ok(src)
}

/// File name for the exported test: `repro_<suite>_<seed>.rs`.
pub fn file_name(report: &RunReport) -> String {
    format!("repro_{}_{}.rs", sanitize(&report.suite), report.seed)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
