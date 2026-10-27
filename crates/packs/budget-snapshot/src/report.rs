//! What a measuring suite hands to the `breakwater` CLI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::snapshot::SuiteSnapshot;

/// Environment variable the CLI sets to the path the suite writes its
/// [`BudgetReport`] to, the way `BREAKWATER_REPORT` carries a `RunReport`.
pub const REPORT_ENV: &str = "BREAKWATER_SNAPSHOT_OUT";

/// A finished measurement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetReport {
    /// Suite name, the key in the snapshot file.
    pub suite: String,
    /// The measurement in the shape the snapshot file stores.
    pub snapshot: SuiteSnapshot,
    /// Calls that did not succeed, per handler. They are not measured.
    pub skipped: BTreeMap<String, u64>,
}

/// Why a measurement could not be turned into a [`BudgetReport`].
#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    /// The suite found a violation, so some sequences stopped early and the
    /// measurement would be incomplete.
    #[error(
        "the suite ended on a violation of `{source_name}`: {message}; \
         a measurement suite must not fail, use `breakwater run` to investigate"
    )]
    Violated {
        /// Invariant or handler that fired.
        source_name: String,
        /// Its message.
        message: String,
    },
    /// The last top-level invocation of a handler was not the declared target.
    #[error(
        "handler(s) {handlers} made another top-level call after the measured one, or called \
         a different function than declared; the measured call must be the handler's last"
    )]
    Unattributed {
        /// Names with the number of affected calls, like `withdraw (3)`.
        handlers: String,
    },
    /// Nothing was measured.
    #[error("no successful call of a measured handler was recorded")]
    NoSamples,
    /// Reading or writing the report failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The report is not valid JSON of the expected shape.
    #[error("invalid measurement report: {0}")]
    Json(#[from] serde_json::Error),
}

impl BudgetReport {
    /// Pretty JSON with a trailing newline.
    pub fn to_json_string(&self) -> Result<String, BudgetError> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }

    /// Writes the report, creating parent directories.
    pub fn write(&self, path: &Path) -> Result<(), BudgetError> {
        let io = |source| BudgetError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(path, self.to_json_string()?).map_err(io)
    }

    /// Writes the report to the path in `BREAKWATER_SNAPSHOT_OUT` if that
    /// variable is set; this is how a suite hands its result to the CLI.
    /// Returns the path written, or `None` when the variable is unset.
    pub fn write_if_requested(&self) -> Result<Option<PathBuf>, BudgetError> {
        match std::env::var_os(REPORT_ENV) {
            Some(path) if !path.is_empty() => {
                let path = PathBuf::from(path);
                self.write(&path)?;
                Ok(Some(path))
            }
            _ => Ok(None),
        }
    }

    /// Reads a report written by [`BudgetReport::write`].
    pub fn read(path: &Path) -> Result<BudgetReport, BudgetError> {
        let text = std::fs::read_to_string(path).map_err(|source| BudgetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(serde_json::from_str(&text)?)
    }
}
