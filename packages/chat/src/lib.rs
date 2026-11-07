//! Minimal persistent Chat domain for one authoritative four-table World.
//!
//! A conversation and each message are ordinary object-items. One relation-item
//! joins a conversation to each message. The World database is the only owner of
//! this state: Chat has no separate index, projection, log or durable worker.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

use rintawa_sdk::{
    world::{ItemId, SchemaKey, SchemaKind},
    world_system::{
        WorldSystemMutation, WorldSystemReadRequest, WorldSystemReadResult,
        WorldSystemServiceRequest, WorldSystemServiceResponse, WorldSystemTransaction,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Maximum title size in UTF-8 bytes.
pub const MAX_TITLE_BYTES: usize = 256;
/// Maximum text in one Chat message in UTF-8 bytes.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Versioned property storing the title of a conversation object.
pub const CONVERSATION_PROPERTY: &str = "chat.conversation@1";
/// Versioned property storing the content of a message object.
pub const MESSAGE_PROPERTY: &str = "chat.message@1";
/// Command that creates a conversation object.
pub const CREATE_CONVERSATION_COMMAND: &str = "chat.create-conversation@1";
/// Command that adds a human-authored message to a conversation.
pub const SEND_MESSAGE_COMMAND: &str = "chat.send-message@1";

fn key(value: &str) -> SchemaKey {
    value
        .parse()
        .expect("Chat's built-in schema names are valid")
}

/// The four property and command schemas declared by the Chat extension.
///
/// Ownership is assigned by the Host to the installed component. The two
/// property schemas become World vocabulary; the commands are runtime-only.
pub fn chat_world_schemas() -> Vec<(SchemaKey, SchemaKind, Value)> {
    vec![
        (
            key(CONVERSATION_PROPERTY),
            SchemaKind::Property,
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "minLength": 1, "maxLength": MAX_TITLE_BYTES }
                },
                "required": ["title"],
                "additionalProperties": false
            }),
        ),
        (
            key(MESSAGE_PROPERTY),
            SchemaKind::Property,
            json!({
                "type": "object",
                "properties": {
                    "role": { "const": "user" },
                    "text": { "type": "string", "minLength": 1, "maxLength": MAX_MESSAGE_BYTES }
                },
                "required": ["role", "text"],
                "additionalProperties": false
            }),
        ),
        (
            key(CREATE_CONVERSATION_COMMAND),
            SchemaKind::Command,
            json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "string", "format": "uuid" },
                    "title": { "type": "string", "minLength": 1, "maxLength": MAX_TITLE_BYTES }
                },
                "required": ["conversation_id", "title"],
                "additionalProperties": false
            }),
        ),
        (
            key(SEND_MESSAGE_COMMAND),
            SchemaKind::Command,
            json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "string", "format": "uuid" },
                    "message_id": { "type": "string", "format": "uuid" },
                    "relation_id": { "type": "string", "format": "uuid" },
                    "text": { "type": "string", "minLength": 1, "maxLength": MAX_MESSAGE_BYTES }
                },
                "required": ["conversation_id", "message_id", "relation_id", "text"],
                "additionalProperties": false
            }),
        ),
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateConversation {
    conversation_id: ItemId,
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendMessage {
    conversation_id: ItemId,
    message_id: ItemId,
    relation_id: ItemId,
    text: String,
}

fn reject(reason: &str) -> WorldSystemServiceResponse {
    WorldSystemServiceResponse::Rejected {
        reason: reason.to_owned(),
    }
}

fn valid_text(text: &str, maximum: usize) -> bool {
    !text.trim().is_empty() && text.len() <= maximum
}

/// Evaluates one Chat-owned World command against a pinned authoritative snapshot.
///
/// A send first asks the Host to read its conversation property. The next round
/// either rejects an absent conversation or proposes object, relation and
/// property creation in one atomic transaction. This function never writes SQLite
/// or invokes an external service.
///
/// A user command cannot claim to be an assistant message: the role is assigned
/// by Chat, not supplied in the command payload.
pub fn evaluate_chat_world_system(
    request: &WorldSystemServiceRequest,
) -> WorldSystemServiceResponse {
    match request.command.schema.to_string().as_str() {
        CREATE_CONVERSATION_COMMAND => {
            let Ok(command) =
                serde_json::from_value::<CreateConversation>(request.command.payload.clone())
            else {
                return reject("Invalid conversation request");
            };
            if !valid_text(&command.title, MAX_TITLE_BYTES) {
                return reject("Conversation title is empty or too long");
            }
            if !request.reads.is_empty() {
                return reject("Unexpected Chat snapshot reads");
            }
            let mut transaction = WorldSystemTransaction::new();
            transaction.push_mutation(WorldSystemMutation::CreateObject {
                item_id: command.conversation_id,
            });
            transaction.push_mutation(WorldSystemMutation::SetProperty {
                item_id: command.conversation_id,
                key: key(CONVERSATION_PROPERTY),
                data: json!({"title": command.title.trim()}),
            });
            WorldSystemServiceResponse::Transaction { transaction }
        }
        SEND_MESSAGE_COMMAND => {
            let Ok(command) =
                serde_json::from_value::<SendMessage>(request.command.payload.clone())
            else {
                return reject("Invalid message request");
            };
            if !valid_text(&command.text, MAX_MESSAGE_BYTES) {
                return reject("Message is empty or too long");
            }
            if command.conversation_id == command.message_id
                || command.conversation_id == command.relation_id
                || command.message_id == command.relation_id
            {
                return reject("Message identifiers must be distinct");
            }
            if request.reads.is_empty() {
                return WorldSystemServiceResponse::Read {
                    requests: vec![WorldSystemReadRequest::Property {
                        item_id: command.conversation_id,
                        key: key(CONVERSATION_PROPERTY),
                    }],
                };
            }
            let conversation_exists = matches!(
                request.reads.as_slice(),
                [WorldSystemReadResult::Property {
                    item_id,
                    key: property_key,
                    value: Some(_),
                }] if *item_id == command.conversation_id
                    && property_key == &key(CONVERSATION_PROPERTY)
            );
            if !conversation_exists {
                return reject("Conversation does not exist");
            }
            let mut transaction = WorldSystemTransaction::new();
            transaction.push_mutation(WorldSystemMutation::CreateObject {
                item_id: command.message_id,
            });
            transaction.push_mutation(WorldSystemMutation::CreateRelation {
                item_id: command.relation_id,
                subject: command.conversation_id,
                object: command.message_id,
            });
            transaction.push_mutation(WorldSystemMutation::SetProperty {
                item_id: command.message_id,
                key: key(MESSAGE_PROPERTY),
                data: json!({"role": "user", "text": command.text}),
            });
            WorldSystemServiceResponse::Transaction { transaction }
        }
        _ => reject("Unknown Chat command"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_sdk::{
        world::{PrincipalId, WorldId},
        world_system::WorldSystemCommand,
    };

    fn request(schema: &str, payload: Value) -> WorldSystemServiceRequest {
        WorldSystemServiceRequest {
            world_id: WorldId::new(),
            command: WorldSystemCommand {
                id: rintawa_sdk::world::CommandId::new(),
                schema: key(schema),
                principal: PrincipalId::new(),
                payload,
            },
            reads: Vec::new(),
        }
    }

    #[test]
    fn test_should_create_only_owned_conversation_state() {
        let id = ItemId::new();
        let result = evaluate_chat_world_system(&request(
            CREATE_CONVERSATION_COMMAND,
            json!({"conversation_id": id, "title": "  Story  "}),
        ));
        let WorldSystemServiceResponse::Transaction { transaction } = result else {
            panic!("valid conversation must produce a transaction");
        };
        assert_eq!(transaction.mutations.len(), 2);
        assert_eq!(
            transaction.mutations[1],
            WorldSystemMutation::SetProperty {
                item_id: id,
                key: key(CONVERSATION_PROPERTY),
                data: json!({"title": "Story"}),
            }
        );
    }

    #[test]
    fn test_should_fail_closed_on_message_impersonation_and_invalid_conversation() {
        let conversation = ItemId::new();
        let message = ItemId::new();
        let link = ItemId::new();
        let command = json!({
            "conversation_id": conversation,
            "message_id": message,
            "relation_id": link,
            "text": "Hello",
            "role": "assistant"
        });
        assert!(matches!(
            evaluate_chat_world_system(&request(SEND_MESSAGE_COMMAND, command)),
            WorldSystemServiceResponse::Rejected { .. }
        ));

        let command = json!({
            "conversation_id": conversation,
            "message_id": message,
            "relation_id": link,
            "text": "Hello"
        });
        let mut first_round = request(SEND_MESSAGE_COMMAND, command);
        assert!(matches!(
            evaluate_chat_world_system(&first_round),
            WorldSystemServiceResponse::Read { .. }
        ));
        first_round.reads.push(WorldSystemReadResult::Property {
            item_id: conversation,
            key: key(CONVERSATION_PROPERTY),
            value: None,
        });
        assert!(matches!(
            evaluate_chat_world_system(&first_round),
            WorldSystemServiceResponse::Rejected { .. }
        ));
    }
}
