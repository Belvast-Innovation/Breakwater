//! Generated steps and the proptest state machine that produces them.

use std::sync::Arc;

use proptest::prelude::*;
use proptest_state_machine::Sequential;

use crate::handler::{DynHandler, StepArgs};
use crate::ledger::LedgerAdvance;
use crate::log::Gap;

/// One generated call: which handler, which of its actors, with what
/// arguments, after what ledger gap.
#[derive(Clone, Debug)]
pub struct Step {
    /// Index into the suite's handler list.
    pub handler: usize,
    /// Index into that handler's actor list.
    pub actor: usize,
    /// Generated arguments.
    pub args: StepArgs,
    /// Ledger advance applied before the call.
    pub gap: Gap,
}

/// Reference state of the proptest state machine. The framework only needs
/// to know how many steps have been generated so far, which drives
/// [`LedgerAdvance::Profile`].
#[derive(Clone, Debug, Default)]
pub(crate) struct RefState {
    pub step: usize,
}

pub(crate) type SequenceStrategy =
    Sequential<RefState, Step, BoxedStrategy<RefState>, BoxedStrategy<Step>>;

/// Builds the sequential strategy for a suite's handlers.
pub(crate) fn sequence_strategy(
    handlers: Arc<[Box<dyn DynHandler>]>,
    advance: LedgerAdvance,
    size: std::ops::RangeInclusive<usize>,
) -> SequenceStrategy {
    Sequential::new(
        size.into(),
        || Just(RefState::default()).boxed(),
        |_: &RefState, _: &Step| true,
        move |state: &RefState| step_strategy(handlers.clone(), advance.clone(), state.step),
        |state: RefState, _: &Step| RefState {
            step: state.step + 1,
        },
    )
}

fn step_strategy(
    handlers: Arc<[Box<dyn DynHandler>]>,
    advance: LedgerAdvance,
    step_index: usize,
) -> BoxedStrategy<Step> {
    let n = handlers.len();
    (0..n)
        .prop_flat_map(move |h| {
            let handler = &handlers[h];
            let actors = handler.actors().len();
            (
                Just(h),
                0..actors,
                handler.strategy(),
                advance.gap_strategy(step_index),
            )
        })
        .prop_map(|(handler, actor, args, gap)| Step {
            handler,
            actor,
            args,
            gap,
        })
        .boxed()
}
