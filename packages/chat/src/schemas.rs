//! Versioned Chat-owned World schema declarations.

use rintawa_sdk::{
    contributions::WorldSchemaContribution,
    world::{SchemaId, SchemaKey, SchemaKind, SchemaVersion},
};
use serde_json::{Value, json};
use thiserror::Error;

use crate::model::{
    MAX_ATTACHMENT_LABEL_BYTES, MAX_CHAT_TITLE_BYTES, MAX_MESSAGE_BLOCK_TEXT_BYTES,
    MAX_MESSAGE_BLOCKS, MAX_PARTICIPANT_NAME_BYTES,
};

const CONVERSATION_ENTITY: &str = "rintawa.chat.conversation";
const PARTICIPANT_ENTITY: &str = "rintawa.chat.participant";
const MESSAGE_ENTITY: &str = "rintawa.chat.message";
const MESSAGE_REVISION_ENTITY: &str = "rintawa.chat.message-revision";

const CHAT_WORLD_INDEX_FACET: &str = "rintawa.chat.world-index";
const CONVERSATION_STATE_FACET: &str = "rintawa.chat.conversation-state";
const PARTICIPANT_IDENTITY_FACET: &str = "rintawa.chat.participant-identity";
const MESSAGE_STATE_FACET: &str = "rintawa.chat.message-state";
const MESSAGE_CONTENT_FACET: &str = "rintawa.chat.message-content";

const CONVERSATION_PARTICIPANT_RELATION: &str = "rintawa.chat.conversation-participant";
const CONVERSATION_MESSAGE_RELATION: &str = "rintawa.chat.conversation-message";
const MESSAGE_AUTHOR_RELATION: &str = "rintawa.chat.message-author";
const MESSAGE_PARENT_RELATION: &str = "rintawa.chat.message-parent";
const MESSAGE_REVISION_RELATION: &str = "rintawa.chat.message-has-revision";
const MESSAGE_VARIANT_RELATION: &str = "rintawa.chat.message-variant";

const CREATE_CONVERSATION_COMMAND: &str = "rintawa.chat.create-conversation";
const ADD_PARTICIPANT_COMMAND: &str = "rintawa.chat.add-participant";
const SEND_MESSAGE_COMMAND: &str = "rintawa.chat.send-message";
const EDIT_MESSAGE_COMMAND: &str = "rintawa.chat.edit-message";
const SELECT_BRANCH_COMMAND: &str = "rintawa.chat.select-branch";
const REQUEST_ALTERNATIVE_COMMAND: &str = "rintawa.chat.request-alternative";

const CONVERSATION_CREATED_EVENT: &str = "rintawa.chat.conversation-created";
const PARTICIPANT_ADDED_EVENT: &str = "rintawa.chat.participant-added";
const MESSAGE_ADDED_EVENT: &str = "rintawa.chat.message-added";
const MESSAGE_EDITED_EVENT: &str = "rintawa.chat.message-edited";
const BRANCH_SELECTED_EVENT: &str = "rintawa.chat.branch-selected";
const ALTERNATIVE_REQUESTED_EVENT: &str = "rintawa.chat.alternative-requested";

const CONVERSATION_PROJECTION: &str = "rintawa.chat.conversation-view";

/// Failure while constructing package-owned Chat schema declarations.
#[derive(Debug, Error)]
pub enum ChatSchemaError {
    /// One compile-time schema identifier unexpectedly failed canonical validation.
    #[error("invalid package-owned Chat schema identity")]
    InvalidKey,
}

/// Returns every schema required by the first persistent Chat domain slice.
///
/// # Errors
///
/// Returns [`ChatSchemaError`] only if a compile-time package schema identifier is malformed.
pub fn chat_all_world_schemas() -> Result<Vec<WorldSchemaContribution>, ChatSchemaError> {
    let mut schemas = Vec::new();
    for id in [
        CONVERSATION_ENTITY,
        PARTICIPANT_ENTITY,
        MESSAGE_ENTITY,
        MESSAGE_REVISION_ENTITY,
    ] {
        schemas.push(contribution(id, SchemaKind::Entity, empty_object_schema())?);
    }
    schemas.extend([
        contribution(
            CHAT_WORLD_INDEX_FACET,
            SchemaKind::Facet,
            chat_world_index_schema(),
        )?,
        contribution(
            CONVERSATION_STATE_FACET,
            SchemaKind::Facet,
            conversation_state_schema(),
        )?,
        contribution(
            PARTICIPANT_IDENTITY_FACET,
            SchemaKind::Facet,
            participant_identity_schema(),
        )?,
        contribution(
            MESSAGE_STATE_FACET,
            SchemaKind::Facet,
            message_state_schema(),
        )?,
        contribution(
            MESSAGE_CONTENT_FACET,
            SchemaKind::Facet,
            message_content_schema(),
        )?,
    ]);
    for id in [
        CONVERSATION_PARTICIPANT_RELATION,
        CONVERSATION_MESSAGE_RELATION,
        MESSAGE_AUTHOR_RELATION,
        MESSAGE_PARENT_RELATION,
        MESSAGE_REVISION_RELATION,
        MESSAGE_VARIANT_RELATION,
    ] {
        schemas.push(contribution(
            id,
            SchemaKind::Relation,
            empty_object_schema(),
        )?);
    }
    schemas.extend([
        contribution(
            CREATE_CONVERSATION_COMMAND,
            SchemaKind::Command,
            create_conversation_schema(),
        )?,
        contribution(
            ADD_PARTICIPANT_COMMAND,
            SchemaKind::Command,
            add_participant_schema(),
        )?,
        contribution(
            SEND_MESSAGE_COMMAND,
            SchemaKind::Command,
            send_message_schema(),
        )?,
        contribution(
            EDIT_MESSAGE_COMMAND,
            SchemaKind::Command,
            edit_message_schema(),
        )?,
        contribution(
            SELECT_BRANCH_COMMAND,
            SchemaKind::Command,
            select_branch_schema(),
        )?,
        contribution(
            REQUEST_ALTERNATIVE_COMMAND,
            SchemaKind::Command,
            request_alternative_schema(),
        )?,
    ]);
    for id in [
        CONVERSATION_CREATED_EVENT,
        PARTICIPANT_ADDED_EVENT,
        MESSAGE_ADDED_EVENT,
        MESSAGE_EDITED_EVENT,
        BRANCH_SELECTED_EVENT,
        ALTERNATIVE_REQUESTED_EVENT,
    ] {
        schemas.push(contribution(id, SchemaKind::Event, event_schema(id))?);
    }
    schemas.push(contribution(
        CONVERSATION_PROJECTION,
        SchemaKind::Projection,
        conversation_projection_schema(),
    )?);
    Ok(schemas)
}

/// Returns the create-conversation command schema key.
pub fn chat_create_conversation_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CREATE_CONVERSATION_COMMAND)
}

/// Returns the add-participant command schema key.
pub fn chat_add_participant_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(ADD_PARTICIPANT_COMMAND)
}

/// Returns the send-message command schema key.
pub fn chat_send_message_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(SEND_MESSAGE_COMMAND)
}

/// Returns the edit-message command schema key.
pub fn chat_edit_message_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(EDIT_MESSAGE_COMMAND)
}

/// Returns the select-branch command schema key.
pub fn chat_select_branch_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(SELECT_BRANCH_COMMAND)
}

/// Returns the request-alternative command schema key.
pub fn chat_request_alternative_command_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(REQUEST_ALTERNATIVE_COMMAND)
}

/// Returns the policy-filtered conversation projection schema key.
pub fn chat_conversation_projection_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_PROJECTION)
}

pub(crate) fn chat_world_index_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CHAT_WORLD_INDEX_FACET)
}

pub(crate) fn conversation_entity_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_ENTITY)
}

pub(crate) fn participant_entity_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(PARTICIPANT_ENTITY)
}

pub(crate) fn message_entity_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_ENTITY)
}

pub(crate) fn message_revision_entity_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_REVISION_ENTITY)
}

pub(crate) fn conversation_state_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_STATE_FACET)
}

pub(crate) fn participant_identity_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(PARTICIPANT_IDENTITY_FACET)
}

pub(crate) fn message_state_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_STATE_FACET)
}

pub(crate) fn message_content_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_CONTENT_FACET)
}

pub(crate) fn conversation_participant_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_PARTICIPANT_RELATION)
}

pub(crate) fn conversation_message_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_MESSAGE_RELATION)
}

pub(crate) fn message_author_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_AUTHOR_RELATION)
}

pub(crate) fn message_parent_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_PARENT_RELATION)
}

pub(crate) fn message_revision_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_REVISION_RELATION)
}

pub(crate) fn message_variant_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_VARIANT_RELATION)
}

pub(crate) fn conversation_created_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(CONVERSATION_CREATED_EVENT)
}

pub(crate) fn participant_added_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(PARTICIPANT_ADDED_EVENT)
}

pub(crate) fn message_added_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_ADDED_EVENT)
}

pub(crate) fn message_edited_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(MESSAGE_EDITED_EVENT)
}

pub(crate) fn branch_selected_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(BRANCH_SELECTED_EVENT)
}

pub(crate) fn alternative_requested_event_schema_key() -> Result<SchemaKey, ChatSchemaError> {
    key(ALTERNATIVE_REQUESTED_EVENT)
}

fn contribution(
    id: &str,
    kind: SchemaKind,
    definition: Value,
) -> Result<WorldSchemaContribution, ChatSchemaError> {
    Ok(WorldSchemaContribution::new(
        key(id)?,
        kind,
        definition.to_string(),
    ))
}

fn key(id: &str) -> Result<SchemaKey, ChatSchemaError> {
    let id = SchemaId::parse(id).map_err(|_| ChatSchemaError::InvalidKey)?;
    let version = SchemaVersion::new(1).map_err(|_| ChatSchemaError::InvalidKey)?;
    Ok(SchemaKey::new(id, version))
}

fn empty_object_schema() -> Value {
    json!({ "type": "object", "additionalProperties": false })
}

fn uuid_schema() -> Value {
    json!({ "type": "string", "minLength": 36, "maxLength": 36 })
}

fn optional_uuid_schema() -> Value {
    let uuid = uuid_schema();
    json!({ "anyOf": [uuid, { "type": "null" }] })
}

fn asset_schema() -> Value {
    json!({
        "type": "object",
        "required": ["digest", "size", "media_type"],
        "properties": {
            "digest": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" },
            "size": { "type": "integer", "minimum": 0 },
            "media_type": { "type": "string", "minLength": 3, "maxLength": 127 }
        },
        "additionalProperties": false
    })
}

fn content_block_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type": "object",
                "required": ["kind", "text"],
                "properties": {
                    "kind": { "const": "text" },
                    "text": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_MESSAGE_BLOCK_TEXT_BYTES
                    }
                },
                "additionalProperties": false
            },
            {
                "type": "object",
                "required": ["kind", "markdown"],
                "properties": {
                    "kind": { "const": "markdown" },
                    "markdown": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_MESSAGE_BLOCK_TEXT_BYTES
                    }
                },
                "additionalProperties": false
            },
            {
                "type": "object",
                "required": ["kind", "asset"],
                "properties": {
                    "kind": { "const": "image-ref" },
                    "asset": asset_schema(),
                    "alt": {
                        "anyOf": [
                            { "type": "string", "maxLength": MAX_ATTACHMENT_LABEL_BYTES },
                            { "type": "null" }
                        ]
                    }
                },
                "additionalProperties": false
            },
            {
                "type": "object",
                "required": ["kind", "asset", "name"],
                "properties": {
                    "kind": { "const": "file-ref" },
                    "asset": asset_schema(),
                    "name": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_ATTACHMENT_LABEL_BYTES
                    }
                },
                "additionalProperties": false
            }
        ]
    })
}

fn chat_world_index_schema() -> Value {
    json!({
        "type": "object",
        "required": ["conversations"],
        "properties": {
            "conversations": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_CONVERSATIONS,
                "items": uuid_schema()
            }
        },
        "additionalProperties": false
    })
}

fn conversation_state_schema() -> Value {
    json!({
        "type": "object",
        "required": ["title", "selected-leaf", "participant-ids", "message-ids"],
        "properties": {
            "title": {
                "anyOf": [
                    { "type": "string", "maxLength": MAX_CHAT_TITLE_BYTES },
                    { "type": "null" }
                ]
            },
            "selected-leaf": optional_uuid_schema(),
            "participant-ids": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_PARTICIPANTS,
                "items": uuid_schema()
            },
            "message-ids": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_MESSAGES,
                "items": uuid_schema()
            }
        },
        "additionalProperties": false
    })
}

fn participant_identity_schema() -> Value {
    json!({
        "type": "object",
        "required": ["display-name", "binding"],
        "properties": {
            "display-name": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_PARTICIPANT_NAME_BYTES
            },
            "binding": {
                "oneOf": [
                    {
                        "type": "object",
                        "required": ["kind", "principal_id"],
                        "properties": {
                            "kind": { "const": "principal" },
                            "principal_id": uuid_schema()
                        },
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "required": ["kind", "entity_id"],
                        "properties": {
                            "kind": { "const": "entity" },
                            "entity_id": uuid_schema()
                        },
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "required": ["kind"],
                        "properties": { "kind": { "const": "uncontrolled" } },
                        "additionalProperties": false
                    }
                ]
            }
        },
        "additionalProperties": false
    })
}

fn message_state_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "conversation-id",
            "author-participant-id",
            "parent-message-id",
            "alternative-of",
            "current-revision"
        ],
        "properties": {
            "conversation-id": uuid_schema(),
            "author-participant-id": uuid_schema(),
            "parent-message-id": optional_uuid_schema(),
            "alternative-of": optional_uuid_schema(),
            "current-revision": uuid_schema()
        },
        "additionalProperties": false
    })
}

fn message_content_schema() -> Value {
    json!({
        "type": "object",
        "required": ["blocks", "effective-at"],
        "properties": {
            "blocks": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_MESSAGE_BLOCKS,
                "items": content_block_schema()
            },
            "effective-at": {
                "anyOf": [{ "type": "integer" }, { "type": "null" }]
            }
        },
        "additionalProperties": false
    })
}

fn create_conversation_schema() -> Value {
    json!({
        "type": "object",
        "required": ["title"],
        "properties": {
            "title": {
                "anyOf": [
                    { "type": "string", "maxLength": MAX_CHAT_TITLE_BYTES },
                    { "type": "null" }
                ]
            }
        },
        "additionalProperties": false
    })
}

fn add_participant_schema() -> Value {
    json!({
        "type": "object",
        "required": ["conversation-id", "display-name", "binding"],
        "properties": {
            "conversation-id": uuid_schema(),
            "display-name": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_PARTICIPANT_NAME_BYTES
            },
            "binding": {
                "oneOf": [
                    {
                        "type": "object",
                        "required": ["kind"],
                        "properties": { "kind": { "const": "current-principal" } },
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "required": ["kind", "entity_id"],
                        "properties": {
                            "kind": { "const": "entity" },
                            "entity_id": uuid_schema()
                        },
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "required": ["kind"],
                        "properties": { "kind": { "const": "uncontrolled" } },
                        "additionalProperties": false
                    }
                ]
            }
        },
        "additionalProperties": false
    })
}

fn send_message_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "conversation-id",
            "participant-id",
            "parent-message-id",
            "alternative-of",
            "blocks"
        ],
        "properties": {
            "conversation-id": uuid_schema(),
            "participant-id": uuid_schema(),
            "parent-message-id": optional_uuid_schema(),
            "alternative-of": optional_uuid_schema(),
            "blocks": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_MESSAGE_BLOCKS,
                "items": content_block_schema()
            }
        },
        "additionalProperties": false
    })
}

fn edit_message_schema() -> Value {
    json!({
        "type": "object",
        "required": ["message-id", "blocks"],
        "properties": {
            "message-id": uuid_schema(),
            "blocks": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_MESSAGE_BLOCKS,
                "items": content_block_schema()
            }
        },
        "additionalProperties": false
    })
}

fn select_branch_schema() -> Value {
    json!({
        "type": "object",
        "required": ["conversation-id", "message-id"],
        "properties": {
            "conversation-id": uuid_schema(),
            "message-id": uuid_schema()
        },
        "additionalProperties": false
    })
}

fn request_alternative_schema() -> Value {
    json!({
        "type": "object",
        "required": ["conversation-id", "message-id", "participant-id"],
        "properties": {
            "conversation-id": uuid_schema(),
            "message-id": uuid_schema(),
            "participant-id": uuid_schema()
        },
        "additionalProperties": false
    })
}

fn conversation_projection_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "conversations",
            "selected-conversation-id",
            "selected-leaf",
            "participants",
            "messages"
        ],
        "properties": {
            "conversations": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_CONVERSATIONS,
                "items": {
                    "type": "object",
                    "required": ["id", "title"],
                    "properties": {
                        "id": uuid_schema(),
                        "title": {
                            "anyOf": [
                                { "type": "string", "maxLength": MAX_CHAT_TITLE_BYTES },
                                { "type": "null" }
                            ]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "selected-conversation-id": optional_uuid_schema(),
            "selected-leaf": optional_uuid_schema(),
            "participants": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_PARTICIPANTS,
                "items": {
                    "type": "object",
                    "required": ["id", "display-name", "linked-entity", "can-send"],
                    "properties": {
                        "id": uuid_schema(),
                        "display-name": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_PARTICIPANT_NAME_BYTES
                        },
                        "linked-entity": optional_uuid_schema(),
                        "can-send": { "type": "boolean" }
                    },
                    "additionalProperties": false
                }
            },
            "messages": {
                "type": "array",
                "maxItems": crate::model::MAX_CHAT_MESSAGES,
                "items": {
                    "type": "object",
                    "required": [
                        "id",
                        "author-participant-id",
                        "parent-message-id",
                        "alternative-of",
                        "revision-id",
                        "blocks",
                        "effective-at"
                    ],
                    "properties": {
                        "id": uuid_schema(),
                        "author-participant-id": uuid_schema(),
                        "parent-message-id": optional_uuid_schema(),
                        "alternative-of": optional_uuid_schema(),
                        "revision-id": uuid_schema(),
                        "blocks": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": MAX_MESSAGE_BLOCKS,
                            "items": content_block_schema()
                        },
                        "effective-at": {
                            "anyOf": [
                                { "type": "integer" },
                                { "type": "null" }
                            ]
                        }
                    },
                    "additionalProperties": false
                }
            }
        },
        "additionalProperties": false
    })
}

fn event_schema(id: &str) -> Value {
    match id {
        CONVERSATION_CREATED_EVENT => json!({
            "type": "object",
            "required": ["conversation-id"],
            "properties": { "conversation-id": uuid_schema() },
            "additionalProperties": false
        }),
        PARTICIPANT_ADDED_EVENT => json!({
            "type": "object",
            "required": ["conversation-id", "participant-id"],
            "properties": {
                "conversation-id": uuid_schema(),
                "participant-id": uuid_schema()
            },
            "additionalProperties": false
        }),
        MESSAGE_ADDED_EVENT => json!({
            "type": "object",
            "required": [
                "conversation-id",
                "message-id",
                "revision-id",
                "participant-id",
                "parent-message-id",
                "alternative-of"
            ],
            "properties": {
                "conversation-id": uuid_schema(),
                "message-id": uuid_schema(),
                "revision-id": uuid_schema(),
                "participant-id": uuid_schema(),
                "parent-message-id": optional_uuid_schema(),
                "alternative-of": optional_uuid_schema()
            },
            "additionalProperties": false
        }),
        MESSAGE_EDITED_EVENT => json!({
            "type": "object",
            "required": ["message-id", "previous-revision-id", "revision-id"],
            "properties": {
                "message-id": uuid_schema(),
                "previous-revision-id": uuid_schema(),
                "revision-id": uuid_schema()
            },
            "additionalProperties": false
        }),
        BRANCH_SELECTED_EVENT => json!({
            "type": "object",
            "required": ["conversation-id", "message-id"],
            "properties": {
                "conversation-id": uuid_schema(),
                "message-id": uuid_schema()
            },
            "additionalProperties": false
        }),
        ALTERNATIVE_REQUESTED_EVENT => json!({
            "type": "object",
            "required": ["conversation-id", "message-id", "participant-id"],
            "properties": {
                "conversation-id": uuid_schema(),
                "message-id": uuid_schema(),
                "participant-id": uuid_schema()
            },
            "additionalProperties": false
        }),
        _ => empty_object_schema(),
    }
}
