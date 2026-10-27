//! World-scoped Narrator reaction policies and typed, bounded inference orchestration.
//!
//! This ordinary rtwKit package composes Chat's principal-filtered view with a
//! provider-neutral inference service; it never reads raw World state or
//! changes canonical Chat messages except through the Chat-owned World System.
//! Durable effect/job handoff is a separate prerequisite for crash-safe MVP
//! delivery and is explicitly not claimed by this first runtime adapter.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

pub mod context;
pub mod controller;

pub use context::{
    MAX_BRANCH_DEPTH, MAX_MESSAGE_INPUT_BYTES, MAX_PROJECTED_MESSAGES, MAX_PROMPT_HISTORY,
    MAX_PROMPT_TEXT_BYTES, MAX_REPLY_TEXT_BYTES, NarratorContextError, PreparedTurn,
    ResponderSelection, TRUSTED_INSTRUCTION, TurnAnchor, prepare_turn, prepare_turn_for_responder,
    reply_command, revalidate_turn,
};
pub use controller::{
    AuthorizedReply, GenerationController, GenerationError, GenerationPhase, GenerationProgress,
    MAX_GENERATION_POLLS, MAX_REVALIDATION_POLLS,
};
