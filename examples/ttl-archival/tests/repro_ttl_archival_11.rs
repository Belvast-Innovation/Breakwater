//! Reproduction exported by `breakwater export` for suite `ttl-archival` with seed 11.
//! Replays the shrunk sequence through `common::replay_json` and asserts
//! that the same invariant fails at the same step.

mod common;

const STEPS: &str = r#"[
  {
    "handler": "create_stream",
    "actor": "sender",
    "args": {
      "amount_usdc": 1000,
      "duration_ledgers": 6307200
    },
    "gap": {
      "ledgers": 17280,
      "seconds": 86400
    }
  },
  {
    "handler": "create_stream",
    "actor": "sender",
    "args": {
      "amount_usdc": 1000,
      "duration_ledgers": 6307200
    },
    "gap": {
      "ledgers": 1555200,
      "seconds": 7776000
    }
  }
]"#;

#[test]
fn repro_ttl_archival_11() {
    let report = common::replay_json(STEPS);
    let failure = report
        .failure()
        .expect("the replayed sequence violates an invariant");
    assert_eq!(failure.violation.source, "no_mid_lifecycle_expiry");
    assert_eq!(failure.failing_step, 1);
    assert_eq!(
        failure.violation.message,
        "stream 0 is archived with 275 days left to run (TTL ran out 60.0 days ago)"
    );
}
