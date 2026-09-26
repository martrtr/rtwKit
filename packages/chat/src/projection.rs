//! Principal-filtered Chat conversation projection over public World reads.

use std::collections::{BTreeMap, BTreeSet};

use rintawa_sdk::{
    world::{EntityId, SchemaKey},
    world_projection::{
        WorldProjectionReadRequest, WorldProjectionReadResult, WorldProjectionServiceRequest,
        WorldProjectionServiceResponse,
    },
    world_system::WorldSystemFacetTarget,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    model::{
        ChatWorldIndex, ContentBlock, ConversationState, MessageContent, MessageState,
        ParticipantBinding, ParticipantIdentity,
    },
    schemas::{
        chat_conversation_projection_schema_key, chat_send_message_command_schema_key,
        chat_world_index_schema_key, conversation_state_schema_key, message_content_schema_key,
        message_state_schema_key, participant_identity_schema_key,
    },
};

const DIAGNOSTIC_INVALID_INPUT: &str = "invalid Chat projection input";
const DIAGNOSTIC_MISSING_STATE: &str = "required Chat projection state is missing or incompatible";
const DIAGNOSTIC_TRANSPORT: &str = "Chat projection snapshot transport is incomplete";
const DIAGNOSTIC_SCHEMA: &str = "Chat projection schema construction failed";

/// Bounded feature input selecting one conversation from the active World.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatProjectionInput {
    /// Exact conversation to project, or `None` to select the newest conversation.
    pub conversation_id: Option<EntityId>,
}

/// One conversation entry exposed to navigation UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatConversationSummary {
    /// Persistent Conversation entity identity.
    pub id: EntityId,
    /// Optional bounded human-facing title.
    pub title: Option<String>,
}

/// One participant exposed to conversation UI after authority filtering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatParticipantView {
    /// Persistent Participant entity identity.
    pub id: EntityId,
    /// Conversation-local display name.
    pub display_name: String,
    /// Linked Character/entity when the participant is entity-controlled.
    pub linked_entity: Option<EntityId>,
    /// Whether the authenticated projection Principal may currently send as this participant.
    pub can_send: bool,
}

/// One logical Message at its current immutable revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatMessageView {
    /// Persistent logical Message identity.
    pub id: EntityId,
    /// Conversation-local author identity.
    pub author_participant_id: EntityId,
    /// Parent Message in the branch graph, when present.
    pub parent_message_id: Option<EntityId>,
    /// Earlier Message for which this Message is an alternative, when present.
    pub alternative_of: Option<EntityId>,
    /// Current immutable MessageRevision identity.
    pub revision_id: EntityId,
    /// Renderer-neutral semantic content blocks.
    pub blocks: Vec<ContentBlock>,
    /// Optional semantic time persisted with the current revision.
    pub effective_at: Option<rintawa_sdk::world::UnixTimeMillis>,
}

/// Complete bounded Chat view for one authenticated Principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatConversationView {
    /// Persistent conversations available in deterministic creation order.
    pub conversations: Vec<ChatConversationSummary>,
    /// Conversation represented by participants/messages, when one exists.
    pub selected_conversation_id: Option<EntityId>,
    /// Selected branch leaf inside the selected conversation.
    pub selected_leaf: Option<EntityId>,
    /// Selected conversation participants in deterministic creation order.
    pub participants: Vec<ChatParticipantView>,
    /// All logical Messages in deterministic creation order, including alternatives.
    pub messages: Vec<ChatMessageView>,
}

/// Evaluates one Chat projection using only owner-filtered continuation reads.
///
/// The Host fixes the authenticated Principal and pinned World position. This
/// function never receives raw storage access and can only request Chat-owned
/// facets plus authoritative `CanControl` decisions.
pub fn evaluate_chat_projection(
    request: &WorldProjectionServiceRequest,
) -> WorldProjectionServiceResponse {
    match evaluate(request) {
        Ok(response) => response,
        Err(ProjectionError::Rejected(reason)) => WorldProjectionServiceResponse::Rejected {
            reason: String::from(reason),
        },
        Err(ProjectionError::Failed(reason)) => WorldProjectionServiceResponse::Failed {
            reason: String::from(reason),
        },
    }
}

fn evaluate(
    request: &WorldProjectionServiceRequest,
) -> Result<WorldProjectionServiceResponse, ProjectionError> {
    if request.projection_schema
        != chat_conversation_projection_schema_key().map_err(schema_error)?
    {
        return Err(ProjectionError::Rejected(DIAGNOSTIC_INVALID_INPUT));
    }
    let input = serde_json::from_value::<ChatProjectionInput>(request.input.clone())
        .map_err(|_| ProjectionError::Rejected(DIAGNOSTIC_INVALID_INPUT))?;
    let world_index_schema = chat_world_index_schema_key().map_err(schema_error)?;
    let world_target = WorldSystemFacetTarget::World;

    if !has_facet_result(request, world_target, &world_index_schema) {
        return Ok(read_response(vec![WorldProjectionReadRequest::Facet {
            target: world_target,
            schema: world_index_schema,
        }]));
    }
    let world_index: ChatWorldIndex =
        optional_facet(request, world_target, &world_index_schema)?.unwrap_or_default();
    world_index
        .validate()
        .map_err(|_| ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
    let selected_conversation_id = match input.conversation_id {
        Some(id) if world_index.conversations.contains(&id) => Some(id),
        Some(_) => return Err(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE)),
        None => world_index.conversations.last().copied(),
    };

    let conversation_state_schema = conversation_state_schema_key().map_err(schema_error)?;
    let conversation_reads = missing_facet_reads(
        request,
        world_index.conversations.iter().copied(),
        &conversation_state_schema,
    );
    if !conversation_reads.is_empty() {
        return Ok(read_response(conversation_reads));
    }

    let mut conversation_states = BTreeMap::new();
    let mut conversations = Vec::with_capacity(world_index.conversations.len());
    for conversation_id in &world_index.conversations {
        let state: ConversationState = required_facet(
            request,
            WorldSystemFacetTarget::Entity(*conversation_id),
            &conversation_state_schema,
        )?;
        state
            .validate()
            .map_err(|_| ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
        conversations.push(ChatConversationSummary {
            id: *conversation_id,
            title: state.title.clone(),
        });
        conversation_states.insert(*conversation_id, state);
    }

    let Some(selected_conversation_id) = selected_conversation_id else {
        return complete(ChatConversationView {
            conversations,
            selected_conversation_id: None,
            selected_leaf: None,
            participants: Vec::new(),
            messages: Vec::new(),
        });
    };
    let selected = conversation_states
        .get(&selected_conversation_id)
        .ok_or(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))?;

    let participant_schema = participant_identity_schema_key().map_err(schema_error)?;
    let message_schema = message_state_schema_key().map_err(schema_error)?;
    let mut detail_reads = missing_facet_reads(
        request,
        selected.participant_ids.iter().copied(),
        &participant_schema,
    );
    detail_reads.extend(missing_facet_reads(
        request,
        selected.message_ids.iter().copied(),
        &message_schema,
    ));
    if !detail_reads.is_empty() {
        return Ok(read_response(detail_reads));
    }

    let send_schema = chat_send_message_command_schema_key().map_err(schema_error)?;
    let mut participant_identities = BTreeMap::new();
    let mut authority_reads = Vec::new();
    for participant_id in &selected.participant_ids {
        let identity: ParticipantIdentity = required_facet(
            request,
            WorldSystemFacetTarget::Entity(*participant_id),
            &participant_schema,
        )?;
        if let ParticipantBinding::Entity { entity_id } = identity.binding
            && !has_can_control_result(request, entity_id, &send_schema)
        {
            authority_reads.push(WorldProjectionReadRequest::CanControl {
                actor_entity: entity_id,
                command_schema: send_schema.clone(),
            });
        }
        participant_identities.insert(*participant_id, identity);
    }

    let mut message_states = BTreeMap::new();
    let mut revision_ids = BTreeSet::new();
    for message_id in &selected.message_ids {
        let state: MessageState = required_facet(
            request,
            WorldSystemFacetTarget::Entity(*message_id),
            &message_schema,
        )?;
        if state.conversation_id != selected_conversation_id
            || !selected
                .participant_ids
                .contains(&state.author_participant_id)
        {
            return Err(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE));
        }
        revision_ids.insert(state.current_revision);
        message_states.insert(*message_id, state);
    }
    for (message_id, state) in &message_states {
        for reference in [state.parent_message_id, state.alternative_of]
            .into_iter()
            .flatten()
        {
            if reference == *message_id || !selected.message_ids.contains(&reference) {
                return Err(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE));
            }
        }
        if let Some(alternative_of) = state.alternative_of {
            let source = message_states
                .get(&alternative_of)
                .ok_or(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
            if source.author_participant_id != state.author_participant_id
                || source.parent_message_id != state.parent_message_id
            {
                return Err(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE));
            }
        }
    }

    let content_schema = message_content_schema_key().map_err(schema_error)?;
    authority_reads.extend(missing_facet_reads(
        request,
        revision_ids.iter().copied(),
        &content_schema,
    ));
    if !authority_reads.is_empty() {
        return Ok(read_response(authority_reads));
    }

    let mut participants = Vec::with_capacity(selected.participant_ids.len());
    for participant_id in &selected.participant_ids {
        let identity = participant_identities
            .get(participant_id)
            .ok_or(ProjectionError::Failed(DIAGNOSTIC_TRANSPORT))?;
        let (linked_entity, can_send) = match identity.binding {
            ParticipantBinding::Principal { principal_id } => {
                (None, principal_id == request.principal)
            }
            ParticipantBinding::Entity { entity_id } => (
                Some(entity_id),
                can_control(request, entity_id, &send_schema)?,
            ),
            ParticipantBinding::Uncontrolled => (None, false),
        };
        participants.push(ChatParticipantView {
            id: *participant_id,
            display_name: identity.display_name.clone(),
            linked_entity,
            can_send,
        });
    }

    let mut messages = Vec::with_capacity(selected.message_ids.len());
    for message_id in &selected.message_ids {
        let state = message_states
            .get(message_id)
            .ok_or(ProjectionError::Failed(DIAGNOSTIC_TRANSPORT))?;
        let content: MessageContent = required_facet(
            request,
            WorldSystemFacetTarget::Entity(state.current_revision),
            &content_schema,
        )?;
        content
            .validate()
            .map_err(|_| ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
        messages.push(ChatMessageView {
            id: *message_id,
            author_participant_id: state.author_participant_id,
            parent_message_id: state.parent_message_id,
            alternative_of: state.alternative_of,
            revision_id: state.current_revision,
            blocks: content.blocks,
            effective_at: content.effective_at,
        });
    }

    complete(ChatConversationView {
        conversations,
        selected_conversation_id: Some(selected_conversation_id),
        selected_leaf: selected.selected_leaf,
        participants,
        messages,
    })
}

fn missing_facet_reads(
    request: &WorldProjectionServiceRequest,
    entities: impl IntoIterator<Item = EntityId>,
    schema: &SchemaKey,
) -> Vec<WorldProjectionReadRequest> {
    entities
        .into_iter()
        .filter_map(|entity_id| {
            let target = WorldSystemFacetTarget::Entity(entity_id);
            (!has_facet_result(request, target, schema)).then(|| {
                WorldProjectionReadRequest::Facet {
                    target,
                    schema: schema.clone(),
                }
            })
        })
        .collect()
}

fn has_facet_result(
    request: &WorldProjectionServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> bool {
    request.reads.iter().any(|result| {
        matches!(
            result,
            WorldProjectionReadResult::Facet {
                target: actual_target,
                schema: actual_schema,
                ..
            } if *actual_target == target && actual_schema == schema
        )
    })
}

fn optional_facet<T: DeserializeOwned>(
    request: &WorldProjectionServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> Result<Option<T>, ProjectionError> {
    let value = request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldProjectionReadResult::Facet {
                target: actual_target,
                schema: actual_schema,
                value,
            } if *actual_target == target && actual_schema == schema => Some(value.as_ref()),
            _ => None,
        })
        .ok_or(ProjectionError::Failed(DIAGNOSTIC_TRANSPORT))?;
    value
        .map(|facet| serde_json::from_value(facet.payload.clone()))
        .transpose()
        .map_err(|_| ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))
}

fn required_facet<T: DeserializeOwned>(
    request: &WorldProjectionServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> Result<T, ProjectionError> {
    optional_facet(request, target, schema)?
        .ok_or(ProjectionError::Rejected(DIAGNOSTIC_MISSING_STATE))
}

fn has_can_control_result(
    request: &WorldProjectionServiceRequest,
    actor_entity: EntityId,
    command_schema: &SchemaKey,
) -> bool {
    request.reads.iter().any(|result| {
        matches!(
            result,
            WorldProjectionReadResult::CanControl {
                actor_entity: actual_actor,
                command_schema: actual_schema,
                ..
            } if *actual_actor == actor_entity && actual_schema == command_schema
        )
    })
}

fn can_control(
    request: &WorldProjectionServiceRequest,
    actor_entity: EntityId,
    command_schema: &SchemaKey,
) -> Result<bool, ProjectionError> {
    request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldProjectionReadResult::CanControl {
                actor_entity: actual_actor,
                command_schema: actual_schema,
                allowed,
            } if *actual_actor == actor_entity && actual_schema == command_schema => Some(*allowed),
            _ => None,
        })
        .ok_or(ProjectionError::Failed(DIAGNOSTIC_TRANSPORT))
}

fn read_response(requests: Vec<WorldProjectionReadRequest>) -> WorldProjectionServiceResponse {
    WorldProjectionServiceResponse::Read { requests }
}

fn complete(view: ChatConversationView) -> Result<WorldProjectionServiceResponse, ProjectionError> {
    let value =
        serde_json::to_value(view).map_err(|_| ProjectionError::Failed(DIAGNOSTIC_SCHEMA))?;
    Ok(WorldProjectionServiceResponse::Complete { value })
}

fn schema_error(_error: crate::schemas::ChatSchemaError) -> ProjectionError {
    ProjectionError::Failed(DIAGNOSTIC_SCHEMA)
}

#[derive(Debug, Clone, Copy)]
enum ProjectionError {
    Rejected(&'static str),
    Failed(&'static str),
}
