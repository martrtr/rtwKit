//! Pure Chat World System evaluation over the public pinned-snapshot transport.

use serde::{Serialize, de::DeserializeOwned};

use rintawa_sdk::{
    world::{EntityId, RelationId, SchemaKey},
    world_system::{
        WorldSystemActor, WorldSystemEntityRecord, WorldSystemEventProposal,
        WorldSystemFacetRecord, WorldSystemFacetTarget, WorldSystemMutation,
        WorldSystemReadRequest, WorldSystemReadResult, WorldSystemRelationRecord,
        WorldSystemServiceRequest, WorldSystemServiceResponse, WorldSystemTransaction,
    },
};

use crate::{
    ids::{command_entity_id, derived_entity_id, relation_id},
    model::{
        AddParticipantCommand, AlternativeRequestedEvent, BranchSelectedEvent, ChatWorldIndex,
        ConversationCreatedEvent, ConversationState, CreateConversationCommand, EditMessageCommand,
        MessageAddedEvent, MessageContent, MessageEditedEvent, MessageState, ParticipantAddedEvent,
        ParticipantBinding, ParticipantBindingRequest, ParticipantIdentity,
        RequestAlternativeCommand, SelectBranchCommand, SendMessageCommand, validate_blocks,
        validate_participant_name, validate_title,
    },
    schemas::{
        alternative_requested_event_schema_key, branch_selected_event_schema_key,
        chat_add_participant_command_schema_key, chat_create_conversation_command_schema_key,
        chat_edit_message_command_schema_key, chat_request_alternative_command_schema_key,
        chat_select_branch_command_schema_key, chat_send_message_command_schema_key,
        chat_world_index_schema_key, conversation_created_event_schema_key,
        conversation_entity_schema_key, conversation_message_schema_key,
        conversation_participant_schema_key, conversation_state_schema_key,
        message_added_event_schema_key, message_author_schema_key, message_content_schema_key,
        message_edited_event_schema_key, message_entity_schema_key, message_parent_schema_key,
        message_revision_entity_schema_key, message_revision_schema_key, message_state_schema_key,
        message_variant_schema_key, participant_added_event_schema_key,
        participant_entity_schema_key, participant_identity_schema_key,
    },
};

const DIAGNOSTIC_INVALID_COMMAND: &str = "invalid Chat command";
const DIAGNOSTIC_MISSING_STATE: &str = "required Chat state is missing or incompatible";
const DIAGNOSTIC_UNAUTHORIZED: &str = "Chat participant is not controlled by this actor";
const DIAGNOSTIC_TRANSPORT: &str = "Chat System snapshot transport is incomplete";
const DIAGNOSTIC_SCHEMA: &str = "Chat package schema construction failed";

/// Evaluates one Chat-owned command against immutable pinned snapshot reads.
///
/// The function is intentionally runtime-neutral. It may request additional bounded
/// reads and only returns ordinary transaction proposals; Core remains authoritative
/// for schema, actor, stale-position, and atomic commit validation.
pub fn evaluate_chat_world_system(
    request: &WorldSystemServiceRequest,
) -> WorldSystemServiceResponse {
    match evaluate(request) {
        Ok(response) => response,
        Err(EvaluationError::Rejected(reason)) => WorldSystemServiceResponse::Rejected {
            reason: String::from(reason),
        },
        Err(EvaluationError::Failed(reason)) => WorldSystemServiceResponse::Failed {
            reason: String::from(reason),
        },
    }
}

fn evaluate(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let schema = &request.command.schema;
    if schema == &chat_create_conversation_command_schema_key().map_err(schema_error)? {
        evaluate_create_conversation(request)
    } else if schema == &chat_add_participant_command_schema_key().map_err(schema_error)? {
        evaluate_add_participant(request)
    } else if schema == &chat_send_message_command_schema_key().map_err(schema_error)? {
        evaluate_send_message(request)
    } else if schema == &chat_edit_message_command_schema_key().map_err(schema_error)? {
        evaluate_edit_message(request)
    } else if schema == &chat_select_branch_command_schema_key().map_err(schema_error)? {
        evaluate_select_branch(request)
    } else if schema == &chat_request_alternative_command_schema_key().map_err(schema_error)? {
        evaluate_request_alternative(request)
    } else {
        Err(EvaluationError::Rejected(DIAGNOSTIC_INVALID_COMMAND))
    }
}

fn evaluate_create_conversation(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: CreateConversationCommand = decode_payload(request)?;
    validate_title(&command.title).map_err(invalid_model)?;
    let world_index_schema = chat_world_index_schema_key().map_err(schema_error)?;
    if !has_facet_result(request, WorldSystemFacetTarget::World, &world_index_schema) {
        return Ok(read_response(vec![WorldSystemReadRequest::Facet {
            target: WorldSystemFacetTarget::World,
            schema: world_index_schema,
        }]));
    }

    let conversation_id = command_entity_id(request.command.id);
    let mut world_index: ChatWorldIndex =
        optional_facet(request, WorldSystemFacetTarget::World, &world_index_schema)?
            .unwrap_or_default();
    world_index.conversations.push(conversation_id);
    world_index.validate().map_err(invalid_model)?;

    let mut transaction = WorldSystemTransaction::new();
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: conversation_id,
        schema: conversation_entity_schema_key().map_err(schema_error)?,
    });
    push_world_facet(&mut transaction, world_index_schema, &world_index)?;
    push_facet(
        &mut transaction,
        conversation_id,
        conversation_state_schema_key().map_err(schema_error)?,
        &ConversationState {
            title: command.title,
            selected_leaf: None,
            participant_ids: Vec::new(),
            message_ids: Vec::new(),
        },
    )?;
    push_event(
        &mut transaction,
        conversation_created_event_schema_key().map_err(schema_error)?,
        &ConversationCreatedEvent { conversation_id },
    )?;
    Ok(transaction_response(transaction))
}

fn evaluate_add_participant(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: AddParticipantCommand = decode_payload(request)?;
    validate_participant_name(&command.display_name).map_err(invalid_model)?;
    if request.reads.is_empty() {
        let mut reads = vec![
            WorldSystemReadRequest::Entity {
                entity_id: command.conversation_id,
            },
            facet_read(
                command.conversation_id,
                conversation_state_schema_key().map_err(schema_error)?,
            ),
        ];
        if let ParticipantBindingRequest::Entity { entity_id } = command.binding {
            reads.push(WorldSystemReadRequest::Entity { entity_id });
        }
        return Ok(read_response(reads));
    }

    expect_entity(
        request,
        command.conversation_id,
        &conversation_entity_schema_key().map_err(schema_error)?,
    )?;
    let binding = match command.binding {
        ParticipantBindingRequest::CurrentPrincipal => {
            require_direct_principal(request)?;
            ParticipantBinding::Principal {
                principal_id: request.command.principal,
            }
        }
        ParticipantBindingRequest::Entity { entity_id } => {
            expect_any_entity(request, entity_id)?;
            if request.command.actor != WorldSystemActor::Entity(entity_id) {
                return Err(EvaluationError::Rejected(DIAGNOSTIC_UNAUTHORIZED));
            }
            ParticipantBinding::Entity { entity_id }
        }
        ParticipantBindingRequest::Uncontrolled => {
            require_direct_principal(request)?;
            ParticipantBinding::Uncontrolled
        }
    };

    let participant_id = command_entity_id(request.command.id);
    let mut conversation: ConversationState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.conversation_id),
        &conversation_state_schema_key().map_err(schema_error)?,
    )?;
    conversation.participant_ids.push(participant_id);
    conversation.validate().map_err(invalid_model)?;

    let mut transaction = WorldSystemTransaction::new();
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: participant_id,
        schema: participant_entity_schema_key().map_err(schema_error)?,
    });
    push_facet(
        &mut transaction,
        participant_id,
        participant_identity_schema_key().map_err(schema_error)?,
        &ParticipantIdentity {
            display_name: command.display_name,
            binding,
        },
    )?;
    push_facet(
        &mut transaction,
        command.conversation_id,
        conversation_state_schema_key().map_err(schema_error)?,
        &conversation,
    )?;
    push_relation(
        &mut transaction,
        "conversation-participant",
        conversation_participant_schema_key().map_err(schema_error)?,
        command.conversation_id,
        participant_id,
    );
    push_event(
        &mut transaction,
        participant_added_event_schema_key().map_err(schema_error)?,
        &ParticipantAddedEvent {
            conversation_id: command.conversation_id,
            participant_id,
        },
    )?;
    Ok(transaction_response(transaction))
}

fn evaluate_send_message(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: SendMessageCommand = decode_payload(request)?;
    validate_blocks(&command.blocks).map_err(invalid_model)?;
    if command.parent_message_id.is_some() && command.parent_message_id == command.alternative_of {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_INVALID_COMMAND));
    }
    if request.reads.is_empty() {
        return Ok(read_response(send_message_reads(&command)?));
    }

    expect_entity(
        request,
        command.conversation_id,
        &conversation_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_entity(
        request,
        command.participant_id,
        &participant_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_relation(
        request,
        relation_id(
            "conversation-participant",
            &[command.conversation_id, command.participant_id],
        ),
        &conversation_participant_schema_key().map_err(schema_error)?,
        command.conversation_id,
        command.participant_id,
    )?;
    let identity: ParticipantIdentity = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.participant_id),
        &participant_identity_schema_key().map_err(schema_error)?,
    )?;
    authorize_participant(request, &identity.binding)?;
    let mut conversation: ConversationState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.conversation_id),
        &conversation_state_schema_key().map_err(schema_error)?,
    )?;
    validate_message_reference(request, command.parent_message_id, command.conversation_id)?;
    if let Some(alternative) =
        validate_message_reference(request, command.alternative_of, command.conversation_id)?
        && (alternative.author_participant_id != command.participant_id
            || alternative.parent_message_id != command.parent_message_id)
    {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_INVALID_COMMAND));
    }

    let message_id = command_entity_id(request.command.id);
    let revision_id = derived_entity_id(request.command.id, "initial-revision");
    let mut transaction = WorldSystemTransaction::new();
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: message_id,
        schema: message_entity_schema_key().map_err(schema_error)?,
    });
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: revision_id,
        schema: message_revision_entity_schema_key().map_err(schema_error)?,
    });
    push_facet(
        &mut transaction,
        message_id,
        message_state_schema_key().map_err(schema_error)?,
        &MessageState {
            conversation_id: command.conversation_id,
            author_participant_id: command.participant_id,
            parent_message_id: command.parent_message_id,
            alternative_of: command.alternative_of,
            current_revision: revision_id,
        },
    )?;
    push_facet(
        &mut transaction,
        revision_id,
        message_content_schema_key().map_err(schema_error)?,
        &MessageContent {
            blocks: command.blocks,
            effective_at: request.command.effective_at,
        },
    )?;
    push_relation(
        &mut transaction,
        "conversation-message",
        conversation_message_schema_key().map_err(schema_error)?,
        command.conversation_id,
        message_id,
    );
    push_relation(
        &mut transaction,
        "message-author",
        message_author_schema_key().map_err(schema_error)?,
        message_id,
        command.participant_id,
    );
    push_relation(
        &mut transaction,
        "message-revision",
        message_revision_schema_key().map_err(schema_error)?,
        message_id,
        revision_id,
    );
    if let Some(parent_id) = command.parent_message_id {
        push_relation(
            &mut transaction,
            "message-parent",
            message_parent_schema_key().map_err(schema_error)?,
            message_id,
            parent_id,
        );
    }
    if let Some(alternative_id) = command.alternative_of {
        push_relation(
            &mut transaction,
            "message-variant",
            message_variant_schema_key().map_err(schema_error)?,
            message_id,
            alternative_id,
        );
    }
    conversation.message_ids.push(message_id);
    conversation.selected_leaf = Some(message_id);
    conversation.validate().map_err(invalid_model)?;
    push_facet(
        &mut transaction,
        command.conversation_id,
        conversation_state_schema_key().map_err(schema_error)?,
        &conversation,
    )?;
    push_event(
        &mut transaction,
        message_added_event_schema_key().map_err(schema_error)?,
        &MessageAddedEvent {
            conversation_id: command.conversation_id,
            message_id,
            revision_id,
            participant_id: command.participant_id,
            parent_message_id: command.parent_message_id,
            alternative_of: command.alternative_of,
        },
    )?;
    Ok(transaction_response(transaction))
}

fn evaluate_edit_message(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: EditMessageCommand = decode_payload(request)?;
    validate_blocks(&command.blocks).map_err(invalid_model)?;
    let message_schema = message_entity_schema_key().map_err(schema_error)?;
    let state_schema = message_state_schema_key().map_err(schema_error)?;
    if request.reads.is_empty() {
        return Ok(read_response(vec![
            WorldSystemReadRequest::Entity {
                entity_id: command.message_id,
            },
            facet_read(command.message_id, state_schema.clone()),
        ]));
    }
    expect_entity(request, command.message_id, &message_schema)?;
    let state: MessageState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.message_id),
        &state_schema,
    )?;
    let identity_schema = participant_identity_schema_key().map_err(schema_error)?;
    if !has_facet_result(
        request,
        WorldSystemFacetTarget::Entity(state.author_participant_id),
        &identity_schema,
    ) {
        return Ok(read_response(vec![
            WorldSystemReadRequest::Entity {
                entity_id: state.author_participant_id,
            },
            WorldSystemReadRequest::Relation {
                relation_id: relation_id(
                    "conversation-participant",
                    &[state.conversation_id, state.author_participant_id],
                ),
            },
            facet_read(state.author_participant_id, identity_schema),
        ]));
    }
    expect_entity(
        request,
        state.author_participant_id,
        &participant_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_relation(
        request,
        relation_id(
            "conversation-participant",
            &[state.conversation_id, state.author_participant_id],
        ),
        &conversation_participant_schema_key().map_err(schema_error)?,
        state.conversation_id,
        state.author_participant_id,
    )?;
    let identity: ParticipantIdentity = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(state.author_participant_id),
        &participant_identity_schema_key().map_err(schema_error)?,
    )?;
    authorize_participant(request, &identity.binding)?;

    let revision_id = derived_entity_id(request.command.id, "edited-revision");
    let mut transaction = WorldSystemTransaction::new();
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: revision_id,
        schema: message_revision_entity_schema_key().map_err(schema_error)?,
    });
    push_facet(
        &mut transaction,
        revision_id,
        message_content_schema_key().map_err(schema_error)?,
        &MessageContent {
            blocks: command.blocks,
            effective_at: request.command.effective_at,
        },
    )?;
    push_facet(
        &mut transaction,
        command.message_id,
        state_schema,
        &MessageState {
            current_revision: revision_id,
            ..state.clone()
        },
    )?;
    push_relation(
        &mut transaction,
        "message-revision",
        message_revision_schema_key().map_err(schema_error)?,
        command.message_id,
        revision_id,
    );
    push_event(
        &mut transaction,
        message_edited_event_schema_key().map_err(schema_error)?,
        &MessageEditedEvent {
            message_id: command.message_id,
            previous_revision_id: state.current_revision,
            revision_id,
        },
    )?;
    Ok(transaction_response(transaction))
}

fn evaluate_select_branch(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: SelectBranchCommand = decode_payload(request)?;
    if request.reads.is_empty() {
        return Ok(read_response(vec![
            WorldSystemReadRequest::Entity {
                entity_id: command.conversation_id,
            },
            WorldSystemReadRequest::Entity {
                entity_id: command.message_id,
            },
            WorldSystemReadRequest::Relation {
                relation_id: relation_id(
                    "conversation-message",
                    &[command.conversation_id, command.message_id],
                ),
            },
            facet_read(
                command.conversation_id,
                conversation_state_schema_key().map_err(schema_error)?,
            ),
        ]));
    }
    expect_entity(
        request,
        command.conversation_id,
        &conversation_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_entity(
        request,
        command.message_id,
        &message_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_relation(
        request,
        relation_id(
            "conversation-message",
            &[command.conversation_id, command.message_id],
        ),
        &conversation_message_schema_key().map_err(schema_error)?,
        command.conversation_id,
        command.message_id,
    )?;
    let mut state: ConversationState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.conversation_id),
        &conversation_state_schema_key().map_err(schema_error)?,
    )?;
    require_direct_principal(request)?;
    state.selected_leaf = Some(command.message_id);
    state.validate().map_err(invalid_model)?;
    let mut transaction = WorldSystemTransaction::new();
    push_facet(
        &mut transaction,
        command.conversation_id,
        conversation_state_schema_key().map_err(schema_error)?,
        &state,
    )?;
    push_event(
        &mut transaction,
        branch_selected_event_schema_key().map_err(schema_error)?,
        &BranchSelectedEvent {
            conversation_id: command.conversation_id,
            message_id: command.message_id,
        },
    )?;
    Ok(transaction_response(transaction))
}

fn evaluate_request_alternative(
    request: &WorldSystemServiceRequest,
) -> Result<WorldSystemServiceResponse, EvaluationError> {
    let command: RequestAlternativeCommand = decode_payload(request)?;
    if request.reads.is_empty() {
        return Ok(read_response(vec![
            WorldSystemReadRequest::Entity {
                entity_id: command.conversation_id,
            },
            WorldSystemReadRequest::Entity {
                entity_id: command.message_id,
            },
            WorldSystemReadRequest::Entity {
                entity_id: command.participant_id,
            },
            WorldSystemReadRequest::Relation {
                relation_id: relation_id(
                    "conversation-message",
                    &[command.conversation_id, command.message_id],
                ),
            },
            WorldSystemReadRequest::Relation {
                relation_id: relation_id(
                    "conversation-participant",
                    &[command.conversation_id, command.participant_id],
                ),
            },
            facet_read(
                command.message_id,
                message_state_schema_key().map_err(schema_error)?,
            ),
        ]));
    }
    require_direct_principal(request)?;
    expect_entity(
        request,
        command.conversation_id,
        &conversation_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_entity(
        request,
        command.message_id,
        &message_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_entity(
        request,
        command.participant_id,
        &participant_entity_schema_key().map_err(schema_error)?,
    )?;
    expect_relation(
        request,
        relation_id(
            "conversation-message",
            &[command.conversation_id, command.message_id],
        ),
        &conversation_message_schema_key().map_err(schema_error)?,
        command.conversation_id,
        command.message_id,
    )?;
    expect_relation(
        request,
        relation_id(
            "conversation-participant",
            &[command.conversation_id, command.participant_id],
        ),
        &conversation_participant_schema_key().map_err(schema_error)?,
        command.conversation_id,
        command.participant_id,
    )?;
    let state: MessageState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(command.message_id),
        &message_state_schema_key().map_err(schema_error)?,
    )?;
    if state.conversation_id != command.conversation_id
        || state.author_participant_id != command.participant_id
    {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE));
    }

    let mut transaction = WorldSystemTransaction::new();
    push_event(
        &mut transaction,
        alternative_requested_event_schema_key().map_err(schema_error)?,
        &AlternativeRequestedEvent {
            conversation_id: command.conversation_id,
            message_id: command.message_id,
            participant_id: command.participant_id,
        },
    )?;
    Ok(transaction_response(transaction))
}

fn send_message_reads(
    command: &SendMessageCommand,
) -> Result<Vec<WorldSystemReadRequest>, EvaluationError> {
    let mut reads = vec![
        WorldSystemReadRequest::Entity {
            entity_id: command.conversation_id,
        },
        WorldSystemReadRequest::Entity {
            entity_id: command.participant_id,
        },
        WorldSystemReadRequest::Relation {
            relation_id: relation_id(
                "conversation-participant",
                &[command.conversation_id, command.participant_id],
            ),
        },
        facet_read(
            command.participant_id,
            participant_identity_schema_key().map_err(schema_error)?,
        ),
        facet_read(
            command.conversation_id,
            conversation_state_schema_key().map_err(schema_error)?,
        ),
    ];
    for message_id in [command.parent_message_id, command.alternative_of]
        .into_iter()
        .flatten()
    {
        reads.push(WorldSystemReadRequest::Entity {
            entity_id: message_id,
        });
        reads.push(facet_read(
            message_id,
            message_state_schema_key().map_err(schema_error)?,
        ));
    }
    Ok(reads)
}

fn validate_message_reference(
    request: &WorldSystemServiceRequest,
    message_id: Option<EntityId>,
    conversation_id: EntityId,
) -> Result<Option<MessageState>, EvaluationError> {
    let Some(message_id) = message_id else {
        return Ok(None);
    };
    expect_entity(
        request,
        message_id,
        &message_entity_schema_key().map_err(schema_error)?,
    )?;
    let state: MessageState = expect_facet(
        request,
        WorldSystemFacetTarget::Entity(message_id),
        &message_state_schema_key().map_err(schema_error)?,
    )?;
    if state.conversation_id != conversation_id {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE));
    }
    Ok(Some(state))
}

fn authorize_participant(
    request: &WorldSystemServiceRequest,
    binding: &ParticipantBinding,
) -> Result<(), EvaluationError> {
    match binding {
        ParticipantBinding::Principal { principal_id }
            if *principal_id == request.command.principal
                && request.command.actor == WorldSystemActor::Principal(*principal_id) =>
        {
            Ok(())
        }
        ParticipantBinding::Entity { entity_id }
            if request.command.actor == WorldSystemActor::Entity(*entity_id) =>
        {
            Ok(())
        }
        _ => Err(EvaluationError::Rejected(DIAGNOSTIC_UNAUTHORIZED)),
    }
}

fn require_direct_principal(request: &WorldSystemServiceRequest) -> Result<(), EvaluationError> {
    if request.command.actor == WorldSystemActor::Principal(request.command.principal) {
        Ok(())
    } else {
        Err(EvaluationError::Rejected(DIAGNOSTIC_UNAUTHORIZED))
    }
}

fn decode_payload<T: DeserializeOwned>(
    request: &WorldSystemServiceRequest,
) -> Result<T, EvaluationError> {
    serde_json::from_value(request.command.payload.clone())
        .map_err(|_| EvaluationError::Rejected(DIAGNOSTIC_INVALID_COMMAND))
}

fn facet_read(entity_id: EntityId, schema: SchemaKey) -> WorldSystemReadRequest {
    WorldSystemReadRequest::Facet {
        target: WorldSystemFacetTarget::Entity(entity_id),
        schema,
    }
}

fn expect_any_entity(
    request: &WorldSystemServiceRequest,
    entity_id: EntityId,
) -> Result<&WorldSystemEntityRecord, EvaluationError> {
    request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldSystemReadResult::Entity {
                entity_id: actual,
                value,
            } if *actual == entity_id => Some(value.as_ref()),
            _ => None,
        })
        .ok_or(EvaluationError::Failed(DIAGNOSTIC_TRANSPORT))?
        .ok_or(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE))
}

fn expect_entity<'a>(
    request: &'a WorldSystemServiceRequest,
    entity_id: EntityId,
    schema: &SchemaKey,
) -> Result<&'a WorldSystemEntityRecord, EvaluationError> {
    let entity = expect_any_entity(request, entity_id)?;
    if &entity.schema != schema {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE));
    }
    Ok(entity)
}

fn expect_relation<'a>(
    request: &'a WorldSystemServiceRequest,
    relation_id: RelationId,
    schema: &SchemaKey,
    from: EntityId,
    to: EntityId,
) -> Result<&'a WorldSystemRelationRecord, EvaluationError> {
    let relation = request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldSystemReadResult::Relation {
                relation_id: actual,
                value,
            } if *actual == relation_id => Some(value.as_ref()),
            _ => None,
        })
        .ok_or(EvaluationError::Failed(DIAGNOSTIC_TRANSPORT))?
        .ok_or(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
    if &relation.schema != schema || relation.from != from || relation.to != to {
        return Err(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE));
    }
    Ok(relation)
}

fn has_facet_result(
    request: &WorldSystemServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> bool {
    request.reads.iter().any(|result| {
        matches!(
            result,
            WorldSystemReadResult::Facet {
                target: actual_target,
                schema: actual_schema,
                ..
            } if *actual_target == target && actual_schema == schema
        )
    })
}

fn expect_facet<T: DeserializeOwned>(
    request: &WorldSystemServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> Result<T, EvaluationError> {
    let facet = request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldSystemReadResult::Facet {
                target: actual_target,
                schema: actual_schema,
                value,
            } if *actual_target == target && actual_schema == schema => Some(value.as_ref()),
            _ => None,
        })
        .ok_or(EvaluationError::Failed(DIAGNOSTIC_TRANSPORT))?
        .ok_or(EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE))?;
    decode_facet(facet)
}

fn decode_facet<T: DeserializeOwned>(facet: &WorldSystemFacetRecord) -> Result<T, EvaluationError> {
    serde_json::from_value(facet.payload.clone())
        .map_err(|_| EvaluationError::Rejected(DIAGNOSTIC_MISSING_STATE))
}

fn optional_facet<T: DeserializeOwned>(
    request: &WorldSystemServiceRequest,
    target: WorldSystemFacetTarget,
    schema: &SchemaKey,
) -> Result<Option<T>, EvaluationError> {
    let value = request
        .reads
        .iter()
        .find_map(|result| match result {
            WorldSystemReadResult::Facet {
                target: actual_target,
                schema: actual_schema,
                value,
            } if *actual_target == target && actual_schema == schema => Some(value.as_ref()),
            _ => None,
        })
        .ok_or(EvaluationError::Failed(DIAGNOSTIC_TRANSPORT))?;
    value.map(decode_facet).transpose()
}

fn push_world_facet<T: Serialize>(
    transaction: &mut WorldSystemTransaction,
    schema: SchemaKey,
    value: &T,
) -> Result<(), EvaluationError> {
    let payload =
        serde_json::to_value(value).map_err(|_| EvaluationError::Failed(DIAGNOSTIC_SCHEMA))?;
    transaction.push_mutation(WorldSystemMutation::SetFacet {
        target: WorldSystemFacetTarget::World,
        schema,
        payload,
    });
    Ok(())
}

fn push_facet<T: Serialize>(
    transaction: &mut WorldSystemTransaction,
    entity_id: EntityId,
    schema: SchemaKey,
    value: &T,
) -> Result<(), EvaluationError> {
    let payload =
        serde_json::to_value(value).map_err(|_| EvaluationError::Failed(DIAGNOSTIC_SCHEMA))?;
    transaction.push_mutation(WorldSystemMutation::SetFacet {
        target: WorldSystemFacetTarget::Entity(entity_id),
        schema,
        payload,
    });
    Ok(())
}

fn push_relation(
    transaction: &mut WorldSystemTransaction,
    tag: &str,
    schema: SchemaKey,
    from: EntityId,
    to: EntityId,
) {
    transaction.push_mutation(WorldSystemMutation::CreateRelation {
        relation_id: relation_id(tag, &[from, to]),
        schema,
        from,
        to,
    });
}

fn push_event<T: Serialize>(
    transaction: &mut WorldSystemTransaction,
    schema: SchemaKey,
    value: &T,
) -> Result<(), EvaluationError> {
    let payload =
        serde_json::to_value(value).map_err(|_| EvaluationError::Failed(DIAGNOSTIC_SCHEMA))?;
    transaction.push_event(WorldSystemEventProposal { schema, payload });
    Ok(())
}

fn read_response(requests: Vec<WorldSystemReadRequest>) -> WorldSystemServiceResponse {
    WorldSystemServiceResponse::Read { requests }
}

fn transaction_response(transaction: WorldSystemTransaction) -> WorldSystemServiceResponse {
    WorldSystemServiceResponse::Transaction { transaction }
}

fn invalid_model(_error: crate::ChatModelError) -> EvaluationError {
    EvaluationError::Rejected(DIAGNOSTIC_INVALID_COMMAND)
}

fn schema_error(_error: crate::schemas::ChatSchemaError) -> EvaluationError {
    EvaluationError::Failed(DIAGNOSTIC_SCHEMA)
}

#[derive(Debug, Clone, Copy)]
enum EvaluationError {
    Rejected(&'static str),
    Failed(&'static str),
}
