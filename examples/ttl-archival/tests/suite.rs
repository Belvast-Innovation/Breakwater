//! Breakwater suite for the payment-stream demo under a usage profile. The
//! fixture and handlers live in `common/mod.rs`; see the notes there.
//!
//! `breakwater_suite` is the test the `breakwater` CLI drives: it honours
//! `BREAKWATER_SEED`, `BREAKWATER_STEPS` and `BREAKWATER_AUTH`, writes the
//! report to `BREAKWATER_REPORT` and, when driven that way, leaves judging the
//! result to the CLI. Under plain `cargo test` it also asserts the known shape.

mod common;

use std::time::Instant;

use breakwater::{Exporter, Outcome, SuiteConfig, ViolationKind, LEDGERS_PER_DAY};
use breakwater_pack_ttl_archival::ResourceBudget;
use common::*;

#[test]
fn breakwater_suite() {
    let driven = std::env::var_os("BREAKWATER_REPORT").is_some();
    let config = SuiteConfig::from_env(default_config(11, 16));
    let started = Instant::now();
    let report = suite_with_config(Variant::Buggy, config)
        .run()
        .expect("suite runs");
    let elapsed = started.elapsed();
    Exporter::write_if_requested(&report).expect("report is written");
    if driven {
        return;
    }
    eprintln!(
        "ttl-archival: {} cases, {} steps in {:?} ({:.2} ms/step)",
        report.cases_run,
        report.steps_run,
        elapsed,
        elapsed.as_secs_f64() * 1_000.0 / report.steps_run.max(1) as f64
    );
    let failure = report
        .failure()
        .expect("the buggy stream must expire mid-lifecycle");
    eprintln!("shrunk sequence:");
    for step in failure.log.steps() {
        eprintln!(
            "  #{} {:<14} {:<10} +{:>8} ledgers ({:>5.1} d)  args={}  -> {:?}",
            step.index,
            step.handler,
            step.actor.name(),
            step.gap.ledgers,
            f64::from(step.gap.ledgers) / f64::from(LEDGERS_PER_DAY),
            step.args,
            step.outcome
        );
    }
    eprintln!(
        "violation: {} ({})",
        failure.violation.message, failure.violation.source
    );
    eprintln!(
        "detail: {}",
        failure
            .violation
            .detail
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default()
    );

    assert_eq!(failure.violation.kind, ViolationKind::Invariant);
    assert_eq!(failure.violation.source, "no_mid_lifecycle_expiry");
    let detail = failure.violation.detail.as_ref().expect("detail");
    assert_eq!(detail["stream"], 0);
    assert!(detail["days_remaining"].as_f64().unwrap() > 200.0);
    assert_eq!(failure.log.steps()[0].handler, "create_stream");
    assert!(
        failure.log.len() <= 3,
        "shrunk to {} steps",
        failure.log.len()
    );
}

#[test]
fn same_seed_reproduces_the_same_sequence() {
    let a = suite(Variant::Buggy, 11, 16).run().expect("suite runs");
    let b = suite(Variant::Buggy, 11, 16).run().expect("suite runs");
    assert_eq!(a.failure().map(|f| &f.log), b.failure().map(|f| &f.log));
}

#[test]
fn fixed_contract_survives_the_same_profile() {
    let report = suite(Variant::Fixed, 11, 16).run().expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.failure().map(|f| &f.violation)
    );
}

/// Restoration budget for the demo, from measuring the buggy contract: a
/// withdrawal from a live stream reads at most one entry from disk and a
/// single archived stream costs one entry (384 bytes); the first call after a
/// quiet quarter restores the instance, the stream and the token balances in
/// one go (5 entries, 1 516 bytes), and a batch over N archived streams costs N.
fn demo_budget() -> ResourceBudget {
    ResourceBudget {
        max_disk_read_entries: Some(3),
        max_disk_read_bytes: Some(1_000),
        ..ResourceBudget::default()
    }
}

#[test]
fn anyone_can_keep_entries_alive_beyond_the_stream() {
    let report = keep_alive_suite(Variant::Buggy, 5, 32)
        .run()
        .expect("suite runs");
    let failure = report
        .failure()
        .expect("the public bump_ttl must overshoot the stream's lifetime");
    eprintln!("keep-alive shrunk sequence:");
    for step in failure.log.steps() {
        eprintln!(
            "  #{} {:<14} {:<8} +{:>6} ledgers args={} -> {:?}",
            step.index,
            step.handler,
            step.actor.name(),
            step.gap.ledgers,
            step.args,
            step.outcome
        );
    }
    eprintln!(
        "violation: {} ({})",
        failure.violation.message, failure.violation.source
    );
    assert_eq!(failure.violation.source, "no_unbounded_ttl_extension");
    assert!(failure.log.steps().iter().any(|s| s.handler == "bump_ttl"));
    assert!(
        failure.log.len() <= 3,
        "shrunk to {} steps",
        failure.log.len()
    );
}

#[test]
fn guarded_bump_ttl_never_overshoots() {
    let report = keep_alive_suite(Variant::Fixed, 5, 32)
        .run()
        .expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.failure().map(|f| &f.violation)
    );
}

#[test]
fn the_first_call_after_a_quiet_quarter_blows_the_restoration_budget() {
    let report = restore_cost_suite(Variant::Buggy, 3, 16, demo_budget())
        .run()
        .expect("suite runs");
    let failure = report
        .failure()
        .expect("restoring everything at once must exceed the budget");
    eprintln!("restore-cost shrunk sequence:");
    for step in failure.log.steps() {
        eprintln!(
            "  #{} {:<14} {:<10} +{:>8} ledgers args={} -> {:?} value={:?}",
            step.index,
            step.handler,
            step.actor.name(),
            step.gap.ledgers,
            step.args,
            step.outcome,
            step.return_value
        );
    }
    eprintln!(
        "violation: {} ({})",
        failure.violation.message, failure.violation.source
    );
    eprintln!(
        "detail: {}",
        failure
            .violation
            .detail
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default()
    );
    assert_eq!(failure.violation.source, "restore_cost_within_budget");
    let detail = failure.violation.detail.as_ref().expect("detail");
    assert_eq!(detail["handler"], "withdraw_many");
    assert!(detail["disk_read_entries"].as_u64().unwrap() > 3);
    assert_eq!(failure.log.last().unwrap().handler, "withdraw_many");
    assert!(
        failure.log.len() <= 4,
        "shrunk to {} steps",
        failure.log.len()
    );
}

#[test]
fn the_fixed_contract_never_restores_anything() {
    let report = restore_cost_suite(Variant::Fixed, 3, 16, demo_budget())
        .run()
        .expect("suite runs");
    assert!(
        matches!(report.outcome, Outcome::Held),
        "{:?}",
        report.failure().map(|f| &f.violation)
    );
}
