//! `policy-bypass` attack pack.
//!
//! Invariants and sequence generators that hunt for multi-step transaction
//! sequences slipping past OpenZeppelin smart-account policies (spending
//! limits, context rules, signer scopes). See `docs/ARCHITECTURE.md` §5.
//!
//! This crate currently contains only the workspace scaffold.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
