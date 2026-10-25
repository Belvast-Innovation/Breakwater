//! Runs a suite through `cargo test` and reads back its report.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context};
use breakwater::RunReport;

/// Run parameters passed to the suite through the environment.
pub struct RunParams<'a> {
    pub seed: Option<u64>,
    pub steps: Option<&'a str>,
    pub auth: Option<&'a str>,
    pub report: Option<&'a Path>,
}

/// Checks that `dir` is a crate with a `tests/suite.rs`.
pub fn validate_suite_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let manifest = dir.join("Cargo.toml");
    if !manifest.is_file() {
        bail!("{} is not a crate directory (no Cargo.toml)", dir.display());
    }
    if !dir.join("tests").join("suite.rs").is_file() {
        bail!("{} has no tests/suite.rs", dir.display());
    }
    Ok(manifest)
}

/// Default report location when `--json` is not given.
pub fn default_report_path(dir: &Path, seed: Option<u64>) -> PathBuf {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "suite".to_owned());
    let seed = seed
        .map(|s| s.to_string())
        .unwrap_or_else(|| "default".to_owned());
    std::env::temp_dir().join(format!("breakwater-{name}-{seed}.json"))
}

/// Runs `cargo test --test suite breakwater_suite` in `dir` with the
/// Breakwater environment and returns the report the suite wrote.
pub fn run(dir: &Path, params: &RunParams<'_>) -> anyhow::Result<(RunReport, PathBuf)> {
    let manifest = validate_suite_dir(dir)?;
    let report_path = params
        .report
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_report_path(dir, params.seed));
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(&report_path);

    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.arg("test")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--test")
        .arg("suite")
        .arg("breakwater_suite")
        .arg("--")
        .arg("--exact")
        .env("BREAKWATER_REPORT", &report_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(seed) = params.seed {
        cmd.env("BREAKWATER_SEED", seed.to_string());
    }
    if let Some(steps) = params.steps {
        cmd.env("BREAKWATER_STEPS", steps);
    }
    if let Some(auth) = params.auth {
        cmd.env("BREAKWATER_AUTH", auth);
    }
    let status = cmd.status().context("running cargo test")?;

    let report = match std::fs::read_to_string(&report_path) {
        Ok(json) => serde_json::from_str::<RunReport>(&json).context("parsing the suite report")?,
        Err(_) => {
            return Err(anyhow!(
                "the suite wrote no report ({}); cargo test exited with {status}",
                report_path.display()
            ))
        }
    };
    Ok((report, report_path))
}
