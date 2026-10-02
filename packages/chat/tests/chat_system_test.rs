//! Integration tests for Chat schemas and public World System evaluation.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use rintawa_chat::{
    AddParticipantCommand, BootstrapConversationCommand, ContentBlock, ConversationState,
    CreateConversationCommand, DeleteMessageCommand, EditMessageCommand, MessageContent,
    MessageState, ParticipantBinding, ParticipantBindingRequest, ParticipantIdentity,
    SendMessageCommand, chat_add_participant_command_schema_key, chat_all_world_schemas,
    chat_bootstrap_conversation_command_schema_key, chat_create_conversation_command_schema_key,
    chat_delete_message_command_schema_key, chat_edit_message_command_schema_key,
    chat_send_message_command_schema_key, chat_surface_contribution, evaluate_chat_world_system,
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

#[test]
fn test_should_keep_chat_surface_out_of_global_activity_navigation() {
    let contribution = chat_surface_contribution();
    assert!(contribution.activity.is_none());
    assert_eq!(contribution.id.as_str(), "rintawa.chat.main");
}

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
    assert_eq!(schemas.len(), 31);
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
fn test_should_bootstrap_primary_conversation_without_granting_character_authority() -> Result<()> {
    let principal = PrincipalId::new();
    let character_entity_id = EntityId::new();
    let request = request(
        chat_bootstrap_conversation_command_schema_key()?,
        serde_json::to_value(BootstrapConversationCommand {
            title: Some(String::from("Alice")),
            character_entity_id,
            character_display_name: String::from("Alice"),
            greeting: Some(String::from("Hello **there**.")),
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        Vec::new(),
    );
    let requested = reads(evaluate_chat_world_system(&request))?;
    assert_eq!(requested.len(), 2);

    let request = with_reads(
        &request,
        vec![
            world_facet_result::<rintawa_chat::ChatWorldIndex>("rintawa.chat.world-index@1", None)?,
            entity_result(character_entity_id, "rintawa.character@1")?,
        ],
    );
    let transaction = transaction(evaluate_chat_world_system(&request))?;
    let conversation_id = created_entity(&transaction, "rintawa.chat.conversation@1")?;
    let conversation: ConversationState = facet_payload(
        &transaction,
        conversation_id,
        "rintawa.chat.conversation-state@1",
    )?;
    assert_eq!(conversation.title.as_deref(), Some("Alice"));
    assert_eq!(conversation.participant_ids.len(), 1);
    assert_eq!(conversation.message_ids.len(), 1);
    assert_eq!(
        conversation.selected_leaf,
        conversation.message_ids.first().copied()
    );

    let identities = conversation
        .participant_ids
        .iter()
        .map(|participant_id| {
            facet_payload::<ParticipantIdentity>(
                &transaction,
                *participant_id,
                "rintawa.chat.participant-identity@1",
            )
        })
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(identities.len(), 1);
    assert!(matches!(
        identities[0].binding,
        ParticipantBinding::Entity { entity_id } if entity_id == character_entity_id
    ));

    let message_id = conversation.message_ids[0];
    let message: MessageState =
        facet_payload(&transaction, message_id, "rintawa.chat.message-state@1")?;
    let character_participant_id = conversation
        .participant_ids
        .iter()
        .copied()
        .find(|participant_id| {
            facet_payload::<ParticipantIdentity>(
                &transaction,
                *participant_id,
                "rintawa.chat.participant-identity@1",
            )
            .is_ok_and(|identity| matches!(identity.binding, ParticipantBinding::Entity { .. }))
        })
        .context("expected entity-bound participant")?;
    assert_eq!(message.author_participant_id, character_participant_id);
    let content: MessageContent = facet_payload(
        &transaction,
        message.current_revision,
        "rintawa.chat.message-content@1",
    )?;
    assert_eq!(
        content.blocks,
        vec![ContentBlock::Markdown {
            markdown: String::from("Hello **there**."),
        }]
    );
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
fn test_should_logically_delete_selected_leaf_and_restore_parent_selection() -> Result<()> {
    let principal = PrincipalId::new();
    let conversation_id = EntityId::new();
    let participant_id = EntityId::new();
    let parent_id = EntityId::new();
    let leaf_id = EntityId::new();
    let parent_state = MessageState {
        conversation_id,
        author_participant_id: participant_id,
        parent_message_id: None,
        alternative_of: None,
        current_revision: EntityId::new(),
    };
    let leaf_state = MessageState {
        conversation_id,
        author_participant_id: participant_id,
        parent_message_id: Some(parent_id),
        alternative_of: None,
        current_revision: EntityId::new(),
    };
    let conversation = ConversationState {
        title: Some(String::from("Session")),
        selected_leaf: Some(leaf_id),
        participant_ids: vec![participant_id],
        message_ids: vec![parent_id, leaf_id],
    };
    let request = request(
        chat_delete_message_command_schema_key()?,
        serde_json::to_value(DeleteMessageCommand {
            message_id: leaf_id,
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        Vec::new(),
    );
    let first = reads(evaluate_chat_world_system(&request))?;
    assert_eq!(first.len(), 2);
    let first_round = vec![
        entity_result(leaf_id, "rintawa.chat.message@1")?,
        facet_result(leaf_id, "rintawa.chat.message-state@1", &leaf_state)?,
    ];
    let second = reads(evaluate_chat_world_system(&with_reads(
        &request,
        first_round.clone(),
    )))?;
    assert_eq!(second.len(), 2);
    let mut second_round = first_round;
    second_round.extend([
        entity_result(conversation_id, "rintawa.chat.conversation@1")?,
        facet_result(
            conversation_id,
            "rintawa.chat.conversation-state@1",
            &conversation,
        )?,
    ]);
    let third = reads(evaluate_chat_world_system(&with_reads(
        &request,
        second_round.clone(),
    )))?;
    assert_eq!(third.len(), 1);
    second_round.push(facet_result(
        parent_id,
        "rintawa.chat.message-state@1",
        &parent_state,
    )?);
    let transaction = transaction(evaluate_chat_world_system(&with_reads(
        &request,
        second_round,
    )))?;
    assert!(
        transaction
            .mutations
            .iter()
            .any(|mutation| matches!(mutation, WorldSystemMutation::DeleteRelation { .. }))
    );
    let updated: ConversationState = facet_payload(
        &transaction,
        conversation_id,
        "rintawa.chat.conversation-state@1",
    )?;
    assert_eq!(updated.message_ids, vec![parent_id]);
    assert_eq!(updated.selected_leaf, Some(parent_id));
    assert!(!transaction.mutations.iter().any(|mutation| matches!(
        mutation,
        WorldSystemMutation::DeleteEntity { entity_id } if *entity_id == leaf_id
    )));
    Ok(())
}

#[test]
fn test_should_reject_logical_delete_when_another_message_references_target() -> Result<()> {
    let principal = PrincipalId::new();
    let conversation_id = EntityId::new();
    let participant_id = EntityId::new();
    let target_id = EntityId::new();
    let child_id = EntityId::new();
    let target = MessageState {
        conversation_id,
        author_participant_id: participant_id,
        parent_message_id: None,
        alternative_of: None,
        current_revision: EntityId::new(),
    };
    let child = MessageState {
        conversation_id,
        author_participant_id: participant_id,
        parent_message_id: Some(target_id),
        alternative_of: None,
        current_revision: EntityId::new(),
    };
    let conversation = ConversationState {
        title: None,
        selected_leaf: Some(target_id),
        participant_ids: vec![participant_id],
        message_ids: vec![target_id, child_id],
    };
    let request = request(
        chat_delete_message_command_schema_key()?,
        serde_json::to_value(DeleteMessageCommand {
            message_id: target_id,
        })?,
        principal,
        WorldSystemActor::Principal(principal),
        vec![
            entity_result(target_id, "rintawa.chat.message@1")?,
            facet_result(target_id, "rintawa.chat.message-state@1", &target)?,
            entity_result(conversation_id, "rintawa.chat.conversation@1")?,
            facet_result(
                conversation_id,
                "rintawa.chat.conversation-state@1",
                &conversation,
            )?,
            facet_result(child_id, "rintawa.chat.message-state@1", &child)?,
        ],
    );
    assert!(matches!(
        evaluate_chat_world_system(&request),
        WorldSystemServiceResponse::Rejected { .. }
    ));
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
