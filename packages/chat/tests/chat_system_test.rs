//! Integration tests for Chat schemas and public World System evaluation.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use rintawa_chat::{
    AddParticipantCommand, ContentBlock, ConversationState, CreateConversationCommand,
    EditMessageCommand, MessageContent, MessageState, ParticipantBinding,
    ParticipantBindingRequest, ParticipantIdentity, SendMessageCommand,
    chat_add_participant_command_schema_key, chat_all_world_schemas,
    chat_create_conversation_command_schema_key, chat_edit_message_command_schema_key,
    chat_send_message_command_schema_key, evaluate_chat_world_system,
};
use rintawa_sdk::{
    world::{CommandId, CorrelationId, EntityId, PrincipalId, SchemaKey, WorldId},
    world_system::{
        WorldSystemActor, WorldSystemCommand, WorldSystemEntityRecord, WorldSystemFacetRecord,
        WorldSystemFacetTarget, WorldSystemMutation, WorldSystemReadRequest, WorldSystemReadResult,
        WorldSystemRelationRecord, WorldSystemServiceRequest, WorldSystemServiceResponse,
        WorldSystemTransaction,
    },
};

fn request(
    schema: SchemaKey,
    payload: serde_json::Value,
    principal: PrincipalId,
    actor: WorldSystemActor,
    reads: Vec<WorldSystemReadResult>,
) -> WorldSystemServiceRequest {
    WorldSystemServiceRequest {
        world_id: WorldId::new(),
        snapshot_position: 0,
        command: WorldSystemCommand {
            id: CommandId::new(),
            schema,
            principal,
            actor,
            expected_position: None,
            causation: None,
            correlation_id: CorrelationId::new(),
            effective_at: None,
            payload,
        },
        reads,
    }
}

fn with_reads(
    request: &WorldSystemServiceRequest,
    reads: Vec<WorldSystemReadResult>,
) -> WorldSystemServiceRequest {
    let mut next = request.clone();
    next.reads = reads;
    next
}

fn transaction(response: WorldSystemServiceResponse) -> Result<WorldSystemTransaction> {
    match response {
        WorldSystemServiceResponse::Transaction { transaction } => Ok(transaction),
        other => bail!("expected transaction response, got {other:?}"),
    }
}

fn reads(response: WorldSystemServiceResponse) -> Result<Vec<WorldSystemReadRequest>> {
    match response {
        WorldSystemServiceResponse::Read { requests } => Ok(requests),
        other => bail!("expected read response, got {other:?}"),
    }
}

fn parse_schema(value: &str) -> Result<SchemaKey> {
    value.parse().map_err(Into::into)
}

fn entity_result(id: EntityId, schema: &str) -> Result<WorldSystemReadResult> {
    Ok(WorldSystemReadResult::Entity {
        entity_id: id,
        value: Some(WorldSystemEntityRecord {
            id,
            schema: schema.parse()?,
        }),
    })
}

fn world_facet_result<T: serde::Serialize>(
    schema: &str,
    value: Option<&T>,
) -> Result<WorldSystemReadResult> {
    let schema: SchemaKey = schema.parse()?;
    let target = WorldSystemFacetTarget::World;
    Ok(WorldSystemReadResult::Facet {
        target,
        schema: schema.clone(),
        value: value
            .map(serde_json::to_value)
            .transpose()?
            .map(|payload| WorldSystemFacetRecord {
                target,
                schema,
                payload,
            }),
    })
}

fn facet_result<T: serde::Serialize>(
    entity_id: EntityId,
    schema: &str,
    value: &T,
) -> Result<WorldSystemReadResult> {
    let schema: SchemaKey = schema.parse()?;
    let target = WorldSystemFacetTarget::Entity(entity_id);
    Ok(WorldSystemReadResult::Facet {
        target,
        schema: schema.clone(),
        value: Some(WorldSystemFacetRecord {
            target,
            schema,
            payload: serde_json::to_value(value)?,
        }),
    })
}

fn requested_relation_id(
    requests: &[WorldSystemReadRequest],
) -> Result<rintawa_sdk::world::RelationId> {
    requests
        .iter()
        .find_map(|request| match request {
            WorldSystemReadRequest::Relation { relation_id } => Some(*relation_id),
            _ => None,
        })
        .context("expected relation read")
}

fn relation_result(
    relation_id: rintawa_sdk::world::RelationId,
    schema: &str,
    from: EntityId,
    to: EntityId,
) -> Result<WorldSystemReadResult> {
    Ok(WorldSystemReadResult::Relation {
        relation_id,
        value: Some(WorldSystemRelationRecord {
            id: relation_id,
            schema: schema.parse()?,
            from,
            to,
        }),
    })
}

fn created_entity(transaction: &WorldSystemTransaction, schema: &str) -> Result<EntityId> {
    let expected = parse_schema(schema)?;
    transaction
        .mutations
        .iter()
        .find_map(|mutation| match mutation {
            WorldSystemMutation::CreateEntity { entity_id, schema } if schema == &expected => {
                Some(*entity_id)
            }
            _ => None,
        })
        .context("expected created entity")
}

fn facet_payload<T: serde::de::DeserializeOwned>(
    transaction: &WorldSystemTransaction,
    entity_id: EntityId,
    schema: &str,
) -> Result<T> {
    let expected = parse_schema(schema)?;
    transaction
        .mutations
        .iter()
        .find_map(|mutation| match mutation {
            WorldSystemMutation::SetFacet {
                target: WorldSystemFacetTarget::Entity(actual),
                schema,
                payload,
            } if *actual == entity_id && schema == &expected => Some(payload.clone()),
            _ => None,
        })
        .context("expected facet mutation")
        .and_then(|payload| serde_json::from_value(payload).map_err(Into::into))
}

#[test]
fn test_should_publish_unique_versioned_chat_schemas() -> Result<()> {
    let schemas = chat_all_world_schemas()?;
    assert_eq!(schemas.len(), 28);
    let keys = schemas
        .iter()
        .map(|schema| schema.key().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.len(), schemas.len());
    for schema in schemas {
        let definition: serde_json::Value = serde_json::from_str(schema.definition_json())?;
        assert!(definition.is_object());
        assert_eq!(schema.key().version().get(), 1);
    }
    Ok(())
}

#[test]
fn test_should_create_conversation_without_private_persistence() -> Result<()> {
    let principal = PrincipalId::new();
    let request = request(
        chat_create_conversation_command_schema_key()?,
        serde_json::to_value(CreateConversationCommand {
            title: Some(String::from("Branching test")),
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        Vec::new(),
    );
    let initial_reads = reads(evaluate_chat_world_system(&request))?;
    assert_eq!(initial_reads.len(), 1);
    let request = with_reads(
        &request,
        vec![world_facet_result::<rintawa_chat::ChatWorldIndex>(
            "rintawa.chat.world-index@1",
            None,
        )?],
    );
    let transaction = transaction(evaluate_chat_world_system(&request))?;
    let conversation_id = created_entity(&transaction, "rintawa.chat.conversation@1")?;
    let state: ConversationState = facet_payload(
        &transaction,
        conversation_id,
        "rintawa.chat.conversation-state@1",
    )?;
    assert_eq!(state.title.as_deref(), Some("Branching test"));
    assert_eq!(state.selected_leaf, None);
    assert_eq!(transaction.events.len(), 1);
    Ok(())
}

#[test]
fn test_should_authorize_participant_send_and_preserve_message_revisions() -> Result<()> {
    let principal = PrincipalId::new();
    let actor = WorldSystemActor::Principal(principal);
    let conversation_id = EntityId::new();

    let add_request = request(
        chat_add_participant_command_schema_key()?,
        serde_json::to_value(AddParticipantCommand {
            conversation_id,
            display_name: String::from("Mart"),
            binding: ParticipantBindingRequest::CurrentPrincipal,
        })?,
        principal,
        actor,
        Vec::new(),
    );
    let add_reads = reads(evaluate_chat_world_system(&add_request))?;
    assert_eq!(add_reads.len(), 2);
    let add_request = with_reads(
        &add_request,
        vec![
            entity_result(conversation_id, "rintawa.chat.conversation@1")?,
            facet_result(
                conversation_id,
                "rintawa.chat.conversation-state@1",
                &ConversationState {
                    title: None,
                    selected_leaf: None,
                    participant_ids: Vec::new(),
                    message_ids: Vec::new(),
                },
            )?,
        ],
    );
    let add_transaction = transaction(evaluate_chat_world_system(&add_request))?;
    let participant_id = created_entity(&add_transaction, "rintawa.chat.participant@1")?;
    let identity: ParticipantIdentity = facet_payload(
        &add_transaction,
        participant_id,
        "rintawa.chat.participant-identity@1",
    )?;
    assert_eq!(
        identity.binding,
        ParticipantBinding::Principal {
            principal_id: principal
        }
    );

    let send_request = request(
        chat_send_message_command_schema_key()?,
        serde_json::to_value(SendMessageCommand {
            conversation_id,
            participant_id,
            parent_message_id: None,
            alternative_of: None,
            blocks: vec![ContentBlock::Text {
                text: String::from("Hello world"),
            }],
        })?,
        principal,
        actor,
        Vec::new(),
    );
    let send_reads = reads(evaluate_chat_world_system(&send_request))?;
    assert_eq!(send_reads.len(), 5);
    let membership_id = requested_relation_id(&send_reads)?;
    let send_request = with_reads(
        &send_request,
        vec![
            entity_result(conversation_id, "rintawa.chat.conversation@1")?,
            entity_result(participant_id, "rintawa.chat.participant@1")?,
            relation_result(
                membership_id,
                "rintawa.chat.conversation-participant@1",
                conversation_id,
                participant_id,
            )?,
            facet_result(
                participant_id,
                "rintawa.chat.participant-identity@1",
                &identity,
            )?,
            facet_result(
                conversation_id,
                "rintawa.chat.conversation-state@1",
                &ConversationState {
                    title: None,
                    selected_leaf: None,
                    participant_ids: vec![participant_id],
                    message_ids: Vec::new(),
                },
            )?,
        ],
    );
    let send_transaction = transaction(evaluate_chat_world_system(&send_request))?;
    let message_id = created_entity(&send_transaction, "rintawa.chat.message@1")?;
    let initial_revision = created_entity(&send_transaction, "rintawa.chat.message-revision@1")?;
    let message_state: MessageState = facet_payload(
        &send_transaction,
        message_id,
        "rintawa.chat.message-state@1",
    )?;
    assert_eq!(message_state.current_revision, initial_revision);
    assert_eq!(message_state.author_participant_id, participant_id);
    let content: MessageContent = facet_payload(
        &send_transaction,
        initial_revision,
        "rintawa.chat.message-content@1",
    )?;
    assert_eq!(
        content.blocks,
        vec![ContentBlock::Text {
            text: String::from("Hello world")
        }]
    );

    let edit_request = request(
        chat_edit_message_command_schema_key()?,
        serde_json::to_value(EditMessageCommand {
            message_id,
            blocks: vec![ContentBlock::Markdown {
                markdown: String::from("**edited**"),
            }],
        })?,
        principal,
        actor,
        Vec::new(),
    );
    let first_edit_reads = reads(evaluate_chat_world_system(&edit_request))?;
    assert_eq!(first_edit_reads.len(), 2);
    let first_round = vec![
        entity_result(message_id, "rintawa.chat.message@1")?,
        facet_result(message_id, "rintawa.chat.message-state@1", &message_state)?,
    ];
    let second_edit_reads = reads(evaluate_chat_world_system(&with_reads(
        &edit_request,
        first_round.clone(),
    )))?;
    assert_eq!(second_edit_reads.len(), 3);
    let membership_id = requested_relation_id(&second_edit_reads)?;
    let mut complete_reads = first_round;
    complete_reads.extend([
        entity_result(participant_id, "rintawa.chat.participant@1")?,
        relation_result(
            membership_id,
            "rintawa.chat.conversation-participant@1",
            conversation_id,
            participant_id,
        )?,
        facet_result(
            participant_id,
            "rintawa.chat.participant-identity@1",
            &identity,
        )?,
    ]);
    let edit_transaction = transaction(evaluate_chat_world_system(&with_reads(
        &edit_request,
        complete_reads,
    )))?;
    let edited_revision = created_entity(&edit_transaction, "rintawa.chat.message-revision@1")?;
    assert_ne!(edited_revision, initial_revision);
    assert!(!edit_transaction.mutations.iter().any(|mutation| matches!(
        mutation,
        WorldSystemMutation::DeleteEntity { entity_id } if *entity_id == initial_revision
    )));
    let edited_state: MessageState = facet_payload(
        &edit_transaction,
        message_id,
        "rintawa.chat.message-state@1",
    )?;
    assert_eq!(edited_state.current_revision, edited_revision);
    Ok(())
}

#[test]
fn test_should_reject_send_from_unrelated_principal() -> Result<()> {
    let principal = PrincipalId::new();
    let other = PrincipalId::new();
    let conversation_id = EntityId::new();
    let participant_id = EntityId::new();
    let request = request(
        chat_send_message_command_schema_key()?,
        serde_json::to_value(SendMessageCommand {
            conversation_id,
            participant_id,
            parent_message_id: None,
            alternative_of: None,
            blocks: vec![ContentBlock::Text {
                text: String::from("spoof"),
            }],
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        Vec::new(),
    );
    let requested = reads(evaluate_chat_world_system(&request))?;
    let membership_id = requested_relation_id(&requested)?;
    let response = evaluate_chat_world_system(&with_reads(
        &request,
        vec![
            entity_result(conversation_id, "rintawa.chat.conversation@1")?,
            entity_result(participant_id, "rintawa.chat.participant@1")?,
            relation_result(
                membership_id,
                "rintawa.chat.conversation-participant@1",
                conversation_id,
                participant_id,
            )?,
            facet_result(
                participant_id,
                "rintawa.chat.participant-identity@1",
                &ParticipantIdentity {
                    display_name: String::from("Other"),
                    binding: ParticipantBinding::Principal {
                        principal_id: other,
                    },
                },
            )?,
            facet_result(
                conversation_id,
                "rintawa.chat.conversation-state@1",
                &ConversationState {
                    title: None,
                    selected_leaf: None,
                    participant_ids: vec![participant_id],
                    message_ids: Vec::new(),
                },
            )?,
        ],
    ));
    assert!(matches!(
        response,
        WorldSystemServiceResponse::Rejected { .. }
    ));
    Ok(())
}

#[test]
fn test_should_bound_message_content_before_proposing_state() -> Result<()> {
    let principal = PrincipalId::new();
    let request = request(
        chat_send_message_command_schema_key()?,
        serde_json::to_value(SendMessageCommand {
            conversation_id: EntityId::new(),
            participant_id: EntityId::new(),
            parent_message_id: None,
            alternative_of: None,
            blocks: vec![ContentBlock::Text {
                text: "x".repeat(64 * 1024 + 1),
            }],
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        Vec::new(),
    );
    assert!(matches!(
        evaluate_chat_world_system(&request),
        WorldSystemServiceResponse::Rejected { .. }
    ));
    Ok(())
}
