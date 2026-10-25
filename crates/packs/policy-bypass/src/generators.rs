//! Generators biased toward known policy-bypass shapes.

use breakwater::LedgerAdvance;
use proptest::prelude::*;

/// Amounts (in whole token units) clustered just under and around `cap`:
/// mostly `cap - 1`, sometimes exactly `cap`, sometimes anything below it,
/// occasionally above it so that single-call rejections are exercised too.
pub fn amount_near_cap(cap: i64) -> BoxedStrategy<i64> {
    let cap = cap.max(2);
    prop_oneof![
        5 => Just(cap - 1),
        2 => Just(cap),
        2 => 1..cap,
        1 => (cap + 1)..=(cap * 3),
    ]
    .boxed()
}

/// Ledger gaps that frequently cross a policy window of `period_ledgers`:
/// uniform over one and a half periods, so roughly a third of the gaps land
/// past the window boundary. Timestamps follow at five seconds per ledger.
pub fn window_straddling_advance(period_ledgers: u32) -> LedgerAdvance {
    let max_ledgers = (period_ledgers / 2).saturating_mul(3).max(2);
    LedgerAdvance::Random {
        ledgers: 1..=max_ledgers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::test_runner::{Config, TestRunner};

    #[test]
    fn amounts_stay_in_range_and_favour_just_under_cap() {
        let mut runner = TestRunner::new(Config::default());
        let mut under = 0;
        for _ in 0..200 {
            let v = amount_near_cap(50).new_tree(&mut runner).unwrap().current();
            assert!((1..=150).contains(&v));
            if v == 49 {
                under += 1;
            }
        }
        assert!(
            under > 50,
            "expected a strong bias toward cap - 1, got {under}/200"
        );
    }

    #[test]
    fn straddling_advance_spans_past_the_window() {
        match window_straddling_advance(1_728) {
            LedgerAdvance::Random { ledgers, .. } => assert_eq!(ledgers, 1..=2_592),
            other => panic!("unexpected advance {other:?}"),
        }
    }
}
