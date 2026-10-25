//! Ledger advance: how sequence number and timestamp move between steps.

use proptest::prelude::*;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::Env;

use crate::log::Gap;

/// Policy for moving `env.ledger()` before each step.
///
/// The gap for each step is generated as part of the step, so it is
/// deterministic for a seed and shrinks together with the sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerAdvance {
    /// Do not move the ledger.
    Still,
    /// Advance by the same amount before every step.
    Fixed {
        /// Ledgers per step.
        ledgers: u32,
        /// Seconds per step.
        seconds: u64,
    },
    /// Advance by a random amount drawn from the given inclusive ranges.
    Random {
        /// Range of ledgers per step.
        ledgers: std::ops::RangeInclusive<u32>,
        /// Range of seconds per step.
        seconds: std::ops::RangeInclusive<u64>,
    },
    /// A usage profile: `(steps, gap)` segments applied in order. After the
    /// last segment is exhausted its gap keeps applying.
    ///
    /// Example: thirty steps of daily use followed by a long silence is
    /// `Profile(vec![(30, Gap { ledgers: 17_280, seconds: 86_400 }), (1, Gap { ledgers: 1_555_200, seconds: 7_776_000 })])`.
    Profile(Vec<(usize, Gap)>),
}

impl Default for LedgerAdvance {
    /// One ledger and five seconds per step, roughly Stellar's close time.
    fn default() -> Self {
        LedgerAdvance::Fixed {
            ledgers: 1,
            seconds: 5,
        }
    }
}

impl LedgerAdvance {
    /// Strategy for the gap before the step at `step_index`.
    pub(crate) fn gap_strategy(&self, step_index: usize) -> BoxedStrategy<Gap> {
        match self {
            LedgerAdvance::Still => Just(Gap::default()).boxed(),
            LedgerAdvance::Fixed { ledgers, seconds } => Just(Gap {
                ledgers: *ledgers,
                seconds: *seconds,
            })
            .boxed(),
            LedgerAdvance::Random { ledgers, seconds } => (ledgers.clone(), seconds.clone())
                .prop_map(|(ledgers, seconds)| Gap { ledgers, seconds })
                .boxed(),
            LedgerAdvance::Profile(segments) => {
                Just(Self::profile_gap(segments, step_index)).boxed()
            }
        }
    }

    fn profile_gap(segments: &[(usize, Gap)], step_index: usize) -> Gap {
        let mut remaining = step_index;
        for (count, gap) in segments {
            if remaining < *count {
                return *gap;
            }
            remaining -= count;
        }
        segments.last().map(|(_, gap)| *gap).unwrap_or_default()
    }

    /// Applies `gap` to the ledger of `env`.
    pub(crate) fn apply(env: &Env, gap: &Gap) {
        if gap.ledgers == 0 && gap.seconds == 0 {
            return;
        }
        env.ledger().with_mut(|li| {
            li.sequence_number = li.sequence_number.saturating_add(gap.ledgers);
            li.timestamp = li.timestamp.saturating_add(gap.seconds);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_walks_segments_then_repeats_last_gap() {
        let a = Gap {
            ledgers: 1,
            seconds: 1,
        };
        let b = Gap {
            ledgers: 2,
            seconds: 2,
        };
        let segments = vec![(2, a), (1, b)];
        assert_eq!(LedgerAdvance::profile_gap(&segments, 0), a);
        assert_eq!(LedgerAdvance::profile_gap(&segments, 1), a);
        assert_eq!(LedgerAdvance::profile_gap(&segments, 2), b);
        assert_eq!(LedgerAdvance::profile_gap(&segments, 9), b);
        assert_eq!(LedgerAdvance::profile_gap(&[], 0), Gap::default());
    }

    #[test]
    fn apply_moves_sequence_and_timestamp() {
        let env = Env::default();
        let before = env.ledger().get();
        LedgerAdvance::apply(
            &env,
            &Gap {
                ledgers: 10,
                seconds: 50,
            },
        );
        assert_eq!(env.ledger().sequence(), before.sequence_number + 10);
        assert_eq!(env.ledger().timestamp(), before.timestamp + 50);
    }
}
