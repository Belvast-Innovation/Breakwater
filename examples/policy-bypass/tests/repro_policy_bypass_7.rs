//! Reproduction exported by `breakwater export` for suite `policy-bypass` with seed 7.
//! Replays the shrunk sequence through `common::replay_json` and asserts
//! that the same invariant fails at the same step.

mod common;

const STEPS: &str = r#"[
  {
    "handler": "transfer",
    "actor": "agent",
    "args": {
      "amount_usdc": 49
    },
    "gap": {
      "ledgers": 1,
      "seconds": 5
    }
  },
  {
    "handler": "transfer",
    "actor": "agent",
    "args": {
      "amount_usdc": 49
    },
    "gap": {
      "ledgers": 10,
      "seconds": 50
    }
  },
  {
    "handler": "transfer",
    "actor": "agent",
    "args": {
      "amount_usdc": 49
    },
    "gap": {
      "ledgers": 1718,
      "seconds": 8590
    }
  }
]"#;

#[test]
fn repro_policy_bypass_7() {
    let report = common::replay_json(STEPS);
    let failure = report
        .failure()
        .expect("the replayed sequence violates an invariant");
    assert_eq!(failure.violation.source, "rolling_cap");
    assert_eq!(failure.failing_step, 2);
    assert_eq!(
        failure.violation.message,
        "agent spent 147 within 17280 ledgers, limit is 100"
    );
}
