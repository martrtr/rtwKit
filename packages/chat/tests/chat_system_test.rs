//! Exercise the real four-table World Runtime using Chat's System protocol.
//!
//! The same evaluator is called by the packaged WASM provider. No Chat-specific
//! storage path or native command handler is introduced into Core.

use anyhow::Result;
use rintawa_chat::{
    CONVERSATION_PROPERTY, CREATE_CONVERSATION_COMMAND, MESSAGE_PROPERTY, SEND_MESSAGE_COMMAND,
    chat_world_schemas, evaluate_chat_world_system,
};
use rintawa_sdk::{
    contracts::ContractKey,
    services::{ServiceCallError, ServiceCallResult},
    types::ExtensionId,
    world::{ItemId, PrincipalId, SchemaKey, SchemaKind, WorldId},
    world_system::WorldSystemServiceRequest,
};
use rintawa_storage::SqliteWorldStorage;
use rintawa_world::{PropertyDefinition, RuntimeSchemaDefinition, WorldCommand};
use rintawa_world_runtime::{ServiceWorldSystem, WorldRuntime, WorldRuntimeBuilder};
use serde_json::json;

fn key(value: &str) -> SchemaKey {
    value.parse().expect("known Chat schema key")
}

fn start_chat(path: &std::path::Path, world: WorldId) -> Result<WorldRuntime> {
    let storage = SqliteWorldStorage::open(path)?;
    let mut builder = WorldRuntimeBuilder::new(storage);
    let owner = ExtensionId::new("rintawa.rtwkit.chat");
    let mut property_definitions = Vec::new();

    for (schema, kind, definition) in chat_world_schemas() {
        if kind == SchemaKind::Property {
            property_definitions.push(PropertyDefinition::new(schema, owner.clone(), definition));
            continue;
        }
        builder.register_runtime_schema(RuntimeSchemaDefinition::new(
            schema.clone(),
            kind,
            owner.clone(),
            definition,
        ))?;
        builder.register_system(ServiceWorldSystem::new(
            world,
            schema,
            |_: &ContractKey, bytes: &[u8]| -> ServiceCallResult<Vec<u8>> {
                let request = serde_json::from_slice::<WorldSystemServiceRequest>(bytes)
                    .map_err(|_| ServiceCallError::Unavailable)?;
                serde_json::to_vec(&evaluate_chat_world_system(&request))
                    .map_err(|_| ServiceCallError::Unavailable)
            },
        ))?;
    }
    let runtime = builder.start()?;
    runtime.register_property_definitions(&property_definitions)?;
    Ok(runtime)
}

#[test]
fn test_should_write_chat_and_reopen_without_a_separate_index() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("world.sqlite");
    let world = WorldId::new();
    let principal = PrincipalId::new();
    SqliteWorldStorage::create(&path, world)?;

    let runtime = start_chat(&path, world)?;
    let conversation = ItemId::new();
    runtime
        .submit(WorldCommand::new(
            key(CREATE_CONVERSATION_COMMAND),
            principal,
            json!({"conversation_id": conversation, "title": "The Tavern"}),
        ))?
        .wait()?;

    let message = ItemId::new();
    let link = ItemId::new();
    runtime
        .submit(WorldCommand::new(
            key(SEND_MESSAGE_COMMAND),
            principal,
            json!({
                "conversation_id": conversation,
                "message_id": message,
                "relation_id": link,
                "text": "Hello from Chat"
            }),
        ))?
        .wait()?;

    let missing_conversation = ItemId::new();
    let rejected_message = ItemId::new();
    let rejected_link = ItemId::new();
    assert!(
        runtime
            .submit(WorldCommand::new(
                key(SEND_MESSAGE_COMMAND),
                principal,
                json!({
                    "conversation_id": missing_conversation,
                    "message_id": rejected_message,
                    "relation_id": rejected_link,
                    "text": "Must not commit"
                }),
            ))?
            .wait()
            .is_err()
    );

    let duplicate = runtime.submit(WorldCommand::new(
        key(SEND_MESSAGE_COMMAND),
        principal,
        json!({
            "conversation_id": conversation,
            "message_id": message,
            "relation_id": ItemId::new(),
            "text": "Must rollback"
        }),
    ))?;
    assert!(duplicate.wait().is_err());
    runtime.shutdown()?;

    let storage = SqliteWorldStorage::open(&path)?;
    assert_eq!(
        storage
            .load_property(conversation, &key(CONVERSATION_PROPERTY))?
            .expect("committed conversation")
            .data(),
        &json!({"title": "The Tavern"})
    );
    assert_eq!(
        storage
            .load_property(message, &key(MESSAGE_PROPERTY))?
            .expect("committed message")
            .data(),
        &json!({"role": "user", "text": "Hello from Chat"})
    );
    assert!(storage.load_item(rejected_message)?.is_none());
    assert!(storage.load_item(rejected_link)?.is_none());
    assert_eq!(
        storage
            .relations_touching_any(&[conversation], None, 32)?
            .len(),
        1
    );
    drop(storage);

    let reopened = start_chat(&path, world)?;
    let next_message = ItemId::new();
    reopened
        .submit(WorldCommand::new(
            key(SEND_MESSAGE_COMMAND),
            principal,
            json!({
                "conversation_id": conversation,
                "message_id": next_message,
                "relation_id": ItemId::new(),
                "text": "After restart"
            }),
        ))?
        .wait()?;
    reopened.shutdown()?;

    let storage = SqliteWorldStorage::open(&path)?;
    assert_eq!(
        storage
            .load_property(next_message, &key(MESSAGE_PROPERTY))?
            .expect("message after restart")
            .data(),
        &json!({"role": "user", "text": "After restart"})
    );
    let links = storage.relations_touching_any(&[conversation], None, 32)?;
    assert_eq!(links.len(), 2);
    assert!(
        links
            .iter()
            .all(|link| link.subject() == Some(conversation))
    );
    Ok(())
}
