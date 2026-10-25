//! Export of run reports.
//!
//! This version writes a JSON step list. Generation of a Rust `#[test]` that
//! replays the sequence lands with the CLI.

use std::path::Path;

use crate::error::SuiteError;
use crate::suite::RunReport;

/// Serialises [`RunReport`]s.
pub struct Exporter;

impl Exporter {
    /// The report as a JSON value. Stable field names: `suite`, `seed`,
    /// `cases_run`, `outcome.status`, and for violations
    /// `outcome.violation`, `outcome.failing_step`, `outcome.log.steps[]`.
    pub fn to_json(report: &RunReport) -> Result<serde_json::Value, SuiteError> {
        Ok(serde_json::to_value(report)?)
    }

    /// Pretty-printed JSON.
    pub fn to_json_string(report: &RunReport) -> Result<String, SuiteError> {
        Ok(serde_json::to_string_pretty(report)?)
    }

    /// Writes the report to the path in `BREAKWATER_REPORT`, if that variable
    /// is set. This is how a suite hands its result to the `breakwater` CLI.
    /// Returns the path written, or `None` when the variable is unset.
    pub fn write_if_requested(
        report: &RunReport,
    ) -> Result<Option<std::path::PathBuf>, SuiteError> {
        match std::env::var_os("BREAKWATER_REPORT") {
            Some(path) if !path.is_empty() => {
                let path = std::path::PathBuf::from(path);
                Self::write_json(report, &path)?;
                Ok(Some(path))
            }
            _ => Ok(None),
        }
    }

    /// Writes pretty-printed JSON to `path`, creating parent directories.
    pub fn write_json(report: &RunReport, path: impl AsRef<Path>) -> Result<(), SuiteError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, Self::to_json_string(report)?)?;
        Ok(())
    }
}
