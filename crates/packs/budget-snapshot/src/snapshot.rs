//! The `.breakwater-snapshot.json` file.
//!
//! One file holds the statistics of any number of suites. It records the
//! toolchain it was taken with, because the metering model lives in
//! `soroban-env-host` and a patch release of it can change every number, and
//! per suite everything that identifies the workload: seed, cases, step
//! range, authorization mode, metering mode and a digest of the generated
//! steps. Only integers are stored and every map is ordered, so the same
//! measurement is the same bytes on any machine.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::resource::Resource;
use crate::stats::HandlerStats;

/// The file layout version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// Default file name, looked up in the current directory.
pub const SNAPSHOT_FILE: &str = ".breakwater-snapshot.json";

/// How the numbers were produced.
///
/// `Native` registers the contract as Rust code in the test host: storage,
/// event and rent figures are exact, while `instructions` and `mem_bytes`
/// leave out Wasm instantiation and VM execution. Other modes (a Wasm build
/// of the contract) can be added without a new file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Contracts run natively in the test host.
    Native,
}

impl Mode {
    /// The value stored in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Native => "native",
        }
    }
}

/// The crate versions the numbers depend on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    /// Resolved `soroban-sdk` version.
    pub soroban_sdk: String,
    /// Resolved `soroban-env-host` version, which holds the cost model.
    pub soroban_env_host: String,
    /// Resolved version of the Breakwater core library.
    pub breakwater: String,
}

/// Everything a snapshot keeps about one suite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteSnapshot {
    /// Generation seed.
    pub seed: u64,
    /// Sequences generated.
    pub cases: u32,
    /// Step range per sequence, `A..=B`.
    pub steps: String,
    /// Authorization mode, `mock` or `real`.
    pub auth: String,
    /// Metering mode.
    pub mode: Mode,
    /// Digest of the executed steps, `fnv1a64:<hex>`.
    pub workload_digest: String,
    /// Statistics per handler name.
    pub handlers: BTreeMap<String, HandlerStats>,
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// File layout version, [`FORMAT_VERSION`].
    pub format: u32,
    /// Versions the numbers were taken with.
    pub toolchain: Toolchain,
    /// Suites by name.
    pub suites: BTreeMap<String, SuiteSnapshot>,
}

/// Why a snapshot file could not be read, written or updated.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// The file does not exist.
    #[error("snapshot file {} does not exist", .0.display())]
    Missing(PathBuf),
    /// Reading or writing failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The content is not a valid snapshot.
    #[error("not a valid snapshot: {0}")]
    Malformed(String),
    /// The file uses a layout this crate does not know.
    #[error("unsupported snapshot format {found}, this version reads format {FORMAT_VERSION}")]
    UnsupportedFormat {
        /// The `format` value found.
        found: u64,
    },
    /// Updating one suite would leave the others labelled with the wrong toolchain.
    #[error(
        "the snapshot was taken with soroban-sdk {} / soroban-env-host {}, now {} / {}; \
         regenerate every suite in it, or use another --file",
        existing.soroban_sdk, existing.soroban_env_host, current.soroban_sdk, current.soroban_env_host
    )]
    ToolchainConflict {
        /// Toolchain recorded in the file.
        existing: Box<Toolchain>,
        /// Toolchain of this run.
        current: Box<Toolchain>,
    },
}

impl Snapshot {
    /// An empty snapshot for `toolchain`.
    pub fn new(toolchain: Toolchain) -> Snapshot {
        Snapshot {
            format: FORMAT_VERSION,
            toolchain,
            suites: BTreeMap::new(),
        }
    }

    /// Parses and validates the text of a snapshot file.
    pub fn parse(text: &str) -> Result<Snapshot, SnapshotError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| SnapshotError::Malformed(e.to_string()))?;
        let format = value
            .get("format")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| SnapshotError::Malformed("missing `format`".into()))?;
        if format != u64::from(FORMAT_VERSION) {
            return Err(SnapshotError::UnsupportedFormat { found: format });
        }
        let snapshot: Snapshot =
            serde_json::from_value(value).map_err(|e| SnapshotError::Malformed(e.to_string()))?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Reads and validates the file at `path`.
    pub fn read(path: &Path) -> Result<Snapshot, SnapshotError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(SnapshotError::Missing(path.to_path_buf()))
            }
            Err(source) => {
                return Err(SnapshotError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        Snapshot::parse(&text)
    }

    /// Pretty JSON with a trailing newline. Equal snapshots give equal bytes.
    pub fn to_json_string(&self) -> Result<String, SnapshotError> {
        let mut text = serde_json::to_string_pretty(self)
            .map_err(|e| SnapshotError::Malformed(e.to_string()))?;
        text.push('\n');
        Ok(text)
    }

    /// Writes the file, creating parent directories.
    pub fn write(&self, path: &Path) -> Result<(), SnapshotError> {
        let io = |source| SnapshotError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(path, self.to_json_string()?).map_err(io)
    }

    /// Inserts or replaces suite `name`, leaving the other suites alone.
    ///
    /// Fails when other suites are present and were taken with a different
    /// toolchain: the file has one toolchain, so keeping them would label old
    /// numbers with new versions.
    pub fn upsert(
        &mut self,
        name: &str,
        toolchain: Toolchain,
        suite: SuiteSnapshot,
    ) -> Result<(), SnapshotError> {
        let has_others = self.suites.keys().any(|k| k != name);
        if has_others && self.toolchain != toolchain {
            return Err(SnapshotError::ToolchainConflict {
                existing: Box::new(self.toolchain.clone()),
                current: Box::new(toolchain),
            });
        }
        self.toolchain = toolchain;
        self.suites.insert(name.to_owned(), suite);
        Ok(())
    }

    fn validate(&self) -> Result<(), SnapshotError> {
        let bad = |msg: String| Err(SnapshotError::Malformed(msg));
        for (name, suite) in &self.suites {
            if breakwater::parse_steps(&suite.steps).is_none() {
                return bad(format!(
                    "suite `{name}`: `steps` is not a range like 1..=20"
                ));
            }
            if suite.auth != "mock" && suite.auth != "real" {
                return bad(format!("suite `{name}`: `auth` must be `mock` or `real`"));
            }
            if !suite.workload_digest.starts_with("fnv1a64:") {
                return bad(format!("suite `{name}`: unknown `workload_digest` format"));
            }
            for (handler, stats) in &suite.handlers {
                if stats.samples == 0 {
                    return bad(format!("suite `{name}`, handler `{handler}`: no samples"));
                }
                let known = Resource::ALL
                    .iter()
                    .all(|r| stats.resources.contains_key(r.name()));
                if !known || stats.resources.len() != Resource::ALL.len() {
                    return bad(format!(
                        "suite `{name}`, handler `{handler}`: expected exactly the {} resources",
                        Resource::ALL.len()
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::Sample;

    pub(crate) fn toolchain() -> Toolchain {
        Toolchain {
            soroban_sdk: "28.0.0".into(),
            soroban_env_host: "28.0.2".into(),
            breakwater: "0.1.0".into(),
        }
    }

    pub(crate) fn suite(instructions: i64) -> SuiteSnapshot {
        let sample = Sample {
            instructions,
            write_entries: 2,
            ..Sample::default()
        };
        let mut handlers = BTreeMap::new();
        handlers.insert(
            "withdraw".to_owned(),
            HandlerStats::from_samples(&[sample]).expect("one sample"),
        );
        SuiteSnapshot {
            seed: 7,
            cases: 16,
            steps: "1..=20".into(),
            auth: "mock".into(),
            mode: Mode::Native,
            workload_digest: "fnv1a64:0123456789abcdef".into(),
            handlers,
        }
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new(toolchain());
        s.upsert("beta", toolchain(), suite(200)).expect("insert");
        s.upsert("alpha", toolchain(), suite(100)).expect("insert");
        s
    }

    #[test]
    fn round_trips_and_is_byte_stable() {
        let s = snapshot();
        let text = s.to_json_string().expect("serialises");
        assert!(text.ends_with("}\n"));
        assert_eq!(Snapshot::parse(&text).expect("parses"), s);
        assert_eq!(text, snapshot().to_json_string().expect("serialises"));
        // Suites are listed in key order, not insertion order.
        assert!(text.find("\"alpha\"").unwrap() < text.find("\"beta\"").unwrap());
        assert!(text.contains("\"mode\": \"native\""));
    }

    #[test]
    fn upsert_replaces_one_suite_and_keeps_the_others() {
        let mut s = snapshot();
        s.upsert("alpha", toolchain(), suite(999)).expect("update");
        assert_eq!(s.suites.len(), 2);
        assert_eq!(s.suites["alpha"], suite(999));
        assert_eq!(s.suites["beta"], suite(200));
    }

    #[test]
    fn upsert_refuses_to_relabel_other_suites_with_a_new_toolchain() {
        let mut s = snapshot();
        let newer = Toolchain {
            soroban_env_host: "28.0.3".into(),
            ..toolchain()
        };
        let err = s.upsert("alpha", newer.clone(), suite(1)).unwrap_err();
        assert!(matches!(err, SnapshotError::ToolchainConflict { .. }));
        // Replacing the only suite is fine: nothing else is mislabelled.
        let mut single = Snapshot::new(toolchain());
        single
            .upsert("alpha", toolchain(), suite(1))
            .expect("insert");
        single
            .upsert("alpha", newer.clone(), suite(2))
            .expect("sole suite");
        assert_eq!(single.toolchain, newer);
    }

    #[test]
    fn rejects_damaged_files() {
        let good = snapshot().to_json_string().unwrap();
        assert!(matches!(
            Snapshot::parse("{ not json"),
            Err(SnapshotError::Malformed(_))
        ));
        assert!(matches!(
            Snapshot::parse("{}"),
            Err(SnapshotError::Malformed(_))
        ));
        assert!(matches!(
            Snapshot::parse(&good.replace("\"format\": 1", "\"format\": 2")),
            Err(SnapshotError::UnsupportedFormat { found: 2 })
        ));
        for (from, to) in [
            ("\"steps\": \"1..=20\"", "\"steps\": \"lots\""),
            ("\"auth\": \"mock\"", "\"auth\": \"none\""),
            ("\"mode\": \"native\"", "\"mode\": \"wasm\""),
            ("fnv1a64:", "sha256:"),
            ("\"write_entries\"", "\"write_entry\""),
            ("\"samples\": 1", "\"samples\": 0"),
            ("\"seed\": 7", "\"seed\": 7,\n \"extra\": 1"),
        ] {
            assert!(
                from.len() > 1 && good.contains(from),
                "fixture lacks {from}"
            );
            assert!(
                matches!(
                    Snapshot::parse(&good.replace(from, to)),
                    Err(SnapshotError::Malformed(_))
                ),
                "{from} -> {to} was accepted"
            );
        }
    }

    #[test]
    fn read_and_write_use_the_file_system() {
        let dir = std::env::temp_dir().join(format!("breakwater-snapshot-{}", std::process::id()));
        let path = dir.join("nested").join(SNAPSHOT_FILE);
        assert!(matches!(
            Snapshot::read(&path),
            Err(SnapshotError::Missing(_))
        ));
        let s = snapshot();
        s.write(&path).expect("writes");
        assert_eq!(Snapshot::read(&path).expect("reads"), s);
        std::fs::write(&path, "garbage").expect("overwrite");
        assert!(matches!(
            Snapshot::read(&path),
            Err(SnapshotError::Malformed(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
