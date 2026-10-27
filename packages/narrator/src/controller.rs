//! Bounded, one-shot Narrator reaction state machine.
//!
//! A provider may stream any number of provisional deltas, but this module
//! cannot emit a Chat command until a successful terminal completion, a fresh
//! principal-filtered projection and affirmative actor authorization arrive.

use rintawa_chat::{ChatConversationView, SendMessageCommand};
use rintawa_inference_contracts::{
    InferenceChunk, InferenceCompleted, MAX_INFERENCE_MESSAGE_BYTES,
};
use serde_json::from_slice;
use thiserror::Error;

use crate::context::{
    MAX_REPLY_TEXT_BYTES, NarratorContextError, TurnAnchor, reply_command, revalidate_turn,
};

/// Maximum number of nonblocking poll iterations for one model generation.
pub const MAX_GENERATION_POLLS: u32 = 1_200;
/// Maximum number of polls after provider completion while awaiting revalidation.
pub const MAX_REVALIDATION_POLLS: u32 = 80;

/// A non-secret, runtime-neutral state machine phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationPhase {
    /// An accepted provider-owned operation may still emit provisional chunks.
    Streaming,
    /// Terminal provider metadata was successful; revalidate the World next.
    NeedsWorldValidation,
    /// The controller has committed its one-shot decision and is no longer live.
    Finalized,
}

/// One final World submit request prepared under a fresh snapshot.
/// The host still owns the command ID, principal, Actor and final evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedReply {
    /// Entity actor which must be submitted to the ordinary World command path.
    pub actor_entity: rintawa_sdk::world::EntityId,
    /// Pinned World position for stale-write rejection by the Host.
    pub expected_position: u64,
    /// Chat-owned structured command, not a direct World mutation.
    pub command: SendMessageCommand,
}

/// Transport-agnostic, redacted reasons for not producing a canonical response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GenerationError {
    /// Stream frame violates the typed inference contract.
    #[error("inference stream was malformed")]
    InvalidChunk,
    /// Completion was missing, malformed or unsupported.
    #[error("inference completion was invalid or truncated")]
    InvalidCompletion,
    /// Stream would exceed Chat's explicitly supported output budget.
    #[error("inference output exceeded the text budget")]
    OutputTooLarge,
    /// Connection or provider failed before terminal success.
    #[error("provider operation failed")]
    ProviderFailed,
    /// Explicit request cancellation or World shutdown.
    #[error("generation cancelled")]
    Cancelled,
    /// The operation exceeded bounded time-to-result.
    #[error("generation timed out")]
    TimedOut,
    /// Stream/World event arrived in the wrong phase or after completion.
    #[error("invalid inference operation state")]
    InvalidState,
    /// The actor no longer controls the selected Chat state or it changed.
    #[error("Chat changed during generation")]
    StaleWorld,
    /// Completed generation yielded no usable text.
    #[error("completed generation produced no usable reply")]
    InvalidReply,
}

/// Result of one on-chain operation frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationProgress {
    /// No canonical mutations; poll for more provisional data.
    Pending,
    /// Provider success; acquire a fresh World projection before any commit.
    ValidateWorld,
}

/// Finite-state controller holding only provisional, bounded text and an anchor.
#[derive(Debug)]
pub struct GenerationController {
    anchor: TurnAnchor,
    phase: GenerationPhase,
    text: String,
    polls: u32,
    revalidation_polls: u32,
}

impl GenerationController {
    /// Begins exactly one generation for one branch revision and actor.
    pub fn new(anchor: TurnAnchor) -> Self {
        Self {
            anchor,
            phase: GenerationPhase::Streaming,
            text: String::new(),
            polls: 0,
            revalidation_polls: 0,
        }
    }

    /// Returns the exact originating Chat revision and authorized actor claim.
    pub const fn anchor(&self) -> &TurnAnchor {
        &self.anchor
    }

    /// Returns the current phase, without exposing provisional text.
    pub const fn phase(&self) -> GenerationPhase {
        self.phase
    }

    /// Records one pending iteration; does not accept an endless live stream.
    pub fn poll_pending(&mut self) -> Result<(), GenerationError> {
        let budget = match self.phase {
            GenerationPhase::Streaming => (&mut self.polls, MAX_GENERATION_POLLS),
            GenerationPhase::NeedsWorldValidation => {
                (&mut self.revalidation_polls, MAX_REVALIDATION_POLLS)
            }
            GenerationPhase::Finalized => return Err(GenerationError::InvalidState),
        };
        *budget.0 = budget.0.saturating_add(1);
        if *budget.0 > budget.1 {
            self.fail();
            return Err(GenerationError::TimedOut);
        }
        Ok(())
    }

    /// Ingests one typed provisional text-delta envelope.
    ///
    /// The caller provides exactly one bounded async-operation payload.
    /// Unknown chunk types or invalid UTF-8 must never be accepted as text.
    pub fn push_chunk(&mut self, bytes: &[u8]) -> Result<(), GenerationError> {
        if self.phase != GenerationPhase::Streaming || bytes.len() > MAX_INFERENCE_MESSAGE_BYTES {
            return Err(self.reject(GenerationError::InvalidChunk));
        }
        let chunk: InferenceChunk =
            from_slice(bytes).map_err(|_| self.reject(GenerationError::InvalidChunk))?;
        let InferenceChunk::TextDelta { text } = chunk;
        if self.text.len().saturating_add(text.len()) > MAX_REPLY_TEXT_BYTES {
            return Err(self.reject(GenerationError::OutputTooLarge));
        }
        self.text.push_str(&text);
        Ok(())
    }

    /// Accepts only a final fully completed provider frame with a normal
    /// stopping reason. A truncated output is never a canonical message.
    pub fn complete(&mut self, bytes: &[u8]) -> Result<GenerationProgress, GenerationError> {
        if self.phase != GenerationPhase::Streaming || bytes.len() > MAX_INFERENCE_MESSAGE_BYTES {
            return Err(self.reject(GenerationError::InvalidCompletion));
        }
        let finished: InferenceCompleted =
            from_slice(bytes).map_err(|_| self.reject(GenerationError::InvalidCompletion))?;
        if finished.model.trim().is_empty()
            || finished.model.len() > 160
            || finished.finish_reason != "stop"
        {
            return Err(self.reject(GenerationError::InvalidCompletion));
        }
        if self.text.trim().is_empty() {
            return Err(self.reject(GenerationError::InvalidReply));
        }
        self.phase = GenerationPhase::NeedsWorldValidation;
        Ok(GenerationProgress::ValidateWorld)
    }

    /// Records a terminal external transport/provider failure, without writing
    /// provisional text to World/Chat or exposing raw HTTP/provider errors.
    pub fn provider_failed(&mut self) -> GenerationError {
        self.reject(GenerationError::ProviderFailed)
    }

    /// Ends this in-progress attempt without a World mutation.
    pub fn cancel(&mut self) -> GenerationError {
        self.reject(GenerationError::Cancelled)
    }

    /// Checks a fresh Chat projection and returns a one-shot authoritative
    /// command proposal. Even if this succeeds the Host must check its Actor,
    /// Principal and expected World position during its normal System path.
    pub fn prepare_reply(
        &mut self,
        view: &ChatConversationView,
        snapshot_position: u64,
    ) -> Result<AuthorizedReply, GenerationError> {
        if self.phase != GenerationPhase::NeedsWorldValidation {
            return Err(self.reject(GenerationError::InvalidState));
        }
        self.phase = GenerationPhase::Finalized;
        let text = std::mem::take(&mut self.text);
        revalidate_turn(&self.anchor, view, snapshot_position)
            .map_err(|_| GenerationError::StaleWorld)?;
        let command =
            reply_command(&self.anchor, &text).map_err(|_| GenerationError::InvalidReply)?;
        Ok(AuthorizedReply {
            actor_entity: self.anchor.responder_entity_id,
            expected_position: snapshot_position,
            command,
        })
    }

    fn reject(&mut self, error: GenerationError) -> GenerationError {
        self.fail();
        error
    }

    fn fail(&mut self) {
        self.phase = GenerationPhase::Finalized;
        self.text.clear();
    }
}

/// Maps all Chat identity and authoring rejection into one safe controller
/// category. No raw World facet or untrusted text is ever exposed in errors.
pub fn map_context_failure(_error: NarratorContextError) -> GenerationError {
    GenerationError::StaleWorld
}
