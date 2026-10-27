//! `breakwater snapshot --check` on the real `examples/ttl-archival` demo, with
//! the committed baseline, under the single tolerance CI used to run with and
//! under per-class tolerances.
//!
//! The suite is built with each of the demo's regression features in turn, so
//! these tests start `cargo test` in the example (the same thing the CLI does
//! for any suite). They are the end-to-end proof that:
//!
//! - the demo as it stands passes every gate;
//! - `regress-demo`, which writes one more entry, fails every gate, and under
//!   the tiered gate only the handler that changed is reported, because the
//!   side effects on the other handlers are inside the compute tolerance;
//! - `regress-compute`, which only costs CPU, moves no storage figure: a gate
//!   that looks at storage alone passes it, and the tiered gate fails it, in
//!   the compute class only.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use breakwater_pack_budget_snapshot::{Resource, ResourceClass};

/// The CI setting.
const TIERED: &str = "storage=0%,compute=5%";
/// Storage at 0% and compute effectively not checked: what a gate that looks
/// at the storage figures alone would say.
const STORAGE_ONLY: &str = "storage=0%,compute=100000%";

/// Each run starts cargo in the same example crate; one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Checked {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct Row {
    handler: String,
    resource: Resource,
    limit: String,
}

impl Checked {
    fn rows(&self) -> Vec<Row> {
        self.stdout
            .lines()
            .filter(|line| line.contains("REGRESSION"))
            .map(|line| {
                let words: Vec<&str> = line.split_whitespace().collect();
                Row {
                    handler: words[0].to_owned(),
                    resource: Resource::from_name(words[1])
                        .unwrap_or_else(|| panic!("unknown resource in `{line}`")),
                    limit: line[line.find("limit ").expect("a limit")..].to_owned(),
                }
            })
            .collect()
    }

    fn in_class(&self, class: ResourceClass) -> Vec<Row> {
        self.rows()
            .into_iter()
            .filter(|r| r.resource.class() == class)
            .collect()
    }

    fn handlers(&self) -> Vec<String> {
        let mut handlers: Vec<String> = self.rows().into_iter().map(|r| r.handler).collect();
        handlers.sort();
        handlers.dedup();
        handlers
    }

    fn explain(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn check(features: Option<&str>, tolerance: &str) -> Checked {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = Command::new(env!("CARGO_BIN_EXE_breakwater"));
    command
        .current_dir(&root)
        .args(["--no-color", "snapshot", "examples/ttl-archival", "--check"])
        .args(["--tolerance", tolerance]);
    if let Some(features) = features {
        command.args(["--features", features]);
    }
    let output = command.output().expect("runs the breakwater binary");
    Checked {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn the_demo_as_it_stands_passes_the_single_and_the_tiered_gate() {
    for tolerance in ["0%", TIERED, STORAGE_ONLY] {
        let run = check(None, tolerance);
        assert_eq!(run.code, Some(0), "{tolerance}\n{}", run.explain());
        assert!(run.stdout.contains("all handlers are within the tolerance"));
    }
    // The header says what was applied. A single value reads as before.
    assert!(check(None, "0%").stdout.contains("tolerance: 0%\n"));
    assert!(check(None, TIERED)
        .stdout
        .contains("tolerance: storage 0%, compute 5%\n"));
}

#[test]
fn regress_demo_fails_both_gates_and_the_tiered_one_names_only_what_changed() {
    let features = Some("regress-demo");

    // Single 0%: the change, and the side effects on handlers nobody touched.
    let single = check(features, "0%");
    assert_eq!(single.code, Some(1), "{}", single.explain());
    assert_eq!(single.handlers(), ["cancel", "create_stream", "withdraw"]);
    assert!(single.rows().iter().all(|r| r.limit == "limit 0% (all)"));
    assert!(single.stdout.contains("(storage "), "{}", single.stdout);

    // Tiered: only `withdraw`, in both classes, each with its own limit.
    let tiered = check(features, TIERED);
    assert_eq!(tiered.code, Some(1), "{}", tiered.explain());
    assert_eq!(tiered.handlers(), ["withdraw"], "{}", tiered.explain());
    let storage = tiered.in_class(ResourceClass::Storage);
    let compute = tiered.in_class(ResourceClass::Compute);
    for resource in [
        Resource::WriteEntries,
        Resource::WriteBytes,
        Resource::PersistentEntryRentBumps,
    ] {
        assert!(
            storage.iter().any(|r| r.resource == resource),
            "{} is missing:\n{}",
            resource.name(),
            tiered.stdout
        );
    }
    assert!(storage.iter().all(|r| r.limit == "limit 0% (storage)"));
    assert!(!compute.is_empty(), "{}", tiered.stdout);
    assert!(compute.iter().all(|r| r.limit == "limit 5% (compute)"));
    assert!(
        tiered.stdout.contains(&format!(
            "{} value(s) over the tolerance (storage {}, compute {})",
            storage.len() + compute.len(),
            storage.len(),
            compute.len()
        )),
        "{}",
        tiered.stdout
    );

    // The storage tolerance alone is enough to catch it.
    let only = check(features, STORAGE_ONLY);
    assert_eq!(only.code, Some(1), "{}", only.explain());
    assert!(only.in_class(ResourceClass::Compute).is_empty());
    assert!(!only.in_class(ResourceClass::Storage).is_empty());
}

#[test]
fn regress_compute_is_missed_by_a_storage_only_gate_and_caught_by_the_tiered_one() {
    let features = Some("regress-compute");

    // Nothing in storage moved, so a gate on storage figures cannot see it.
    let blind = check(features, STORAGE_ONLY);
    assert_eq!(blind.code, Some(0), "{}", blind.explain());
    assert!(blind
        .stdout
        .contains("all handlers are within the tolerance"));

    // The tiered gate fails, and not because of storage.
    let tiered = check(features, TIERED);
    assert_eq!(tiered.code, Some(1), "{}", tiered.explain());
    assert!(
        tiered.in_class(ResourceClass::Storage).is_empty(),
        "{}",
        tiered.stdout
    );
    assert_eq!(tiered.handlers(), ["cancel"], "{}", tiered.stdout);
    let compute = tiered.in_class(ResourceClass::Compute);
    assert!(compute.iter().any(|r| r.resource == Resource::Instructions));
    assert!(compute.iter().all(|r| r.limit == "limit 5% (compute)"));
    assert!(
        tiered
            .stdout
            .contains(&format!("(storage 0, compute {})", compute.len())),
        "{}",
        tiered.stdout
    );

    // The single 0% fails it too: the old gate, noisy but not blind.
    let single = check(features, "0%");
    assert_eq!(single.code, Some(1), "{}", single.explain());
    assert_eq!(single.handlers(), ["cancel"]);
}
