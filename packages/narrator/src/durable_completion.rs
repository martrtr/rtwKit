//! Durable Narrator EffectJob completion into an ordinary fenced Chat command.
//!
//! Only the post-inference worker may call these pure functions with a fresh,
//! principal-filtered Chat projection and Host-owned EffectJob identities.
//! A validated proposal is NOT authority: Core must still check Principal,
//! ControlGrant, current World position and claim fencing at final commit.

use rintawa_chat::{ChatConversationView, chat_send_message_command_schema_key};
use rintawa_sdk::{
    world::{CommandId, CorrelationId, EffectJobId, PrincipalId, WorldId},
    world_system::{WorldSystemActor, WorldSystemCausation, WorldSystemCommand},
};
use thiserror::Error;

use crate::{
    ControllerMode, ControllerPolicy, DurableReaction, PreparedTurn, ReactionIntent,
    prepare_turn_for_responder, reply_command,
};

/// Fatal or stale conditions which cannot authorize a canonical Chat reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DurableReplyError {
    /// Job World, trusted policy World or source event identity does not match.
    #[error("durable reply source identity does not match the pinned World")]
    WrongSource,
    /// Persisted controller revision or exact conversation/character has changed.
    #[error("durable reply controller policy changed or is not explicit")]
    ControllerChanged,
    /// Current Chat branch no longer matches the original triggering revision.
    #[error("durable reply was invalidated by a Chat branch or actor change")]
    StaleChat,
    /// Provider text is empty, oversized or cannot become a typed Chat command.
    #[error("durable reply content was rejected by Chat")]
    InvalidReply,
    /// Package-owned schema or serialization failed.
    #[error("durable reply command could not be encoded")]
    Encoding,
}

/// Pinned identity from the claimed Host EffectJob, not a model or guest input.
///
/// The final WorldCommand's principal, correlation and completion ID must
/// originate in the actual claimed EffectJob, never an extension preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCompletionIdentity {
    /// World scoped to the Host claim.
    pub world_id: WorldId,
    /// EffectJobId provides the exact idempotent completion CommandId.
    pub job_id: EffectJobId,
    /// Principal authenticated by the original enqueue WorldCommand.
    pub principal: PrincipalId,
    /// Original committed enqueue correlation for causal traceability.
    pub correlation_id: CorrelationId,
}

/// Prepare an exact source-bound turn from a fresh authorized Chat projection.
///
/// This is also used before inference to avoid calling a model for a stale
/// conversation, missing responder grant or changed controller revision.
///
/// # Errors
///
/// Refuses a foreign World, invalid source version, legacy controller policy,
/// a reparented/edited trigger, a changed responder or absent Chat authority.
pub fn prepare_durable_turn(
    reaction: &DurableReaction,
    policy: &ControllerPolicy,
    view: &ChatConversationView,
    snapshot_position: u64,
    expected_world: WorldId,
) -> Result<PreparedTurn, DurableReplyError> {
    if policy.world_id() != expected_world || reaction.intent.world_id() != expected_world {
        return Err(DurableReplyError::WrongSource);
    }
    if reaction.controller_policy_revision == 0
        || policy.revision() != reaction.controller_policy_revision
    {
        return Err(DurableReplyError::ControllerChanged);
    }
    let ControllerMode::Explicit {
        conversation_id,
        participant_id,
    } = policy.mode()
    else {
        return Err(DurableReplyError::ControllerChanged);
    };
    if conversation_id != reaction.intent.conversation_id()
        || participant_id != reaction.responder_participant_id
    {
        return Err(DurableReplyError::ControllerChanged);
    }
    if reaction.intent.source_position() > snapshot_position {
        return Err(DurableReplyError::WrongSource);
    }
    let identity = reaction
        .intent
        .encode()
        .map_err(|_| DurableReplyError::WrongSource)?;
    ReactionIntent::decode(expected_world, &identity)
        .map_err(|_| DurableReplyError::WrongSource)?;

    let prepared =
        prepare_turn_for_responder(view, snapshot_position, reaction.responder_participant_id)
            .map_err(|_| DurableReplyError::StaleChat)?;
    let anchor = &prepared.anchor;
    if anchor.conversation_id != reaction.intent.conversation_id()
        || anchor.trigger_message_id != reaction.intent.message_id()
        || anchor.trigger_revision_id != reaction.intent.revision_id()
        || anchor.trigger_author_participant_id != reaction.intent.author_participant_id()
        || anchor.trigger_parent_message_id != reaction.intent.parent_message_id()
        || anchor.trigger_alternative_of != reaction.intent.alternative_of()
        || anchor.responder_participant_id != reaction.responder_participant_id
        || anchor.responder_entity_id != reaction.responder_entity_id
    {
        return Err(DurableReplyError::StaleChat);
    }
    Ok(prepared)
}

/// Create the only allowed Chat completion from a claimed durable EffectJob.
///
/// Call only after a provider reports terminal success and the worker has
/// obtained a *second*, fresh principal-filtered Chat projection. Host/Chat
/// still checks this entity actor's current ControlGrant atomically at commit.
///
/// # Errors
///
/// Refuses stale/mismatched identity, invalid response or a missing schema.
pub fn complete_durable_turn(
    identity: DurableCompletionIdentity,
    reaction: &DurableReaction,
    policy: &ControllerPolicy,
    fresh_view: &ChatConversationView,
    fresh_position: u64,
    generated_text: &str,
) -> Result<WorldSystemCommand, DurableReplyError> {
    let prepared = prepare_durable_turn(
        reaction,
        policy,
        fresh_view,
        fresh_position,
        identity.world_id,
    )?;
    let command = reply_command(&prepared.anchor, generated_text)
        .map_err(|_| DurableReplyError::InvalidReply)?;
    let payload = serde_json::to_value(command).map_err(|_| DurableReplyError::Encoding)?;
    let schema = chat_send_message_command_schema_key().map_err(|_| DurableReplyError::Encoding)?;

    Ok(WorldSystemCommand {
        id: CommandId::from_bytes(identity.job_id.into_bytes()),
        schema,
        principal: identity.principal,
        actor: WorldSystemActor::Entity(reaction.responder_entity_id),
        expected_position: Some(fresh_position),
        causation: Some(WorldSystemCausation::Effect(identity.job_id)),
        correlation_id: identity.correlation_id,
        effective_at: None,
        payload,
    })
}
