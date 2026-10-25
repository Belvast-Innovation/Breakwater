//! Deriving live streams from the transaction log.

use breakwater::{StepRecord, TxLog};

/// A stream the log says exists and has not been cancelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveStream {
    /// Stream id, assigned by creation order starting at `first_id`.
    pub id: u64,
    /// Step that created it.
    pub created_step: usize,
    /// Ledger at which it was created.
    pub created_at: u32,
    /// Ledger at which it has fully vested.
    pub end_ledger: u32,
}

/// How to read stream lifecycle events out of the log.
///
/// Ids are inferred from creation order because the log does not carry return
/// values; this matches contracts that hand out ids from a counter.
#[derive(Clone, Debug)]
pub struct StreamModel {
    /// Handler whose success creates a stream.
    pub create_handler: String,
    /// Argument field of the create handler holding the duration in ledgers.
    pub duration_field: String,
    /// Handler whose success ends a stream early.
    pub cancel_handler: String,
    /// Argument field of the cancel handler holding the stream id.
    pub id_field: String,
    /// Id of the first stream created.
    pub first_id: u64,
}

impl StreamModel {
    /// Streams that exist at the end of `log`, oldest first.
    pub fn live_streams(&self, log: &TxLog) -> Vec<LiveStream> {
        let mut streams: Vec<LiveStream> = Vec::new();
        let mut next_id = self.first_id;
        for step in log.successes() {
            if step.handler == self.create_handler {
                let duration = u32::try_from(self.field(step, &self.duration_field)).unwrap_or(0);
                streams.push(LiveStream {
                    id: next_id,
                    created_step: step.index,
                    created_at: step.ledger_sequence,
                    end_ledger: step.ledger_sequence.saturating_add(duration),
                });
                next_id += 1;
            } else if step.handler == self.cancel_handler {
                let id = u64::try_from(self.field(step, &self.id_field)).unwrap_or(u64::MAX);
                streams.retain(|s| s.id != id);
            }
        }
        streams
    }

    /// Live streams that have not reached their end ledger at `now`.
    pub fn running_streams(&self, log: &TxLog, now: u32) -> Vec<LiveStream> {
        self.live_streams(log)
            .into_iter()
            .filter(|s| s.end_ledger > now)
            .collect()
    }

    fn field(&self, step: &StepRecord, name: &str) -> i64 {
        step.args.get(name).and_then(|v| v.as_i64()).unwrap_or(-1)
    }
}

impl Default for StreamModel {
    fn default() -> Self {
        Self {
            create_handler: "create_stream".into(),
            duration_field: "duration_ledgers".into(),
            cancel_handler: "cancel".into(),
            id_field: "stream".into(),
            first_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breakwater::{Actor, Gap, StepOutcome};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env};

    fn rec(
        index: usize,
        handler: &str,
        args: serde_json::Value,
        ledger: u32,
        ok: bool,
    ) -> StepRecord {
        let env = Env::default();
        StepRecord {
            index,
            handler: handler.into(),
            actor: Actor::from_address("x", &Address::generate(&env)),
            args,
            gap: Gap::default(),
            ledger_sequence: ledger,
            timestamp: 0,
            outcome: if ok {
                StepOutcome::Success
            } else {
                StepOutcome::ContractError(1)
            },
        }
    }

    #[test]
    fn ids_follow_creation_order_and_cancel_removes() {
        let model = StreamModel::default();
        let log = TxLog::from_records(vec![
            rec(
                0,
                "create_stream",
                serde_json::json!({ "duration_ledgers": 100 }),
                10,
                true,
            ),
            rec(
                1,
                "create_stream",
                serde_json::json!({ "duration_ledgers": 100 }),
                11,
                false,
            ),
            rec(
                2,
                "create_stream",
                serde_json::json!({ "duration_ledgers": 50 }),
                12,
                true,
            ),
            rec(3, "cancel", serde_json::json!({ "stream": 0 }), 13, true),
        ]);
        let live = model.live_streams(&log);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, 1);
        assert_eq!(live[0].end_ledger, 62);
        assert!(model.running_streams(&log, 62).is_empty());
        assert_eq!(model.running_streams(&log, 61).len(), 1);
    }
}
